import type { ForkOverview, OrphanedBlock, ReorgEvent } from '@/lib/api';
import { formatPercent } from '@/lib/ckb-decimal';

/**
 * D7: a fork event lives at `/forks/<fork_point_height>/<orphan_hash_prefix8>`, the first
 * 8 hex digits of the dropped block right after the fork point. Stable across store rebuilds.
 */
export function forkDetailHref(forkPointNumber: number, orphanHash: string): string {
  if (!Number.isSafeInteger(forkPointNumber) || forkPointNumber < 0)
    throw new Error(`Fork route: invalid fork point ${forkPointNumber}`);
  if (!/^0x[0-9a-f]{64}$/.test(orphanHash))
    throw new Error(`Fork route: orphan ${orphanHash} is not a 32-byte hash`);
  return `/forks/${forkPointNumber}/${orphanHash.slice(2, 10)}`;
}

/** The list row's link, from the API's first-orphan preview; null when it names no orphan. */
export function forkEventHref(event: ReorgEvent): string | null {
  return event.preview ? forkDetailHref(event.forkPointNumber, event.preview.oldBlockHash) : null;
}

export function parseForkRoute(
  height: string,
  orphan: string
): { height: number; orphan: string } | null {
  if (!/^(0|[1-9]\d*)$/.test(height) || !/^[0-9a-f]{8}$/.test(orphan)) return null;
  const value = Number(height);
  return Number.isSafeInteger(value) ? { height: value, orphan } : null;
}

const FATE_WORDS: Record<OrphanedBlock['fate'], string> = {
  included_as_uncle: 'uncle',
  canonical_now: 'back on the main chain',
  not_included_as_uncle: 'not taken as an uncle',
  uncle_fate_pending: 'uncle inclusion still possible',
  awaiting_canonical_tip: 'waiting for the indexed tip',
  canonical_evidence_unavailable: 'fate unavailable',
};

/** What became of a dropped block, in the board's words (Forks V67). */
export function orphanFateWords(fate: OrphanedBlock['fate']): string {
  const words = FATE_WORDS[fate];
  if (!words) throw new Error(`Fork: unknown orphan fate ${fate}`);
  return words;
}

export const UNCLE_RATE_TARGET = '2.50%';

/** The window's uncle rate from the API's exact per-day counts; null for an empty window. */
export function uncleRateFigure(
  overview: Pick<ForkOverview, 'days' | 'unclesInWindow'>
): { rate: string; uncles: number; blocks: number } | null {
  const uncles = overview.days.reduce((sum, day) => sum + day.uncles, 0);
  const blocks = overview.days.reduce((sum, day) => sum + day.canonicalBlocks, 0);
  if (uncles !== overview.unclesInWindow)
    throw new Error(
      `Fork overview: days hold ${uncles} uncles but the window says ${overview.unclesInWindow}`
    );
  if (blocks === 0) {
    if (uncles > 0) throw new Error(`Fork overview: ${uncles} uncles in a window with no blocks`);
    return null;
  }
  return { rate: `${formatPercent(BigInt(uncles), BigInt(blocks))}%`, uncles, blocks };
}
