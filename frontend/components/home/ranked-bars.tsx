'use client';

import Link from '@/components/ui/link';
import { formatShare } from '@/lib/activity-stats';
import { formatPercent } from '@/lib/ckb-decimal';
import { cn } from '@/lib/utils';

export interface RankedRow {
  key: string;
  label: string;
  /** The row's count and the total it is a share of: the bar and the percentage read both. */
  count: number;
  total: number;
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
    'grid items-center gap-2.5 py-1 text-small',
    wideLabels
      ? 'grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_52px]'
      : 'grid-cols-[92px_minmax(0,1fr)_52px]'
  );
  return (
    <div data-testid={testId} className="flex min-w-0 flex-col gap-1.5">
      {/* the ranked list's title, its key (as a chart card's title): the chart it comes from */}
      {titleHref ? (
        <Link href={titleHref} look="key" className="text-small w-fit">
          {title}
        </Link>
      ) : (
        <span className="text-text-dim text-small">{title}</span>
      )}
      <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
        {rows.map((row) => {
          const body = (
            <>
              <span
                className={cn(
                  'text-text truncate transition-colors',
                  row.href && 'group-hover:text-text-bright group-focus-visible:text-text-bright'
                )}
                title={row.label}
              >
                {row.label}
              </span>
              <span className="bg-bar-track flex h-1.5">
                <span
                  data-role="share-fill"
                  className="bg-jade scan-fill h-1.5 min-w-[2px]"
                  style={{
                    width: `${formatPercent(BigInt(row.count), BigInt(row.total), 2)}%`,
                  }}
                />
              </span>
              <span className="text-text-bright text-right tabular-nums">
                {formatShare(row.count, row.total)}
              </span>
            </>
          );
          return (
            <li key={row.key} data-rank-row={row.key}>
              {row.href ? (
                <Link
                  href={row.href}
                  // a row that is one link: what it holds draws its inks
                  look="block"
                  className={cn(
                    grid,
                    'hover:bg-base-elevated/60 focus-visible:bg-base-elevated/60 group -mx-1 px-1 outline-none transition-colors'
                  )}
                >
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
