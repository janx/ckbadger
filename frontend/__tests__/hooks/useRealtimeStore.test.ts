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

    it('reset clears connection state and latest block/tx', () => {
      const { setConnected, setLatestTx, reset } = useRealtimeStore.getState();
      setConnected(true);
      setLatestTx({
        hash: '0xdef456',
        blockNumber: 12345,
        inputsCount: 2,
        outputsCount: 3,
        fee: '1000',
        timestamp: '2024-01-01T00:00:00Z',
      });
      useRealtimeStore.setState({
        latestBlock: {
          number: 12345,
          hash: '0xabc',
          timestamp: '2024-01-01T00:00:00Z',
          transactionsCount: 1,
        } as never,
      });

      useRealtimeStore.setState({ socketHeld: true });

      reset();

      const state = useRealtimeStore.getState();
      expect(state.isConnected).toBe(false);
      expect(state.latestBlock).toBeNull();
      expect(state.latestTx).toBeNull();
      // Whether the page holds the socket is the subscriber count's business, not the data reset's.
      expect(state.socketHeld).toBe(true);
      useRealtimeStore.setState({ socketHeld: false });
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

    // Mirrors the cache merge in useRealtimeData: block-derived facts advance,
    // window-averaged network statistics stay untouched (they belong to
    // /statistics/network). The behavioural test drives the real hook in
    // useRealtimeData.test.tsx.
    it('merges only block-derived facts into network-stats', () => {
      const queryClient = new QueryClient();

      const oldStats = {
        latestBlock: 12340,
        epoch: '100(445/1800)',
        avgBlockTime: '9.62s',
        estimatedEpochTime: '3h 36m',
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
              }
            | undefined
        ) => {
          if (!old) return old;
          const epochString = `${blockData.epochNumber}(${blockData.epochIndex}/${blockData.epochLength})`;
          return {
            ...old,
            latestBlock: blockData.number,
            epoch: epochString,
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
      expect(updatedStats.avgBlockTime).toBe('9.62s');
      expect(updatedStats.estimatedEpochTime).toBe('3h 36m');
    });
  });
});
