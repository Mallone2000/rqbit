use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use parking_lot::Mutex;

#[derive(Clone, Copy)]
struct ProgressSnapshot {
    progress_bytes: u64,
    instant: Instant,
}

/// Estimates download/upload speed in a sliding time window.
pub struct SpeedEstimator {
    latest_per_second_snapshots: Mutex<VecDeque<ProgressSnapshot>>,
    bytes_per_second: AtomicU64,
    time_remaining_millis: AtomicU64,
}

impl Default for SpeedEstimator {
    fn default() -> Self {
        Self::new(5)
    }
}

impl SpeedEstimator {
    pub fn new(window_seconds: usize) -> Self {
        assert!(window_seconds > 1);
        Self {
            latest_per_second_snapshots: Mutex::new(VecDeque::with_capacity(window_seconds)),
            bytes_per_second: Default::default(),
            time_remaining_millis: Default::default(),
        }
    }

    pub fn time_remaining(&self) -> Option<Duration> {
        let tr = self.time_remaining_millis.load(Ordering::Relaxed);
        if tr == 0 {
            return None;
        }
        Some(Duration::from_millis(tr))
    }

    pub fn bps(&self) -> u64 {
        self.bytes_per_second.load(Ordering::Relaxed)
    }

    pub fn mbps(&self) -> f64 {
        self.bps() as f64 / 1024f64 / 1024f64
    }

    pub fn add_snapshot(
        &self,
        progress_bytes: u64,
        remaining_bytes: Option<u64>,
        instant: Instant,
    ) {
        let first = {
            let mut g = self.latest_per_second_snapshots.lock();

            let current = ProgressSnapshot {
                progress_bytes,
                instant,
            };

            if g.is_empty() {
                g.push_back(current);
                return;
            } else if g.len() < g.capacity() {
                g.push_back(current);
                g.front().copied().unwrap()
            } else {
                let first = g.pop_front().unwrap();
                g.push_back(current);
                first
            }
        };

        let downloaded_bytes_diff = progress_bytes - first.progress_bytes;
        let elapsed = instant - first.instant;
        let bps = downloaded_bytes_diff as f64 / elapsed.as_secs_f64();

        let time_remaining_millis_rounded: u64 = if downloaded_bytes_diff > 0 {
            let time_remaining_secs = remaining_bytes.unwrap_or_default() as f64 / bps;
            (time_remaining_secs * 1000f64) as u64
        } else {
            0
        };
        self.time_remaining_millis
            .store(time_remaining_millis_rounded, Ordering::Relaxed);
        self.bytes_per_second.store(bps as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::SpeedEstimator;

    #[test]
    fn first_snapshot_has_no_rate_or_eta() {
        let estimator = SpeedEstimator::new(3);
        estimator.add_snapshot(100, Some(900), Instant::now());
        assert_eq!(estimator.bps(), 0);
        assert_eq!(estimator.time_remaining(), None);
    }

    #[test]
    fn computes_rate_mbps_and_remaining_time() {
        let estimator = SpeedEstimator::new(3);
        let start = Instant::now();
        estimator.add_snapshot(0, Some(2048), start);
        estimator.add_snapshot(1024, Some(2048), start + Duration::from_secs(1));

        assert_eq!(estimator.bps(), 1024);
        assert_eq!(estimator.mbps(), 1.0 / 1024.0);
        assert_eq!(estimator.time_remaining(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn sliding_window_discards_oldest_snapshot() {
        let estimator = SpeedEstimator::new(2);
        let start = Instant::now();
        estimator.add_snapshot(0, None, start);
        estimator.add_snapshot(10, None, start + Duration::from_secs(1));
        estimator.add_snapshot(40, None, start + Duration::from_secs(2));
        assert_eq!(estimator.bps(), 20);
        estimator.add_snapshot(100, None, start + Duration::from_secs(3));
        assert_eq!(estimator.bps(), 45);
    }

    #[test]
    fn stalled_progress_clears_rate_and_eta() {
        let estimator = SpeedEstimator::new(2);
        let start = Instant::now();
        estimator.add_snapshot(10, Some(100), start);
        estimator.add_snapshot(10, Some(100), start + Duration::from_secs(1));
        assert_eq!(estimator.bps(), 0);
        assert_eq!(estimator.time_remaining(), None);
    }

    #[test]
    #[should_panic]
    fn window_must_contain_at_least_two_snapshots() {
        SpeedEstimator::new(1);
    }
}
