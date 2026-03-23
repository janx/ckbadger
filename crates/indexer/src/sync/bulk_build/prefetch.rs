use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::binary_facts::RawCkbBlock;
use super::BULK_BUILD_MIN_BLOCK_SPAN;
use crate::sync::indexer::Indexer;
use ckb_store_reader::CkbChainReader;

/// How long the worker sleeps when gated by the ahead limit before
/// re-checking.
const AHEAD_GATE_POLL_MS: u64 = 20;

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
    pub ahead_gate_count: u64,
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
        max_depth: usize,
        ckb_store: Arc<CkbChainReader>,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
        initial_span: u64,
        ahead_rx: tokio::sync::watch::Receiver<u64>,
    ) -> Self {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(max_depth);
        let (span_tx, span_rx) = tokio::sync::watch::channel(initial_span);

        let worker_handle = tokio::task::spawn_blocking(move || {
            Self::prefetch_worker(
                result_tx,
                span_rx,
                ahead_rx,
                ckb_store,
                fetch_pool,
                start_block,
                handoff_target,
            )
        });

        Self {
            result_rx,
            depth: max_depth,
            span_tx,
            worker_handle,
        }
    }

    fn prefetch_worker(
        result_tx: tokio::sync::mpsc::Sender<Result<PrefetchResult>>,
        span_rx: tokio::sync::watch::Receiver<u64>,
        ahead_rx: tokio::sync::watch::Receiver<u64>,
        ckb_store: Arc<CkbChainReader>,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
    ) -> Result<PrefetchWorkerStats> {
        let mut stats = PrefetchWorkerStats {
            total_fetches: 0,
            total_blocks: 0,
            ahead_gate_count: 0,
            exit_reason: PrefetchExitReason::Completed,
        };
        let mut position = start_block;

        while position <= handoff_target {
            // Dynamic ahead gate: the bottleneck controller sets how many
            // batches to prefetch ahead.  When the channel has that many
            // pending, we wait — freeing CPU and I/O for build/flush.
            let ahead_limit = *ahead_rx.borrow();
            let pending = result_tx.max_capacity() - result_tx.capacity();
            if pending >= ahead_limit as usize {
                let mut gated = false;
                loop {
                    let current_pending = result_tx.max_capacity() - result_tx.capacity();
                    let current_limit = *ahead_rx.borrow();
                    if current_pending < current_limit as usize {
                        break;
                    }
                    if !gated {
                        stats.ahead_gate_count += 1;
                        gated = true;
                    }
                    std::thread::sleep(Duration::from_millis(AHEAD_GATE_POLL_MS));
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
                ahead_gate_count: 0,
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
                ahead_gate_count: 0,
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
                ahead_gate_count: 0,
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
                ahead_gate_count: 0,
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
                ahead_gate_count: 0,
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
    async fn prefetch_ahead_gate_blocks_when_at_limit() {
        let (_ahead_tx, ahead_rx) = tokio::sync::watch::channel(3u64); // limit=3

        let (result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<PrefetchResult>>(8);

        // Pre-fill channel to pending=3 so the gate should engage.
        for _ in 0..3 {
            result_tx
                .send(Ok(PrefetchResult {
                    blocks: vec![],
                    fetch_elapsed: Duration::from_millis(1),
                    effective_end: 0,
                }))
                .await
                .unwrap();
        }

        let tx_for_check = result_tx.clone();
        let ahead_for_check = ahead_rx.clone();
        let gate_task = tokio::task::spawn_blocking(move || {
            let pending = tx_for_check.max_capacity() - tx_for_check.capacity();
            assert_eq!(pending, 3, "channel should have 3 pending");
            let limit = *ahead_for_check.borrow();
            // pending (3) >= limit (3) → should gate
            pending >= limit as usize
        });
        assert!(gate_task.await.unwrap());

        drop(result_rx);
    }

    #[tokio::test]
    async fn prefetch_ahead_gate_open_when_below_limit() {
        let (_ahead_tx, ahead_rx) = tokio::sync::watch::channel(4u64); // limit=4

        let (result_tx, _result_rx) = tokio::sync::mpsc::channel::<Result<PrefetchResult>>(8);

        // Channel is empty (pending = 0) — should NOT gate
        let tx_for_check = result_tx.clone();
        let ahead_for_check = ahead_rx.clone();
        let gate_task = tokio::task::spawn_blocking(move || {
            let pending = tx_for_check.max_capacity() - tx_for_check.capacity();
            assert_eq!(pending, 0, "channel should be empty");
            let limit = *ahead_for_check.borrow();
            pending < limit as usize // should be open
        });
        assert!(gate_task.await.unwrap());
    }
}
