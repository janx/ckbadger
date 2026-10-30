import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

export interface ListColumn<T> {
  key: string;
  label: string;
  render: (row: T) => ReactNode;
  /** Explicit width budget prevents neighbouring columns from overlapping. */
  minWidth: number;
  width?: string;
  numeric?: boolean;
  className?: string;
  /** F2 blocks: a richer column header (e.g. a bar column's 0 … scale ticks); `label` stays the plain name. */
  header?: ReactNode;
}
export interface ListTableProps<T> {
  label: string;
  columns: readonly ListColumn<T>[];
  rows: readonly T[];
  rowKey: (row: T) => string;
  empty?: ReactNode;
  /** A compact representation at narrow widths; defaults to labelled facts. */
  mobileRow?: (row: T) => ReactNode;
  rowClassName?: (row: T) => string;
  onRowHover?: (row: T | null) => void;
  className?: string;
}

export function ListTable<T>({
  label,
  columns,
  rows,
  rowKey,
  empty = 'No records.',
  mobileRow,
  rowClassName,
  onRowHover,
  className,
}: ListTableProps<T>) {
  for (const c of columns)
    if (!Number.isFinite(c.minWidth) || c.minWidth <= 0)
      throw new Error(`${label}: invalid minimum width for ${c.key}`);
  if (!rows.length) return <p className="text-text-dim py-6 text-xs">{empty}</p>;
  return (
    <div className={cn('min-w-0', className)}>
      <div className="hidden max-w-full overflow-x-auto md:block">
        <table
          aria-label={label}
          className="w-full table-fixed border-collapse text-xs"
          style={{ minWidth: columns.reduce((sum, c) => sum + c.minWidth, 0) }}
        >
          <colgroup>
            {columns.map((c) => (
              <col
                key={c.key}
                style={{ width: c.width ?? `${c.minWidth}px`, minWidth: c.minWidth }}
              />
            ))}
          </colgroup>
          <thead>
            <tr>
              {columns.map((c) => (
                <th
                  key={c.key}
                  scope="col"
                  className={cn(
                    'border-base-border-subtle text-text-dim border-b py-3 pr-4 font-normal last:pr-0',
                    c.numeric ? 'text-right' : 'text-left'
                  )}
                >
                  {c.header ?? c.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr
                key={rowKey(row)}
                onMouseEnter={() => onRowHover?.(row)}
                onMouseLeave={() => onRowHover?.(null)}
                className={cn('border-base-elevated border-b', rowClassName?.(row))}
              >
                {columns.map((c) => (
                  <td
                    key={c.key}
                    className={cn(
                      'min-w-0 py-3 pr-4 align-top last:pr-0',
                      c.numeric ? 'whitespace-nowrap text-right tabular-nums' : 'break-words',
                      c.className
                    )}
                  >
                    {c.render(row)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div role="list" aria-label={`${label}, compact`} className="md:hidden">
        {rows.map((row) => (
          <div
            role="listitem"
            key={rowKey(row)}
            className={cn('border-base-elevated border-b py-4', rowClassName?.(row))}
          >
            {mobileRow ? (
              mobileRow(row)
            ) : (
              <dl className="space-y-2">
                {columns.map((c) => (
                  <div
                    key={c.key}
                    className="flex min-w-0 flex-wrap items-baseline justify-between gap-x-3 gap-y-1 text-xs"
                  >
                    <dt className="text-text-dim">{c.label}</dt>
                    <dd
                      className={cn(
                        'text-text-bright min-w-0 max-w-full break-words',
                        c.numeric && 'text-right tabular-nums'
                      )}
                    >
                      {c.render(row)}
                    </dd>
                  </div>
                ))}
              </dl>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
