import { afterEach, describe, expect, expectTypeOf, it, vi } from 'vitest';
import type { ReactNode } from 'react';
import { act, render, screen, within } from '@testing-library/react';
import {
  ListTable,
  WIDE_ROW_MIN_WIDTH,
  listTableForm,
  type ListColumn,
  type ListTableProps,
} from '@/components/ui/list-table';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

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
