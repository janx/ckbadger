export type CapacityRangeKey = '30d' | '90d' | '1y' | 'all';

/** The script pages' capacity range, in the one spelling of a span (components/ui/range-select.tsx). */
export const CAPACITY_RANGE_OPTIONS: ReadonlyArray<{ key: CapacityRangeKey; label: string }> = [
  { key: '30d', label: '30 d' },
  { key: '90d', label: '90 d' },
  { key: '1y', label: '1 y' },
  { key: 'all', label: 'all' },
];

function formatUtcDate(date: Date): string {
  const year = date.getUTCFullYear();
  const month = `${date.getUTCMonth() + 1}`.padStart(2, '0');
  const day = `${date.getUTCDate()}`.padStart(2, '0');
  return `${year}-${month}-${day}`;
}

export function getCapacityRangeParams(
  range: CapacityRangeKey
): { from: string; to: string } | undefined {
  if (range === 'all') return undefined;

  const days = range === '30d' ? 30 : range === '90d' ? 90 : 365;
  const now = new Date();
  const to = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
  const from = new Date(to);
  from.setUTCDate(from.getUTCDate() - (days - 1));

  return {
    from: formatUtcDate(from),
    to: formatUtcDate(to),
  };
}
