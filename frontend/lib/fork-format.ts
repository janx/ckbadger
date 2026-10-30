import type {
  ForkEventRef,
  ForkFirstOrphanStatus,
  ForkOverview,
  OrphanedBlock,
  ReorgEvent,
} from '@/lib/api';
import { formatPercent } from '@/lib/ckb-decimal';
import { formatUtc8DateTime } from '@/lib/utils';

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

/**
 * A row's link (R2b-b): from the API's `firstOrphanHash`, never from the preview or the ms id;
 * null when the event has no D7 address. A recorded / traced status without a hash is a broken
 * contract.
 */
export function forkEventHref(event: ReorgEvent): string | null {
  return forkRefHref(event);
}

/** Another event by its D7 address (`revertedByEvent`, `reversesEvents`). */
export function forkRefHref(
  ref: Pick<ForkEventRef, 'forkPointNumber' | 'firstOrphanHash' | 'firstOrphanStatus'>
): string | null {
  if (ref.firstOrphanHash === null) {
    if (ref.firstOrphanStatus === 'recorded' || ref.firstOrphanStatus === 'traced')
      throw new Error(
        `fork #${ref.forkPointNumber} is ${ref.firstOrphanStatus} but has no first orphan hash`
      );
    return null;
  }
  return forkDetailHref(ref.forkPointNumber, ref.firstOrphanHash);
}

const MISSING_ADDRESS_WORDS: Record<
  Exclude<ForkFirstOrphanStatus, 'recorded' | 'traced'>,
  string
> = {
  not_observed: 'nothing was rolled back, so no detail page',
  needs_node_reader: 'its first dropped block needs the node reader',
  old_branch_unavailable: 'the node no longer has the dropped branch',
};

/** Why a switch has no D7 address, in plain words. */
export function missingAddressWords(status: ForkFirstOrphanStatus): string {
  if (status === 'recorded' || status === 'traced')
    throw new Error(`fork first orphan status ${status} has an address`);
  const words = MISSING_ADDRESS_WORDS[status];
  if (!words) throw new Error(`Fork: unknown first orphan status ${status}`);
  return words;
}

const NUMBER_WORDS = [
  'zero',
  'one',
  'two',
  'three',
  'four',
  'five',
  'six',
  'seven',
  'eight',
  'nine',
];

/** "51 one block later, 215 two blocks later" (Forks V67 hero). */
export function uncleDistanceWords(
  distances: readonly { distance: number; count: number }[]
): string {
  return distances
    .map(({ distance, count }) => {
      if (!Number.isSafeInteger(distance) || distance < 1)
        throw new Error(`Fork: invalid uncle distance ${distance}`);
      const d = NUMBER_WORDS[distance] ?? distance.toLocaleString('en-US');
      return `${count.toLocaleString('en-US')} ${d} block${distance === 1 ? '' : 's'} later`;
    })
    .join(', ');
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

/** A block header time on the site's UTC+8 clock with its milliseconds: `09:12:32.761`. */
export function blockClock(timestamp: string): string {
  const ms = Date.parse(timestamp);
  if (!Number.isSafeInteger(ms) || ms < 0)
    throw new Error(`Invalid block timestamp ${JSON.stringify(timestamp)}`);
  return `${formatUtc8DateTime(ms).slice(11, 19)}.${String(ms % 1000).padStart(3, '0')}`;
}
