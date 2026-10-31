'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api, ApiRequestError } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { readErrorText } from '@/lib/read-error';

/**
 * Round 3 (contracts R-B · R-10, CH-27): a block chart's evidence for one UTC+8 day — the first
 * and last block the API finds on that day's chain time (exact, two binary searches), each linked
 * by hash. Shown as the API states it; nothing is estimated here.
 */
export function BlocksOfDay({ date }: { date: string }) {
  const day = useQuery({
    queryKey: ['blocks-by-day', date],
    queryFn: () => api.getBlocksByDay(date),
  });
  const title = `Blocks of ${date} (UTC+8)`;
  if (day.error)
    return (
      <span role="alert" className="text-rouge">
        {title}:{' '}
        {day.error instanceof ApiRequestError && day.error.apiMessage
          ? day.error.apiMessage
          : readErrorText(day.error)}
      </span>
    );
  if (!day.data) return <span className="text-text-dim">{title}: reading…</span>;
  const { first, last, blocksCount, complete } = day.data;
  if (day.data.date !== date || last.number - first.number + 1 !== blocksCount)
    throw new Error(`${title}: the API answered ${JSON.stringify(day.data)}`);
  return (
    <span data-testid="blocks-of-day">
      {title}:{' '}
      <Link href={`/blocks/${first.hash}`} className="text-jade hover:underline">
        {formatBlockNumber(first.number)}
      </Link>{' '}
      –{' '}
      <Link href={`/blocks/${last.hash}`} className="text-jade hover:underline">
        {formatBlockNumber(last.number)}
      </Link>
      <span className="text-text-dim">
        {' '}
        · {blocksCount.toLocaleString('en-US')} {blocksCount === 1 ? 'block' : 'blocks'}
        {complete ? '' : ', so far: the day is not over'}
      </span>
    </span>
  );
}
