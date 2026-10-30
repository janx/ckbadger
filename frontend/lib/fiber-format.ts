import type { FiberChannelState, FiberEpochDelay } from '@/lib/api';
import { formatDurationMs } from '@/lib/utils';
import { formatEpochPosition } from '@/lib/epoch';

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

/** A decoded `since` epoch delay in words: "6 epochs", "6 1/2 epochs", "until epoch 180". */
export function epochDelayWords(delay: FiberEpochDelay): string {
  const { relative, epochs, index, length } = delay;
  if (
    ![epochs, index, length].every((n) => Number.isSafeInteger(n) && n >= 0) ||
    length === 0 ||
    index >= length
  )
    throw new Error(`Fiber: invalid epoch delay ${JSON.stringify(delay)}`);
  const amount = `${epochs.toLocaleString('en-US')}${index ? ` ${index}/${length}` : ''}`;
  if (!relative) return `until epoch ${amount}`;
  return `${amount} epoch${epochs === 1 && index === 0 ? '' : 's'}`;
}

/** An evidence stage's epoch (`number+index/length`) on the site's one epoch form. */
export function stageEpoch(epoch: string): string {
  const match = /^(0|[1-9]\d*)\+(0|[1-9]\d*)\/([1-9]\d*)$/.exec(epoch);
  if (!match)
    throw new Error(`Fiber stage epoch ${JSON.stringify(epoch)} is not number+index/length`);
  return formatEpochPosition({
    number: Number(match[1]),
    index: Number(match[2]),
    length: Number(match[3]),
  });
}
