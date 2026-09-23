/**
 * When the address page re-asks the API for unconfirmed transactions.
 *
 * Only page one carries tx-pool rows (a pool transaction can only land in a
 * future block, so it is always newer than every committed row, and never
 * enters a cursor), and only the tab the user is looking at is worth polling.
 * Everything that matters — ordering, dedup, filters, provisional fields —
 * is decided server-side, so this interval changes latency and idle traffic
 * only, never correctness.
 */
export const POOL_POLL_INTERVAL_MS = 5000;

export function poolRefetchInterval({
  isPageOne,
  isActiveTab,
}: {
  isPageOne: boolean;
  isActiveTab: boolean;
}): number | false {
  return isPageOne && isActiveTab ? POOL_POLL_INTERVAL_MS : false;
}
