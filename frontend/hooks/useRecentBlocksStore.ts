'use client';

import { create } from 'zustand';
import { useEffect, useMemo } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, RecentBlockItem } from '@/lib/api';
import { useRealtimeStore } from './useRealtimeStore';

const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;

interface RecentBlocksState {
  blocks: RecentBlockItem[];
  initialized: boolean;
  setBlocks: (blocks: RecentBlockItem[]) => void;
  addBlock: (block: RecentBlockItem) => void;
  pruneOldBlocks: (referenceTime: number) => void;
}

export const useRecentBlocksStore = create<RecentBlocksState>((set, get) => ({
  blocks: [],
  initialized: false,

  setBlocks: (blocks) => set({ blocks, initialized: true }),

  addBlock: (block) => {
    const { blocks } = get();
    const exists = blocks.some((b) => b.timestamp === block.timestamp);
    if (!exists) {
      set({ blocks: [...blocks, block].sort((a, b) => a.timestamp - b.timestamp) });
    }
  },

  pruneOldBlocks: (referenceTime) => {
    const cutoff = referenceTime - DAY_MS;
    set((state) => ({
      blocks: state.blocks.filter((b) => b.timestamp > cutoff),
    }));
  },
}));

export function useRecentBlocks() {
  const queryClient = useQueryClient();
  const { blocks, initialized, setBlocks, addBlock, pruneOldBlocks } = useRecentBlocksStore();
  const latestBlock = useRealtimeStore((state) => state.latestBlock);

  const { data: initialData } = useQuery({
    queryKey: ['recent-blocks'],
    queryFn: () => api.getRecentBlocks(),
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    enabled: !initialized,
  });

  useEffect(() => {
    if (initialData && !initialized) {
      setBlocks(initialData.blocks);
    }
  }, [initialData, initialized, setBlocks]);

  useEffect(() => {
    if (latestBlock && initialized) {
      const timestamp = new Date(latestBlock.timestamp).getTime();
      addBlock({
        timestamp,
        transactionsCount: latestBlock.transactionsCount,
      });
      pruneOldBlocks(timestamp);
    }
  }, [latestBlock, initialized, addBlock, pruneOldBlocks]);

  const stats = useMemo(() => {
    if (!blocks.length) {
      return { txsLastHour: 0, txsLast24Hours: 0 };
    }

    const latestTimestamp = blocks[blocks.length - 1]?.timestamp ?? Date.now();
    const hourAgo = latestTimestamp - HOUR_MS;
    const dayAgo = latestTimestamp - DAY_MS;

    let txsLastHour = 0;
    let txsLast24Hours = 0;

    for (const block of blocks) {
      if (block.timestamp > dayAgo) {
        txsLast24Hours += block.transactionsCount;
        if (block.timestamp > hourAgo) {
          txsLastHour += block.transactionsCount;
        }
      }
    }

    return { txsLastHour, txsLast24Hours };
  }, [blocks]);

  return {
    blocks,
    initialized,
    ...stats,
  };
}
