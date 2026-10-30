import type { FiberChannelState } from '@/lib/api';
import { formatDurationMs } from '@/lib/utils';

/** The board's words for a channel's on-chain state (Fiber V65 / V66). */
export const FIBER_STATE_WORDS: Record<FiberChannelState, string> = {
  open: 'open',
  cooperativelyClosed: 'closed together',
  forceClosed: 'force-closed · not settled',
  settled: 'force-closed · settled',
};

/** An exact millisecond string from the API on the site's one duration form. */
export function fiberDuration(ms: string, what: string): string {
  if (!/^(0|[1-9]\d*)$/.test(ms)) throw new Error(`Fiber ${what}: invalid duration ${ms}`);
  return formatDurationMs(Number(ms));
}
