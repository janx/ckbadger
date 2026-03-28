//! Streaming block buffer for the bulk-build pipeline.
//!
//! The prefetch side sends fixed-size chunks of [`BufferedBlock`] via an mpsc channel.
//! Each chunk is `Result<Vec<BufferedBlock>>` — errors from the prefetch worker are
//! propagated to the build loop.
//!
//! The build loop receives them through [`BlockBufferHandle`], which keeps a local
//! `VecDeque` so the build loop can peek ahead and drain by a bytes budget.

use std::collections::VecDeque;

use anyhow::Result;
use tokio::sync::mpsc::Receiver;

use super::binary_facts::RawCkbBlock;

// ---------------------------------------------------------------------------
// BufferedBlock
// ---------------------------------------------------------------------------

/// A prefetched CKB block paired with pre-computed size metrics.
///
/// `block_bytes` and `cell_count` are computed once at prefetch time so the
/// build loop can make budget decisions without re-serializing or re-counting.
pub(crate) struct BufferedBlock {
    pub(crate) raw: RawCkbBlock,
    pub(crate) block_bytes: usize,
    #[allow(dead_code)] // wired in by later cell-based drain task
    pub(crate) cell_count: u64,
}

// ---------------------------------------------------------------------------
// BlockBufferHandle
// ---------------------------------------------------------------------------

/// Async interface wrapping an mpsc receiver of block chunks.
///
/// The build loop calls [`fill_to_budget`] to pull enough blocks for the next
/// batch, then [`drain`] to take them for processing.  `fill_to_budget` blocks
/// until at least one chunk arrives, then non-blocking pulls more chunks until
/// the local buffer has enough bytes — not greedy, not stingy.
pub(crate) struct BlockBufferHandle {
    chunk_rx: Receiver<Result<Vec<BufferedBlock>>>,
    local: VecDeque<BufferedBlock>,
    local_bytes: usize,
}

impl BlockBufferHandle {
    pub(crate) fn new(chunk_rx: Receiver<Result<Vec<BufferedBlock>>>) -> Self {
        Self {
            chunk_rx,
            local: VecDeque::new(),
            local_bytes: 0,
        }
    }

    /// Absorb a chunk of blocks into the local buffer.
    fn absorb(&mut self, chunk: Vec<BufferedBlock>) {
        for block in chunk {
            self.local_bytes += block.block_bytes;
            self.local.push_back(block);
        }
    }

    /// Pull enough blocks so the local buffer has at least `target_bytes`.
    ///
    /// Blocks (async) until at least one chunk arrives when the local buffer
    /// is empty, then non-blocking pulls additional ready chunks until the
    /// bytes target is met.  This is the correct middle ground between
    /// "one chunk only" (starves build when chunks are small) and "greedily
    /// drain everything" (defeats channel backpressure).
    ///
    /// Returns `Ok(true)` when blocks are available, `Ok(false)` at end of
    /// stream (channel closed + local empty), or `Err` on prefetch error.
    pub(crate) async fn fill_to_budget(&mut self, target_bytes: u64) -> Result<bool> {
        // Ensure at least one chunk is available.
        if self.local.is_empty() {
            match self.chunk_rx.recv().await {
                Some(result) => self.absorb(result?),
                None => return Ok(false),
            }
        }
        // Non-blocking: pull more chunks until budget is met.
        while (self.local_bytes as u64) < target_bytes {
            match self.chunk_rx.try_recv() {
                Ok(result) => self.absorb(result?),
                Err(_) => break,
            }
        }
        Ok(true)
    }

    /// Average bytes-per-block across all blocks in the local buffer (O(1)).
    ///
    /// Uses the running `local_bytes` total — no iteration needed.
    /// Returns `0.0` if the buffer is empty.
    pub(crate) fn density(&self) -> f64 {
        if self.local.is_empty() {
            return 0.0;
        }
        self.local_bytes as f64 / self.local.len() as f64
    }

    /// Number of blocks currently in the local buffer.
    pub(crate) fn available(&self) -> usize {
        self.local.len()
    }

    /// Take up to `n` blocks from the front of the local buffer.
    pub(crate) fn drain(&mut self, n: usize) -> Vec<BufferedBlock> {
        let count = n.min(self.local.len());
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(block) = self.local.pop_front() {
                self.local_bytes -= block.block_bytes;
                result.push(block);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    fn make_dummy_raw_block() -> RawCkbBlock {
        use ckb_types::core::BlockBuilder;
        RawCkbBlock {
            block: BlockBuilder::default().build(),
            cycles: vec![],
        }
    }

    fn make_buffered_block(bytes: usize) -> BufferedBlock {
        BufferedBlock {
            raw: make_dummy_raw_block(),
            block_bytes: bytes,
            cell_count: 0,
        }
    }

    #[test]
    fn density_averages_correctly() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        handle.local.push_back(make_buffered_block(1000));
        handle.local.push_back(make_buffered_block(3000));
        handle.local_bytes = 4000;

        assert_eq!(
            handle.density(),
            2000.0,
            "average of 1000 and 3000 should be 2000"
        );
    }

    #[test]
    fn density_empty_buffer_returns_zero() {
        let (_tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        let handle = BlockBufferHandle::new(rx);
        assert_eq!(handle.density(), 0.0);
    }

    #[tokio::test]
    async fn fill_to_budget_pulls_enough_chunks() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);

        // Send two chunks: 3 blocks (6000 bytes) then 2 blocks (2000 bytes).
        let chunk1: Vec<BufferedBlock> = vec![
            make_buffered_block(1000),
            make_buffered_block(2000),
            make_buffered_block(3000),
        ];
        let chunk2: Vec<BufferedBlock> = vec![make_buffered_block(500), make_buffered_block(1500)];
        tx.send(Ok(chunk1)).await.unwrap();
        tx.send(Ok(chunk2)).await.unwrap();
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);

        // Budget 7000: chunk1 (6000) not enough, should also pull chunk2 (2000).
        let ok = handle.fill_to_budget(7000).await.unwrap();
        assert!(ok);
        assert_eq!(handle.available(), 5);
        assert_eq!(handle.local_bytes, 8000);

        // density over all 5 blocks: (1000+2000+3000+500+1500)/5 = 1600.
        let density = handle.density();
        assert_eq!(density, 1600.0);

        // drain 2 blocks.
        let drained = handle.drain(2);
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].block_bytes, 1000);
        assert_eq!(drained[1].block_bytes, 2000);
        assert_eq!(handle.available(), 3);
        assert_eq!(handle.local_bytes, 5000);

        // Drain remaining 3.
        let rest = handle.drain(10);
        assert_eq!(rest.len(), 3);
        assert_eq!(handle.available(), 0);
        assert_eq!(handle.local_bytes, 0);

        // Channel closed + local empty → end of stream.
        let eos = handle.fill_to_budget(1).await.unwrap();
        assert!(!eos, "fill_to_budget should return false at end of stream");
    }

    #[tokio::test]
    async fn fill_to_budget_stops_at_budget() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);

        // 3 chunks of 1000 bytes each.
        for _ in 0..3 {
            tx.send(Ok(vec![make_buffered_block(1000)])).await.unwrap();
        }
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);

        // Budget 1500: should pull 2 chunks (2000 >= 1500), NOT all 3.
        let ok = handle.fill_to_budget(1500).await.unwrap();
        assert!(ok);
        assert_eq!(handle.local_bytes, 2000);
        assert_eq!(handle.available(), 2);
    }

    #[tokio::test]
    async fn fill_to_budget_propagates_prefetch_error() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);

        tx.send(Err(anyhow::anyhow!("fetch failed"))).await.unwrap();
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        let result = handle.fill_to_budget(1000).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("fetch failed"));
    }

    #[tokio::test]
    async fn fill_to_budget_reuses_local_without_channel_recv() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);

        tx.send(Ok(vec![make_buffered_block(5000)])).await.unwrap();
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);

        // First fill: pulls from channel.
        handle.fill_to_budget(1000).await.unwrap();
        assert_eq!(handle.available(), 1);

        // Second fill: local already has 5000 >= 1000, no channel recv.
        let ok = handle.fill_to_budget(1000).await.unwrap();
        assert!(ok);
        assert_eq!(handle.available(), 1); // unchanged
    }
}
