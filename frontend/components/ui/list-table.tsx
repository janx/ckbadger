'use client';

import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
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

/**
 * From this container width a phone row takes its two-line arrangement (the 640–767 band's 608 px
 * content width); below it, its three lines (a 358 px phone).
 */
export const WIDE_ROW_MIN_WIDTH = 560;

export interface ListTableProps<T> {
  label: string;
  columns: readonly ListColumn<T>[];
  /**
   * A second, narrower table for a container that cannot hold `columns` but holds these (the
   * boards' 640 tables); the same rule picks it.
   */
  narrowColumns?: readonly ListColumn<T>[];
  rows: readonly T[];
  rowKey: (row: T) => string;
  empty?: ReactNode;
  /**
   * The designed phone row (R7): two or three lines that keep every column. `wide` is true from
   * `WIDE_ROW_MIN_WIDTH`, where the row takes its two-line arrangement.
   */
  mobileRow: (row: T, wide: boolean) => ReactNode;
  rowClassName?: (row: T) => string;
  onRowHover?: (row: T | null) => void;
  className?: string;
}

export type ListTableForm = 'table' | 'narrow-table' | 'rows';

export function minimumWidth<T>(label: string, columns: readonly ListColumn<T>[]): number {
  let sum = 0;
  for (const c of columns) {
    if (!Number.isFinite(c.minWidth) || c.minWidth <= 0)
      throw new Error(`${label}: invalid minimum width for ${c.key}`);
    sum += c.minWidth;
  }
  return sum;
}

/** Both tables' minimum widths; the narrow table must be narrower than the full one. */
function minimumWidths<T>(
  label: string,
  columns: readonly ListColumn<T>[],
  narrowColumns?: readonly ListColumn<T>[]
): [number, number | null] {
  const full = minimumWidth(label, columns);
  const narrow = narrowColumns ? minimumWidth(label, narrowColumns) : null;
  if (narrow !== null && narrow >= full)
    throw new Error(`${label}: the narrow table (${narrow} px) is not narrower than ${full} px`);
  return [full, narrow];
}

/**
 * R1: a table when the measured container holds every column's minimum width, the narrower table
 * when it holds that one's, otherwise the phone rows. No breakpoint takes part.
 */
export function listTableForm<T>(
  label: string,
  width: number,
  columns: readonly ListColumn<T>[],
  narrowColumns?: readonly ListColumn<T>[]
): ListTableForm {
  if (!Number.isFinite(width) || width <= 0)
    throw new Error(`${label}: cannot choose a form for a container ${width} px wide`);
  const [full, narrow] = minimumWidths(label, columns, narrowColumns);
  if (width >= full) return 'table';
  if (narrow !== null && width >= narrow) return 'narrow-table';
  return 'rows';
}

/**
 * The container's width, measured before paint and kept current; null until it has one (the render
 * before the first layout effect, a closed `<details>`, jsdom). The container's inline size is
 * contained, so a table drawn in it never props it wider than the space it is given.
 */
function useContainerWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = (w: number) =>
      setWidth((prev) => (w <= 0 || (prev !== null && Math.abs(prev - w) < 0.5) ? prev : w));
    read(el.getBoundingClientRect().width);
    const ro = new ResizeObserver((entries) => read(entries[entries.length - 1].contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, width] as const;
}

export function ListTable<T>({
  label,
  columns,
  narrowColumns,
  rows,
  rowKey,
  empty = 'No records.',
  mobileRow,
  rowClassName,
  onRowHover,
  className,
}: ListTableProps<T>) {
  const [minimum, narrowMinimum] = minimumWidths(label, columns, narrowColumns);
  const [ref, width] = useContainerWidth();
  // Until the container has a width the table is drawn: in a browser the layout effect measures
  // the container and replaces it before the first paint; a hidden container (a closed details)
  // shows nothing either way, and re-measures when it opens.
  const form: ListTableForm =
    width === null ? 'table' : listTableForm(label, width, columns, narrowColumns);
  const shown = form === 'narrow-table' ? narrowColumns! : columns;
  return (
    <div
      ref={ref}
      data-list-form={form}
      data-list-measured={width !== null}
      className={cn('min-w-0 [contain:inline-size]', className)}
    >
      {!rows.length ? (
        <p className="text-text-dim text-small py-6">{empty}</p>
      ) : form === 'rows' ? (
        <div role="list" aria-label={`${label}, compact`}>
          {rows.map((row) => (
            <div
              role="listitem"
              key={rowKey(row)}
              className={cn('border-base-elevated border-b py-2.5', rowClassName?.(row))}
            >
              {mobileRow(row, width! >= WIDE_ROW_MIN_WIDTH)}
            </div>
          ))}
        </div>
      ) : (
        <table
          aria-label={label}
          className="text-small w-full table-fixed border-collapse"
          style={{ minWidth: form === 'table' ? minimum : narrowMinimum! }}
        >
          <colgroup>
            {shown.map((c) => (
              <col
                key={c.key}
                style={{ width: c.width ?? `${c.minWidth}px`, minWidth: c.minWidth }}
              />
            ))}
          </colgroup>
          <thead>
            <tr>
              {shown.map((c) => (
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
                {shown.map((c) => (
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
      )}
    </div>
  );
}
