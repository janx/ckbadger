'use client';

import { cn } from '@/lib/utils';

interface CursorPaginationProps {
  total?: number;
  totalLabel?: string;
  page?: number;
  pageSize?: number;
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
  hasMore,
  hasPrevious,
  onNext,
  onPrevious,
  className,
}: CursorPaginationProps) {
  const totalPages = total !== undefined && pageSize ? Math.ceil(total / pageSize) : undefined;

  return (
    <div className={cn('flex w-full items-center justify-between', className)}>
      {total !== undefined ? (
        <span className="font-mono text-sm text-slate-500">
          {total.toLocaleString()} {totalLabel}
          {pageSize !== undefined && <>, {pageSize} per page</>}
        </span>
      ) : (
        <span />
      )}
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={onPrevious}
          disabled={!hasPrevious}
          className="hover:border-terminal-green hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-2 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          Previous
        </button>
        {page !== undefined && (
          <span className="font-mono text-sm text-slate-500">
            {page}
            {totalPages !== undefined ? ` / ${totalPages}` : ''}
          </span>
        )}
        <button
          type="button"
          onClick={onNext}
          disabled={!hasMore}
          className="hover:border-terminal-green hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-2 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          Next
        </button>
      </div>
    </div>
  );
}
