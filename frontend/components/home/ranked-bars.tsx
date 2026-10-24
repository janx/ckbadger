'use client';

import Link from '@/components/ui/link';
import { formatShare } from '@/lib/activity-stats';
import { cn } from '@/lib/utils';

export interface RankedRow {
  key: string;
  label: string;
  /** count / total, in [0, 1]. */
  share: number;
  /** Where the row leads, if anywhere. */
  href?: string;
}

/**
 * Ranked shares as thin bars: a label, a 6 px track with a jade fill (at least 2 px), and the
 * percentage. Replaces the homepage donuts; every row is a link when it has somewhere to go.
 */
export function RankedBars({
  title,
  titleHref,
  rows,
  wideLabels = false,
  testId,
}: {
  title: string;
  titleHref?: string;
  rows: RankedRow[];
  /** Long labels (script names) get a flexible column; short ones (types) a fixed 92 px. */
  wideLabels?: boolean;
  testId?: string;
}) {
  const grid = cn(
    'grid items-center gap-2.5 py-1 text-xs',
    wideLabels
      ? 'grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_52px]'
      : 'grid-cols-[92px_minmax(0,1fr)_52px]'
  );
  return (
    <div data-testid={testId} className="flex min-w-0 flex-col gap-1.5">
      {titleHref ? (
        <Link
          href={titleHref}
          className="text-text-dim hover:text-jade w-fit text-xs transition-colors"
        >
          {title}
        </Link>
      ) : (
        <span className="text-text-dim text-xs">{title}</span>
      )}
      <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
        {rows.map((row) => {
          const body = (
            <>
              <span className="text-text truncate" title={row.label}>
                {row.label}
              </span>
              <span className="bg-base-border-subtle flex h-1.5 rounded-[3px]">
                <span
                  data-role="share-fill"
                  className="bg-jade h-1.5 min-w-[2px] rounded-[3px]"
                  style={{ width: `${(row.share * 100).toFixed(2)}%` }}
                />
              </span>
              <span className="text-text-bright text-right tabular-nums">
                {formatShare(row.share)}
              </span>
            </>
          );
          return (
            <li key={row.key} data-rank-row={row.key}>
              {row.href ? (
                <Link href={row.href} className={cn(grid, 'hover:bg-base-elevated/40 rounded')}>
                  {body}
                </Link>
              ) : (
                <div className={grid}>{body}</div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
