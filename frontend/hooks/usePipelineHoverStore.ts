'use client';

import { create } from 'zustand';

/**
 * What a touch tap in the pipeline pinned: what that tap would open, and the second tap opens (a
 * transaction's page or a block's). A pointer that hovers opens at once and pins nothing.
 */
export type PipelinePin = { kind: 'tx'; hash: string } | { kind: 'block'; number: number };

interface PipelineHoverState {
  /** Block whose card / Latest Blocks row is highlighted. */
  hoverBlock: number | null;
  /** Tx whose square / Latest Transactions row is highlighted. */
  hoverTx: string | null;
  /** The tx or block a tap pinned: its readout stands under the canvas. */
  pinned: PipelinePin | null;
  /** Height H whose proposal window is drawn instead of the next block's. */
  windowBlock: number | null;
  setHover: (h: { block: number | null; tx: string | null; window: number | null }) => void;
  clearHover: () => void;
  pin: (p: PipelinePin) => void;
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
  pin: (p) => set({ pinned: p }),
  clearPin: () => set((s) => (s.pinned === null ? s : { pinned: null })),
  clear: () => set({ hoverBlock: null, hoverTx: null, pinned: null, windowBlock: null }),
}));
