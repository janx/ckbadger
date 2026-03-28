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
    pub(crate) cell_count: u64,
}

// ---------------------------------------------------------------------------
// BlockBufferHandle
// ---------------------------------------------------------------------------

/// Async interface wrapping an mpsc receiver of block chunks.
///
/// The build loop calls [`fill_to_cell_budget`] to pull enough blocks for the
/// next batch, then [`drain_by_cells`] to take them for processing.
/// `fill_to_cell_budget` blocks until at least one chunk arrives (to establish
/// real cell density), then non-blocking pulls more chunks until the local
/// buffer has enough cells — not greedy, not stingy.
pub(crate) struct BlockBufferHandle {
    chunk_rx: Receiver<Result<Vec<BufferedBlock>>>,
    local: VecDeque<BufferedBlock>,
    local_bytes: usize,
    pub(crate) local_cells: u64,
}

impl BlockBufferHandle {
    pub(crate) fn new(chunk_rx: Receiver<Result<Vec<BufferedBlock>>>) -> Self {
        Self {
            chunk_rx,
            local: VecDeque::new(),
            local_bytes: 0,
            local_cells: 0,
        }
    }

    /// Absorb a chunk of blocks into the local buffer.
    fn absorb(&mut self, chunk: Vec<BufferedBlock>) {
        for block in chunk {
            self.local_bytes += block.block_bytes;
            self.local_cells += block.cell_count;
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
    #[cfg(test)] // Production callers use fill_to_cell_budget; kept for targeted tests.
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

    /// Pull enough blocks so the local buffer has approximately `target_cells`
    /// worth of data, estimated from actual buffer density.
    ///
    /// When the buffer is empty, blocks until the first chunk arrives to
    /// establish real density data, then computes the bytes target from
    /// `cells / cell_density`.  This avoids the fixed-fallback problem where
    /// an empty buffer would use an arbitrary byte estimate that under-fills
    /// relative to the cell target.
    ///
    /// Returns `Ok(true)` when blocks are available, `Ok(false)` at end of
    /// stream (channel closed + local empty), or `Err` on prefetch error.
    pub(crate) async fn fill_to_cell_budget(&mut self, target_cells: u64) -> Result<bool> {
        // Ensure at least one chunk so cell_density() has real data.
        if self.local.is_empty() {
            match self.chunk_rx.recv().await {
                Some(result) => self.absorb(result?),
                None => return Ok(false),
            }
        }
        // Compute bytes target from actual density in the buffer.
        let target_bytes = {
            let cpb = self.cell_density();
            if cpb > 0.0 {
                (target_cells as f64 / cpb) as u64
            } else {
                // All blocks have zero cells — density-based estimate impossible.
                // The buffer already has one chunk; return what we have.
                return Ok(true);
            }
        };
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
    #[allow(dead_code)] // Used by tests; kept as general-purpose density metric
    pub(crate) fn density(&self) -> f64 {
        if self.local.is_empty() {
            return 0.0;
        }
        self.local_bytes as f64 / self.local.len() as f64
    }

    /// Cells per byte across all blocks in the local buffer (O(1)).
    ///
    /// Returns `0.0` if the buffer is empty or has zero bytes.
    pub(crate) fn cell_density(&self) -> f64 {
        if self.local_bytes == 0 {
            return 0.0;
        }
        self.local_cells as f64 / self.local_bytes as f64
    }

    /// Number of blocks currently in the local buffer.
    #[allow(dead_code)] // Used by tests
    pub(crate) fn available(&self) -> usize {
        self.local.len()
    }

    /// Number of chunks currently pending in the underlying mpsc channel.
    ///
    /// This reflects how many prefetch chunks the worker has produced but
    /// have not yet been absorbed into the local buffer.
    pub(crate) fn channel_len(&self) -> usize {
        self.chunk_rx.len()
    }

    /// Take up to `n` blocks from the front of the local buffer.
    #[allow(dead_code)] // Used by tests; kept as general-purpose drain alternative
    pub(crate) fn drain(&mut self, n: usize) -> Vec<BufferedBlock> {
        let count = n.min(self.local.len());
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(block) = self.local.pop_front() {
                self.local_bytes -= block.block_bytes;
                self.local_cells -= block.cell_count;
                result.push(block);
            }
        }
        result
    }

    /// Drain blocks until the cumulative cell count reaches `target_cells`
    /// OR cumulative bytes reaches `max_bytes`, whichever comes first.
    ///
    /// Always drains at least one block (prevents starvation when a single
    /// block exceeds both budgets).  No block-count cap — `max_bytes`
    /// (RAM-derived) is the memory safety ceiling.
    pub(crate) fn drain_by_cells(
        &mut self,
        target_cells: u64,
        max_bytes: u64,
    ) -> Vec<BufferedBlock> {
        let mut result = Vec::with_capacity(256);
        let mut cum_cells: u64 = 0;
        let mut cum_bytes: u64 = 0;

        while let Some(block) = self.local.pop_front() {
            cum_cells += block.cell_count;
            cum_bytes += block.block_bytes as u64;
            self.local_bytes -= block.block_bytes;
            self.local_cells -= block.cell_count;
            result.push(block);

            // Stop when either budget is met (but always at least one block).
            if cum_cells >= target_cells || cum_bytes >= max_bytes {
                break;
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

    fn make_buffered_block_with_cells(bytes: usize, cells: u64) -> BufferedBlock {
        BufferedBlock {
            raw: make_dummy_raw_block(),
            block_bytes: bytes,
            cell_count: cells,
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

    #[test]
    fn cell_density_computes_cells_per_byte() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        // 2 blocks: 1000 bytes / 50 cells, 3000 bytes / 150 cells
        // total: 4000 bytes, 200 cells → 0.05 cells/byte
        let chunk = vec![
            make_buffered_block_with_cells(1000, 50),
            make_buffered_block_with_cells(3000, 150),
        ];
        handle.absorb(chunk);
        assert!(
            (handle.cell_density() - 0.05).abs() < 1e-10,
            "200 cells / 4000 bytes = 0.05, got {}",
            handle.cell_density()
        );
    }

    #[test]
    fn cell_density_empty_returns_zero() {
        let (_tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        let handle = BlockBufferHandle::new(rx);
        assert_eq!(handle.cell_density(), 0.0);
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

    #[test]
    fn drain_by_cells_stops_at_cell_budget() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        // 4 blocks: 100 cells each, 1000 bytes each.
        for _ in 0..4 {
            let block = make_buffered_block_with_cells(1000, 100);
            handle.local_cells += block.cell_count;
            handle.local_bytes += block.block_bytes;
            handle.local.push_back(block);
        }

        // Budget: 250 cells, max_bytes very large (not limiting).
        let drained = handle.drain_by_cells(250, u64::MAX);
        // Should drain 3 blocks (300 cells >= 250 target), not all 4.
        assert_eq!(drained.len(), 3);
        assert_eq!(handle.local_cells, 100);
        assert_eq!(handle.local_bytes, 1000);
        assert_eq!(handle.available(), 1);
    }

    #[test]
    fn drain_by_cells_stops_at_byte_cap() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        // 4 blocks: 10 cells each, 500_000 bytes each.
        for _ in 0..4 {
            let block = make_buffered_block_with_cells(500_000, 10);
            handle.local_cells += block.cell_count;
            handle.local_bytes += block.block_bytes;
            handle.local.push_back(block);
        }

        // Cell budget allows all 4 (40 >= 40), but byte cap limits to 2.
        let drained = handle.drain_by_cells(40, 1_000_000);
        assert_eq!(drained.len(), 2);
        assert_eq!(handle.local_cells, 20);
        assert_eq!(handle.local_bytes, 1_000_000);
    }

    #[test]
    fn drain_by_cells_always_drains_at_least_one() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        let block = make_buffered_block_with_cells(1_000_000, 999_999);
        handle.local_cells += block.cell_count;
        handle.local_bytes += block.block_bytes;
        handle.local.push_back(block);

        // Both budgets are 1 — but we always drain at least one block.
        let drained = handle.drain_by_cells(1, 1);
        assert_eq!(drained.len(), 1);
        assert_eq!(handle.local_cells, 0);
        assert_eq!(handle.local_bytes, 0);
    }

    #[test]
    fn local_cells_tracks_absorb_and_drain() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        assert_eq!(handle.local_cells, 0);

        let chunk = vec![
            make_buffered_block_with_cells(100, 50),
            make_buffered_block_with_cells(200, 75),
        ];
        handle.absorb(chunk);
        assert_eq!(handle.local_cells, 125);

        let drained = handle.drain(1);
        assert_eq!(drained.len(), 1);
        assert_eq!(handle.local_cells, 75);
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

    #[tokio::test]
    async fn fill_to_cell_budget_uses_density_from_first_chunk() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);

        // 4 chunks, each 1 block: 1000 bytes, 10 cells.
        // cell_density = 10/1000 = 0.01 cells/byte.
        for _ in 0..4 {
            tx.send(Ok(vec![make_buffered_block_with_cells(1000, 10)]))
                .await
                .unwrap();
        }
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);

        // target_cells = 25.  Density = 0.01 → need 2500 bytes → 3 chunks.
        let ok = handle.fill_to_cell_budget(25).await.unwrap();
        assert!(ok);
        assert_eq!(handle.available(), 3);
        assert_eq!(handle.local_cells, 30);
        assert_eq!(handle.local_bytes, 3000);
    }

    #[tokio::test]
    async fn fill_to_cell_budget_empty_then_drain_then_refill() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);

        // Simulate the problematic scenario: low cell density blocks.
        // 8 chunks, each 50 blocks: 2800 bytes/block, 5.8 cells/block.
        // cell_density = 5.8/2800 ≈ 0.00207 cells/byte.
        for _ in 0..8 {
            let chunk: Vec<BufferedBlock> = (0..50)
                .map(|i| make_buffered_block_with_cells(2800, if i % 10 < 6 { 6 } else { 5 }))
                .collect();
            tx.send(Ok(chunk)).await.unwrap();
        }
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);

        // First fill: target 620 cells.
        // Density from first chunk ≈ 0.00207 → need ~299K bytes → ~3 chunks.
        let ok = handle.fill_to_cell_budget(620).await.unwrap();
        assert!(ok);
        // Should have pulled enough chunks to cover 620 cells.
        assert!(
            handle.local_cells >= 620,
            "expected >= 620 cells, got {}",
            handle.local_cells
        );

        // Drain all (simulating batch that exhausts buffer).
        let count = handle.available();
        let _ = handle.drain(count);
        assert_eq!(handle.local_cells, 0);
        assert_eq!(handle.local_bytes, 0);

        // Second fill: buffer is empty again, but fill_to_cell_budget should
        // seed from the channel and compute correct target — NOT under-fill.
        let ok = handle.fill_to_cell_budget(620).await.unwrap();
        assert!(ok);
        assert!(
            handle.local_cells >= 620,
            "after re-fill expected >= 620 cells, got {}",
            handle.local_cells
        );
    }

    #[tokio::test]
    async fn fill_to_cell_budget_zero_cell_blocks_returns_ok() {
        let (tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);

        // Blocks with zero cells — density cannot be computed.
        tx.send(Ok(vec![make_buffered_block(5000)])).await.unwrap();
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);
        let ok = handle.fill_to_cell_budget(100).await.unwrap();
        assert!(ok);
        // Should have absorbed the one chunk and returned.
        assert_eq!(handle.available(), 1);
    }

    #[tokio::test]
    async fn fill_to_cell_budget_eos_returns_false() {
        let (_tx, rx) = mpsc::channel::<Result<Vec<BufferedBlock>>>(8);
        drop(_tx);

        let mut handle = BlockBufferHandle::new(rx);
        let eos = handle.fill_to_cell_budget(100).await.unwrap();
        assert!(!eos, "should return false at end of stream");
    }
}
