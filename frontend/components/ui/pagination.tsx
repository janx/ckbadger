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

  return (
    <div className={cn('flex items-center justify-center gap-1', className)}>
      <button
        type="button"
        onClick={() => onPageChange(page - 1)}
        disabled={page <= 1}
        className="hover:text-terminal-green rounded px-3 py-1.5 font-mono text-sm text-slate-400 transition-colors hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-50"
      >
        Prev
      </button>

      {pages.map((p, i) =>
        p === '...' ? (
          <span key={`ellipsis-${i}`} className="px-2 text-slate-500">
            ...
          </span>
        ) : (
          <button
            type="button"
            key={p}
            onClick={() => onPageChange(p as number)}
            className={cn(
              'rounded px-3 py-1.5 font-mono text-sm transition-colors',
              page === p
                ? 'bg-terminal-green text-slate-950'
                : 'hover:text-terminal-green text-slate-400 hover:bg-slate-800'
            )}
          >
            {p}
          </button>
        )
      )}

      <button
        type="button"
        onClick={() => onPageChange(page + 1)}
        disabled={page >= totalPages}
        className="hover:text-terminal-green rounded px-3 py-1.5 font-mono text-sm text-slate-400 transition-colors hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-50"
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
