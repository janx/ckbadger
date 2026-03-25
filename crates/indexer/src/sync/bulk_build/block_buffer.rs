//! Streaming block buffer for the bulk-build pipeline.
//!
//! The prefetch side sends fixed-size chunks of [`BufferedBlock`] via an mpsc channel.
//! The build loop receives them through [`BlockBufferHandle`], which keeps a local
//! `VecDeque` so the build loop can peek ahead and drain by a bytes budget.

use std::collections::VecDeque;

use tokio::sync::mpsc::Receiver;

use super::binary_facts::RawCkbBlock;

// ---------------------------------------------------------------------------
// BufferedBlock
// ---------------------------------------------------------------------------

/// A prefetched CKB block paired with its pre-computed serialized byte size.
///
/// `block_bytes` is computed once at prefetch time so the build loop can make
/// bytes-budget decisions without re-serializing.
pub(crate) struct BufferedBlock {
    pub(crate) raw: RawCkbBlock,
    pub(crate) block_bytes: usize,
}

// ---------------------------------------------------------------------------
// BlockBufferHandle
// ---------------------------------------------------------------------------

/// Async interface wrapping an mpsc receiver of block chunks.
///
/// The build loop calls [`ensure_blocks`] to wait for at least one block to be
/// available, [`try_fill`] to greedily drain any additional ready chunks, and
/// [`drain`] to take a batch for processing.
pub(crate) struct BlockBufferHandle {
    chunk_rx: Receiver<Vec<BufferedBlock>>,
    local: VecDeque<BufferedBlock>,
    local_bytes: usize,
}

impl BlockBufferHandle {
    pub(crate) fn new(chunk_rx: Receiver<Vec<BufferedBlock>>) -> Self {
        Self {
            chunk_rx,
            local: VecDeque::new(),
            local_bytes: 0,
        }
    }

    /// Wait until at least one block is available in the local buffer.
    ///
    /// Returns `true` if a block is available, `false` if the channel is closed
    /// and the local buffer is empty (end of stream).
    pub(crate) async fn ensure_blocks(&mut self) -> bool {
        if !self.local.is_empty() {
            return true;
        }
        match self.chunk_rx.recv().await {
            Some(chunk) => {
                for block in chunk {
                    self.local_bytes += block.block_bytes;
                    self.local.push_back(block);
                }
                true
            }
            None => false,
        }
    }

    /// Non-blocking: drain all immediately-available chunks from the channel
    /// into the local buffer.
    pub(crate) fn try_fill(&mut self) {
        loop {
            match self.chunk_rx.try_recv() {
                Ok(chunk) => {
                    for block in chunk {
                        self.local_bytes += block.block_bytes;
                        self.local.push_back(block);
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// Average bytes-per-block across the first `n` blocks in the local buffer.
    ///
    /// Returns `0.0` if `n == 0` or the buffer is empty.
    pub(crate) fn peek_density(&self, n: usize) -> f64 {
        if n == 0 || self.local.is_empty() {
            return 0.0;
        }
        let count = n.min(self.local.len());
        let total_bytes: usize = self.local.iter().take(count).map(|b| b.block_bytes).sum();
        total_bytes as f64 / count as f64
    }

    /// Number of blocks currently in the local buffer.
    pub(crate) fn available(&self) -> usize {
        self.local.len()
    }

    /// Total bytes represented by blocks in the local buffer.
    pub(crate) fn local_bytes(&self) -> usize {
        self.local_bytes
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
        }
    }

    #[test]
    fn peek_bytes_per_block_averages_correctly() {
        let (tx, rx) = mpsc::channel(8);
        drop(tx); // channel closed immediately; we only populate local manually

        let mut handle = BlockBufferHandle::new(rx);
        handle.local.push_back(make_buffered_block(1000));
        handle.local.push_back(make_buffered_block(3000));
        handle.local_bytes = 4000;

        let avg = handle.peek_density(2);
        assert_eq!(avg, 2000.0, "average of 1000 and 3000 should be 2000");
    }

    #[tokio::test]
    async fn buffer_handle_ensure_and_drain() {
        let (tx, rx) = mpsc::channel(8);

        // Send two chunks: 3 blocks then 2 blocks.
        let chunk1: Vec<BufferedBlock> = vec![
            make_buffered_block(1000),
            make_buffered_block(2000),
            make_buffered_block(3000),
        ];
        let chunk2: Vec<BufferedBlock> = vec![
            make_buffered_block(500),
            make_buffered_block(1500),
        ];
        tx.send(chunk1).await.unwrap();
        tx.send(chunk2).await.unwrap();
        drop(tx);

        let mut handle = BlockBufferHandle::new(rx);

        // Starts empty.
        assert_eq!(handle.available(), 0);
        assert_eq!(handle.local_bytes(), 0);

        // ensure_blocks waits for the first chunk.
        let ok = handle.ensure_blocks().await;
        assert!(ok, "ensure_blocks should return true when data available");
        assert_eq!(handle.available(), 3);
        assert_eq!(handle.local_bytes(), 6000);

        // try_fill should pull the second chunk.
        handle.try_fill();
        assert_eq!(handle.available(), 5);
        assert_eq!(handle.local_bytes(), 8000);

        // peek_density over first 3 blocks: (1000+2000+3000)/3 = 2000.
        let density = handle.peek_density(3);
        assert_eq!(density, 2000.0);

        // drain 2 blocks.
        let drained = handle.drain(2);
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].block_bytes, 1000);
        assert_eq!(drained[1].block_bytes, 2000);
        assert_eq!(handle.available(), 3);
        assert_eq!(handle.local_bytes(), 5000);

        // Drain remaining 3.
        let rest = handle.drain(10); // request more than available
        assert_eq!(rest.len(), 3);
        assert_eq!(handle.available(), 0);
        assert_eq!(handle.local_bytes(), 0);

        // Channel is closed and local is empty: end of stream.
        let eos = handle.ensure_blocks().await;
        assert!(!eos, "ensure_blocks should return false at end of stream");
    }
}
