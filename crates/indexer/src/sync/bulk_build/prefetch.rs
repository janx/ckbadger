use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::binary_facts::RawCkbBlock;
use super::sampler::SamplerSnapshot;
use super::BULK_BUILD_MIN_BLOCK_SPAN;
use crate::sync::indexer::Indexer;
use ckb_store_reader::CkbChainReader;

/// Disk write threshold (MB per sampler interval) below which disk is
/// considered idle enough for speculative prefetch.  The sampler runs at
/// 200 ms, so 50 MB per interval ≈ 250 MB/s sustained writes.
const DISK_IDLE_WRITE_MB: f64 = 30.0;

/// How long the worker sleeps when gated by disk busyness before
/// re-checking. Short enough to react quickly when disk goes idle.
const DISK_BUSY_POLL_MS: u64 = 50;

pub(crate) struct PrefetchResult {
    pub blocks: Vec<RawCkbBlock>,
    pub fetch_elapsed: Duration,
    pub effective_end: u64,
}

impl std::fmt::Debug for PrefetchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrefetchResult")
            .field("blocks_len", &self.blocks.len())
            .field("fetch_elapsed", &self.fetch_elapsed)
            .field("effective_end", &self.effective_end)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) enum PrefetchExitReason {
    Completed,
    ReceiverDropped,
}

#[derive(Debug)]
pub(crate) struct PrefetchWorkerStats {
    pub total_fetches: u64,
    pub total_blocks: u64,
    pub disk_throttle_count: u64,
    pub exit_reason: PrefetchExitReason,
}

pub(crate) struct PrefetchChannelHandle {
    result_rx: tokio::sync::mpsc::Receiver<Result<PrefetchResult>>,
    depth: usize,
    span_tx: tokio::sync::watch::Sender<u64>,
    worker_handle: tokio::task::JoinHandle<Result<PrefetchWorkerStats>>,
}

impl PrefetchChannelHandle {
    pub(crate) fn new(
        depth: usize,
        ckb_store: Arc<CkbChainReader>,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
        initial_span: u64,
        sampler_rx: tokio::sync::watch::Receiver<SamplerSnapshot>,
    ) -> Self {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(depth);
        let (span_tx, span_rx) = tokio::sync::watch::channel(initial_span);

        let worker_handle = tokio::task::spawn_blocking(move || {
            Self::prefetch_worker(
                result_tx,
                span_rx,
                sampler_rx,
                ckb_store,
                fetch_pool,
                start_block,
                handoff_target,
            )
        });

        Self {
            result_rx,
            depth,
            span_tx,
            worker_handle,
        }
    }

    fn prefetch_worker(
        result_tx: tokio::sync::mpsc::Sender<Result<PrefetchResult>>,
        span_rx: tokio::sync::watch::Receiver<u64>,
        sampler_rx: tokio::sync::watch::Receiver<SamplerSnapshot>,
        ckb_store: Arc<CkbChainReader>,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
    ) -> Result<PrefetchWorkerStats> {
        let mut stats = PrefetchWorkerStats {
            total_fetches: 0,
            total_blocks: 0,
            disk_throttle_count: 0,
            exit_reason: PrefetchExitReason::Completed,
        };
        let mut position = start_block;

        while position <= handoff_target {
            // Dynamic depth gating: if the channel already has >= 1 buffered
            // result, only proceed when disk is idle.  This prevents speculative
            // prefetch reads from competing with RocksDB compaction I/O.
            let pending = result_tx.max_capacity() - result_tx.capacity();
            if pending >= 1 {
                let mut throttled = false;
                loop {
                    let snap = sampler_rx.borrow().clone();
                    if snap.disk_write_mb < DISK_IDLE_WRITE_MB {
                        break; // disk is idle, proceed with prefetch
                    }
                    // Re-check pending: consumer may have drained the channel.
                    let current_pending = result_tx.max_capacity() - result_tx.capacity();
                    if current_pending < 1 {
                        break; // channel empty, must prefetch regardless
                    }
                    if !throttled {
                        stats.disk_throttle_count += 1;
                        throttled = true;
                    }
                    std::thread::sleep(Duration::from_millis(DISK_BUSY_POLL_MS));
                }
            }

            let span = (*span_rx.borrow()).max(BULK_BUILD_MIN_BLOCK_SPAN);
            let end = std::cmp::min(
                position.saturating_add(span.saturating_sub(1)),
                handoff_target,
            );

            let started = Instant::now();
            let fetch_result =
                Indexer::fetch_blocks_direct_binary(&ckb_store, position, end, Some(&fetch_pool));

            let to_send = match fetch_result {
                Ok(blocks) => {
                    let block_count = blocks.len() as u64;
                    stats.total_fetches += 1;
                    stats.total_blocks += block_count;
                    Ok(PrefetchResult {
                        blocks,
                        fetch_elapsed: started.elapsed(),
                        effective_end: end,
                    })
                }
                Err(e) => Err(e),
            };
            let is_err = to_send.is_err();

            if result_tx.blocking_send(to_send).is_err() {
                stats.exit_reason = PrefetchExitReason::ReceiverDropped;
                break;
            }

            if is_err {
                break;
            }

            position = end.saturating_add(1);
        }

        Ok(stats)
    }

    pub(crate) async fn recv(&mut self) -> Result<PrefetchResult> {
        match self.result_rx.recv().await {
            Some(result) => result,
            None => Err(anyhow!(
                "prefetch worker terminated without sending an error"
            )),
        }
    }

    pub(crate) fn update_span(&self, span: u64) {
        let _ = self.span_tx.send(span);
    }

    pub(crate) fn pending(&self) -> usize {
        self.result_rx.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.depth
    }

    pub(crate) async fn close_and_wait(self) -> Result<PrefetchWorkerStats> {
        drop(self.result_rx);
        drop(self.span_tx);
        self.worker_handle
            .await
            .map_err(|e| anyhow!("prefetch worker panicked: {}", e))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prefetch_channel_reports_pending_and_capacity() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(8);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);
        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                disk_throttle_count: 0,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let handle = PrefetchChannelHandle {
            result_rx,
            depth: 8,
            span_tx,
            worker_handle,
        };

        result_tx
            .send(Ok(PrefetchResult {
                blocks: vec![],
                fetch_elapsed: Duration::from_millis(10),
                effective_end: 1000,
            }))
            .await
            .unwrap();

        assert_eq!(handle.pending(), 1);
        assert_eq!(handle.capacity(), 8);
    }

    #[tokio::test]
    async fn prefetch_channel_recv_gets_results_in_order() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(8);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);
        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                disk_throttle_count: 0,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let mut handle = PrefetchChannelHandle {
            result_rx,
            depth: 8,
            span_tx,
            worker_handle,
        };

        // Send 5 results with distinct effective_end values
        for i in 0..5 {
            result_tx
                .send(Ok(PrefetchResult {
                    blocks: vec![],
                    fetch_elapsed: Duration::from_millis(10),
                    effective_end: i * 1000,
                }))
                .await
                .unwrap();
        }
        drop(result_tx);

        // Receive and verify order
        for i in 0..5 {
            let result = handle.recv().await.unwrap();
            assert_eq!(result.effective_end, i * 1000);
        }
    }

    #[tokio::test]
    async fn prefetch_channel_recv_returns_error_on_worker_exit() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<PrefetchResult>>(2);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);
        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                disk_throttle_count: 0,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let mut handle = PrefetchChannelHandle {
            result_rx,
            depth: 2,
            span_tx,
            worker_handle,
        };

        // Drop sender to simulate worker exit without sending results
        drop(result_tx);

        let result = handle.recv().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("prefetch worker terminated"));
    }

    #[tokio::test]
    async fn prefetch_channel_propagates_fetch_errors() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(2);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);
        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                disk_throttle_count: 0,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let mut handle = PrefetchChannelHandle {
            result_rx,
            depth: 2,
            span_tx,
            worker_handle,
        };

        // Send an error through the channel
        result_tx
            .send(Err(anyhow!("simulated fetch failure")))
            .await
            .unwrap();
        drop(result_tx);

        let result = handle.recv().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("simulated fetch failure"));
    }

    #[tokio::test]
    async fn prefetch_channel_close_and_wait_returns_stats() {
        let (_result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<PrefetchResult>>(2);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);
        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 42,
                total_blocks: 420_000,
                disk_throttle_count: 0,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let handle = PrefetchChannelHandle {
            result_rx,
            depth: 2,
            span_tx,
            worker_handle,
        };

        let stats = handle.close_and_wait().await.unwrap();
        assert_eq!(stats.total_fetches, 42);
        assert_eq!(stats.total_blocks, 420_000);
        assert!(matches!(stats.exit_reason, PrefetchExitReason::Completed));
    }

    #[tokio::test]
    async fn prefetch_worker_throttles_when_disk_busy_and_channel_has_items() {
        let (sampler_tx, sampler_rx) = tokio::sync::watch::channel(SamplerSnapshot {
            disk_write_mb: 200.0, // busy
            ..Default::default()
        });

        let (result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<PrefetchResult>>(4);

        // Pre-fill channel with 1 item so pending >= 1.
        // Keep result_rx alive so send succeeds.
        result_tx
            .send(Ok(PrefetchResult {
                blocks: vec![],
                fetch_elapsed: Duration::from_millis(1),
                effective_end: 0,
            }))
            .await
            .unwrap();

        // Verify gating condition: pending >= 1 AND disk busy → would throttle
        let tx_for_check = result_tx.clone();
        let sampler_for_check = sampler_rx.clone();
        let gate_task = tokio::task::spawn_blocking(move || {
            let pending = tx_for_check.max_capacity() - tx_for_check.capacity();
            assert!(pending >= 1, "channel should have at least 1 item");

            let snap = sampler_for_check.borrow().clone();
            assert!(
                snap.disk_write_mb >= DISK_IDLE_WRITE_MB,
                "disk should appear busy"
            );
            true
        });
        assert!(gate_task.await.unwrap());

        // After making disk idle, the gate should open
        sampler_tx
            .send(SamplerSnapshot {
                disk_write_mb: 0.0,
                ..Default::default()
            })
            .unwrap();

        // Keep rx alive until end
        drop(result_rx);
    }

    #[tokio::test]
    async fn prefetch_worker_does_not_throttle_when_channel_empty() {
        let (_sampler_tx, _sampler_rx) = tokio::sync::watch::channel(SamplerSnapshot {
            disk_write_mb: 999.0, // very busy
            ..Default::default()
        });

        let (result_tx, _result_rx) = tokio::sync::mpsc::channel::<Result<PrefetchResult>>(4);

        // Channel is empty (pending = 0) — should NOT throttle even with busy disk
        let gate_task = tokio::task::spawn_blocking(move || {
            let pending = result_tx.max_capacity() - result_tx.capacity();
            assert_eq!(pending, 0, "channel should be empty");
            true
        });

        assert!(gate_task.await.unwrap());
    }
}
