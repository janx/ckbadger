'use client';

import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { unitNode } from '@/components/ui/units';
import { BELOW_STICKY_HEADER } from '@/components/layout/sticky-header';

export interface ListColumn<T> {
  key: string;
  label: string;
  render: (row: T) => ReactNode;
  /** Explicit width budget prevents neighbouring columns from overlapping. */
  minWidth: number;
  width?: string;
  numeric?: boolean;
  /**
   * The cells' own classes, and only these families, with no variant (`cellClassOwner`): ink and
   * size (`text-*`), vertical padding (`py-*`, `pt-*`, `pb-*`), white-space (`whitespace-*`),
   * truncation (`truncate`), alignment (`text-left` … `text-right`, `align-*`). Never a class that
   * takes a horizontal pixel (a padding, a margin, a width, a side border): the table owns those
   * (`CELL_HORIZONTAL_CLASSES`), and a mark a column draws beside its content is in its minWidth.
   */
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
  /**
   * Cell padding (table-rows plan T2). `default`: `py-3 align-top`, rows grow with their tallest
   * cell. `row`: `py-[11px] align-middle`, so one 18 px line makes a 40 px row; heads unchanged.
   */
  density?: ListTableDensity;
  /**
   * The row's hover text, written to the `<tr>` and to the phone `listitem` as `title`: where the
   * fields that left a one-line row are kept (with the detail page). Never empty.
   */
  rowTitle?: (row: T) => string;
  /**
   * Told the form this list chose, and again whenever its measured width changes it, before the
   * browser paints. For a part of the page that belongs to one form only — a legend of the wide
   * table's glyphs, a switch of its bar (PA-02, PA-03) — so it is drawn exactly with that form,
   * from the same measurement, never from a breakpoint of its own.
   */
  onForm?: (form: ListTableForm) => void;
  /**
   * Where the column heads stick while the rows scroll under them (readability B5): `page`, the
   * default, right under the site header and its vitals strip, at the height the header publishes;
   * `box`, at the top of a scroll box the caller draws around the list (raw data in a
   * `max-h-* overflow-y-auto`). Stuck to the page inside a scroll box, a head would stand the
   * header's height down into the box.
   */
  headSticksTo?: 'page' | 'box';
}

export type ListTableDensity = 'default' | 'row';

/**
 * The gap a column keeps after its content, its cells' `pr-4` (the last column keeps none,
 * `last:pr-0`): every column but the last budgets it in its minimum width. It is a cell's ONLY
 * horizontal padding: `pl-0` is written out because a browser gives a table cell 1 px of padding
 * on every side by default, and a pixel the budget does not count breaks the column that is
 * budgeted to the pixel.
 */
export const CELL_GAP_PX = 16;

/**
 * The gap as the class that draws it: Tailwind's `pr-4`, four of its 0.25 rem steps, 16 px at the
 * root's 16 px (`__tests__/components/list-table.test.tsx` reads the step from the config, so the
 * class and CELL_GAP_PX cannot move apart).
 */
export const CELL_GAP_CLASS = 'pr-4';

/**
 * Every horizontal class a head and a cell carry, and the only ones: no left padding, the gap,
 * none after the last column.
 */
export const CELL_HORIZONTAL_CLASSES: readonly string[] = ['pl-0', CELL_GAP_CLASS, 'last:pr-0'];

/**
 * A column head (B5; a label, so quiet by R3): sticky, on the vitals strip's own opaque ground so no row shows through, one
 * layer under the site header (z-40). Its rule is its own 1 px line in the row-rule ink: a
 * collapsed table border is painted by the table and would stay behind where the head was.
 */
const HEAD_CLASS = [
  'sticky z-20 bg-[var(--m-header-strip)] text-text-dim text-meta py-3 font-normal after:absolute after:inset-x-0 after:bottom-0 after:h-px after:bg-row-rule',
  ...CELL_HORIZONTAL_CLASSES,
].join(' ');
const HEAD_TOP = { page: BELOW_STICKY_HEADER, box: 'top-0' } as const;

/** `td` classes per density; `th` is the same in both. */
const CELL_CLASS: Record<ListTableDensity, string> = {
  default: ['min-w-0 py-3 align-top', ...CELL_HORIZONTAL_CLASSES].join(' '),
  row: ['min-w-0 py-[11px] align-middle', ...CELL_HORIZONTAL_CLASSES].join(' '),
};

/**
 * A cell holds its content (readability 2026-11-18, the lead's rulings on the fb reviews): nothing
 * in a cell draws outside it, and how its content may break is the table's rule, not a page's.
 *
 * - A figure — a number with its sign, its `#`, its decimals — is one unbreakable unit. No cell
 *   lets a word break where it overflows (no `overflow-wrap: break-word` here, as a non-numeric
 *   cell once had), so nothing in a table can cut a figure: a column budgeted to the pixel holds
 *   its figure on one line, and a figure past its column's bound shows the shortfall past the
 *   figure's edge, never "#21,045,84" over "5". A numeric column's cell is one figure with its
 *   label and is held on one line whole (`NUMERIC_CELL`).
 * - Words break between them, never inside one: a phrase is never one unbreakable run in a cell
 *   (a headline, "this device's clock is behind"), so it makes its row taller, never its column
 *   wider; a separated list breaks between its parts first (`Dotted`). A column holds its longest
 *   word at its minimum.
 * - A chip (a row tag, a composition badge) is one unit as a figure is, budgeted whole.
 *
 * `__tests__/utils/list-table-cells.ts` reads every table the suite renders against these.
 */
const NUMERIC_CELL = 'whitespace-nowrap text-right tabular-nums';

export type ListTableForm = 'table' | 'narrow-table' | 'rows';

/**
 * What a column may write on its cells (`ListColumn.className`), family by family, each a bare
 * utility with no variant and no arbitrary value (`!` important allowed): ink, size and text
 * alignment (`text-` and a word: a palette colour, a step of the type scale, `text-right`),
 * vertical padding, vertical alignment, white-space, truncation. Every column of the site sets
 * only these (read 2026-11-19: `whitespace-nowrap`, `!whitespace-normal`, `truncate`,
 * `text-text-dim`, `text-body`, `py-1.5`, `py-[9px]`, `py-3`, `align-top`).
 */
const COLUMN_CLASS_FAMILIES: readonly RegExp[] = [
  /^text-[a-z]+(?:-[a-z0-9]+)*(?:\/\d{1,3})?$/,
  /^p[ytb]-(?:px|\d+(?:\.\d+)?|\[\d+(?:\.\d+)?px\])$/,
  /^align-(?:baseline|top|middle|bottom|text-top|text-bottom|sub|super)$/,
  /^whitespace-(?:normal|nowrap|pre|pre-line|pre-wrap|break-spaces)$/,
  /^truncate$/,
];

/** Every class the table writes on its heads and cells itself. */
const TABLE_CELL_CLASSES: ReadonlySet<string> = new Set(
  [
    HEAD_CLASS,
    ...Object.values(HEAD_TOP),
    ...Object.values(CELL_CLASS),
    NUMERIC_CELL,
    'text-left',
    'text-right',
  ].flatMap((classes) => classes.split(/\s+/))
);

/**
 * Whose a class on a head or a cell is (review fb-logic N2, the lead's ruling: one rule, an
 * allow-list): `column` for a class a column may set (`COLUMN_CLASS_FAMILIES`), `table` for one
 * the table writes itself (its `pl-0`, its gap, its head's stickiness), null for anything else.
 * The table refuses a column's class that is not `column`; the after-test check
 * (`__tests__/utils/list-table-cells.ts`) refuses a cell's class that is neither.
 */
export function cellClassOwner(cls: string): 'column' | 'table' | null {
  const important = cls.replace(/^!/, '');
  if (COLUMN_CLASS_FAMILIES.some((family) => family.test(important))) return 'column';
  return TABLE_CELL_CLASSES.has(cls) ? 'table' : null;
}

/**
 * A column's own classes take no horizontal pixel of its cells, and nothing else a column's doc
 * does not permit (review fb-tests 5 and fb-logic N2, the lead's rulings): the table writes `pl-0`
 * and the gap on every cell, and a column's `px-2` would replace both through cn() and draw its
 * content in pixels its minWidth never counted; a class the rule cannot place is refused too.
 */
function refuseColumnClasses<T>(label: string, column: ListColumn<T>): void {
  for (const cls of column.className?.split(/\s+/).filter(Boolean) ?? []) {
    if (cellClassOwner(cls) !== 'column')
      throw new Error(
        `${label}: column ${column.key} sets a class a column may not (${cls}): a column sets its ` +
          "cells' ink, size, vertical padding, white-space, truncation and alignment, with no " +
          `variant; a cell's horizontal space is the table's, its ${CELL_GAP_PX} px gap, and a ` +
          'column budgets what it draws beside its content in minWidth'
      );
  }
}

export function minimumWidth<T>(label: string, columns: readonly ListColumn<T>[]): number {
  let sum = 0;
  for (const c of columns) {
    if (!Number.isFinite(c.minWidth) || c.minWidth <= 0)
      throw new Error(`${label}: invalid minimum width for ${c.key}`);
    refuseColumnClasses(label, c);
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
  density = 'default',
  rowTitle,
  onForm,
  headSticksTo = 'page',
}: ListTableProps<T>) {
  const [minimum, narrowMinimum] = minimumWidths(label, columns, narrowColumns);
  const titleOf = (row: T): string | undefined => {
    if (!rowTitle) return undefined;
    const title = rowTitle(row);
    if (!title.trim()) throw new Error(`${label}: row ${rowKey(row)} has an empty title`);
    return title;
  };
  const [ref, width] = useContainerWidth();
  // Until the container has a width the table is drawn: in a browser the layout effect measures
  // the container and replaces it before the first paint; a hidden container (a closed details)
  // shows nothing either way, and re-measures when it opens.
  const form: ListTableForm =
    width === null ? 'table' : listTableForm(label, width, columns, narrowColumns);
  const shown = form === 'narrow-table' ? narrowColumns! : columns;
  useLayoutEffect(() => onForm?.(form), [form, onForm]);
  return (
    <div
      ref={ref}
      data-list-form={form}
      data-list-measured={width !== null}
      className={cn('min-w-0 [contain:inline-size]', className)}
    >
      {!rows.length ? (
        // the empty list's sentence says what is (not) there: the reading ink (R3)
        <p className="text-text text-small py-6">{empty}</p>
      ) : form === 'rows' ? (
        <div role="list" aria-label={`${label}, compact`}>
          {rows.map((row) => (
            <div
              role="listitem"
              key={rowKey(row)}
              title={titleOf(row)}
              className={cn('border-row-rule border-b py-2.5', rowClassName?.(row))}
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
                  // Table-rows T5: column heads at meta (11 px), dim, in every table; B5: in view.
                  className={cn(
                    HEAD_CLASS,
                    HEAD_TOP[headSticksTo],
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
                title={titleOf(row)}
                onMouseEnter={() => onRowHover?.(row)}
                onMouseLeave={() => onRowHover?.(null)}
                // Midnight, continued (move 4): a hovered row gets the row-scan sweep; its rule
                // is the row-rule ink, #1a1f30 (material pass v2, move 6).
                className={cn('row-scan border-row-rule border-b', rowClassName?.(row))}
              >
                {shown.map((c) => (
                  <td
                    key={c.key}
                    className={cn(CELL_CLASS[density], c.numeric && NUMERIC_CELL, c.className)}
                  >
                    {unitNode(c.render(row))}
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
