use std::path::PathBuf;

use crate::{
    AutomationCategory, api::TorrentIdOrHash, bitv::BitV, bitv_factory::BitVFactory,
    session::TorrentId, torrent_state::ManagedTorrentHandle, type_aliases::BF,
};
use anyhow::Context;
use futures::{StreamExt, stream::BoxStream};
use librqbit_core::{Id20, spawn_utils::spawn};
use sqlx::{Pool, Postgres};
use tracing::debug_span;

use super::{SerializedPendingTorrent, SerializedTorrent, SessionPersistenceStore};

#[derive(Debug)]
pub struct PostgresSessionStorage {
    pool: Pool<Postgres>,
}

#[derive(sqlx::FromRow)]
struct TorrentsTableRecord {
    id: i32,
    info_hash: Vec<u8>,
    torrent_bytes: Vec<u8>,
    trackers: Vec<String>,
    output_folder: String,
    only_files: Option<Vec<i32>>,
    is_paused: bool,
    automation_metadata: String,
}

impl TorrentsTableRecord {
    fn into_serialized_torrent(self) -> anyhow::Result<(TorrentId, SerializedTorrent)> {
        Ok((
            self.id as TorrentId,
            SerializedTorrent {
                info_hash: Id20::from_bytes(&self.info_hash)
                    .context("invalid stored torrent hash")?,
                torrent_bytes: self.torrent_bytes.into(),
                trackers: self.trackers.into_iter().collect(),
                output_folder: PathBuf::from(self.output_folder),
                only_files: self
                    .only_files
                    .map(|v| v.into_iter().map(|v| v as usize).collect()),
                is_paused: self.is_paused,
                automation: serde_json::from_str(&self.automation_metadata)
                    .context("invalid stored automation metadata")?,
            },
        ))
    }
}

impl PostgresSessionStorage {
    pub async fn new(connection_string: &str) -> anyhow::Result<Self> {
        use sqlx::postgres::PgPoolOptions;

        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(connection_string)
            .await?;

        macro_rules! exec {
            ($q:expr) => {
                sqlx::query($q)
                    .execute(&pool)
                    .await
                    .context($q)
                    .context("error running query")?;
            };
        }

        exec!("CREATE SEQUENCE IF NOT EXISTS torrents_id AS integer;");

        exec!(
            "CREATE TABLE IF NOT EXISTS torrents (
          id INTEGER PRIMARY KEY DEFAULT nextval('torrents_id'),
          info_hash BYTEA NOT NULL,
          torrent_bytes BYTEA NOT NULL,
          trackers TEXT[] NOT NULL,
          output_folder TEXT NOT NULL,
          only_files INTEGER[],
          is_paused BOOLEAN NOT NULL
        )"
        );

        exec!("ALTER TABLE torrents ADD COLUMN IF NOT EXISTS have_bitfield BYTEA");
        exec!(
            "ALTER TABLE torrents ADD COLUMN IF NOT EXISTS automation_metadata TEXT NOT NULL DEFAULT '{}'"
        );
        exec!("CREATE TABLE IF NOT EXISTS automation_categories (name TEXT PRIMARY KEY)");
        exec!(
            "CREATE TABLE IF NOT EXISTS automation_preferences (id INTEGER PRIMARY KEY CHECK (id = 1), excluded_file_names_enabled BOOLEAN NOT NULL, excluded_file_names TEXT NOT NULL)"
        );

        exec!(
            "CREATE TABLE IF NOT EXISTS pending_torrents (info_hash BYTEA PRIMARY KEY, submission TEXT NOT NULL)"
        );
        exec!(
            "CREATE TABLE IF NOT EXISTS torrent_queue (id INTEGER PRIMARY KEY CHECK (id = 1), snapshot TEXT NOT NULL)"
        );
        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl SessionPersistenceStore for PostgresSessionStorage {
    async fn store_queue_and_exclusions(
        &self,
        snapshot: &crate::QueueSnapshot,
        enabled: bool,
        patterns: &str,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO torrent_queue (id, snapshot) VALUES (1, $1) ON CONFLICT (id) DO UPDATE SET snapshot=EXCLUDED.snapshot")
            .bind(serde_json::to_string(snapshot)?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO automation_preferences (id, excluded_file_names_enabled, excluded_file_names) VALUES (1, $1, $2) ON CONFLICT (id) DO UPDATE SET excluded_file_names_enabled=EXCLUDED.excluded_file_names_enabled, excluded_file_names=EXCLUDED.excluded_file_names")
            .bind(enabled).bind(patterns).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn load_queue(&self) -> anyhow::Result<crate::QueueSnapshot> {
        let value =
            sqlx::query_scalar::<_, String>("SELECT snapshot FROM torrent_queue WHERE id = 1")
                .fetch_optional(&self.pool)
                .await?;
        value
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map(|v| v.unwrap_or_default())
            .context("invalid queue snapshot")
    }
    async fn store_queue(&self, snapshot: &crate::QueueSnapshot) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO torrent_queue (id, snapshot) VALUES (1, $1) ON CONFLICT (id) DO UPDATE SET snapshot = EXCLUDED.snapshot")
            .bind(serde_json::to_string(snapshot)?).execute(&self.pool).await?;
        Ok(())
    }

    async fn load_excluded_file_names(&self) -> anyhow::Result<(bool, String)> {
        Ok(sqlx::query_as::<_, (bool, String)>(
            "SELECT excluded_file_names_enabled, excluded_file_names FROM automation_preferences WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .context("error loading excluded file names")?
        .unwrap_or((false, String::new())))
    }

    async fn store_excluded_file_names(&self, enabled: bool, patterns: &str) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO automation_preferences (id, excluded_file_names_enabled, excluded_file_names) VALUES (1, $1, $2) ON CONFLICT (id) DO UPDATE SET excluded_file_names_enabled = EXCLUDED.excluded_file_names_enabled, excluded_file_names = EXCLUDED.excluded_file_names")
            .bind(enabled)
            .bind(patterns)
            .execute(&self.pool)
            .await
            .context("error storing excluded file names")?;
        Ok(())
    }
    async fn load_pending(&self) -> anyhow::Result<Vec<SerializedPendingTorrent>> {
        let rows: Vec<(Vec<u8>, String)> =
            sqlx::query_as("SELECT info_hash, submission FROM pending_torrents")
                .fetch_all(&self.pool)
                .await
                .context("error loading pending torrents")?;
        rows.into_iter()
            .map(|(hash, payload)| {
                let record: SerializedPendingTorrent =
                    serde_json::from_str(&payload).context("invalid pending torrent record")?;
                anyhow::ensure!(
                    record.info_hash.0.as_slice() == hash.as_slice(),
                    "pending torrent identity mismatch"
                );
                Ok(record)
            })
            .collect()
    }

    async fn store_pending(&self, torrent: &SerializedPendingTorrent) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO pending_torrents (info_hash, submission) VALUES ($1, $2) ON CONFLICT (info_hash) DO UPDATE SET submission = EXCLUDED.submission")
            .bind(&torrent.info_hash.0[..]).bind(serde_json::to_string(torrent)?)
            .execute(&self.pool).await.context("error storing pending torrent")?;
        Ok(())
    }

    async fn delete_pending(&self, info_hash: Id20) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM pending_torrents WHERE info_hash = $1")
            .bind(&info_hash.0[..])
            .execute(&self.pool)
            .await
            .context("error deleting pending torrent")?;
        Ok(())
    }

    async fn defer_metadata(
        &self,
        id: TorrentId,
        torrent: &SerializedPendingTorrent,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        let removed = sqlx::query("DELETE FROM torrents WHERE id = $1 AND info_hash = $2")
            .bind(i32::try_from(id)?)
            .bind(&torrent.info_hash.0[..])
            .execute(&mut *tx)
            .await?;
        anyhow::ensure!(removed.rows_affected() == 1, "torrent identity mismatch");
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT submission FROM pending_torrents WHERE info_hash = $1 FOR UPDATE",
        )
        .bind(&torrent.info_hash.0[..])
        .fetch_optional(&mut *tx)
        .await?;
        let pending = match existing {
            Some(payload) => serde_json::from_str::<SerializedPendingTorrent>(&payload)
                .context("invalid pending torrent record")?,
            None => torrent.clone(),
        }
        .with_reserved_id(id)?;
        anyhow::ensure!(
            pending.info_hash == torrent.info_hash,
            "pending torrent identity mismatch"
        );
        sqlx::query("INSERT INTO pending_torrents (info_hash, submission) VALUES ($1, $2) ON CONFLICT (info_hash) DO UPDATE SET submission = EXCLUDED.submission")
            .bind(&torrent.info_hash.0[..])
            .bind(serde_json::to_string(&pending)?)
            .execute(&mut *tx)
            .await?;
        tx.commit()
            .await
            .context("error deferring torrent metadata")
    }

    async fn next_id(&self) -> anyhow::Result<TorrentId> {
        let (id,): (i32,) = sqlx::query_as("SELECT nextval('torrents_id')::int")
            .fetch_one(&self.pool)
            .await
            .context("error executing SELECT nextval")?;
        Ok(id as usize)
    }

    async fn store(&self, id: TorrentId, torrent: &ManagedTorrentHandle) -> anyhow::Result<()> {
        let torrent_bytes = torrent
            .metadata
            .load()
            .as_ref()
            .map(|i| i.torrent_bytes.clone())
            .unwrap_or_default();
        let mut transaction = self.pool.begin().await?;
        let q = "INSERT INTO torrents (id, info_hash, torrent_bytes, trackers, output_folder, only_files, is_paused, automation_metadata)
        VALUES($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT(id) DO UPDATE SET id = EXCLUDED.id WHERE torrents.info_hash = EXCLUDED.info_hash";
        let inserted = sqlx::query(q)
            .bind::<i32>(id.try_into()?)
            .bind(&torrent.info_hash().0[..])
            .bind(torrent_bytes.as_ref())
            .bind(
                torrent
                    .shared()
                    .trackers
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>(),
            )
            .bind(
                torrent
                    .shared()
                    .options
                    .output_folder
                    .to_str()
                    .context("output_folder")?
                    .to_owned(),
            )
            .bind(torrent.only_files().map(|o| {
                o.into_iter()
                    .filter_map(|o| o.try_into().ok())
                    .collect::<Vec<i32>>()
            }))
            .bind(torrent.is_paused())
            .bind(serde_json::to_string(&torrent.automation_metadata())?)
            .execute(&mut *transaction)
            .await
            .context("error executing INSERT INTO torrents")?;
        anyhow::ensure!(
            inserted.rows_affected() == 1,
            "torrent ID is already in use"
        );
        sqlx::query("DELETE FROM pending_torrents WHERE info_hash = $1")
            .bind(&torrent.info_hash().0[..])
            .execute(&mut *transaction)
            .await?;
        transaction
            .commit()
            .await
            .context("error committing torrent registration")?;
        Ok(())
    }

    async fn delete(&self, id: TorrentId) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM torrents WHERE id = $1")
            .bind::<i32>(id.try_into()?)
            .execute(&self.pool)
            .await
            .context("error executing DELETE FROM torrents")?;
        Ok(())
    }

    async fn get(&self, id: TorrentId) -> anyhow::Result<SerializedTorrent> {
        let row = sqlx::query_as::<_, TorrentsTableRecord>("SELECT * FROM torrents WHERE id = $1")
            .bind::<i32>(id.try_into()?)
            .fetch_one(&self.pool)
            .await
            .context("error executing SELECT * FROM torrents")?;
        row.into_serialized_torrent()
            .context("invalid stored torrent")
            .map(|(_, st)| st)
    }

    async fn update_metadata(
        &self,
        id: TorrentId,
        torrent: &ManagedTorrentHandle,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE torrents SET only_files = $1, is_paused = $2, automation_metadata = $3 WHERE id = $4",
        )
            .bind(torrent.only_files().map(|v| {
                v.into_iter()
                    .filter_map(|f| f.try_into().ok())
                    .collect::<Vec<i32>>()
            }))
            .bind(torrent.is_paused())
            .bind(serde_json::to_string(&torrent.automation_metadata())?)
            .bind::<i32>(id.try_into()?)
            .execute(&self.pool)
            .await
            .context("error executing UPDATE torrents")?;
        Ok(())
    }

    async fn load_automation_categories(&self) -> anyhow::Result<Vec<AutomationCategory>> {
        let names: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM automation_categories ORDER BY name")
                .fetch_all(&self.pool)
                .await
                .context("error loading automation categories")?;
        Ok(names
            .into_iter()
            .map(|(name,)| AutomationCategory { name })
            .collect())
    }

    async fn store_automation_category(&self, category: &AutomationCategory) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO automation_categories (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(&category.name)
            .execute(&self.pool)
            .await
            .context("error storing automation category")?;
        Ok(())
    }

    async fn delete_automation_category(&self, name: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM automation_categories WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await
            .context("error deleting automation category")?;
        Ok(())
    }

    async fn stream_all(
        &self,
    ) -> anyhow::Result<BoxStream<'_, anyhow::Result<(TorrentId, SerializedTorrent)>>> {
        let torrents = sqlx::query_as::<_, TorrentsTableRecord>("SELECT * FROM torrents")
            .fetch_all(&self.pool)
            .await
            .context("error executing SELECT * FROM torrents")?
            .into_iter()
            .map(TorrentsTableRecord::into_serialized_torrent);
        Ok(futures::stream::iter(torrents).boxed())
    }
}

struct PgBitfield {
    torrent_id: TorrentIdOrHash,
    inmem: BF,
    pool: Pool<Postgres>,
}

impl BitV for PgBitfield {
    fn as_slice(&self) -> &bitvec::prelude::BitSlice<u8, bitvec::prelude::Msb0> {
        self.inmem.as_bitslice()
    }

    fn as_slice_mut(&mut self) -> &mut bitvec::prelude::BitSlice<u8, bitvec::prelude::Msb0> {
        self.inmem.as_mut_bitslice()
    }

    fn into_dyn(self) -> Box<dyn BitV> {
        Box::new(self)
    }

    fn as_bytes(&self) -> &[u8] {
        self.inmem.as_raw_slice()
    }

    fn flush(&mut self, _flush_async: bool) -> anyhow::Result<()> {
        // TODO: make flush async, and don't spawn this, to avoid allocations and capture the result.
        spawn(
            debug_span!("pg_update_bitfield", id=?self.torrent_id),
            "pg_update_bitfield",
            {
                let hb = self.as_bytes().to_owned();
                let pool = self.pool.clone();
                let torrent_id = self.torrent_id;

                macro_rules! exec {
                    ($q:expr, $bf:expr, $id:expr) => {
                        sqlx::query($q)
                            .bind($bf)
                            .bind($id)
                            .execute(&pool)
                            .await
                            .context($q)
                            .context("error executing query")
                    };
                }

                async move {
                    match torrent_id {
                        TorrentIdOrHash::Id(id) => {
                            let id: i32 = id.try_into()?;
                            exec!(
                                "UPDATE torrents SET have_bitfield = $1 WHERE id = $2",
                                &hb,
                                id
                            )?;
                        }
                        TorrentIdOrHash::Hash(h) => {
                            exec!(
                                "UPDATE torrents SET have_bitfield = $1 WHERE info_hash = $2",
                                &hb,
                                &h.0[..]
                            )?;
                        }
                    };
                    Ok::<_, anyhow::Error>(())
                }
            },
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl BitVFactory for PostgresSessionStorage {
    async fn load(&self, id: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>> {
        #[derive(sqlx::FromRow)]
        struct HaveBitfield {
            have_bitfield: Option<Vec<u8>>,
        }

        macro_rules! exec {
            ($q:expr, $v:expr) => {
                sqlx::query_as($q)
                    .bind($v)
                    .fetch_one(&self.pool)
                    .await
                    .context($q)
                    .context("error executing query")?
            };
        }

        let hb: HaveBitfield = match id {
            TorrentIdOrHash::Id(id) => {
                let id: i32 = id.try_into()?;
                exec!("SELECT have_bitfield FROM torrents WHERE id = $1", id)
            }
            TorrentIdOrHash::Hash(h) => {
                exec!(
                    "SELECT have_bitfield FROM torrents WHERE info_hash = $1",
                    &h.0[..]
                )
            }
        };

        let hb = hb.have_bitfield;
        Ok(hb.map(|b| {
            PgBitfield {
                torrent_id: id,
                inmem: BF::from_boxed_slice(b.into_boxed_slice()),
                pool: self.pool.clone(),
            }
            .into_dyn()
        }))
    }

    async fn store_initial_check(
        &self,
        id: TorrentIdOrHash,
        b: BF,
    ) -> anyhow::Result<Box<dyn BitV>> {
        let mut bf = PgBitfield {
            torrent_id: id,
            inmem: b,
            pool: self.pool.clone(),
        };
        bf.flush(false)?;
        Ok(bf.into_dyn())
    }

    async fn clear(&self, id: TorrentIdOrHash) -> anyhow::Result<()> {
        macro_rules! exec {
            ($q:expr, $v:expr) => {
                sqlx::query($q)
                    .bind($v)
                    .execute(&self.pool)
                    .await
                    .context($q)
                    .context("error executing query")?
            };
        }

        match id {
            TorrentIdOrHash::Id(id) => {
                let id: i32 = id.try_into()?;
                exec!("UPDATE torrents SET have_bitfield = NULL WHERE id = $1", id);
            }
            TorrentIdOrHash::Hash(h) => {
                exec!(
                    "UPDATE torrents SET have_bitfield = NULL WHERE info_hash = $1",
                    &h.0[..]
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod automation_tests {
    use super::TorrentsTableRecord;
    use crate::TorrentAutomationMetadata;

    fn row(automation_metadata: String) -> TorrentsTableRecord {
        TorrentsTableRecord {
            id: 1,
            info_hash: vec![1; 20],
            torrent_bytes: vec![],
            trackers: vec![],
            output_folder: "/downloads".to_owned(),
            only_files: None,
            is_paused: true,
            automation_metadata,
        }
    }

    #[test]
    fn automation_settings_restore_without_silent_data_loss() {
        let metadata = TorrentAutomationMetadata {
            sequential_download: true,
            first_last_piece_priority: true,
            force_start: true,
            use_subfolder: true,
            inactive_seeding_time_limit_seconds: Some(600),
            ..Default::default()
        };
        let (_, stored) = row(serde_json::to_string(&metadata).unwrap())
            .into_serialized_torrent()
            .unwrap();
        let (_, options) = stored.into_add_torrent().unwrap();
        assert_eq!(options.automation, metadata);
        let (_, old) = row("{}".to_owned()).into_serialized_torrent().unwrap();
        assert_eq!(
            old.into_add_torrent().unwrap().1.automation,
            TorrentAutomationMetadata::default()
        );
        assert!(
            row("broken JSON".to_owned())
                .into_serialized_torrent()
                .is_err()
        );
        assert!(
            row(r#"{"inactive_seeding_time_limit_seconds":"invalid"}"#.to_owned())
                .into_serialized_torrent()
                .is_err()
        );
    }
    #[tokio::test]
    #[ignore = "requires a dedicated RQBIT_TEST_POSTGRES_URL database"]
    async fn missing_metadata_postgres_does_not_block_startup_or_resurrect_after_deletion() {
        use super::{PostgresSessionStorage, SessionPersistenceStore};
        use std::time::Duration;
        let url =
            std::env::var("RQBIT_TEST_POSTGRES_URL").expect("dedicated test database is required");
        let store = PostgresSessionStorage::new(&url).await.unwrap();
        let id = store.next_id().await.unwrap();
        let hash = librqbit_core::Id20::new([0x6b; 20]);
        let output = tempfile::tempdir().unwrap();
        sqlx::query("INSERT INTO torrents (id, info_hash, torrent_bytes, trackers, output_folder, is_paused) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(i32::try_from(id).unwrap()).bind(&hash.0[..]).bind(Vec::<u8>::new())
            .bind(vec!["udp://127.0.0.1:9/announce"]).bind(output.path().to_str().unwrap()).bind(false)
            .execute(&store.pool).await.unwrap();
        let restore = || {
            crate::Session::new_with_opts(
                output.path().to_owned(),
                crate::SessionOptions {
                    dht: None,
                    listen: None,
                    disable_local_service_discovery: true,
                    persistence: Some(crate::SessionPersistenceConfig::Postgres {
                        connection_string: url.clone(),
                    }),
                    ..Default::default()
                },
            )
        };
        let session = tokio::time::timeout(Duration::from_secs(3), restore())
            .await
            .expect("metadata recovery blocked PostgreSQL startup")
            .unwrap();
        let pending = session
            .pending_automation_torrents()
            .into_iter()
            .find(|p| p.info_hash == hash)
            .unwrap();
        assert!(!pending.paused);
        assert_eq!(
            store
                .load_pending()
                .await
                .unwrap()
                .into_iter()
                .find(|p| p.info_hash == hash)
                .unwrap()
                .reserved_id(),
            Some(id)
        );
        assert!(store.get(id).await.is_err());
        session
            .set_pending_automation_paused(hash, true)
            .await
            .unwrap();
        session.stop().await;
        let restored = restore().await.unwrap();
        assert!(
            restored
                .pending_automation_torrents()
                .into_iter()
                .find(|p| p.info_hash == hash)
                .unwrap()
                .paused
        );
        restored.delete(hash.into(), false).await.unwrap();
        restored.stop().await;
        let restored = restore().await.unwrap();
        assert!(
            !restored
                .pending_automation_torrents()
                .iter()
                .any(|p| p.info_hash == hash)
        );
        assert!(restored.get(hash.into()).is_none());
        restored.stop().await;
    }

    #[tokio::test]
    #[ignore = "requires a dedicated RQBIT_TEST_POSTGRES_URL database"]
    async fn metadata_deferral_postgres_is_atomic_and_survives_reconnection() {
        use super::{PostgresSessionStorage, SessionPersistenceStore};
        let url =
            std::env::var("RQBIT_TEST_POSTGRES_URL").expect("dedicated test database is required");
        let store = PostgresSessionStorage::new(&url).await.unwrap();
        let id = store.next_id().await.unwrap();
        let hash = librqbit_core::Id20::new([0x6a; 20]);
        sqlx::query("INSERT INTO torrents (id, info_hash, torrent_bytes, trackers, output_folder, is_paused) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(i32::try_from(id).unwrap()).bind(&hash.0[..]).bind(Vec::<u8>::new())
            .bind(Vec::<String>::new()).bind(".").bind(true)
            .execute(&store.pool).await.unwrap();
        let pending = store.get(id).await.unwrap().into_pending(id).unwrap();

        // Force the second half of the transaction to fail after DELETE runs.
        sqlx::query("ALTER TABLE pending_torrents ADD CONSTRAINT recovery_test_failure CHECK (false) NOT VALID")
            .execute(&store.pool).await.unwrap();
        assert!(store.defer_metadata(id, &pending).await.is_err());
        assert!(
            store.get(id).await.is_ok(),
            "failed transaction lost managed record"
        );
        assert!(
            !store
                .load_pending()
                .await
                .unwrap()
                .iter()
                .any(|r| r.info_hash == hash)
        );
        sqlx::query("ALTER TABLE pending_torrents DROP CONSTRAINT recovery_test_failure")
            .execute(&store.pool)
            .await
            .unwrap();

        let mut existing = pending.clone();
        existing.metadata_attempts = 3;
        existing.last_error = Some(crate::session::PendingAutomationError::MetadataTimeout);
        existing.options["preferred_id"] = serde_json::Value::Null;
        store.store_pending(&existing).await.unwrap();
        let mut wrong_hash = pending.clone();
        wrong_hash.info_hash = librqbit_core::Id20::new([1; 20]);
        assert!(store.defer_metadata(id, &wrong_hash).await.is_err());
        store.defer_metadata(id, &pending).await.unwrap();
        drop(store);
        let reopened = PostgresSessionStorage::new(&url).await.unwrap();
        assert!(reopened.get(id).await.is_err());
        let saved = reopened
            .load_pending()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.info_hash == hash)
            .unwrap();
        assert_eq!(saved.reserved_id(), Some(id));
        assert_eq!(saved.metadata_attempts, 3);
        assert_eq!(saved.last_error, existing.last_error);
        assert!(reopened.next_id().await.unwrap() > id);
        reopened.delete_pending(hash).await.unwrap();
        drop(reopened);
        let reopened = PostgresSessionStorage::new(&url).await.unwrap();
        assert!(reopened.get(id).await.is_err());
        assert!(
            !reopened
                .load_pending()
                .await
                .unwrap()
                .iter()
                .any(|r| r.info_hash == hash)
        );
    }

    #[tokio::test]
    #[ignore = "requires a dedicated RQBIT_TEST_POSTGRES_URL database"]
    async fn queue_postgres_atomic_preferences_survive_reconnection() {
        use super::{PostgresSessionStorage, SessionPersistenceStore};
        let url =
            std::env::var("RQBIT_TEST_POSTGRES_URL").expect("dedicated test database is required");
        let store = PostgresSessionStorage::new(&url).await.unwrap();
        let old = store.load_queue().await.unwrap();
        let old_filter = store.load_excluded_file_names().await.unwrap();
        let queue = crate::QueueSnapshot {
            config: crate::QueueConfig {
                enabled: true,
                max_active_downloads: 2,
                ..Default::default()
            },
            order: vec![librqbit_core::Id20::new([17; 20])],
        };
        store
            .store_queue_and_exclusions(&queue, true, "*.exe")
            .await
            .unwrap();
        drop(store);
        let restored = PostgresSessionStorage::new(&url).await.unwrap();
        let result = restored.load_queue().await.unwrap();
        assert_eq!(result.config, queue.config);
        assert_eq!(result.order, queue.order);
        assert_eq!(
            restored.load_excluded_file_names().await.unwrap(),
            (true, "*.exe".to_owned())
        );
        restored
            .store_queue_and_exclusions(&old, old_filter.0, &old_filter.1)
            .await
            .unwrap();
    }
}
