import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart } from '@/components/ui/chart';
import { LineChart } from '@/components/ui/line-chart';
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';
import { Hash } from '@/components/ui/hash';
import { TxStructure } from '@/components/transaction/tx-structure';
import type { CellTileData } from '@/components/ui/cell-tile';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';

// Midnight, continued (review 2026-11-03, item 5): five moves, each one rule on a shared piece.
// Where a move is CSS only, the test pins the rule in globals.css and the class a component
// renders for it; where a move changes markup, the markup.

const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');

describe('the rules in globals.css', () => {
  it('move 1a: the section mark before every tier-1 h2, and `.section-mark` for a frame', () => {
    expect(css).toMatch(/\.section-mark,\s*h2\.text-section::before\s*\{/);
    expect(css).toMatch(/h2\.text-section::before\s*\{\s*content: '';\s*margin-right: 10px;/);
    // a tier-2 h2 is `text-body`: no rule for it
    expect(css).not.toMatch(/h2\.text-body::before/);
  });

  it('move 1b: one lit figure per page — a halo, never a colour', () => {
    const lit = css.match(
      /\.text-display,\s*\.sm\\:text-display,\s*h1\.text-entity,\s*h1\.text-index\s*\{([^}]*)\}/
    );
    expect(lit).not.toBeNull();
    expect(lit![1]).toContain('text-shadow');
    expect(lit![1]).toContain('0 0 14px rgb(var(--rgb-jade) / calc(0.28 * var(--m-glow)))');
    expect(lit![1]).not.toMatch(/\bcolor:/);
    // the figure's quieter companions (decimals, unit) and a ghost zero are not lit
    expect(css).toMatch(
      /\.text-text-ghost:is\(\.text-display, \.sm\\:text-display\)\s*\{\s*text-shadow: none;/
    );
  });

  it('move 1c: a hovered link turns jade and glows; SVG links keep their own hover', () => {
    expect(css).toMatch(
      /\na:hover\s*\{\s*color: var\(--c-jade\);\s*text-shadow: 0 0 8px rgb\(var\(--rgb-jade\) \/ calc\(0\.45 \* var\(--m-glow\)\)\);\s*\}/
    );
    expect(css).toMatch(/svg a:hover\s*\{\s*color: inherit;\s*text-shadow: none;/);
    expect(css).toMatch(/\.hover-lit:hover\s*\{\s*color: var\(--c-jade\);\s*text-shadow: 0 0 8px/);
  });

  // Rubbing × Song print, phase 2: the three tubes are set flat — a plain 1 px line in the line
  // ink, no jade, no gradient; the class names (and so the markup) stay.
  it('move 2: the three hairlines, a plain 1 px line', () => {
    expect(css).toMatch(
      /\.hairline-tube\s*\{\s*height: 1px;\s*background: var\(--c-base-border\);\s*\}/
    );
    expect(css).toMatch(/\.hairline-tube-border\s*\{\s*border-color: var\(--c-base-border\);\s*\}/);
    expect(css).toMatch(/\.hairline-tube-header\s*\{\s*border-color: var\(--c-base-border\);\s*\}/);
    for (const rule of css.match(/\.hairline-tube[\w-]*\s*\{[^}]*\}/g)!) {
      expect(rule).not.toMatch(/border-image|gradient|--rgb-jade/);
    }
  });

  it('move 3: the frame edge at .45', () => {
    expect(css).toMatch(/var\(--neon-color, rgb\(var\(--rgb-jade\) \/ 0\.45\)\)/);
  });

  it('move 4: the block cursor and a row-scan a <tr> can carry; reduced motion stills both', () => {
    expect(css).toMatch(
      /\.cursor\s*\{\s*display: inline-block;\s*width: 7px;\s*height: 14px;\s*background: var\(--c-jade\);[^}]*box-shadow: 0 0 6px rgb\(var\(--rgb-jade\) \/ calc\(0\.6 \* var\(--m-glow\)\)\);/
    );
    // the sweep is the row's own background (no positioned pseudo-element: a table row clips
    // nothing), crossing once in 0.5 s
    const scan = css.match(/\.row-scan\s*\{([^}]*)\}/)!;
    expect(scan[1]).toContain('background-image: linear-gradient(');
    expect(scan[1]).toContain('rgb(var(--rgb-jade) / 0.07) 50%');
    expect(scan[1]).toContain('background-size: 300% 100%');
    expect(scan[1]).not.toMatch(/\n\s*position:/);
    expect(css).toMatch(
      /\.row-scan:hover\s*\{\s*animation: row-scan-sweep 0\.5s ease-out forwards;/
    );
    expect(css).not.toMatch(/\.row-scan::after/);
    // one reduced-motion block stills the sweep, the cursor and the live mark together
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\)\s*\{\s*\.row-scan:hover\s*\{\s*animation: none;\s*\}\s*\.cursor,\s*\.section-mark-live\s*\{\s*animation: none;/
    );
  });

  it('move 5: brighter pools, one CRT pair at the combined strength, the chart halo', () => {
    // material pass v2 (move 1): the pools are painted from --pool-1/2/3 (so a page's class hue
    // can replace jade); the approved .10 / .07 / .07 are the variables' :root values
    expect(css).toContain('--pool-1: rgb(var(--rgb-jade) / 0.1);');
    expect(css).toContain('--pool-2: rgb(var(--rgb-aqua) / 0.07);');
    expect(css).toContain('--pool-3: rgb(var(--rgb-gold) / 0.07);');
    expect(css).toContain('var(--pool-1) 0%, transparent 70%');
    // the doubled atmosphere is gone: no body::before / body::after
    expect(css).not.toMatch(/\nbody::after\s*\{/);
    expect(css).not.toMatch(/\nbody::before\s*\{/);
    // visual finish V2 (2026-11-04): 1 px in 3, blue-black at 6 %
    expect(css).toMatch(
      /\.crt-scanlines\s*\{[^}]*var\(--m-scanline-ink\) 2px,\s*var\(--m-scanline-ink\) 3px/
    );
    // visual finish V1 (2026-11-04): the vignette moved under the content, lighter and wider
    expect(css).toMatch(
      /\.crt-vignette\s*\{[^}]*radial-gradient\(ellipse at center, transparent 45%, var\(--m-vignette\) 100%\)/
    );
    expect(css).toMatch(
      /\.chart-line-halo\s*\{\s*filter: drop-shadow\(0 0 2\.5px var\(--halo-color, rgb\(var\(--rgb-jade\) \/ 0\.55\)\)\);/
    );
  });
});

describe('move 1c on a hash', () => {
  it('a copyable hash carries the hover glow; a plain one does not', () => {
    const hash = `0x${'ab'.repeat(32)}`;
    const { rerender } = render(<Hash hash={hash} />);
    expect(screen.getByText(/0xabababab/)).toHaveClass('hover-lit', 'cursor-pointer');
    rerender(<Hash hash={hash} copyable={false} />);
    expect(screen.getByText(/0xabababab/)).not.toHaveClass('hover-lit');
  });
});

describe('move 5 on the chart lines', () => {
  beforeEach(() => stubChartWidth(390));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const data = [
    { label: 'Oct 24', values: { a: '2.5', b: '1' } },
    { label: 'Oct 25', values: { a: '3.0', b: '2' } },
  ];
  const lines = (container: HTMLElement) =>
    Array.from(container.querySelectorAll('path[stroke-width="1.5"][fill="none"]'));

  it('halos the first line of a line chart in jade, and no other line', () => {
    const { container } = render(
      <Chart
        title="Hash rate"
        subtitle="Daily"
        data={data}
        series={[
          { key: 'a', label: 'A' },
          { key: 'b', label: 'B' },
        ]}
        unit="PH/s"
        formatValue={(v) => v}
      />
    );
    const [first, second] = lines(container);
    expect(first).toHaveClass('chart-line-halo');
    // the default ramp's first shade is jade: the halo's own colour, no override
    expect(first).toHaveAttribute('stroke', 'var(--c-jade)');
    expect((first as HTMLElement).style.getPropertyValue('--halo-color')).toBe('');
    expect(second).not.toHaveClass('chart-line-halo');
  });

  it('halos a non-jade primary line in its own colour', () => {
    const { container } = render(
      <Chart
        title="Deposited"
        subtitle="Daily"
        data={data}
        series={[{ key: 'a', label: 'A', color: 'var(--c-gold)' }]}
        unit="CKB"
        formatValue={(v) => v}
      />
    );
    const [line] = lines(container);
    expect(line).toHaveClass('chart-line-halo');
    expect((line as HTMLElement).style.getPropertyValue('--halo-color')).toBe(
      'color-mix(in srgb, var(--c-gold) 55%, transparent)'
    );
  });

  it('leaves a dashed companion, a projection, areas and bars bare', () => {
    const { container, rerender } = render(
      <Chart
        title="Rate"
        subtitle="Daily"
        data={data}
        series={[{ key: 'a', label: 'A', dashed: true }]}
        unit="%"
        formatValue={(v) => v}
      />
    );
    expect(lines(container)[0]).not.toHaveClass('chart-line-halo');
    rerender(
      <Chart
        title="Rate"
        subtitle="Daily"
        data={data}
        series={[{ key: 'a', label: 'A' }]}
        unit="%"
        projectedAfter={0}
        formatValue={(v) => v}
      />
    );
    // the observed part is the data line; the model's dashed projection is not
    const observed = container.querySelector('path.chart-line-halo');
    expect(observed).not.toBeNull();
    expect(observed).not.toHaveAttribute('data-projected');
    expect(container.querySelector('path[data-projected]')).not.toHaveClass('chart-line-halo');
    for (const kind of ['area', 'stacked', 'bars'] as const) {
      rerender(
        <Chart
          title="Rate"
          subtitle="Daily"
          data={data}
          series={[{ key: 'a', label: 'A' }]}
          unit="%"
          kind={kind}
          formatValue={(v) => v}
        />
      );
      expect(container.querySelector('.chart-line-halo')).toBeNull();
    }
  });

  it('halos LineChart’s primary path and MultiSeriesLineChart’s first series only', () => {
    const { container, unmount } = render(
      <LineChart
        yAxisLabel="Net Flow"
        data={[
          { date: 'a', value: '1' },
          { date: 'b', value: '2' },
        ]}
      />
    );
    const primary = screen.getByTestId('line-series-primary');
    expect(primary).toHaveClass('chart-line-halo');
    expect(primary.style.getPropertyValue('--halo-color')).toBe(
      `color-mix(in srgb, ${CHART_PRIMARY_COLOR} 55%, transparent)`
    );
    expect(container.querySelector('rect.chart-line-halo')).toBeNull();
    unmount();
    const multi = render(
      <MultiSeriesLineChart
        data={[
          { date: 'a', values: { holders: '1000', volume: '50' } },
          { date: 'b', values: { holders: '1100', volume: '60' } },
        ]}
        series={[
          { key: 'holders', label: 'Holders', color: '#00c389' },
          { key: 'volume', label: 'Volume', color: '#ffb000' },
        ]}
      />
    );
    const paths = Array.from(multi.container.querySelectorAll('path[stroke-width="2"]'));
    expect(paths).toHaveLength(2);
    expect(paths[0]).toHaveClass('chart-line-halo');
    expect((paths[0] as HTMLElement).style.getPropertyValue('--halo-color')).toBe(
      'color-mix(in srgb, #00c389 55%, transparent)'
    );
    expect(paths[1]).not.toHaveClass('chart-line-halo');
  });
});

describe('move 3 on the tx page’s cells diagram', () => {
  const tile = (id: string, state: string): CellTileData => ({
    id,
    href: `/cell/0xabc:${id}`,
    label: id,
    owner: 'Alice',
    lockKind: 'JoyID',
    kind: 'ckb',
    content: 'CKB',
    capacity: '106200000000',
    occupied: '6100000000',
    state,
  });
  it('sits in the one instrument frame; the prose under it stays flat', () => {
    const { container } = render(
      <TxStructure
        txHash="0xtx"
        inputs={[tile('in:0', 'consumed')]}
        outputs={[tile('out:0', 'live')]}
        fee="1000"
        poolCapacity="106200000000"
      />
    );
    const frame = screen.getByTestId('tx-cells-frame');
    expect(frame).toHaveAttribute('data-instrument-frame');
    expect(frame).toHaveAttribute('aria-label', 'Cells consumed and created');
    // material pass v2 (move 2): the frame's surface is glass (`.glass`), not the flat colour
    expect(frame).toHaveClass('neon-edge-top', 'rounded-lg', 'border', 'glass');
    expect(frame).toContainElement(container.querySelector('[data-tx-canvas]') as HTMLElement);
    // the column heads are the frame's eyebrows
    expect(frame).toHaveTextContent('1 cell consumed');
    expect(frame).toHaveTextContent('1 cell created');
    const pool = screen.getByText(/Capacity entering the pool/);
    expect(frame).not.toContainElement(pool);
  });
});
