//! Session admission control. Durable user intent is independent of engine pauses.
use super::*;
use crate::TorrentStatsState;
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct QueueConfig {
    pub enabled: bool,
    pub max_active_downloads: i32,
    pub max_active_uploads: i32,
    pub max_active_torrents: i32,
    pub dont_count_slow_torrents: bool,
    pub slow_torrent_dl_rate_threshold: u32,
    pub slow_torrent_ul_rate_threshold: u32,
    pub slow_torrent_inactive_timer: u32,
}
impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_active_downloads: 3,
            max_active_uploads: 3,
            max_active_torrents: 5,
            dont_count_slow_torrents: false,
            slow_torrent_dl_rate_threshold: 2,
            slow_torrent_ul_rate_threshold: 2,
            slow_torrent_inactive_timer: 60,
        }
    }
}
impl QueueConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            [
                self.max_active_downloads,
                self.max_active_uploads,
                self.max_active_torrents
            ]
            .into_iter()
            .all(|n| n >= -1),
            "queue limits must be -1 or nonnegative"
        );
        anyhow::ensure!(
            self.slow_torrent_dl_rate_threshold <= i32::MAX as u32 / 1024
                && self.slow_torrent_ul_rate_threshold <= i32::MAX as u32 / 1024
                && self.slow_torrent_inactive_timer <= i32::MAX as u32,
            "queue threshold out of range"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueueConfigPatch {
    pub enabled: Option<bool>,
    pub max_active_downloads: Option<i32>,
    pub max_active_uploads: Option<i32>,
    pub max_active_torrents: Option<i32>,
    pub dont_count_slow_torrents: Option<bool>,
    pub slow_torrent_dl_rate_threshold: Option<u32>,
    pub slow_torrent_ul_rate_threshold: Option<u32>,
    pub slow_torrent_inactive_timer: Option<u32>,
}
impl QueueConfigPatch {
    pub fn apply(&self, config: &mut QueueConfig) {
        if let Some(value) = self.enabled {
            config.enabled = value;
        }
        if let Some(value) = self.max_active_downloads {
            config.max_active_downloads = value;
        }
        if let Some(value) = self.max_active_uploads {
            config.max_active_uploads = value;
        }
        if let Some(value) = self.max_active_torrents {
            config.max_active_torrents = value;
        }
        if let Some(value) = self.dont_count_slow_torrents {
            config.dont_count_slow_torrents = value;
        }
        if let Some(value) = self.slow_torrent_dl_rate_threshold {
            config.slow_torrent_dl_rate_threshold = value;
        }
        if let Some(value) = self.slow_torrent_ul_rate_threshold {
            config.slow_torrent_ul_rate_threshold = value;
        }
        if let Some(value) = self.slow_torrent_inactive_timer {
            config.slow_torrent_inactive_timer = value;
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSnapshot {
    pub config: QueueConfig,
    #[serde(with = "queue_hashes")]
    pub order: Vec<Id20>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueOperation {
    Top,
    Bottom,
    Up,
    Down,
}

#[derive(Default)]
pub(super) struct QueueRuntime {
    pub snapshot: QueueSnapshot,
    pub(super) admitted: HashSet<Id20>,
    slow: HashMap<Id20, SlowState>,
    pub(super) resolving: HashMap<Id20, (CancellationToken, Arc<std::sync::atomic::AtomicBool>)>,
}

impl QueueRuntime {
    pub(super) fn new(snapshot: QueueSnapshot) -> Self {
        Self {
            snapshot,
            ..Default::default()
        }
    }
}

#[derive(Default)]
struct SlowState {
    inactive: bool,
    pending: Option<(bool, Instant)>,
}
impl SlowState {
    // libtorrent debounces both transitions and uses download speed for
    // downloaders, upload speed for seeds (torrent::is_inactive_internal).
    fn observe(&mut self, below_threshold: bool, now: Instant, grace: Duration) -> bool {
        if below_threshold == self.inactive {
            self.pending = None;
        } else {
            let (_, since) = *self.pending.get_or_insert((below_threshold, now));
            if now.duration_since(since) >= grace {
                self.inactive = below_threshold;
                self.pending = None;
            }
        }
        self.inactive
    }
}

fn reorder(order: &mut Vec<Id20>, selected: &HashSet<Id20>, operation: QueueOperation) {
    match operation {
        QueueOperation::Top | QueueOperation::Bottom => {
            let (mut chosen, mut rest): (Vec<_>, Vec<_>) =
                order.iter().copied().partition(|h| selected.contains(h));
            if matches!(operation, QueueOperation::Top) {
                chosen.append(&mut rest);
                *order = chosen;
            } else {
                rest.append(&mut chosen);
                *order = rest;
            }
        }
        QueueOperation::Up => {
            for i in 1..order.len() {
                if selected.contains(&order[i]) && !selected.contains(&order[i - 1]) {
                    order.swap(i, i - 1);
                }
            }
        }
        QueueOperation::Down => {
            for i in (0..order.len().saturating_sub(1)).rev() {
                if selected.contains(&order[i]) && !selected.contains(&order[i + 1]) {
                    order.swap(i, i + 1);
                }
            }
        }
    }
}

// Mirrors libtorrent 2.0 seed_rank: unmet seed targets, scarce seeds and
// recently started seeds take precedence. Unknown scrape counts use peer data.
#[derive(Clone, Copy)]
struct SeedRankInput {
    active: u64,
    finished: u64,
    uploaded: u64,
    downloaded: u64,
    full_seed: bool,
    running: bool,
    seeds: u64,
    downloaders: u64,
}
fn seed_rank(input: SeedRankInput) -> u64 {
    let SeedRankInput {
        active,
        finished,
        uploaded,
        downloaded,
        full_seed,
        running,
        seeds,
        downloaders,
    } = input;
    let mut rank = 0;
    let download_time = active.saturating_sub(finished);
    if finished < 86400
        && download_time > 1
        && finished.saturating_mul(100) / download_time < 700
        && downloaded > 0
        && uploaded.saturating_mul(100) / downloaded < 200
    {
        rank |= 0x40000000;
    }
    if running && active < 1800 {
        rank |= 0x10000000;
    }
    if let Some(score) = downloaders
        .saturating_add(1)
        .saturating_mul(if full_seed { 1000 } else { 500 })
        .checked_div(seeds)
    {
        rank |= score & 0x0fffffff;
    } else {
        rank |= 0x20000000 | (downloaders & 0x0fffffff);
    }
    rank
}

struct Candidate {
    hash: Id20,
    seed: bool,
    forced: bool,
    slow: bool,
    rank: u64,
}
fn admit(config: &QueueConfig, candidates: &[Candidate]) -> HashSet<Id20> {
    let mut admitted = HashSet::new();
    let (mut downloads, mut seeds, mut total) = (0, 0, 0);
    let fits = |used: i32, limit: i32| limit == -1 || used < limit;
    for c in candidates {
        if c.forced || !config.enabled {
            admitted.insert(c.hash);
            continue;
        }
        let slow = config.dont_count_slow_torrents && c.slow;
        let count = if c.seed { &mut seeds } else { &mut downloads };
        let limit = if c.seed {
            config.max_active_uploads
        } else {
            config.max_active_downloads
        };
        if fits(total, config.max_active_torrents) && (slow || fits(*count, limit)) {
            admitted.insert(c.hash);
            total += 1;
            if !slow {
                *count += 1;
            }
        }
    }
    admitted
}

impl Session {
    pub fn queue_config(&self) -> QueueConfig {
        self.queue.read().snapshot.config.clone()
    }
    pub(crate) fn queue_allows(&self, hash: Id20) -> bool {
        let q = self.queue.read();
        !q.snapshot.config.enabled || q.admitted.contains(&hash)
    }
    pub fn queue_position(&self, hash: Id20) -> Option<usize> {
        let q = self.queue.read();
        q.snapshot
            .config
            .enabled
            .then(|| {
                q.snapshot
                    .order
                    .iter()
                    .position(|h| *h == hash)
                    .map(|p| p + 1)
            })
            .flatten()
    }
    pub(super) async fn persist_queue(&self, snapshot: &QueueSnapshot) -> anyhow::Result<()> {
        if let Some(store) = &self.persistence {
            store.store_queue(snapshot).await?;
        }
        Ok(())
    }
    pub(crate) async fn patch_queue_preferences(
        self: &Arc<Self>,
        patch: QueueConfigPatch,
        enabled: Option<bool>,
        patterns: Option<String>,
    ) -> anyhow::Result<()> {
        {
            let _queue_guard = self.queue_mutation.lock().await;
            let _filter_guard = self.excluded_file_names_mutation.lock().await;
            let mut snapshot = self.queue.read().snapshot.clone();
            patch.apply(&mut snapshot.config);
            snapshot.config.validate()?;
            let old = self.excluded_file_names();
            let filter =
                ExcludedFileNames::new(enabled.unwrap_or(old.0), patterns.unwrap_or(old.1))?;
            if let Some(store) = &self.persistence {
                store
                    .store_queue_and_exclusions(&snapshot, filter.enabled, &filter.patterns)
                    .await?;
            }
            *self.excluded_file_names.write() = filter;
            self.queue.write().snapshot = snapshot;
        }
        self.reconcile_queue().await
    }
    pub async fn set_queue_config(self: &Arc<Self>, config: QueueConfig) -> anyhow::Result<()> {
        config.validate()?;
        {
            let _guard = self.queue_mutation.lock().await;
            let mut snapshot = self.queue.read().snapshot.clone();
            snapshot.config = config;
            self.persist_queue(&snapshot).await?;
            self.queue.write().snapshot = snapshot;
        }
        self.reconcile_queue().await
    }
    pub async fn reorder_queue(
        self: &Arc<Self>,
        hashes: Vec<Id20>,
        operation: QueueOperation,
    ) -> anyhow::Result<()> {
        {
            let _guard = self.queue_mutation.lock().await;
            anyhow::ensure!(self.queue_config().enabled, "torrent queueing is disabled");
            let mut snapshot = self.queue.read().snapshot.clone();
            self.normalize_queue(&mut snapshot);
            let selected = hashes.into_iter().collect();
            reorder(&mut snapshot.order, &selected, operation);
            self.persist_queue(&snapshot).await?;
            self.queue.write().snapshot = snapshot;
        }
        self.reconcile_queue().await
    }
    fn normalize_queue(&self, snapshot: &mut QueueSnapshot) {
        let mut torrents = self.with_torrents(|ts| {
            ts.map(|(_, t)| {
                (
                    t.info_hash(),
                    t.automation_metadata().added_at_unix_seconds,
                    t.stats().finished,
                )
            })
            .collect::<Vec<_>>()
        });
        torrents.extend(
            self.pending_automation_torrents()
                .into_iter()
                .map(|p| (p.info_hash, p.automation.added_at_unix_seconds, false)),
        );
        torrents.sort_by_key(|(hash, added, _)| (*added, hash.as_string()));
        let valid: HashSet<_> = torrents
            .iter()
            .filter(|(_, _, seed)| !seed)
            .map(|(hash, _, _)| *hash)
            .collect();
        let mut seen = HashSet::new();
        snapshot
            .order
            .retain(|h| valid.contains(h) && seen.insert(*h));
        for (hash, _, seed) in torrents {
            if !seed && seen.insert(hash) {
                snapshot.order.push(hash);
            }
        }
    }
    pub(crate) async fn reconcile_queue(self: &Arc<Self>) -> anyhow::Result<()> {
        let _guard = self.queue_mutation.lock().await;
        if self.queue_stopping.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut snapshot = self.queue.read().snapshot.clone();
        self.normalize_queue(&mut snapshot);
        if snapshot.order != self.queue.read().snapshot.order {
            self.persist_queue(&snapshot).await?;
            self.queue.write().snapshot = snapshot.clone();
        }
        let torrents = self.with_torrents(|ts| ts.map(|(_, t)| t.clone()).collect::<Vec<_>>());
        let mut candidates = Vec::new();
        let now = Instant::now();
        for t in &torrents {
            let (stats, metadata, _) = t.refresh_automation_stats(unix_time_seconds());
            if t.is_paused()
                || matches!(
                    stats.state,
                    TorrentStatsState::Error | TorrentStatsState::Initializing { .. }
                )
            {
                continue;
            }
            let running = matches!(stats.state, TorrentStatsState::Live);
            let below_threshold = stats.live.as_ref().is_some_and(|l| {
                if stats.finished {
                    l.upload_speed.as_bytes()
                        < u64::from(snapshot.config.slow_torrent_ul_rate_threshold) * 1024
                } else {
                    l.download_speed.as_bytes()
                        < u64::from(snapshot.config.slow_torrent_dl_rate_threshold) * 1024
                }
            });
            let slow = if running
                && snapshot.config.enabled
                && snapshot.config.dont_count_slow_torrents
            {
                self.queue
                    .write()
                    .slow
                    .entry(t.info_hash())
                    .or_default()
                    .observe(
                        below_threshold,
                        now,
                        Duration::from_secs(u64::from(snapshot.config.slow_torrent_inactive_timer)),
                    )
            } else {
                self.queue.write().slow.remove(&t.info_hash());
                false
            };
            let total_size = t
                .metadata
                .load()
                .as_ref()
                .map(|m| m.lengths().total_length())
                .unwrap_or(stats.total_bytes);
            let full_seed = stats.progress_bytes == total_size;
            let announces = self.tracker_announces(t.info_hash());
            let peer_counts = t.live().map(|live| live.queue_peer_counts());
            let seeds = announces
                .values()
                .filter_map(|a| a.seeds)
                .max()
                .map(|n| n.saturating_sub(u64::from(running && full_seed)))
                .or_else(|| peer_counts.map(|p| p.0))
                .or(metadata.queue_last_known_seeds)
                .unwrap_or(0);
            let leechers = announces
                .values()
                .filter_map(|a| a.leeches)
                .max()
                .or_else(|| peer_counts.map(|p| p.1))
                .or(metadata.queue_last_known_leechers)
                .unwrap_or(0);
            {
                let mut a = t.shared.automation.write();
                a.queue_last_known_seeds = Some(seeds);
                a.queue_last_known_leechers = Some(leechers);
            }
            let rank = seed_rank(SeedRankInput {
                active: metadata.active_seconds,
                finished: metadata.seeding_seconds,
                uploaded: metadata.uploaded_bytes,
                downloaded: metadata.payload_downloaded_bytes.max(total_size),
                full_seed,
                running,
                seeds,
                downloaders: leechers,
            });
            candidates.push(Candidate {
                hash: t.info_hash(),
                seed: stats.finished,
                forced: metadata.force_start,
                slow,
                rank,
            });
        }
        for p in self.pending_automation_torrents() {
            if !p.paused
                && matches!(p.state, PendingAutomationTorrentState::ResolvingMetadata)
                && !torrents.iter().any(|t| t.info_hash() == p.info_hash)
            {
                candidates.push(Candidate {
                    hash: p.info_hash,
                    seed: false,
                    forced: p.automation.force_start,
                    slow: false,
                    rank: 0,
                });
            }
        }
        let positions: HashMap<_, _> = snapshot
            .order
            .iter()
            .enumerate()
            .map(|(position, hash)| (*hash, position))
            .collect();
        candidates.sort_by(|a, b| {
            a.seed
                .cmp(&b.seed)
                .then_with(|| {
                    if a.seed {
                        b.rank.cmp(&a.rank)
                    } else {
                        positions.get(&a.hash).cmp(&positions.get(&b.hash))
                    }
                })
                .then_with(|| a.hash.as_string().cmp(&b.hash.as_string()))
        });
        let admitted = admit(&snapshot.config, &candidates);
        // Revoke admission first, then stop displaced engines before publishing new permits.
        self.queue.write().admitted.retain(|h| admitted.contains(h));
        let displaced = self
            .queue
            .read()
            .resolving
            .iter()
            .filter(|(h, _)| !admitted.contains(h))
            .map(|(_, v)| v.clone())
            .collect::<Vec<_>>();
        for (cancel, _) in &displaced {
            cancel.cancel();
        }
        for (_, active) in displaced {
            while active.load(Ordering::Acquire) {
                tokio::select! {
                    _ = self.cancellation_token.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(Duration::from_millis(5)) => {}
                }
            }
        }
        for t in &torrents {
            if !admitted.contains(&t.info_hash()) && t.live().is_some() {
                t.hold_for_queue()?;
            }
        }
        {
            let mut q = self.queue.write();
            q.slow.retain(|h, _| admitted.contains(h));
            q.resolving
                .retain(|_, (_, active)| active.load(Ordering::Acquire));
            q.admitted = admitted.clone();
        }
        for t in torrents {
            if admitted.contains(&t.info_hash())
                && !t.is_paused()
                && matches!(t.stats().state, TorrentStatsState::Paused)
            {
                let peers = self.make_peer_rx_managed_torrent(&t, true);
                t.start(peers, false)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(n: u8) -> Id20 {
        Id20::new([n; 20])
    }
    fn c(n: u8, seed: bool, forced: bool, slow: bool) -> Candidate {
        Candidate {
            hash: h(n),
            seed,
            forced,
            slow,
            rank: 0,
        }
    }
    #[test]
    fn limits_and_forced() {
        let config = QueueConfig {
            enabled: true,
            max_active_downloads: 1,
            max_active_uploads: 1,
            max_active_torrents: 2,
            ..Default::default()
        };
        assert_eq!(
            admit(
                &config,
                &[
                    c(1, false, false, false),
                    c(2, false, false, false),
                    c(3, true, false, false),
                    c(4, true, true, false)
                ]
            ),
            HashSet::from([h(1), h(3), h(4)])
        );
        assert!(
            admit(
                &QueueConfig {
                    max_active_torrents: 0,
                    ..config
                },
                &[c(1, false, false, false)]
            )
            .is_empty()
        );
    }
    #[test]
    fn slow_still_counts_total() {
        let config = QueueConfig {
            enabled: true,
            max_active_downloads: 1,
            max_active_torrents: 2,
            dont_count_slow_torrents: true,
            ..Default::default()
        };
        assert_eq!(
            admit(
                &config,
                &[
                    c(1, false, false, true),
                    c(2, false, false, false),
                    c(3, false, false, true)
                ]
            ),
            HashSet::from([h(1), h(2)])
        );
    }
    #[test]
    fn stable_reorders() {
        for (op, expected) in [
            (QueueOperation::Top, vec![2, 3, 1, 4]),
            (QueueOperation::Bottom, vec![1, 4, 2, 3]),
            (QueueOperation::Up, vec![2, 3, 1, 4]),
            (QueueOperation::Down, vec![1, 4, 2, 3]),
        ] {
            let mut order = vec![h(1), h(2), h(3), h(4)];
            reorder(&mut order, &HashSet::from([h(2), h(3)]), op);
            assert_eq!(order, expected.into_iter().map(h).collect::<Vec<_>>());
        }
    }
    #[test]
    fn validation_and_ranking() {
        assert!(
            QueueConfig {
                max_active_downloads: -2,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            QueueConfig {
                slow_torrent_ul_rate_threshold: u32::MAX,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let base = SeedRankInput {
            active: 2000,
            finished: 2000,
            uploaded: 0,
            downloaded: 100,
            full_seed: true,
            running: false,
            seeds: 1,
            downloaders: 1,
        };
        assert!(seed_rank(SeedRankInput { seeds: 0, ..base }) > seed_rank(base));
        assert!(
            seed_rank(base)
                > seed_rank(SeedRankInput {
                    full_seed: false,
                    ..base
                })
        );
        let recent = SeedRankInput {
            active: 100,
            finished: 100,
            ..base
        };
        assert!(
            seed_rank(SeedRankInput {
                running: true,
                ..recent
            }) > seed_rank(recent)
        );
    }
}

mod queue_hashes {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        hashes: &[Id20],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        hashes
            .iter()
            .map(Id20::as_string)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Id20>, D::Error> {
        Vec::<String>::deserialize(deserializer)?
            .into_iter()
            .map(|s| s.parse().map_err(serde::de::Error::custom))
            .collect()
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, create_torrent};

    async fn session(downloads: &Path, store: &Path) -> Arc<Session> {
        Session::new_with_opts(
            downloads.to_owned(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                disable_trackers: true,
                persistence: Some(SessionPersistenceConfig::Json {
                    folder: Some(store.to_owned()),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }
    async fn torrent(
        session: &Arc<Session>,
        fixture: &Path,
        name: &str,
        paused: bool,
    ) -> ManagedTorrentHandle {
        let source = fixture.join(name);
        std::fs::write(&source, vec![42; 16384]).unwrap();
        let file = create_torrent(
            &source,
            CreateTorrentOptions::default(),
            &BlockingSpawner::new(1),
        )
        .await
        .unwrap();
        let t = session
            .add_torrent(
                AddTorrent::TorrentFileBytes(file.as_bytes().unwrap()),
                Some(AddTorrentOptions {
                    paused,
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        t.wait_until_initialized().await.unwrap();
        t
    }
    fn limited() -> QueueConfig {
        QueueConfig {
            enabled: true,
            max_active_downloads: 1,
            max_active_uploads: 1,
            max_active_torrents: 1,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn queue_lifecycle_preserves_order_and_stop_intent_across_restart() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let s = session(downloads.path(), store.path()).await;
        s.set_queue_config(limited()).await.unwrap();
        let a = torrent(&s, fixtures.path(), "first.bin", false).await;
        let b = torrent(&s, fixtures.path(), "second.bin", false).await;
        s.reconcile_queue().await.unwrap();
        assert!(a.live().is_some());
        assert!(b.is_queued());
        assert!(!b.is_paused());
        s.reorder_queue(vec![b.info_hash()], QueueOperation::Top)
            .await
            .unwrap();
        assert!(b.live().is_some());
        assert!(a.is_queued());
        s.pause(&b).await.unwrap();
        s.reconcile_queue().await.unwrap();
        assert!(b.is_paused());
        assert!(a.live().is_some());
        s.set_torrent_force_start(b.info_hash().into(), true)
            .await
            .unwrap();
        s.unpause(&b).await.unwrap();
        assert!(a.live().is_some() && b.live().is_some());
        s.set_torrent_force_start(b.info_hash().into(), false)
            .await
            .unwrap();
        s.reconcile_queue().await.unwrap();
        assert!(b.live().is_some());
        assert!(a.is_queued());
        s.pause(&a).await.unwrap();
        let (ah, bh) = (a.info_hash(), b.info_hash());
        s.stop().await;
        drop(a);
        drop(b);
        drop(s);
        let restored = session(downloads.path(), store.path()).await;
        for t in restored.with_torrents(|ts| ts.map(|(_, t)| t.clone()).collect::<Vec<_>>()) {
            t.wait_until_initialized().await.unwrap();
        }
        restored.reconcile_queue().await.unwrap();
        let a = restored.get(ah.into()).unwrap();
        let b = restored.get(bh.into()).unwrap();
        assert!(a.is_paused());
        assert!(b.live().is_some());
        assert_eq!(restored.queue_position(bh), Some(1));
        restored
            .set_queue_config(QueueConfig::default())
            .await
            .unwrap();
        assert!(a.is_paused());
        assert!(!b.is_paused());
        restored.stop().await;
    }

    #[tokio::test]
    async fn failed_queue_write_does_not_publish_new_settings_or_order() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let s = session(downloads.path(), store.path()).await;
        s.set_queue_config(limited()).await.unwrap();
        let a = torrent(&s, fixtures.path(), "a.bin", false).await;
        let b = torrent(&s, fixtures.path(), "b.bin", false).await;
        s.reconcile_queue().await.unwrap();
        let before = s.queue.read().snapshot.order.clone();
        // Replace the storage directory with a file to make all atomic writes fail.
        let saved = store.path().with_extension("saved-queue-test");
        std::fs::rename(store.path(), &saved).unwrap();
        std::fs::write(store.path(), b"unavailable").unwrap();
        assert!(
            s.set_queue_config(QueueConfig {
                enabled: false,
                ..limited()
            })
            .await
            .is_err()
        );
        assert!(s.queue_config().enabled);
        assert!(
            s.reorder_queue(vec![b.info_hash()], QueueOperation::Top)
                .await
                .is_err()
        );
        assert_eq!(s.queue.read().snapshot.order, before);
        assert!(a.live().is_some());
        assert!(b.is_queued());
        std::fs::remove_file(store.path()).unwrap();
        std::fs::rename(saved, store.path()).unwrap();
        s.stop().await;
    }

    #[tokio::test]
    async fn completion_changes_to_seed_slots_and_deletion_releases_admission() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let s = session(downloads.path(), store.path()).await;
        s.set_queue_config(QueueConfig {
            max_active_uploads: 0,
            max_active_torrents: 2,
            ..limited()
        })
        .await
        .unwrap();
        std::fs::write(downloads.path().join("seed.bin"), vec![42; 16384]).unwrap();
        let seed = torrent(&s, fixtures.path(), "seed.bin", false).await;
        let download = torrent(&s, fixtures.path(), "download.bin", false).await;
        s.reconcile_queue().await.unwrap();
        assert!(seed.stats().finished);
        assert!(seed.is_queued());
        assert!(download.live().is_some());
        s.set_torrent_force_start(seed.info_hash().into(), true)
            .await
            .unwrap();
        s.reconcile_queue().await.unwrap();
        assert!(seed.live().is_some());
        s.delete(download.info_hash().into(), false).await.unwrap();
        s.reconcile_queue().await.unwrap();
        assert!(s.get(download.info_hash().into()).is_none());
        assert!(
            !s.queue
                .read()
                .snapshot
                .order
                .contains(&download.info_hash())
        );
        s.stop().await;
    }
    #[tokio::test]
    async fn concurrent_patches_preserve_independent_settings_and_failed_pause_keeps_running() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let s = session(downloads.path(), store.path()).await;
        let (a, b) = tokio::join!(
            s.patch_queue_preferences(
                QueueConfigPatch {
                    max_active_downloads: Some(2),
                    ..Default::default()
                },
                None,
                None
            ),
            s.patch_queue_preferences(
                QueueConfigPatch {
                    max_active_uploads: Some(4),
                    ..Default::default()
                },
                None,
                None
            )
        );
        a.unwrap();
        b.unwrap();
        assert_eq!(s.queue_config().max_active_downloads, 2);
        assert_eq!(s.queue_config().max_active_uploads, 4);
        let t = torrent(&s, fixtures.path(), "running.bin", false).await;
        assert!(t.live().is_some());
        let saved = store.path().with_extension("saved-pause-test");
        std::fs::rename(store.path(), &saved).unwrap();
        std::fs::write(store.path(), b"unavailable").unwrap();
        assert!(s.pause(&t).await.is_err());
        assert!(!t.is_paused());
        assert!(t.live().is_some());
        assert!(
            s.patch_queue_preferences(
                QueueConfigPatch {
                    enabled: Some(true),
                    ..Default::default()
                },
                Some(true),
                Some("*.exe".to_owned())
            )
            .await
            .is_err()
        );
        assert!(!s.queue_config().enabled);
        assert!(!s.excluded_file_names().0);
        std::fs::remove_file(store.path()).unwrap();
        std::fs::rename(saved, store.path()).unwrap();
        s.stop().await;
    }
    #[tokio::test]
    async fn disabling_queue_does_not_restart_share_limit_stops() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let s = session(downloads.path(), store.path()).await;
        s.set_queue_config(limited()).await.unwrap();
        std::fs::write(downloads.path().join("limited-seed.bin"), vec![42; 16384]).unwrap();
        let seed = torrent(&s, fixtures.path(), "limited-seed.bin", false).await;
        s.set_torrent_automation_limits(seed.info_hash().into(), None, Some(0))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !seed.is_paused() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(seed.automation_metadata().stopped_by_share_limit);
        s.set_queue_config(QueueConfig::default()).await.unwrap();
        assert!(seed.is_paused());
        assert!(seed.live().is_none());
        s.set_torrent_automation_limits(seed.info_hash().into(), None, None)
            .await
            .unwrap();
        s.unpause(&seed).await.unwrap();
        assert!(!seed.automation_metadata().stopped_by_share_limit);
        assert!(seed.live().is_some());
        s.stop().await;
    }
    #[tokio::test]
    async fn queued_magnet_resolves_downloads_and_enters_seed_queue() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let seeder = Session::new_with_opts(
            fixtures.path().to_owned(),
            SessionOptions {
                dht: None,
                listen: Some(crate::ListenerOptions {
                    listen_addr: ([127, 0, 0, 1], 0).into(),
                    ..Default::default()
                }),
                disable_trackers: true,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let seed = torrent(&seeder, fixtures.path(), "magnet-fixture.bin", false).await;
        assert!(seed.stats().finished);
        let hash = seed.info_hash();
        let s = session(downloads.path(), store.path()).await;
        s.set_queue_config(QueueConfig {
            max_active_downloads: 0,
            max_active_uploads: 0,
            ..limited()
        })
        .await
        .unwrap();
        s.add_pending_automation_magnet(
            format!("magnet:?xt=urn:btih:{}", hash.as_string()),
            AddTorrentOptions {
                overwrite: true,
                initial_peers: Some(vec![seeder.listen_addr().unwrap()]),
                automation: TorrentAutomationMetadata {
                    sequential_download: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(s.pending_automation_torrents()[0].metadata_attempts, 0);
        s.set_queue_config(QueueConfig {
            max_active_uploads: 0,
            ..limited()
        })
        .await
        .unwrap();
        let managed = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(t) = s.get(hash.into())
                    && t.stats().finished
                    && s.pending_automation_torrents().is_empty()
                {
                    break t;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        s.reconcile_queue().await.unwrap();
        assert!(managed.is_queued());
        assert!(!managed.is_paused());
        assert!(managed.automation_metadata().sequential_download);
        assert_eq!(
            std::fs::read(downloads.path().join("magnet-fixture.bin")).unwrap(),
            vec![42; 16384]
        );
        s.stop().await;
        seeder.stop().await;
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    #[test]
    fn slow_transitions_require_sustained_rates_and_reset_on_recovery() {
        let now = Instant::now();
        let grace = Duration::from_secs(60);
        let mut state = SlowState::default();
        assert!(!state.observe(true, now, grace));
        assert!(!state.observe(true, now + Duration::from_secs(59), grace));
        assert!(!state.observe(false, now + Duration::from_secs(59), grace));
        assert!(!state.observe(true, now + Duration::from_secs(60), grace));
        assert!(state.observe(true, now + Duration::from_secs(120), grace));
        assert!(state.observe(false, now + Duration::from_secs(121), grace));
        assert!(state.observe(true, now + Duration::from_secs(122), grace));
        assert!(state.observe(false, now + Duration::from_secs(123), grace));
        assert!(!state.observe(false, now + Duration::from_secs(183), grace));
    }
    #[test]
    fn queue_snapshot_roundtrip_and_old_defaults() {
        let snapshot = QueueSnapshot {
            config: QueueConfig {
                enabled: true,
                ..Default::default()
            },
            order: vec![Id20::new([7; 20])],
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["order"][0], "07".repeat(20));
        let restored: QueueSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(restored.order, snapshot.order);
        assert_eq!(restored.config, snapshot.config);
        let old: QueueSnapshot = serde_json::from_str("{}").unwrap();
        assert!(!old.config.enabled);
        assert!(old.order.is_empty());
        assert!(serde_json::from_str::<QueueSnapshot>(r#"{"order":["invalid"]}"#).is_err());
    }
}
