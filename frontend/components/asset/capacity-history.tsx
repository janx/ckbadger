'use client';
import type { AssetCapacityHistory, CollectionInsights } from '@/lib/api';
import { AssetHistory } from '@/components/asset/asset-history';
import { exactUnsignedInteger } from '@/lib/exact-integer';
export interface CapacityHistoryProps {
  history: AssetCapacityHistory;
  subject: string;
  /** The collection's weekly activity, or why there is none to draw here. */
  weekly: CollectionInsights['weeklyActivity'] | { unavailable: string };
  /** PB-12: the collection's class (object or identity), whose colour its bars are drawn in. */
  assetClass: 'objects' | 'identities';
}
/** Adapt the canonical capacity series to the same range, axis and cursor as token history. */
export function CapacityHistory({ history, subject, weekly, assetClass }: CapacityHistoryProps) {
  if (history.source !== 'indexedChainTimeUtc8')
    throw new Error(`${subject}: unsupported capacity calendar ${history.source}`);
  // the route states the whole each day's cells hold beside its parts (`capacity`, `used`,
  // `unused`): the page reads all three and sums and subtracts nothing (it was occupied + free,
  // added here; then free was capacity − occupied, in AssetHistory: lane P2's third round, item 13)
  const daily = history.data.map((point) => ({
    date: point.date,
    capacity: exactUnsignedInteger(
      point.values.capacity,
      `${subject} ${point.date}: capacity`
    ).toString(),
    occupiedCapacity: exactUnsignedInteger(
      point.values.used,
      `${subject} ${point.date}: occupied`
    ).toString(),
    freeCapacity: exactUnsignedInteger(
      point.values.unused,
      `${subject} ${point.date}: free`
    ).toString(),
  }));
  return (
    <section aria-label={`${subject} capacity history`}>
      <AssetHistory
        daily={daily}
        weekly={
          'unavailable' in weekly
            ? weekly
            : weekly.map((w) => ({
                weekStart: w.weekStart,
                count: w.transferTransactions,
              }))
        }
        weeklyNoun="transfers"
        completeThrough={history.completeThrough}
        assetClass={assetClass}
      />
    </section>
  );
}
