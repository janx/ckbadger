import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
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

/**
 * A horizontal 1 px line drawn in the hair or the elevated tint: a class string that holds one of
 * them as a border or divide colour and a top, bottom or y border (or a y divide). A frame (`border`
 * on every side) is not a rule and is not read.
 */
const HAIR_COLOUR =
  /(?<![\w-])(?:[\w-]+:)*(?:border|divide)-base-(?:border-subtle|elevated)(?![\w-])/;
const HORIZONTAL = /(?<![\w-])(?:[\w-]+:)*(?:border-[tby]|divide-y)(?![\w-])/;
const LITERAL = /(['"`])((?:(?!\1).)*)\1/g;

function hairRules(code: string): { line: number; what: string }[] {
  const out: { line: number; what: string }[] = [];
  code.split('\n').forEach((text, i) => {
    for (const m of text.matchAll(LITERAL))
      if (HAIR_COLOUR.test(m[2]) && HORIZONTAL.test(m[2]))
        out.push({ line: i + 1, what: m[2].match(HAIR_COLOUR)![0] });
  });
  return out;
}

/** The hair rules that are not row rules, per file in order: a header's or a strip's edge. */
const EDGE =
  'the edge of the page header or its vitals strip: a frame’s border, not a rule between rows';
const HAIR_EDGES: Record<string, readonly string[]> = {
  'components/layout/header.tsx': [EDGE, EDGE],
  'components/error-page-shell.tsx': [EDGE],
};

describe('a horizontal rule in the content is a row rule (R5, DY-01)', () => {
  it('finds a hair or elevated rule, never a frame or a row-rule', () => {
    const code = [
      '<div className="border-base-border-subtle flex border-b py-2">',
      "const r = 'divide-base-elevated divide-y'; const f = 'border-base-border-subtle rounded border';",
      '<li className="border-row-rule border-t" /><i className="border-base-elevated border-l" />',
    ].join('\n');
    expect(hairRules(code)).toEqual([
      { line: 1, what: 'border-base-border-subtle' },
      { line: 2, what: 'divide-base-elevated' },
    ]);
  });

  it('draws every rule between rows, under a head and above a closing block in the row-rule ink', () => {
    const root = join(__dirname, '..', '..');
    const files: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) walk(path);
        else if (/\.tsx?$/.test(name)) files.push(path);
      }
    };
    walk(join(root, 'app'));
    walk(join(root, 'components'));
    const short: string[] = [];
    for (const file of files) {
      const path = relative(root, file);
      const found = hairRules(readFileSync(file, 'utf8'));
      const edges = HAIR_EDGES[path] ?? [];
      found.slice(edges.length).forEach((r) => short.push(`${path}:${r.line} ${r.what}`));
      if (found.length < edges.length)
        short.push(`${path}: ${edges.length} edges listed, ${found.length} found`);
    }
    expect(short).toEqual([]);
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
