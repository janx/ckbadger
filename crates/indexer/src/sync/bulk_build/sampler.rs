use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ckbadger_store::CkbadgerStore;

use crate::sys_info;

#[derive(Clone, Default, Debug)]
pub(crate) struct SamplerSnapshot {
    pub compaction_pending_mb: u64,
    pub l0_files: u64,
    pub imm_memtables: u64,
    pub load_avg_1m: f64,
    pub mem_available_mb: u64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
}

pub(crate) struct BackgroundSampler {
    latest_rx: tokio::sync::watch::Receiver<SamplerSnapshot>,
    shutdown: Arc<AtomicBool>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
}

impl BackgroundSampler {
    pub(crate) fn new(store: Arc<CkbadgerStore>, interval: Duration, disk_device: String) -> Self {
        let (tx, rx) = tokio::sync::watch::channel(SamplerSnapshot::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);

        let handle = std::thread::Builder::new()
            .name("bg-sampler".into())
            .spawn(move || {
                let mut disk_tracker = sys_info::DiskStatsTracker::new(disk_device);
                while !shutdown_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    let stats = store.memory_stats();
                    let env = sys_info::read_batch_environment(&mut disk_tracker);
                    let snapshot = SamplerSnapshot {
                        compaction_pending_mb: stats.compaction_pending_bytes / (1024 * 1024),
                        l0_files: stats.l0_files_count,
                        imm_memtables: stats.immutable_memtables,
                        load_avg_1m: env.load_avg_1m,
                        mem_available_mb: env.mem_available_mb,
                        disk_read_mb: env.disk_read_mb,
                        disk_write_mb: env.disk_write_mb,
                    };
                    if tx.send(snapshot).is_err() {
                        break;
                    }
                }
            })
            .expect("failed to spawn background sampler thread");

        Self {
            latest_rx: rx,
            shutdown,
            worker_handle: Some(handle),
        }
    }

    pub(crate) fn latest(&self) -> SamplerSnapshot {
        self.latest_rx.borrow().clone()
    }

    /// Clone the watch receiver so another component (e.g. prefetch worker)
    /// can read the latest snapshot independently.
    #[allow(dead_code)]
    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<SamplerSnapshot> {
        self.latest_rx.clone()
    }

    pub(crate) fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for BackgroundSampler {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_snapshot_default_is_zeroed() {
        let snap = SamplerSnapshot::default();
        assert_eq!(snap.l0_files, 0);
        assert_eq!(snap.compaction_pending_mb, 0);
        assert_eq!(snap.load_avg_1m, 0.0);
        assert_eq!(snap.mem_available_mb, 0);
    }

    #[test]
    fn sampler_shutdown_joins_thread() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let (tx, rx) = tokio::sync::watch::channel(SamplerSnapshot::default());

        let handle = std::thread::Builder::new()
            .name("test-sampler".into())
            .spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                    if tx
                        .send(SamplerSnapshot {
                            l0_files: 42,
                            ..Default::default()
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .unwrap();

        let sampler = BackgroundSampler {
            latest_rx: rx,
            shutdown,
            worker_handle: Some(handle),
        };

        std::thread::sleep(Duration::from_millis(50));
        let snap = sampler.latest();
        assert_eq!(snap.l0_files, 42);

        sampler.shutdown();
    }

    #[test]
    fn sampler_drop_signals_shutdown() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let shutdown_check = Arc::clone(&shutdown);
        let (_tx, rx) = tokio::sync::watch::channel(SamplerSnapshot::default());

        let handle = std::thread::Builder::new()
            .name("test-sampler-drop".into())
            .spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
            .unwrap();

        let sampler = BackgroundSampler {
            latest_rx: rx,
            shutdown,
            worker_handle: Some(handle),
        };

        drop(sampler);
        assert!(shutdown_check.load(Ordering::Relaxed));
    }
}
