import { afterEach, describe, expect, expectTypeOf, it, vi } from 'vitest';
import type { ReactNode } from 'react';
import { act, render, screen, within } from '@testing-library/react';
import resolveConfig from 'tailwindcss/resolveConfig';
import tailwindConfig from '@/tailwind.config';
import {
  CELL_GAP_CLASS,
  CELL_GAP_PX,
  CELL_HORIZONTAL_CLASSES,
  ListTable,
  WIDE_ROW_MIN_WIDTH,
  cellClassOwner,
  listTableForm,
  minimumWidth,
  type ListColumn,
  type ListTableProps,
} from '@/components/ui/list-table';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { BELOW_STICKY_HEADER } from '@/components/layout/sticky-header';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

type Row = { name: string; amount: string };
const ROWS: Row[] = [
  { name: 'Token', amount: '12.5' },
  { name: 'Coin', amount: '7' },
];
// 180 + 240 = 420 px of column minimums.
const COLUMNS: ListColumn<Row>[] = [
  { key: 'name', label: 'Name', minWidth: 180, render: (r) => r.name },
  { key: 'amount', label: 'Amount', minWidth: 240, numeric: true, render: (r) => r.amount },
];
// 180 + 100 = 280 px.
const NARROW: ListColumn<Row>[] = [
  { key: 'name', label: 'Name', minWidth: 180, render: (r) => r.name },
  { key: 'amount', label: 'Amt', minWidth: 100, numeric: true, render: (r) => r.amount },
];

function table(props: Partial<ListTableProps<Row>> = {}) {
  return (
    <ListTable
      label="Holdings"
      columns={COLUMNS}
      rows={ROWS}
      rowKey={(r) => r.name}
      mobileRow={(r, wide) => (
        <span data-testid="phone-row" data-wide={wide}>
          {r.name} holds {r.amount}
        </span>
      )}
      {...props}
    />
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('listTableForm (R1)', () => {
  it('draws the table from the sum of the column minimums, else the phone rows', () => {
    expect(listTableForm('t', 420, COLUMNS)).toBe('table');
    expect(listTableForm('t', 419, COLUMNS)).toBe('rows');
    expect(listTableForm('t', 1248, COLUMNS)).toBe('table');
  });
  it('picks the narrower table when only it fits', () => {
    expect(listTableForm('t', 420, COLUMNS, NARROW)).toBe('table');
    expect(listTableForm('t', 300, COLUMNS, NARROW)).toBe('narrow-table');
    expect(listTableForm('t', 280, COLUMNS, NARROW)).toBe('narrow-table');
    expect(listTableForm('t', 279, COLUMNS, NARROW)).toBe('rows');
  });
  it('refuses a width it cannot judge and a narrow table that is not narrower', () => {
    expect(() => listTableForm('t', 0, COLUMNS)).toThrow('cannot choose a form');
    expect(() => listTableForm('t', 300, NARROW, COLUMNS)).toThrow('is not narrower');
    expect(() =>
      listTableForm('t', 300, [{ key: 'x', label: 'X', minWidth: 0, render: () => null }])
    ).toThrow('invalid minimum width for x');
  });
});

describe('ListTable measures its container (R1, R7)', () => {
  it('shows only the table when the measured container holds every column', () => {
    stubChartWidth(608);
    const { container } = render(table());
    const t = screen.getByRole('table', { name: 'Holdings' });
    expect(t).toHaveClass('table-fixed');
    expect(t).toHaveStyle({ minWidth: '420px' });
    expect(within(t).getByRole('cell', { name: '12.5' })).toHaveClass('text-right', 'tabular-nums');
    expect(screen.queryByRole('list', { name: 'Holdings, compact' })).toBeNull();
    expect(screen.queryByTestId('phone-row')).toBeNull();
    expect(container.firstElementChild).toHaveAttribute('data-list-form', 'table');
  });

  it('shows only the designed phone rows when the container is narrower than the columns', () => {
    stubChartWidth(358);
    const { container } = render(table());
    expect(screen.queryByRole('table')).toBeNull();
    const list = screen.getByRole('list', { name: 'Holdings, compact' });
    const rows = within(list).getAllByTestId('phone-row');
    expect(rows.map((r) => r.textContent)).toEqual(['Token holds 12.5', 'Coin holds 7']);
    // 358 px is a phone: the row's three-line arrangement.
    expect(rows[0]).toHaveAttribute('data-wide', 'false');
    // The generic label/value dump is gone (R7).
    expect(container.querySelector('dl')).toBeNull();
    expect(container.firstElementChild).toHaveAttribute('data-list-form', 'rows');
  });

  it('tells the phone row it is wide from 560 px (the 640 band)', () => {
    stubChartWidth(WIDE_ROW_MIN_WIDTH);
    render(
      table({ columns: [...COLUMNS, { key: 'x', label: 'X', minWidth: 200, render: () => 1 }] })
    );
    expect(screen.getAllByTestId('phone-row')[0]).toHaveAttribute('data-wide', 'true');
  });

  it('draws the narrower table where only it fits', () => {
    stubChartWidth(300);
    render(table({ narrowColumns: NARROW }));
    const t = screen.getByRole('table', { name: 'Holdings' });
    expect(Array.from(t.querySelectorAll('th')).map((th) => th.textContent)).toEqual([
      'Name',
      'Amt',
    ]);
    expect(t).toHaveStyle({ minWidth: '280px' });
    expect(screen.queryByTestId('phone-row')).toBeNull();
  });

  it('never wraps the table in a sideways scroller nor hides a form behind a breakpoint', () => {
    stubChartWidth(608);
    const { container } = render(table());
    const html = container.innerHTML;
    expect(html).not.toMatch(/overflow-x-auto|md:hidden|md:block/);
    // The container's inline size is contained: the table cannot prop it wider than it is given.
    expect(container.firstElementChild).toHaveClass('[contain:inline-size]');
  });

  it('switches form when the container is resized', () => {
    let report: ((width: number) => void) | null = null;
    class ResizeObserverStub {
      constructor(private readonly cb: ResizeObserverCallback) {
        report = (width) =>
          this.cb(
            [{ contentRect: { width } } as ResizeObserverEntry],
            this as unknown as ResizeObserver
          );
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 608,
    } as DOMRect);
    const { rerender } = render(table());
    expect(screen.getByRole('table')).toBeInTheDocument();
    act(() => report!(390));
    rerender(table());
    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.getAllByTestId('phone-row')).toHaveLength(2);
  });

  // PA-02, PA-03: a part of the page that belongs to one form (the blocks legend, the scripts bar
  // switch) follows the form this list chose, from the same measurement.
  it('tells onForm the form it chose, and again when a resize changes it', () => {
    let report: ((width: number) => void) | null = null;
    class ResizeObserverStub {
      constructor(private readonly cb: ResizeObserverCallback) {
        report = (width) =>
          this.cb(
            [{ contentRect: { width } } as ResizeObserverEntry],
            this as unknown as ResizeObserver
          );
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 608,
    } as DOMRect);
    const onForm = vi.fn();
    render(table({ narrowColumns: NARROW, onForm }));
    expect(onForm).toHaveBeenLastCalledWith('table');
    act(() => report!(300));
    expect(onForm).toHaveBeenLastCalledWith('narrow-table');
    act(() => report!(200));
    expect(onForm).toHaveBeenLastCalledWith('rows');
    // once per form, not once per render
    expect(onForm.mock.calls.map(([form]) => form)).toEqual(['table', 'narrow-table', 'rows']);
  });

  it('keeps measuring when rows arrive after an empty first page', () => {
    stubChartWidth(358);
    const { rerender } = render(table({ rows: [] }));
    expect(screen.getByText('No records.')).toBeInTheDocument();
    rerender(table());
    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.getAllByTestId('phone-row')).toHaveLength(2);
  });

  it('requires a designed phone row: a list without mobileRow does not compile (R7)', () => {
    const props: ListTableProps<Row> = {
      label: 'Holdings',
      columns: COLUMNS,
      rows: ROWS,
      rowKey: (r) => r.name,
      mobileRow: (r) => r.name,
    };
    // @ts-expect-error mobileRow is required: a table without a designed phone row fails type-check.
    const missing: ListTableProps<Row> = { ...props, mobileRow: undefined };
    expect(missing.mobileRow).toBeUndefined();
    expectTypeOf<ListTableProps<Row>['mobileRow']>().toEqualTypeOf<
      (row: Row, wide: boolean) => ReactNode
    >();
  });
});

// Readability 2026-11-18, lane B5 (pJ): a long table's column heads scrolled away with its first
// rows, so from the second screen on fifty rows of figures had no names. The head stays in view
// under the site's sticky header and vitals strip.
describe('ListTable keeps its head in view (readability B5)', () => {
  const headsOf = (t: HTMLElement) => Array.from(t.querySelectorAll('thead th')) as HTMLElement[];
  /** The z-index the site header's sticky block takes (header.tsx: the md+ wrapper, the row). */
  const headerLayers = () => {
    const source = readFileSync(join(__dirname, '../../components/layout/header.tsx'), 'utf8');
    const layers = [
      ...source.matchAll(/md:sticky md:top-0 md:z-(\d+)/g),
      ...source.matchAll(/ sticky top-0 z-(\d+)/g),
    ].map((m) => Number(m[1]));
    expect(layers).toHaveLength(2);
    return layers;
  };
  const layerOf = (el: HTMLElement) => {
    const z = el.className.match(/(?:^|\s)z-(\d+)(?:\s|$)/);
    if (!z) throw new Error(`no z-index class on ${el.className}`);
    return Number(z[1]);
  };

  it('sticks every head right under the header, on the strip’s opaque ground, below the header', () => {
    stubChartWidth(608);
    render(table());
    const heads = headsOf(screen.getByRole('table', { name: 'Holdings' }));
    expect(heads).toHaveLength(2);
    for (const th of heads) {
      // the offset the header publishes (row + strip from md, the row alone below it)
      expect(th).toHaveClass('sticky', BELOW_STICKY_HEADER);
      // the vitals strip's own ground (opaque by palette.test): a row never shows through
      expect(th).toHaveClass('bg-[var(--m-header-strip)]');
      for (const header of headerLayers()) expect(layerOf(th)).toBeLessThan(header);
    }
  });

  it('draws the head’s rule with the head, so a stuck head keeps its rule over the rows', () => {
    // A collapsed table border is painted by the table and stays where the row was; the head's
    // own line, in the row-rule ink, moves with it.
    stubChartWidth(608);
    render(table());
    for (const th of headsOf(screen.getByRole('table', { name: 'Holdings' }))) {
      expect(th).toHaveClass('after:absolute', 'after:inset-x-0', 'after:bottom-0', 'after:h-px');
      expect(th).toHaveClass('after:bg-row-rule');
      expect(th).not.toHaveClass('border-b');
    }
  });

  it('sticks the narrower table’s head the same way, and the phone rows have none', () => {
    stubChartWidth(300);
    const { unmount } = render(table({ narrowColumns: NARROW }));
    for (const th of headsOf(screen.getByRole('table', { name: 'Holdings' })))
      expect(th).toHaveClass('sticky', BELOW_STICKY_HEADER, 'bg-[var(--m-header-strip)]');
    unmount();
    vi.restoreAllMocks();
    stubChartWidth(200);
    const { container } = render(table({ narrowColumns: NARROW }));
    expect(container.querySelector('th, thead, .sticky')).toBeNull();
  });

  it('sticks to the top of the caller’s own scroll box when the list scrolls inside one', () => {
    // Under the header inside a scroll box, the head would stand the header's height down into
    // the box at rest.
    stubChartWidth(608);
    render(table({ headSticksTo: 'box' }));
    for (const th of headsOf(screen.getByRole('table', { name: 'Holdings' }))) {
      expect(th).toHaveClass('sticky', 'top-0', 'bg-[var(--m-header-strip)]');
      expect(th).not.toHaveClass(BELOW_STICKY_HEADER);
    }
  });
});

describe('ListTable row scan (Midnight, continued, move 4)', () => {
  // A hovered table row gets the row-scan sweep (`.row-scan`: a faint jade band crossing once,
  // 0.5 s; globals.css paints it as the row's background so a <tr> can carry it). Column heads
  // and the phone rows do not sweep.
  it('puts row-scan on every body row of the table, not on the head or the phone rows', () => {
    stubChartWidth(608);
    const { unmount } = render(table());
    const t = screen.getByRole('table', { name: 'Holdings' });
    const body = Array.from(t.querySelectorAll('tbody tr'));
    expect(body).toHaveLength(2);
    // material pass v2 (move 6): a row rule is the row-rule ink; R5 (re-pinned on purpose) gives
    // it a key of its own, the hair by night, darker by day to clear the ruled paper
    for (const tr of body) expect(tr).toHaveClass('row-scan', 'border-b', 'border-row-rule');
    expect(t.querySelector('thead tr')).not.toHaveClass('row-scan');
    unmount();
    vi.restoreAllMocks();
    stubChartWidth(358);
    render(table());
    for (const item of screen.getAllByRole('listitem')) expect(item).not.toHaveClass('row-scan');
  });

  it('keeps a caller row class beside row-scan', () => {
    stubChartWidth(608);
    render(table({ rowClassName: (r) => (r.name === 'Coin' ? 'bg-jade/5' : '') }));
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows[1]).toHaveClass('row-scan', 'bg-jade/5');
    expect(rows[0]).toHaveClass('row-scan');
    expect(rows[0]).not.toHaveClass('bg-jade/5');
  });
});

describe('ListTable row density and row title (table-rows plan T2, lane R1)', () => {
  const cellsOf = (t: HTMLElement) => Array.from(t.querySelectorAll('tbody td')) as HTMLElement[];
  const headsOf = (t: HTMLElement) => Array.from(t.querySelectorAll('thead th')) as HTMLElement[];

  it('keeps the default cells: 12 px padding, top-aligned (the old contract, unchanged)', () => {
    stubChartWidth(608);
    render(table());
    const t = screen.getByRole('table', { name: 'Holdings' });
    for (const td of cellsOf(t)) {
      expect(td).toHaveClass('min-w-0', 'py-3', 'pr-4', 'align-top', 'last:pr-0');
      expect(td).not.toHaveClass('py-[11px]', 'align-middle');
    }
  });

  // Regression (the lead's Chrome check of F3-e, 2026-11-18): a browser gives every table cell 1 px
  // of padding on each side unless a class says otherwise. `py-*` and `pr-4` set three sides, so the
  // left side kept that pixel and every column's content box was 1 px narrower than its budget
  // (content + CELL_GAP_PX): in the 608 px blocks table the 92.4 px block number stood in 92 px and
  // broke after its tenth glyph. A cell's only horizontal padding is its gap.
  it('gives a cell and a head no left padding: the budget is the content and its gap', () => {
    for (const density of ['default', 'row'] as const) {
      stubChartWidth(608);
      const { unmount } = render(table({ density }));
      const t = screen.getByRole('table', { name: 'Holdings' });
      for (const cell of [...headsOf(t), ...cellsOf(t)]) expect(cell).toHaveClass('pl-0');
      unmount();
    }
  });

  it('draws one-line rows with density="row": 11 px padding, middle-aligned, heads unchanged', () => {
    stubChartWidth(608);
    render(table({ density: 'row' }));
    const t = screen.getByRole('table', { name: 'Holdings' });
    // 11 + one 18 px line + 11 = a 40 px row (T2).
    for (const td of cellsOf(t)) {
      expect(td).toHaveClass('min-w-0', 'py-[11px]', 'pr-4', 'align-middle', 'last:pr-0');
      expect(td).not.toHaveClass('py-3', 'align-top');
    }
    // Column kinds keep their own classes.
    expect(within(t).getByRole('cell', { name: '12.5' })).toHaveClass('text-right', 'tabular-nums');
    // th is the same in both densities.
    const plain = headsOf(t).map((th) => th.className);
    expect(plain.every((c) => c.includes('py-3'))).toBe(true);
  });

  // Table-rows T5 (coordinator ruling, lane M): column heads at meta (11 px), dim, in both
  // densities and both table forms; the 12 px padding is unchanged.
  it('sets every column head at 11 px dim (T5)', () => {
    stubChartWidth(608);
    const { unmount } = render(table());
    for (const th of headsOf(screen.getByRole('table', { name: 'Holdings' }))) {
      expect(th).toHaveClass('text-meta', 'text-text-dim', 'py-3', 'font-normal');
      expect(th).not.toHaveClass('text-small');
    }
    unmount();
    stubChartWidth(300);
    render(table({ narrowColumns: NARROW, density: 'row' }));
    for (const th of headsOf(screen.getByRole('table', { name: 'Holdings' })))
      expect(th).toHaveClass('text-meta', 'text-text-dim', 'py-3');
  });

  it('gives the narrower table the same density', () => {
    stubChartWidth(300);
    render(table({ narrowColumns: NARROW, density: 'row' }));
    const t = screen.getByRole('table', { name: 'Holdings' });
    for (const td of cellsOf(t)) expect(td).toHaveClass('py-[11px]', 'align-middle');
  });

  it('writes rowTitle to each <tr>, so fields that left the row stay on its hover', () => {
    stubChartWidth(608);
    render(table({ rowTitle: (r) => `${r.name} · ${r.amount} exact` }));
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows.map((r) => r.getAttribute('title'))).toEqual([
      'Token · 12.5 exact',
      'Coin · 7 exact',
    ]);
  });

  it('writes rowTitle to each phone listitem too', () => {
    stubChartWidth(358);
    render(table({ rowTitle: (r) => `${r.name} · ${r.amount} exact` }));
    const items = within(screen.getByRole('list', { name: 'Holdings, compact' })).getAllByRole(
      'listitem'
    );
    expect(items.map((i) => i.getAttribute('title'))).toEqual([
      'Token · 12.5 exact',
      'Coin · 7 exact',
    ]);
  });

  it('writes no title without rowTitle', () => {
    stubChartWidth(608);
    const { unmount } = render(table());
    for (const r of within(screen.getByRole('table')).getAllByRole('row'))
      expect(r).not.toHaveAttribute('title');
    unmount();
    vi.restoreAllMocks();
    stubChartWidth(358);
    render(table());
    for (const i of screen.getAllByRole('listitem')) expect(i).not.toHaveAttribute('title');
  });

  it('refuses an empty row title: a field moved to the hover must not vanish', () => {
    stubChartWidth(608);
    expect(() => render(table({ rowTitle: (r) => (r.name === 'Coin' ? ' ' : r.name) }))).toThrow(
      'Holdings: row Coin has an empty title'
    );
  });
});

// Review fb-tests 5 (the lead's ruling: ListTable owns every horizontal pixel of a cell). A
// column's budget is its content and CELL_GAP_PX; a padding, a margin, a width or a side border a
// column wrote on its cells would take pixels no budget counts (cn() lets a column's `px-2` replace
// the table's `pl-0` and `pr-4`), so the table refuses it when it is built.
describe('ListTable owns every horizontal pixel of a cell (review fb-tests 5)', () => {
  const named = (className: string): ListColumn<Row> => ({
    key: 'name',
    label: 'Name',
    minWidth: 180,
    className,
    render: (r) => r.name,
  });

  // Re-pinned on purpose (review fb-logic N2 with fb-tests SHOULD-3, the lead's ruling): the rule
  // was a list of what a column may NOT set, so a class it did not parse passed — a variant it did
  // not strip (`max-[600px]:px-2`, `[&_a]:ml-2`), a utility it did not list (`space-x-2`,
  // `indent-4`), a border value it did not read (`border-l-[var(--w)]`). It is a list of what a
  // column MAY set now (`cellClassOwner`): its cells' ink, size, vertical padding, white-space,
  // truncation and alignment, with no variant; anything else is refused by name (the message was
  // "sets its own horizontal space", which no longer names every refusal).
  const refused = (cls: string) => `Holdings: column name sets a class a column may not (${cls})`;
  it('refuses a column that sets anything but its cells’ ink, size, vertical padding, white-space, truncation or alignment, naming it', () => {
    for (const cls of [
      'px-2',
      'pl-1',
      'pr-3',
      'p-2',
      'ps-1',
      'pe-1',
      'ml-2',
      '-mr-1',
      'mx-auto',
      'w-24',
      'min-w-0',
      'min-w-[80px]',
      'max-w-xs',
      'border',
      'border-l',
      'border-x-2',
      'border-r-[3px]',
      '!pl-2',
      'md:px-4',
      'last:pr-2',
      '[padding-left:3px]',
      // what the list of refusals let through (review fb-logic N2)
      'max-[600px]:px-2',
      '*:px-2',
      '[&_a]:ml-2',
      'group-hover/row:px-2',
      'data-[x=y]:pl-2',
      'space-x-2',
      'indent-4',
      '-indent-[2ch]',
      'translate-x-1',
      'tracking-wide',
      'border-l-[length:3px]',
      'border-l-[var(--w)]',
      // no family a column's doc permits (moved here from the passing list on purpose: no column
      // sets them, and the list is what columns may set)
      'border-b',
      'border-t-2',
      'border-row-rule',
      'font-medium',
      'tabular-nums',
      // a permitted family under a variant: a column's classes have none
      'md:whitespace-nowrap',
      'hover:text-jade',
      // an arbitrary value or property
      'text-[13px]',
      '[white-space:nowrap]',
    ])
      expect(() => listTableForm('Holdings', 608, [named(`text-body ${cls}`), COLUMNS[1]])).toThrow(
        refused(cls)
      );
    // the narrower table's columns, and the sum a page computes at load, are held to it too
    expect(() => listTableForm('Holdings', 300, COLUMNS, [named('px-2'), NARROW[1]])).toThrow(
      refused('px-2')
    );
    expect(() => minimumWidth('Holdings', [named('pl-3')])).toThrow(refused('pl-3'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    stubChartWidth(608);
    expect(() => render(table({ columns: [named('px-2'), COLUMNS[1]] }))).toThrow(refused('px-2'));
  });

  // Every class a column of the site sets (the 18 files that build ListColumns, read 2026-11-19),
  // and each family the list permits.
  it('lets a column set its cells’ ink, size, vertical padding, white-space, truncation and alignment', () => {
    for (const cls of [
      'py-1.5',
      'py-[9px] align-top',
      'py-3 align-top',
      'text-body py-1.5',
      'text-body text-text-dim',
      'text-text-dim',
      'truncate',
      'whitespace-nowrap',
      'whitespace-nowrap truncate',
      'whitespace-normal',
      '!whitespace-normal',
      '!whitespace-normal text-text-dim',
      'py-[9px] truncate',
      'text-meta text-jade/80 text-right align-middle pt-1 pb-px',
    ])
      expect(listTableForm('Holdings', 608, [named(cls), COLUMNS[1]])).toBe('table');
    expect(cellClassOwner('py-3')).toBe('column');
    expect(cellClassOwner('sticky')).toBe('table');
    expect(cellClassOwner('pl-0')).toBe('table');
    expect(cellClassOwner('px-2')).toBeNull();
  });

  // The gap and the class that draws it are one fact: the class's step read from the Tailwind
  // config (the root's 16 px a rem: no rule in globals.css sets the root's font size) is the
  // number every budget adds. Moving either alone fails here.
  it('draws the gap with the class whose step is CELL_GAP_PX', () => {
    const { padding } = resolveConfig(tailwindConfig).theme as { padding: Record<string, string> };
    const step = /^pr-(.+)$/.exec(CELL_GAP_CLASS);
    expect(step).not.toBeNull();
    const rem = /^([\d.]+)rem$/.exec(padding[step![1]]);
    expect(rem).not.toBeNull();
    const css = readFileSync(join(__dirname, '..', '..', 'app', 'globals.css'), 'utf8');
    expect(css).not.toMatch(/(^|[\s,}])html\s*\{[^}]*font-size/);
    expect(Number(rem![1]) * 16).toBe(CELL_GAP_PX);
  });

  // Head and cell pinned alike, in both densities and both tables: their horizontal classes are
  // exactly the table's, no more and no fewer.
  it('gives every head and every cell exactly pl-0, the gap and last:pr-0', () => {
    expect(CELL_HORIZONTAL_CLASSES).toEqual(['pl-0', CELL_GAP_CLASS, 'last:pr-0']);
    const horizontal = (el: Element) =>
      Array.from(el.classList)
        .filter((c) =>
          /^(\w+:)*!?-?(p|px|pl|pr|ps|pe|m|mx|ml|mr|ms|me|w|max-w)-|^border-[xlrse]/.test(c)
        )
        .sort();
    for (const [width, props] of [
      [608, {}],
      [608, { density: 'row' }],
      [300, { narrowColumns: NARROW }],
      [300, { narrowColumns: NARROW, density: 'row' }],
    ] as const) {
      stubChartWidth(width);
      const { unmount } = render(table(props));
      const cells = Array.from(screen.getByRole('table').querySelectorAll('th, td'));
      expect(cells.length).toBeGreaterThan(4);
      for (const cell of cells) expect(horizontal(cell)).toEqual(['last:pr-0', 'pl-0', 'pr-4']);
      unmount();
      vi.restoreAllMocks();
    }
  });
});
