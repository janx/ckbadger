'use client';

import { cn } from '@/lib/utils';

interface PaginationProps {
  page: number;
  totalPages: number;
  onPageChange: (page: number) => void;
  className?: string;
}

export function Pagination({ page, totalPages, onPageChange, className }: PaginationProps) {
  const pages = getPageNumbers(page, totalPages);

  // Below sm, Prev + seven page buttons + Next (~420 px of unshrinkable buttons) do not fit a
  // 358 px column: the page numbers take their own row and Prev / Next share the row under it.
  return (
    <div
      className={cn(
        'flex flex-wrap items-center justify-between gap-1 sm:flex-nowrap sm:justify-center',
        className
      )}
    >
      <button
        type="button"
        onClick={() => onPageChange(page - 1)}
        disabled={page <= 1}
        className="hover:text-jade text-text-dim hover:bg-base-elevated text-body whitespace-nowrap rounded px-3 py-2 font-mono transition-colors disabled:cursor-not-allowed disabled:opacity-50"
      >
        Prev
      </button>

      <div
        data-role="page-numbers"
        className="order-first flex basis-full flex-wrap items-center justify-center gap-1 sm:order-none sm:basis-auto"
      >
        {pages.map((p, i) =>
          p === '...' ? (
            <span key={`ellipsis-${i}`} className="text-text-dim px-2">
              ...
            </span>
          ) : (
            <button
              type="button"
              key={p}
              onClick={() => onPageChange(p as number)}
              className={cn(
                'text-body rounded px-3 py-2 font-mono transition-colors',
                page === p
                  ? 'bg-emphasis text-base-bg'
                  : 'hover:text-jade text-text-dim hover:bg-base-elevated'
              )}
            >
              {p}
            </button>
          )
        )}
      </div>

      <button
        type="button"
        onClick={() => onPageChange(page + 1)}
        disabled={page >= totalPages}
        className="hover:text-jade text-text-dim hover:bg-base-elevated text-body whitespace-nowrap rounded px-3 py-2 font-mono transition-colors disabled:cursor-not-allowed disabled:opacity-50"
      >
        Next
      </button>
    </div>
  );
}

function getPageNumbers(current: number, total: number): (number | string)[] {
  if (total <= 7) {
    return Array.from({ length: total }, (_, i) => i + 1);
  }

  if (current <= 3) {
    return [1, 2, 3, 4, 5, '...', total];
  }

  if (current >= total - 2) {
    return [1, '...', total - 4, total - 3, total - 2, total - 1, total];
  }

  return [1, '...', current - 1, current, current + 1, '...', total];
}
