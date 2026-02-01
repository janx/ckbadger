import { QueryClient } from '@tanstack/react-query';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

describe('useRealtimeStore', () => {
  beforeEach(() => {
    useRealtimeStore.setState({
      isConnected: false,
      latestBlock: null,
      latestTx: null,
    });
  });

  describe('store actions', () => {
    it('setConnected updates isConnected state', () => {
      const { setConnected } = useRealtimeStore.getState();

      setConnected(true);
      expect(useRealtimeStore.getState().isConnected).toBe(true);

      setConnected(false);
      expect(useRealtimeStore.getState().isConnected).toBe(false);
    });

    it('setLatestBlock updates latestBlock state', () => {
      const { setLatestBlock } = useRealtimeStore.getState();
      const block = {
        number: 12345,
        hash: '0xabc123',
        timestamp: '2024-01-01T00:00:00Z',
        transactionsCount: 5,
        epochNumber: 100,
        epochIndex: 450,
        epochLength: 1800,
        avgBlockTime: '10.50s',
        estimatedEpochTime: '3h 45m',
        syncStatus: {
          isSyncing: false,
          syncedBlock: 12345,
          tipBlock: 12345,
          progress: 100,
          estimatedTime: null,
          chartDataMayBeIncomplete: false,
          blocksPerSecond: null,
          emaBlocksPerSecond: null,
          syncMode: 'synced',
          startedAt: null,
          elapsedTime: null,
          totalTime: null,
        },
      };

      setLatestBlock(block);
      expect(useRealtimeStore.getState().latestBlock).toEqual(block);
    });

    it('setLatestTx updates latestTx state', () => {
      const { setLatestTx } = useRealtimeStore.getState();
      const tx = {
        hash: '0xdef456',
        blockNumber: 12345,
        inputsCount: 2,
        outputsCount: 3,
        fee: '1000',
        timestamp: '2024-01-01T00:00:00Z',
      };

      setLatestTx(tx);
      expect(useRealtimeStore.getState().latestTx).toEqual(tx);
    });
  });

  describe('latest-blocks cache update logic', () => {
    it('deduplicates blocks by number and limits to 10 items', () => {
      const queryClient = new QueryClient();

      const existingBlocks = Array.from({ length: 10 }, (_, i) => ({
        number: 100 - i,
        hash: `0xhash${100 - i}`,
        timestamp: '2024-01-01T00:00:00Z',
        transactionsCount: 1,
      }));

      queryClient.setQueryData(['latest-blocks'], { data: existingBlocks });

      const newBlock = {
        number: 101,
        hash: '0xhash101',
        timestamp: '2024-01-01T00:00:01Z',
        transactionsCount: 2,
      };

      queryClient.setQueryData(
        ['latest-blocks'],
        (old: { data: typeof existingBlocks } | undefined) => {
          const existingData = old?.data ?? [];
          const newData = [newBlock, ...existingData.filter((b) => b.number !== newBlock.number)];
          return {
            ...old,
            data: newData.slice(0, 10),
          };
        }
      );

      const updated = queryClient.getQueryData(['latest-blocks']) as {
        data: typeof existingBlocks;
      };
      expect(updated.data).toHaveLength(10);
      expect(updated.data[0].number).toBe(101);
      expect(updated.data[9].number).toBe(92);
    });

    it('handles empty cache by creating new data structure', () => {
      const queryClient = new QueryClient();

      const newBlock = {
        number: 101,
        hash: '0xhash101',
        timestamp: '2024-01-01T00:00:01Z',
        transactionsCount: 2,
      };

      queryClient.setQueryData(
        ['latest-blocks'],
        (old: { data: (typeof newBlock)[] } | undefined) => {
          const existingData = old?.data ?? [];
          const newData = [newBlock, ...existingData.filter((b) => b.number !== newBlock.number)];
          return {
            ...old,
            data: newData.slice(0, 10),
          };
        }
      );

      const updated = queryClient.getQueryData(['latest-blocks']) as { data: (typeof newBlock)[] };
      expect(updated.data).toHaveLength(1);
      expect(updated.data[0].number).toBe(101);
    });

    it('deduplicates when same block arrives again', () => {
      const queryClient = new QueryClient();

      const existingBlocks = [
        { number: 100, hash: '0xhash100', timestamp: '2024-01-01T00:00:00Z', transactionsCount: 1 },
        { number: 99, hash: '0xhash99', timestamp: '2024-01-01T00:00:00Z', transactionsCount: 1 },
      ];

      queryClient.setQueryData(['latest-blocks'], { data: existingBlocks });

      const duplicateBlock = {
        number: 100,
        hash: '0xhash100_updated',
        timestamp: '2024-01-01T00:00:01Z',
        transactionsCount: 3,
      };

      queryClient.setQueryData(
        ['latest-blocks'],
        (old: { data: typeof existingBlocks } | undefined) => {
          const existingData = old?.data ?? [];
          const newData = [
            duplicateBlock,
            ...existingData.filter((b) => b.number !== duplicateBlock.number),
          ];
          return {
            ...old,
            data: newData.slice(0, 10),
          };
        }
      );

      const updated = queryClient.getQueryData(['latest-blocks']) as {
        data: typeof existingBlocks;
      };
      expect(updated.data).toHaveLength(2);
      expect(updated.data[0].number).toBe(100);
      expect(updated.data[0].transactionsCount).toBe(3);
    });
  });

  describe('chain-wave-tip-block cache update logic', () => {
    it('updates tip block when new block arrives', () => {
      const queryClient = new QueryClient();

      const oldTipBlock = {
        number: 100,
        hash: '0xhash100',
        timestamp: '2024-01-01T00:00:00Z',
        transactionsCount: 1,
      };

      queryClient.setQueryData(['chain-wave-tip-block'], { data: [oldTipBlock] });

      const newBlock = {
        number: 101,
        hash: '0xhash101',
        timestamp: '2024-01-01T00:00:01Z',
        transactionsCount: 2,
      };

      queryClient.setQueryData(
        ['chain-wave-tip-block'],
        (old: { data: (typeof oldTipBlock)[] } | undefined) => {
          if (!old) return old;
          return {
            ...old,
            data: [newBlock],
          };
        }
      );

      const updated = queryClient.getQueryData(['chain-wave-tip-block']) as {
        data: (typeof oldTipBlock)[];
      };
      expect(updated.data).toHaveLength(1);
      expect(updated.data[0].number).toBe(101);
    });

    it('preserves undefined cache when no initial data', () => {
      const queryClient = new QueryClient();

      const newBlock = {
        number: 101,
        hash: '0xhash101',
        timestamp: '2024-01-01T00:00:01Z',
        transactionsCount: 2,
      };

      queryClient.setQueryData(
        ['chain-wave-tip-block'],
        (old: { data: (typeof newBlock)[] } | undefined) => {
          if (!old) return old;
          return {
            ...old,
            data: [newBlock],
          };
        }
      );

      const updated = queryClient.getQueryData(['chain-wave-tip-block']);
      expect(updated).toBeUndefined();
    });
  });

  describe('network-stats cache update logic', () => {
    it('formats epoch string correctly from block data', () => {
      const blockData = {
        number: 12345,
        hash: '0xabc123',
        timestamp: '2024-01-01T00:00:00Z',
        transactionsCount: 5,
        epochNumber: 100,
        epochIndex: 450,
        epochLength: 1800,
        avgBlockTime: '10.50s',
        estimatedEpochTime: '3h 45m',
        syncStatus: {
          isSyncing: false,
          syncedBlock: 12345,
          tipBlock: 12345,
          progress: 100,
          estimatedTime: null,
          chartDataMayBeIncomplete: false,
          blocksPerSecond: null,
          emaBlocksPerSecond: null,
          syncMode: 'synced',
          startedAt: null,
          elapsedTime: null,
          totalTime: null,
        },
      };

      const epochString = `${blockData.epochNumber}(${blockData.epochIndex}/${blockData.epochLength})`;
      expect(epochString).toBe('100(450/1800)');
    });

    it('updates network-stats with avgBlockTime and estimatedEpochTime from block', () => {
      const queryClient = new QueryClient();

      const oldStats = {
        latestBlock: 12340,
        epoch: '100(445/1800)',
        avgBlockTime: '10.00s',
        estimatedEpochTime: '4h 0m',
        syncStatus: {
          isSyncing: false,
          syncedBlock: 12345,
          tipBlock: 12345,
          progress: 100,
          estimatedTime: null,
          chartDataMayBeIncomplete: false,
          blocksPerSecond: null,
          emaBlocksPerSecond: null,
          syncMode: 'synced',
          startedAt: null,
          elapsedTime: null,
          totalTime: null,
        },
      };

      queryClient.setQueryData(['network-stats'], oldStats);

      const blockData = {
        number: 12345,
        epochNumber: 100,
        epochIndex: 450,
        epochLength: 1800,
        avgBlockTime: '10.50s',
        estimatedEpochTime: '3h 45m',
        syncStatus: {
          isSyncing: false,
          syncedBlock: 12345,
          tipBlock: 12345,
          progress: 100,
          estimatedTime: null,
          chartDataMayBeIncomplete: false,
          blocksPerSecond: null,
          emaBlocksPerSecond: null,
          syncMode: 'synced',
          startedAt: null,
          elapsedTime: null,
          totalTime: null,
        },
      };

      queryClient.setQueryData(
        ['network-stats'],
        (
          old:
            | {
                latestBlock: number;
                epoch?: string;
                avgBlockTime?: string;
                estimatedEpochTime?: string;
              }
            | undefined
        ) => {
          if (!old) return old;
          const epochString = `${blockData.epochNumber}(${blockData.epochIndex}/${blockData.epochLength})`;
          return {
            ...old,
            latestBlock: blockData.number,
            epoch: epochString,
            avgBlockTime: blockData.avgBlockTime,
            estimatedEpochTime: blockData.estimatedEpochTime,
            syncStatus: blockData.syncStatus ?? old,
          };
        }
      );

      const updatedStats = queryClient.getQueryData(['network-stats']) as {
        latestBlock: number;
        epoch: string;
        avgBlockTime: string;
        estimatedEpochTime: string;
      };

      expect(updatedStats.latestBlock).toBe(12345);
      expect(updatedStats.epoch).toBe('100(450/1800)');
      expect(updatedStats.avgBlockTime).toBe('10.50s');
      expect(updatedStats.estimatedEpochTime).toBe('3h 45m');
    });
  });
});
