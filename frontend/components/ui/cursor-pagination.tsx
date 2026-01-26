'use client';

import { cn } from '@/lib/utils';

interface CursorPaginationProps {
  total?: number;
  totalLabel?: string;
  hasMore: boolean;
  hasPrevious: boolean;
  onNext: () => void;
  onPrevious: () => void;
  className?: string;
}

export function CursorPagination({
  total,
  totalLabel = 'items',
  hasMore,
  hasPrevious,
  onNext,
  onPrevious,
  className,
}: CursorPaginationProps) {
  return (
    <div className={cn('flex items-center justify-between', className)}>
      {total !== undefined ? (
        <span className="font-mono text-sm text-slate-500">
          Total: <span className="text-terminal-green">{total.toLocaleString()}</span> {totalLabel}
        </span>
      ) : (
        <span />
      )}
      <div className="flex gap-2">
        <button
          onClick={onPrevious}
          disabled={!hasPrevious}
          className="hover:border-terminal-green hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-2 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          Previous
        </button>
        <button
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
