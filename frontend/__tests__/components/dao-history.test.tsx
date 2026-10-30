import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { DaoHistory } from '@/components/dao/dao-history';

const DAY = 86_400_000;
const START = Date.UTC(2024, 0, 1);
const date = (i: number) => new Date(START + i * DAY).toISOString().slice(0, 10);

/**
 * 22 whole weeks + 1 day of deposits. Every week nets +10 CKB except week 3 (−10) and week 5
 * (+1,000, the outlier the p95 × 1.25 cap clips).
 */
function series() {
  const weekNet = (w: number) => (w === 3 ? -10 : w === 5 ? 1000 : 10);
  const values: bigint[] = [BigInt(100_000) * BigInt(1e8)];
  for (let w = 0; w < 22; w++)
    for (let d = 0; d < 7; d++)
      values.push(
        values[values.length - 1] + (d === 6 ? BigInt(weekNet(w)) * BigInt(1e8) : BigInt(0))
      );
  const ckb = (v: bigint) => `${v / BigInt(1e8)}`;
  return {
    deposited: {
      data: values.map((v, i) => ({ date: date(i), value: ckb(v), value2: String(100 + i) })),
      title: 'Total Deposit',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Current depositors',
      peak: { date: date(values.length - 1), value: ckb(values[values.length - 1]) },
    },
    ratio: {
      data: values.map((_, i) => ({ date: date(i), value: (10 + i / 100).toFixed(4) })),
      title: 'Ratio',
      yAxisLabel: '%',
    },
  };
}

describe('DaoHistory (DAO-11, DAO-18)', () => {
  afterEach(() => vi.restoreAllMocks());

  it('draws one figure with three panels on one time axis and one reading', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<DaoHistory deposited={deposited} ratio={ratio} />);
    const svgs = container.querySelectorAll('svg');
    expect(svgs).toHaveLength(1);
    expect(
      screen.getByRole('img', {
        name: /CKB deposited, its share of circulating CKB, and the net change each week/,
      })
    ).toBeInTheDocument();
    expect(container.querySelectorAll('[data-panel]')).toHaveLength(3);
    expect(screen.getByRole('heading', { name: 'History' })).toBeInTheDocument();
    const reading = screen.getByLabelText('History reading');
    expect(reading).toHaveTextContent(`${date(154)} · last full day`);
    expect(reading).toHaveTextContent('deposited101,190.00000000');
    expect(reading).toHaveTextContent('share of circulation11.5400%');
    // Weeks are named by the day they start (board: "week from"), the last one ends today.
    expect(reading).toHaveTextContent(`net change, week from ${date(148)}+10.00000000 CKB`);
  });

  it('moves the one reading with the pointer across all panels', () => {
    stubChartWidth(1200);
    stubPointerEvents();
    const { deposited, ratio } = series();
    const { container } = render(<DaoHistory deposited={deposited} ratio={ratio} />);
    const overlay = container.querySelector('[data-hover-overlay]')!;
    const plot = overlay.getBoundingClientRect();
    // getBoundingClientRect is stubbed to width 1200 at left 0 for every element.
    fireEvent.pointerMove(overlay, { clientX: plot.left + 0 });
    const reading = screen.getByLabelText('History reading');
    expect(reading).toHaveTextContent(`${date(0)}deposited100,000.00000000`);
    expect(reading).not.toHaveTextContent('last full day');
    // Before the first whole week there is no weekly figure to read.
    expect(reading).not.toHaveTextContent('net change');
    expect(container.querySelectorAll('[data-cursor]')).toHaveLength(1);
    fireEvent.pointerLeave(overlay);
    expect(reading).toHaveTextContent('last full day');
  });

  it('colours falling weeks red and rising weeks jade (DAO-18)', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<DaoHistory deposited={deposited} ratio={ratio} />);
    const bars = Array.from(container.querySelectorAll('[data-week]'));
    expect(bars).toHaveLength(22);
    const bar = (start: string) => container.querySelector(`[data-week="${start}"]`)!;
    expect(bar(date(1 + 3 * 7))).toHaveClass('fill-negative');
    expect(bar(date(1))).toHaveClass('fill-positive');
  });

  it('clips weeks beyond p95 × 1.25 at the cap and marks them (DAO-11)', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<DaoHistory deposited={deposited} ratio={ratio} />);
    const weekly = container.querySelector('[data-panel="weekly"]')!;
    expect(weekly).toHaveTextContent('1 week beyond the scale is cut and marked');
    // Inline after the title; no-break spaces, since SVG drops a text node's leading space.
    expect(container.querySelector('[data-role="clip-note"]')!.textContent).toBe(
      '\u00a0·\u00a01 week beyond the scale is cut and marked'
    );
    const clipped = container.querySelectorAll('[data-week][data-clipped="true"]');
    expect(clipped).toHaveLength(1);
    expect(clipped[0]).toHaveAttribute('data-week', date(1 + 5 * 7));
    expect(container.querySelectorAll('[data-clip-mark]')).toHaveLength(1);
    // The cap is a round number above p95 × 1.25 (10 × 1.25 → 15), printed on the axis.
    expect(within(weekly as HTMLElement).getByText('+15')).toBeInTheDocument();
    expect(weekly).toHaveTextContent(`+1,000 · week of ${date(36)}`);
  });

  it('refuses a deposit history with a missing day', () => {
    const { deposited, ratio } = series();
    const gap = { ...deposited, data: deposited.data.filter((_, i) => i !== 9) };
    expect(() => render(<DaoHistory deposited={gap} ratio={ratio} />)).toThrow(
      `DAO deposit history skips from ${date(8)} to ${date(10)}`
    );
  });
});

describe('DaoHistory at phone width (DAO-22)', () => {
  afterEach(() => vi.restoreAllMocks());

  it('puts the clipping note on its own line and keeps the largest-week label inside the plot', () => {
    stubChartWidth(358);
    const { deposited, ratio } = series();
    // Move the outlier into the last week, next to the right edge.
    const values = deposited.data.map((p, i) => ({
      ...p,
      value: i === 154 ? String(Number(p.value) + 5000) : p.value,
    }));
    const { container } = render(
      <DaoHistory deposited={{ ...deposited, data: values }} ratio={ratio} />
    );
    const note = container.querySelector('[data-role="clip-note"]')!;
    expect(note.tagName.toLowerCase()).toBe('text');
    expect(note).toHaveTextContent('1 week beyond the scale is cut and marked');
    const label = container.querySelector('[data-role="largest-week"]')!;
    expect(label).toHaveAttribute('text-anchor', 'end');
    expect(Number(label.getAttribute('x'))).toBeLessThanOrEqual(358 - 8);
  });

  it('keeps a long largest-week label clear of the axis when it fits on neither side', () => {
    stubChartWidth(358);
    const { deposited, ratio } = series();
    // +1,000,000,000 CKB in week 10: the label is ~35 characters, wider than either side of its bar.
    const jump = BigInt(1_000_000_000) * BigInt(1e8);
    const values = deposited.data.map((p, i) => ({
      ...p,
      value: i >= 77 ? String((BigInt(p.value) * BigInt(1e8) + jump) / BigInt(1e8)) : p.value,
    }));
    const { container } = render(
      <DaoHistory deposited={{ ...deposited, data: values }} ratio={ratio} />
    );
    const label = container.querySelector('[data-role="largest-week"]')!;
    expect(label.textContent).toMatch(/^\+1,000,000,010 · week of /);
    const x = Number(label.getAttribute('x'));
    expect(label).toHaveAttribute('text-anchor', 'end');
    expect(x).toBeLessThanOrEqual(358 - 8);
    // ends at the plot edge, so it starts right of the 64 px axis column
    expect(x - label.textContent!.length * 6.3).toBeGreaterThanOrEqual(64);
  });

  it('starts the deposited axis at a round top near the peak, not twice it', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<DaoHistory deposited={deposited} ratio={ratio} />);
    const ticks = Array.from(
      container.querySelectorAll('[data-panel="deposited"] text[text-anchor="end"]')
    ).map((t) => t.textContent);
    // 101,190 CKB at most: four round ticks up to 150k, not 0 / 100k / 200k.
    expect(ticks).toEqual(['0', '50k', '100k', '150k']);
  });
});
