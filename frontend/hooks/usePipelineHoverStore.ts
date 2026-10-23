'use client';

import { create } from 'zustand';

interface PipelineHoverState {
  /** Block whose card / Latest Blocks row is highlighted. */
  hoverBlock: number | null;
  /** Tx whose square / Latest Transactions row is highlighted. */
  hoverTx: string | null;
  /** Tx followed from the pool to the block that commits it. */
  pinned: string | null;
  /** Height H whose proposal window is drawn instead of the next block's. */
  windowBlock: number | null;
  setHover: (h: { block: number | null; tx: string | null; window: number | null }) => void;
  clearHover: () => void;
  togglePin: (hash: string) => void;
  clearPin: () => void;
  clear: () => void;
}

/** Hover / pin state shared by the pipeline panel and the Latest Blocks / Transactions lists. */
export const usePipelineHoverStore = create<PipelineHoverState>((set) => ({
  hoverBlock: null,
  hoverTx: null,
  pinned: null,
  windowBlock: null,
  // Pointer moves call these constantly: an unchanged hover returns the same state (no notify).
  setHover: ({ block, tx, window }) =>
    set((s) =>
      s.hoverBlock === block && s.hoverTx === tx && s.windowBlock === window
        ? s
        : { hoverBlock: block, hoverTx: tx, windowBlock: window }
    ),
  clearHover: () =>
    set((s) =>
      s.hoverBlock === null && s.hoverTx === null && s.windowBlock === null
        ? s
        : { hoverBlock: null, hoverTx: null, windowBlock: null }
    ),
  togglePin: (hash) => set((s) => ({ pinned: s.pinned === hash ? null : hash })),
  clearPin: () => set({ pinned: null }),
  clear: () => set({ hoverBlock: null, hoverTx: null, pinned: null, windowBlock: null }),
}));
