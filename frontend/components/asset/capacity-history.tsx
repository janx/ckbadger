'use client';
import type { AssetCapacityHistory, CollectionInsights } from '@/lib/api';
import { AssetHistory } from '@/components/asset/asset-history';
import { exactUnsignedInteger } from '@/lib/exact-integer';
export interface CapacityHistoryProps {
  history: AssetCapacityHistory;
  subject: string;
  /** The collection's weekly activity, or why there is none to draw here. */
  weekly: CollectionInsights['weeklyActivity'] | { unavailable: string };
}
/** Adapt the canonical capacity series to the same range, axis and cursor as token history. */
export function CapacityHistory({ history, subject, weekly }: CapacityHistoryProps) {
  if (history.source !== 'indexedChainTimeUtc8')
    throw new Error(`${subject}: unsupported capacity calendar ${history.source}`);
  const daily = history.data.map((point) => {
    const occupied = exactUnsignedInteger(point.values.used, `${subject} ${point.date}: occupied`);
    const free = exactUnsignedInteger(point.values.unused, `${subject} ${point.date}: free`);
    return {
      date: point.date,
      capacity: (occupied + free).toString(),
      occupiedCapacity: occupied.toString(),
    };
  });
  return (
    <section aria-label={`${subject} capacity history`}>
      <AssetHistory
        daily={daily}
        weekly={
          'unavailable' in weekly
            ? weekly
            : weekly.map((w) => ({
                weekStart: w.weekStart,
                transfersCount: w.transferTransactions,
              }))
        }
        completeThrough={history.completeThrough}
      />
    </section>
  );
}
