'use client';

import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface CursorPaginationProps {
  /** null = the API does not know the count yet (round 3 L2): shown like no count at all. */
  total?: number | null;
  totalLabel?: string;
  page?: number;
  pageSize?: number;
  currentCount?: number;
  hasMore: boolean;
  hasPrevious: boolean;
  onNext: () => void;
  onPrevious: () => void;
  className?: string;
  /** F2 blocks: a list's own range sentence ("50 newest of 20,876,569") and button words. */
  summary?: ReactNode;
  previousLabel?: string;
  nextLabel?: string;
}

const PAGER_BUTTON =
  'hover:border-jade hover:text-jade border-base-border bg-base-elevated text-text shrink-0 whitespace-nowrap rounded border px-3 py-2 font-mono text-body transition-colors disabled:cursor-not-allowed disabled:opacity-50 sm:px-4';

export function CursorPagination({
  total,
  totalLabel = 'items',
  page,
  pageSize,
  currentCount,
  hasMore,
  hasPrevious,
  onNext,
  onPrevious,
  className,
  summary,
  previousLabel = 'Previous',
  nextLabel = 'Next',
}: CursorPaginationProps) {
  // An empty list has no pages: no pager, and never "Page 1 of 0" (FF-L5).
  if (total === 0 && !hasMore && !hasPrevious) return null;
  // Unknown (null) and not given (undefined) both show no count.
  const known = total === null ? undefined : total;
  const totalPages =
    known !== undefined && known > 0 && pageSize ? Math.ceil(known / pageSize) : undefined;
  const canShowRange = page !== undefined && pageSize !== undefined && currentCount !== undefined;
  if (canShowRange && currentCount > pageSize)
    throw new Error(
      `${totalLabel} pager: ${currentCount} rows on a page of ${pageSize} (page ${page})`
    );
  const rangeStart = canShowRange && currentCount > 0 ? (page - 1) * pageSize + 1 : 0;
  const rangeEnd = canShowRange && currentCount > 0 ? rangeStart + currentCount - 1 : 0;
  const perPage = pageSize !== undefined ? ` · ${pageSize.toLocaleString('en-US')} per page` : '';
  // Consistency round C11: one wording on every list, the range as one phrase (en dash).
  const sentence = canShowRange
    ? `Showing ${rangeStart.toLocaleString('en-US')}–${rangeEnd.toLocaleString('en-US')}${
        known !== undefined ? ` of ${known.toLocaleString('en-US')}` : ''
      } ${totalLabel}${perPage}`
    : known !== undefined
      ? `${known.toLocaleString('en-US')} ${totalLabel}${perPage}`
      : null;

  // Below sm two rows: the count sentence, then Previous · Page x of y · Next on one line (four
  // items in one 358 px row broke "Page 1 of 10,419" over three lines). One row from sm up; in a
  // column too narrow for both (the address page's half-width lists) the controls drop under the
  // sentence as one unit — they never wrap among themselves.
  return (
    <div
      className={cn(
        'flex w-full flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-center sm:justify-between sm:gap-x-4',
        className
      )}
    >
      <span className="text-text-dim text-body font-mono">
        {summary !== undefined ? summary : (sentence ?? '\u00a0')}
      </span>
      <div
        data-role="pager-controls"
        className="flex items-center justify-between gap-2 sm:justify-start sm:gap-3"
      >
        <button type="button" onClick={onPrevious} disabled={!hasPrevious} className={PAGER_BUTTON}>
          {previousLabel}
        </button>
        {page !== undefined && (
          <span className="text-text-dim text-body whitespace-nowrap font-mono">
            Page {page.toLocaleString('en-US')}
            {totalPages !== undefined ? ` of ${totalPages.toLocaleString('en-US')}` : ''}
          </span>
        )}
        <button
          type="button"
          onClick={onNext}
          disabled={!hasMore}
          className={cn(PAGER_BUTTON, 'ml-auto sm:ml-0')}
        >
          {nextLabel}
        </button>
      </div>
    </div>
  );
}
