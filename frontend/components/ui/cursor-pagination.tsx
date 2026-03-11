'use client';

import { cn } from '@/lib/utils';

interface CursorPaginationProps {
  total?: number;
  totalLabel?: string;
  page?: number;
  pageSize?: number;
  currentCount?: number;
  hasMore: boolean;
  hasPrevious: boolean;
  onNext: () => void;
  onPrevious: () => void;
  className?: string;
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
}: CursorPaginationProps) {
  const totalPages = total !== undefined && pageSize ? Math.ceil(total / pageSize) : undefined;
  const canShowRange = page !== undefined && pageSize !== undefined && currentCount !== undefined;
  const rangeStart = canShowRange && currentCount > 0 ? (page - 1) * pageSize + 1 : 0;
  const rangeEnd = canShowRange && currentCount > 0 ? rangeStart + currentCount - 1 : 0;

  return (
    <div className={cn('flex w-full items-center justify-between', className)}>
      <span className="text-text-dim font-mono text-sm">
        {canShowRange ? (
          <>
            Showing {rangeStart.toLocaleString()}-{rangeEnd.toLocaleString()}
            {total !== undefined ? ` of ${total.toLocaleString()}` : ''} {totalLabel}
            {pageSize !== undefined ? `, ${pageSize} per page` : ''}
          </>
        ) : total !== undefined ? (
          <>
            {total.toLocaleString()} {totalLabel}
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
          Previous
        </button>
        {page !== undefined && (
          <span className="text-text-dim font-mono text-sm">
            Page {page}
            {totalPages !== undefined ? ` of ${totalPages}` : ''}
          </span>
        )}
        <button
          type="button"
          onClick={onNext}
          disabled={!hasMore}
          className="hover:border-jade hover:text-jade border-base-border bg-base-elevated text-text rounded border px-4 py-2 font-mono text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          Next
        </button>
      </div>
    </div>
  );
}
