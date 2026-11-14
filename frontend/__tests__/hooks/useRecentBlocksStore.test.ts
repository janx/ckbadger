import { useRecentBlocksStore } from '@/hooks/useRecentBlocksStore';

const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;

describe('useRecentBlocksStore', () => {
  beforeEach(() => {
    useRecentBlocksStore.setState({
      blocks: [],
      initialized: false,
    });
  });

  describe('setBlocks', () => {
    it('sets blocks and marks as initialized', () => {
      const { setBlocks } = useRecentBlocksStore.getState();
      const blocks = [
        { timestamp: 1000, transactionsCount: 5 },
        { timestamp: 2000, transactionsCount: 10 },
      ];

      setBlocks(blocks);

      const state = useRecentBlocksStore.getState();
      expect(state.blocks).toEqual(blocks);
      expect(state.initialized).toBe(true);
    });
  });

  describe('addBlock', () => {
    it('adds new block and maintains sorted order', () => {
      const { setBlocks, addBlock } = useRecentBlocksStore.getState();
      setBlocks([
        { timestamp: 1000, transactionsCount: 5 },
        { timestamp: 3000, transactionsCount: 15 },
      ]);

      addBlock({ timestamp: 2000, transactionsCount: 10 });

      const state = useRecentBlocksStore.getState();
      expect(state.blocks).toHaveLength(3);
      expect(state.blocks[0].timestamp).toBe(1000);
      expect(state.blocks[1].timestamp).toBe(2000);
      expect(state.blocks[2].timestamp).toBe(3000);
    });

    it('does not add duplicate block with same timestamp', () => {
      const { setBlocks, addBlock } = useRecentBlocksStore.getState();
      setBlocks([{ timestamp: 1000, transactionsCount: 5 }]);

      addBlock({ timestamp: 1000, transactionsCount: 99 });

      const state = useRecentBlocksStore.getState();
      expect(state.blocks).toHaveLength(1);
      expect(state.blocks[0].transactionsCount).toBe(5);
    });
  });

  describe('reset', () => {
    it('clears the series and the initialized latch', () => {
      const { setBlocks, reset } = useRecentBlocksStore.getState();
      setBlocks([{ timestamp: 1000, transactionsCount: 5 }]);

      reset();

      const state = useRecentBlocksStore.getState();
      expect(state.blocks).toEqual([]);
      // Without clearing the latch the hook would never re-fetch the new
      // network's series.
      expect(state.initialized).toBe(false);
    });
  });

  describe('pruneOldBlocks', () => {
    it('removes blocks older than 24 hours from reference time', () => {
      const { setBlocks, pruneOldBlocks } = useRecentBlocksStore.getState();
      const now = Date.now();

      setBlocks([
        { timestamp: now - DAY_MS - HOUR_MS, transactionsCount: 1 },
        { timestamp: now - DAY_MS + HOUR_MS, transactionsCount: 2 },
        { timestamp: now - HOUR_MS, transactionsCount: 3 },
      ]);

      pruneOldBlocks(now);

      const state = useRecentBlocksStore.getState();
      expect(state.blocks).toHaveLength(2);
      expect(state.blocks[0].transactionsCount).toBe(2);
      expect(state.blocks[1].transactionsCount).toBe(3);
    });
  });
});
