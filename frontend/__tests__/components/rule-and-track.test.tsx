import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { ShareBar } from '@/components/ui/share-bar';
import { AlignedBar } from '@/components/ui/aligned-bar';
import { BlockPosition } from '@/components/blocks/block-position';
import { FREE_CAPACITY_COLOR } from '@/lib/design-tokens';
import { color } from '@/lib/palette';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';

// UI review 2026-11-14, ruling R5 (DY-01, DY-08): by day the page is ruled paper, on which the
// hair (`base-border-subtle`) and the elevated tint (`base-elevated`) are 1.09:1 and 1.03:1 against
// a ruled line — a row rule is one more ruled line and a bar's empty track is not there. A row rule
// is drawn in the row-rule ink and a track in the bar-track ink, the keys that clear the paper and
// a ruled line by 1.25:1 by day (palette.test). A tile is the surface under its own 1 px border.

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

type Row = { name: string; amount: string };
const COLUMNS: ListColumn<Row>[] = [
  { key: 'name', label: 'Name', minWidth: 180, render: (r) => r.name },
  { key: 'amount', label: 'Amount', minWidth: 240, numeric: true, render: (r) => r.amount },
];

function table() {
  return (
    <ListTable
      label="Holdings"
      columns={COLUMNS}
      rows={[
        { name: 'Token', amount: '12.5' },
        { name: 'Coin', amount: '7' },
      ]}
      rowKey={(r) => r.name}
      mobileRow={(r) => r.name}
    />
  );
}

describe('a row rule (R5, DY-01)', () => {
  it('rules the table’s rows and its head in the row-rule ink', () => {
    stubChartWidth(608);
    render(table());
    const t = screen.getByRole('table', { name: 'Holdings' });
    for (const tr of t.querySelectorAll('tbody tr'))
      expect(tr).toHaveClass('border-b', 'border-row-rule');
    for (const th of t.querySelectorAll('thead th'))
      expect(th).toHaveClass('border-b', 'border-row-rule');
    expect(t.innerHTML).not.toContain('border-base-border-subtle');
  });

  it('rules the phone rows in the row-rule ink', () => {
    stubChartWidth(358);
    render(table());
    for (const item of screen.getAllByRole('listitem'))
      expect(item).toHaveClass('border-b', 'border-row-rule');
  });
});

describe('a bar’s empty track (R5, DY-01)', () => {
  it('lays the share bar and the aligned bar on the bar-track ink', () => {
    const { container } = render(
      <>
        <ShareBar value={1} scale={10} color={color('jade')} track={80} />
        <AlignedBar
          label="Size"
          scale={1000}
          segments={[{ key: 'value', label: 'Size', value: 400, tone: 'data' }]}
        />
      </>
    );
    expect(container.querySelector('[data-share-track]')).toHaveClass('bg-bar-track');
    expect(container.querySelector('[data-track]')).toHaveClass('bg-bar-track');
    expect(container.innerHTML).not.toContain('bg-base-elevated');
  });

  it('draws a cell’s free capacity, the empty part of its bar, in the bar-track ink', () => {
    expect(FREE_CAPACITY_COLOR).toBe(color('bar-track'));
  });
});

describe('the neighbour tiles of “Position in the chain” (R5, DY-08)', () => {
  it('draws every block tile as the surface under its own 1 px border', () => {
    const { container } = render(
      <BlockPosition
        block={{
          ...blockEvidence,
          number: 932,
          hash: `0x${'c'.repeat(64)}`,
          parentHash: `0x${'d'.repeat(64)}`,
          timestamp: '2026-10-26T08:55:37Z',
          confirmations: 3,
          proposalsCount: 0,
          unclesCount: 0,
          uncles: [],
        }}
        transactions={[]}
        proposals={[]}
        page={1}
        hasMore={false}
      />
    );
    const tiles = [...container.querySelectorAll('a[aria-label^="Block #"] > rect')];
    expect(tiles.length).toBeGreaterThanOrEqual(3);
    for (const rect of tiles) {
      expect(rect).toHaveAttribute('stroke', color('slate-other'));
      // a 1 px line on the pixel grid: inset half a pixel, the tile's size kept
      expect(rect.getAttribute('stroke-width') ?? '1').toBe('1');
      expect(rect).toHaveAttribute('x', '0.5');
      expect(rect).toHaveAttribute('y', '0.5');
    }
  });
});
