'use client';

import { create } from 'zustand';
import type { RecentBlockItem } from '@/lib/api';

const DAY_MS = 24 * 60 * 60 * 1000;

interface RecentBlocksState {
  blocks: RecentBlockItem[];
  initialized: boolean;
  setBlocks: (blocks: RecentBlockItem[]) => void;
  addBlock: (block: RecentBlockItem) => void;
  pruneOldBlocks: (referenceTime: number) => void;
  reset: () => void;
}

export const useRecentBlocksStore = create<RecentBlocksState>((set, get) => ({
  blocks: [],
  initialized: false,

  setBlocks: (blocks) => set({ blocks, initialized: true }),

  // Drops the cached 24h series AND the `initialized` latch, so the next network
  // re-fetches instead of inheriting the previous one's blocks.
  reset: () => set({ blocks: [], initialized: false }),

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
