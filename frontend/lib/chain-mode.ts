import type { NetworkStats } from '@/lib/api';

/** The session's mode, as the status line's first block says it. */
export type ChainMode = 'LIVE' | 'SYNCING' | 'OFFLINE';

export interface ChainModeInput {
  /** The chain vitals (`/statistics/network`), once they have arrived. */
  stats: NetworkStats | undefined;
  /** The vitals fetch failed: the explorer's API cannot be read. */
  failed: boolean;
  /** The page holds the realtime socket (useRealtimeStore `socketHeld`). */
  socketHeld: boolean;
  /** The realtime socket is connected (useRealtimeStore `isConnected`). */
  connected: boolean;
}

/**
 * The mode from the same state the vitals strip reads (its query and its Live mark): OFFLINE when
 * the vitals cannot be read, or the page holds the realtime socket and it is down (the strip's "Not
 * live"); SYNCING while the indexer is behind the node; LIVE otherwise. Null until the vitals first
 * arrive — the mode is not known yet, and nothing is claimed.
 */
export function chainMode({
  stats,
  failed,
  socketHeld,
  connected,
}: ChainModeInput): ChainMode | null {
  if (failed) return 'OFFLINE';
  if (stats === undefined) return null;
  if (socketHeld && !connected) return 'OFFLINE';
  if (stats.syncStatus.isSyncing) return 'SYNCING';
  return 'LIVE';
}
