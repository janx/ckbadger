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
  const rangeStart = canShowRange && currentCount > 0 ? (page - 1) * pageSize + 1 : 0;
  const rangeEnd = canShowRange && currentCount > 0 ? rangeStart + currentCount - 1 : 0;

  return (
    <div className={cn('flex w-full items-center justify-between', className)}>
      <span className="text-text-dim font-mono text-sm">
        {summary !== undefined ? (
          summary
        ) : canShowRange ? (
          <>
            Showing {rangeStart.toLocaleString()}-{rangeEnd.toLocaleString()}
            {known !== undefined ? ` of ${known.toLocaleString()}` : ''} {totalLabel}
            {pageSize !== undefined ? `, ${pageSize} per page` : ''}
          </>
        ) : known !== undefined ? (
          <>
            {known.toLocaleString()} {totalLabel}
            {pageSize !== undefined ? `, ${pageSize} per page` : ''}
          </>
        ) : (
          '\u00a0'
        )}
      </span>
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={onPrevious}
          disabled={!hasPrevious}
          className="hover:border-jade hover:text-jade border-base-border bg-base-elevated text-text rounded border px-4 py-2 font-mono text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          {previousLabel}
        </button>
        {page !== undefined && (
          <span className="text-text-dim font-mono text-sm">
            Page {page.toLocaleString('en-US')}
            {totalPages !== undefined ? ` of ${totalPages.toLocaleString('en-US')}` : ''}
          </span>
        )}
        <button
          type="button"
          onClick={onNext}
          disabled={!hasMore}
          className="hover:border-jade hover:text-jade border-base-border bg-base-elevated text-text rounded border px-4 py-2 font-mono text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          {nextLabel}
        </button>
      </div>
    </div>
  );
}
