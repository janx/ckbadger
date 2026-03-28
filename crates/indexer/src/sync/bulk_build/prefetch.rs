use anyhow::{anyhow, Result};
use std::sync::Arc;

use super::binary_facts::RawCkbBlock;
use super::block_buffer::BufferedBlock;
use crate::sync::indexer::Indexer;
use ckb_store_reader::CkbChainReader;

/// Target bytes per prefetch chunk.  The worker adapts block count each
/// iteration so that chunk size stays roughly constant regardless of
/// on-chain density.  50 MB keeps fetch time stable (~200-500 ms).
const CHUNK_BYTES_TARGET: u64 = 50_000_000; // 50 MB

/// Block count bounds for a single prefetch fetch.
const MIN_CHUNK_BLOCKS: u64 = 500;
const MAX_CHUNK_BLOCKS: u64 = 500_000;

/// Initial density estimate (bytes/block) before any real data.
const INITIAL_DENSITY_EMA: f64 = 100.0;

const DENSITY_EMA_ALPHA: f64 = 0.5;

/// Return the molecule-serialized byte size of a raw CKB block.
///
/// Computed once at prefetch time so the build loop can make bytes-budget
/// decisions without re-serializing.
pub(crate) fn block_bytes_for_raw(raw: &RawCkbBlock) -> usize {
    raw.block.data().total_size()
}

/// Count output cells in a raw CKB block using molecule header access.
///
/// Each `tx.outputs().len()` is O(1) — reads the molecule vector header
/// (item count in first 4 bytes), no deserialization of actual outputs.
/// Computed once at prefetch time so the build loop can make cell-budget
/// decisions without parsing.
pub(crate) fn cell_count_for_raw(raw: &RawCkbBlock) -> u64 {
    raw.block
        .transactions()
        .iter()
        .map(|tx| tx.outputs().len() as u64)
        .sum()
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
    pub exit_reason: PrefetchExitReason,
}

pub(crate) struct PrefetchChannelHandle {
    result_rx: Option<tokio::sync::mpsc::Receiver<Result<Vec<BufferedBlock>>>>,
    worker_handle: tokio::task::JoinHandle<Result<PrefetchWorkerStats>>,
}

impl PrefetchChannelHandle {
    pub(crate) fn new(
        channel_depth: usize,
        ckb_store: Arc<CkbChainReader>,
        start_block: u64,
        handoff_target: u64,
        threads_rx: tokio::sync::watch::Receiver<u32>,
    ) -> Self {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(channel_depth);

        let worker_handle = tokio::task::spawn_blocking(move || {
            Self::prefetch_worker(
                result_tx,
                threads_rx,
                ckb_store,
                start_block,
                handoff_target,
            )
        });

        Self {
            result_rx: Some(result_rx),
            worker_handle,
        }
    }

    fn prefetch_worker(
        result_tx: tokio::sync::mpsc::Sender<Result<Vec<BufferedBlock>>>,
        threads_rx: tokio::sync::watch::Receiver<u32>,
        ckb_store: Arc<CkbChainReader>,
        start_block: u64,
        handoff_target: u64,
    ) -> Result<PrefetchWorkerStats> {
        let mut stats = PrefetchWorkerStats {
            total_fetches: 0,
            total_blocks: 0,
            exit_reason: PrefetchExitReason::Completed,
        };
        let mut position = start_block;
        let mut density_ema: f64 = INITIAL_DENSITY_EMA;

        while position <= handoff_target {
            // Backpressure is provided by the bounded channel — when the
            // channel is full, `blocking_send` below naturally blocks until
            // the build loop consumes a chunk.

            // Adapt chunk block count from bytes target and density EMA.
            let chunk_blocks = (CHUNK_BYTES_TARGET as f64 / density_ema) as u64;
            let chunk_blocks = chunk_blocks.clamp(MIN_CHUNK_BLOCKS, MAX_CHUNK_BLOCKS);

            let end = std::cmp::min(
                position.saturating_add(chunk_blocks.saturating_sub(1)),
                handoff_target,
            );

            let fetch_threads = *threads_rx.borrow();
            let fetch_result =
                Indexer::fetch_blocks_direct_binary(&ckb_store, position, end, fetch_threads);

            let to_send = match fetch_result {
                Ok(blocks) => {
                    let block_count = blocks.len() as u64;
                    stats.total_fetches += 1;
                    stats.total_blocks += block_count;
                    let buffered: Vec<BufferedBlock> = blocks
                        .into_iter()
                        .map(|raw| {
                            let block_bytes = block_bytes_for_raw(&raw);
                            let cell_count = cell_count_for_raw(&raw);
                            BufferedBlock {
                                raw,
                                block_bytes,
                                cell_count,
                            }
                        })
                        .collect();
                    // Update density EMA from actual chunk data.
                    if !buffered.is_empty() {
                        let chunk_bytes: usize = buffered.iter().map(|b| b.block_bytes).sum();
                        let actual_density = chunk_bytes as f64 / buffered.len() as f64;
                        density_ema = density_ema * (1.0 - DENSITY_EMA_ALPHA)
                            + actual_density * DENSITY_EMA_ALPHA;
                    }
                    Ok(buffered)
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

    /// Take ownership of the receiver for use with [`BlockBufferHandle`].
    ///
    /// May only be called once; panics if called again after the receiver has
    /// already been taken.
    pub(crate) fn take_receiver(
        &mut self,
    ) -> tokio::sync::mpsc::Receiver<Result<Vec<BufferedBlock>>> {
        self.result_rx
            .take()
            .expect("take_receiver called more than once")
    }

    pub(crate) async fn close_and_wait(mut self) -> Result<PrefetchWorkerStats> {
        drop(self.result_rx.take());
        self.worker_handle
            .await
            .map_err(|e| anyhow!("prefetch worker panicked: {}", e))?
    }
}

#[cfg(test)]
mod tests {
    use ckb_types::core::BlockBuilder;

    use super::*;

    fn make_dummy_raw_block() -> RawCkbBlock {
        RawCkbBlock {
            block: BlockBuilder::default().build(),
            cycles: vec![],
        }
    }

    #[test]
    fn block_bytes_for_raw_reads_molecule_size() {
        let raw = make_dummy_raw_block();
        let bytes = block_bytes_for_raw(&raw);
        assert!(
            bytes > 0,
            "molecule total_size should be non-zero for a default block, got {bytes}"
        );
    }

    #[tokio::test]
    async fn prefetch_channel_close_and_wait_returns_stats() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<Vec<BufferedBlock>>>(2);
        drop(result_tx); // simulate worker done

        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 42,
                total_blocks: 420_000,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let handle = PrefetchChannelHandle {
            result_rx: Some(result_rx),
            worker_handle,
        };

        let stats = handle.close_and_wait().await.unwrap();
        assert_eq!(stats.total_fetches, 42);
        assert_eq!(stats.total_blocks, 420_000);
        assert!(matches!(stats.exit_reason, PrefetchExitReason::Completed));
    }

    #[tokio::test]
    async fn prefetch_channel_take_receiver_returns_rx() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<Vec<BufferedBlock>>>(4);

        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let mut handle = PrefetchChannelHandle {
            result_rx: Some(result_rx),
            worker_handle,
        };

        // Send a chunk so we can verify the receiver is the right one
        let chunk = vec![BufferedBlock {
            raw: make_dummy_raw_block(),
            block_bytes: 100,
            cell_count: 0,
        }];
        result_tx.send(Ok(chunk)).await.unwrap();

        let mut rx = handle.take_receiver();
        let received = rx.recv().await.unwrap().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].block_bytes, 100);
    }

    #[test]
    fn cell_count_for_raw_counts_outputs() {
        use ckb_types::core::TransactionBuilder;
        use ckb_types::packed::CellOutputBuilder;
        use ckb_types::prelude::Builder;

        let output = CellOutputBuilder::default().build();
        let tx1 = TransactionBuilder::default()
            .outputs(vec![output.clone(); 3])
            .outputs_data(vec![ckb_types::packed::Bytes::default(); 3])
            .build();
        let tx2 = TransactionBuilder::default()
            .outputs(vec![output; 2])
            .outputs_data(vec![ckb_types::packed::Bytes::default(); 2])
            .build();
        let block = ckb_types::core::BlockBuilder::default()
            .transactions(vec![tx1, tx2])
            .build();
        let raw = RawCkbBlock {
            block,
            cycles: vec![],
        };
        assert_eq!(cell_count_for_raw(&raw), 5);
    }

    #[test]
    fn cell_count_for_raw_empty_block_is_zero() {
        let raw = make_dummy_raw_block();
        assert_eq!(cell_count_for_raw(&raw), 0);
    }

    #[tokio::test]
    #[should_panic(expected = "take_receiver called more than once")]
    async fn take_receiver_panics_on_second_call() {
        let (_result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<Vec<BufferedBlock>>>(2);

        let worker_handle = tokio::task::spawn_blocking(|| {
            Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                exit_reason: PrefetchExitReason::Completed,
            })
        });

        let mut handle = PrefetchChannelHandle {
            result_rx: Some(result_rx),
            worker_handle,
        };

        let _rx1 = handle.take_receiver();
        let _rx2 = handle.take_receiver(); // should panic
    }
}
