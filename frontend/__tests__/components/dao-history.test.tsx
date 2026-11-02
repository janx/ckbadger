import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import {
  DaoHistoryFigure,
  DaoHistoryReading,
  MAIN_PLOT_H,
  MULTIPLE_H,
  cutShare,
  historySeries,
  type HistoryRange,
} from '@/components/dao/dao-history';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import type { ChartResponse } from '@/lib/api';

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

/** The hero's wiring (dao-board): one pin for the three panels and the reading under them. */
function Picture({
  deposited,
  ratio,
  range = 'All',
}: {
  deposited: ChartResponse & { peak?: { date: string; value: string } | null };
  ratio: ChartResponse;
  range?: HistoryRange;
}) {
  const pin = usePinnedReadout<number>();
  const s = historySeries(deposited, ratio, range);
  return (
    <>
      <DaoHistoryFigure series={s} pin={pin} />
      <DaoHistoryReading series={s} pin={pin} peak={deposited.peak ?? null} />
    </>
  );
}

/** jsdom has no SVG layout: every svg reports `width` px from x = 0. */
function stubSvgWidth(width: number) {
  vi.spyOn(SVGElement.prototype, 'getBoundingClientRect').mockReturnValue({
    width,
    height: 0,
    top: 0,
    left: 0,
    right: width,
    bottom: 0,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
}

describe('DaoHistory picture (hierarchy v3, R13; DAO-11, DAO-18)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('draws deposited large with its name and values inside the plot, the share and the weeks as two 72 px multiples under it', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const panels = Array.from(container.querySelectorAll('svg[data-panel]'));
    expect(panels.map((p) => p.getAttribute('data-panel'))).toEqual([
      'deposited',
      'share',
      'weekly',
    ]);
    const [main, share, weekly] = panels;
    // R13: the main picture is at least 280 px tall; its time labels sit under the plot.
    expect(MAIN_PLOT_H).toBeGreaterThanOrEqual(280);
    expect(Number(main.getAttribute('height'))).toBeGreaterThanOrEqual(MAIN_PLOT_H);
    expect(share).toHaveAttribute('height', String(MULTIPLE_H));
    expect(weekly).toHaveAttribute('height', String(MULTIPLE_H));
    expect(MULTIPLE_H).toBe(72);
    // Names inside the plots, no caption, no separate heading.
    expect(main.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      `CKB deposited · every complete day since ${date(0)}`
    );
    expect(share.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      'share of circulating CKB'
    );
    expect(weekly.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      /^net change per week/
    );
    expect(container.querySelector('figcaption')).toBeNull();
    expect(screen.queryByRole('heading')).toBeNull();
    // The latest values at the multiples' ends: the share cut to 2 dp, the week in its sign colour.
    expect(share.querySelector('[data-role="end-label"]')).toHaveTextContent('11.54%');
    const weekEnd = weekly.querySelector('[data-role="end-label"]')!;
    expect(weekEnd).toHaveTextContent('+10.00');
    expect(weekEnd).toHaveClass('fill-positive');
    // The two multiples sit side by side from sm up and stack below it.
    expect(share.parentElement!.parentElement).toHaveClass('grid-cols-1', 'sm:grid-cols-2');
  });

  it('reads the last complete day in one row under the picture: deposited, share, depositors, the week and the peak, exact', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const reading = screen.getByRole('status', { name: 'History reading' });
    expect(reading).toHaveTextContent(
      `${date(154)} · deposited 101,190.00000000 · share 11.5400% · depositors 254 · week from ${date(148)} +10.00000000 · peak 101,190.00000000 on ${date(154)}tap or hover`
    );
    expect(reading).toHaveClass('border-t', 'text-small');
    // Evidence first: the reading follows every panel.
    for (const svg of Array.from(container.querySelectorAll('svg')))
      expect(svg.compareDocumentPosition(reading) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('moves the one reading with the pointer across all panels; a tap pins the day, a second tap lets go (R6)', () => {
    stubChartWidth(1200);
    stubSvgWidth(1200);
    stubPointerEvents();
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const share = container.querySelector('svg[data-panel="share"]')!;
    const reading = screen.getByRole('status', { name: 'History reading' });
    fireEvent.pointerMove(share, { clientX: 0 });
    expect(reading).toHaveTextContent(`${date(0)} · deposited 100,000.00000000`);
    // Before the first whole week there is no weekly figure to read.
    expect(reading).not.toHaveTextContent('week from');
    // One cursor in each of the three panels.
    expect(container.querySelectorAll('[data-cursor]')).toHaveLength(3);
    fireEvent.pointerLeave(share);
    expect(reading).toHaveTextContent(new RegExp(`^${date(154)} · deposited`));
    // A tap on the main plot pins the day it lands on; leaving keeps it.
    const main = container.querySelector('svg[data-panel="deposited"]')!;
    fireEvent.pointerMove(main, { clientX: 600 });
    fireEvent.click(main, { clientX: 600 });
    fireEvent.pointerLeave(main);
    expect(reading).toHaveTextContent(new RegExp(`^tapped ${date(77)} · deposited`));
    // "tapped" opens the same row of items, so it shares their line.
    const row = within(reading).getByText(date(77)).parentElement!;
    expect(row).toHaveClass('flex', 'flex-wrap');
    expect(row.firstElementChild).toHaveTextContent(/^tapped$/);
    fireEvent.click(main, { clientX: 600 });
    expect(reading).toHaveTextContent(new RegExp(`^${date(154)} · deposited`));
  });

  it('colours falling weeks red and rising weeks jade (DAO-18)', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const bars = Array.from(container.querySelectorAll('[data-week]'));
    expect(bars).toHaveLength(22);
    const bar = (start: string) => container.querySelector(`[data-week="${start}"]`)!;
    expect(bar(date(1 + 3 * 7))).toHaveClass('fill-negative');
    expect(bar(date(1))).toHaveClass('fill-positive');
  });

  it('clips weeks beyond p95 × 1.25 at the cap and names the scale inside the plot (DAO-11)', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const weekly = container.querySelector('[data-panel="weekly"]')!;
    // The cap is a round number above p95 × 1.25 (10 × 1.25 → 15); the note follows the name.
    expect(weekly.querySelector('[data-role="chart-name"]')!.textContent).toBe(
      'net change per week\u00a0·\u00a0±15 scale, 1 week cut'
    );
    const clipped = container.querySelectorAll('[data-week][data-clipped="true"]');
    expect(clipped).toHaveLength(1);
    expect(clipped[0]).toHaveAttribute('data-week', date(1 + 5 * 7));
    expect(container.querySelectorAll('[data-clip-mark]')).toHaveLength(1);
    // The largest week is named in the plot's accessible name, not as a label over the bars.
    expect(weekly).toHaveAccessibleName(
      `Net change in deposited CKB in each whole week from ${date(0)} to ${date(154)}; the largest was +1,000 CKB in the week of ${date(36)}`
    );
    expect(container.querySelector('[data-role="largest-week"]')).toBeNull();
  });

  it('shows the latest week at the weekly end cut to two decimals, never rounded, in its sign colour', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    // The last week (from day 148) starts at 101,180 CKB and ends at 101,167.65432109: −12.34567891.
    const data = deposited.data.map((p, i) => (i === 154 ? { ...p, value: '101167.65432109' } : p));
    const { container } = render(<Picture deposited={{ ...deposited, data }} ratio={ratio} />);
    const end = container.querySelector('[data-panel="weekly"] [data-role="end-label"]')!;
    expect(end).toHaveTextContent('−12.34');
    expect(end).toHaveClass('fill-negative');
    expect(screen.getByRole('status', { name: 'History reading' })).toHaveTextContent(
      'week from 2024-05-28 −12.34567891'
    );
  });

  it('refuses a deposit history with a missing day', () => {
    const { deposited, ratio } = series();
    const gap = { ...deposited, data: deposited.data.filter((_, i) => i !== 9) };
    expect(() => historySeries(gap, ratio, 'All')).toThrow(
      `DAO deposit history skips from ${date(8)} to ${date(10)}`
    );
  });

  it('ends a time label at the plot edge instead of running past it (1M)', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} range="1M" />);
    const ticks = Array.from(
      container.querySelectorAll('[data-panel="deposited"] [data-role="time-tick"]')
    );
    // 1M: a label every 7 days back from the last day, which sits on the right edge.
    const last = ticks[ticks.length - 1];
    expect(last).toHaveTextContent(date(154).slice(5));
    expect(last).toHaveAttribute('text-anchor', 'end');
    expect(Number(last.getAttribute('x'))).toBeLessThanOrEqual(1200);
    expect(ticks[0]).not.toHaveAttribute('text-anchor', 'end');
    // Each week's bar spans its first to its last day, so the newest ends inside the plot.
    for (const bar of Array.from(container.querySelectorAll('[data-week]')))
      expect(Number(bar.getAttribute('x')) + Number(bar.getAttribute('width'))).toBeLessThanOrEqual(
        1200
      );
  });

  it('draws only the days of the chosen range', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    expect(historySeries(deposited, ratio, '1M').days).toHaveLength(30);
    expect(historySeries(deposited, ratio, '1Y').days).toHaveLength(155);
    const { container } = render(<Picture deposited={deposited} ratio={ratio} range="1M" />);
    expect(
      container.querySelector('[data-panel="deposited"] [data-role="chart-name"]')
    ).toHaveTextContent(`since ${date(125)}`);
  });

  it('starts the deposited scale at a round top near the peak and writes its values inside the plot', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const ticks = Array.from(
      container.querySelectorAll('[data-panel="deposited"] [data-role="y-tick"]')
    );
    // 101,190 CKB at most: round values up to 150k; 0 is the baseline and 150k the name's band.
    expect(ticks.map((t) => t.textContent)).toEqual(['50k', '100k']);
    for (const t of ticks) {
      expect(t).toHaveAttribute('x', '8');
      expect(t).not.toHaveAttribute('text-anchor', 'end');
      expect(t).toHaveClass('fill-text-ghost');
    }
  });
});

describe('DaoHistory at phone width (DAO-22)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('puts the weekly scale note and the deposited name on a second line inside the plot', () => {
    stubChartWidth(358);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const note = container.querySelector('[data-panel="weekly"] [data-role="scale-note"]')!;
    expect(note.tagName.toLowerCase()).toBe('tspan');
    expect(note).toHaveTextContent('±15 scale, 1 week cut');
    expect(note).toHaveAttribute('x', '8');
    expect(note).toHaveAttribute('dy', '12');
    const name = container.querySelector('[data-panel="deposited"] [data-role="chart-name"]')!;
    expect(name.querySelector('tspan')).toHaveTextContent(`every complete day since ${date(0)}`);
  });

  it('keeps the note on the name line where both fit', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const note = container.querySelector('[data-panel="weekly"] [data-role="scale-note"]')!;
    expect(note).not.toHaveAttribute('dy');
    expect(
      container.querySelector('[data-panel="deposited"] [data-role="chart-name"] tspan')
    ).toBeNull();
  });
});

// Responsive fix plan 2026-11-01 (A3, report A#14): at 390 the reading broke the week's date
// ("2026-/10-25") and split an amount.
describe('DaoHistory reading units', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('never breaks the week date or an amount', () => {
    stubChartWidth(358);
    const { deposited, ratio } = series();
    render(<Picture deposited={deposited} ratio={ratio} />);
    const reading = screen.getByRole('status', { name: 'History reading' });
    expect(within(reading).getByText(date(148))).toHaveClass('whitespace-nowrap');
    const holders = within(reading).getAllByText((_, el) => el?.textContent === '+10.00000000');
    expect(holders[holders.length - 1]).toHaveClass('whitespace-nowrap');
  });
});

describe('cutShare', () => {
  it('cuts a share to two decimals without rounding', () => {
    expect(cutShare('16.3507')).toBe('16.35');
    expect(cutShare('16.3599')).toBe('16.35');
    expect(cutShare('1.25')).toBe('1.25');
    expect(cutShare('1,234.5')).toBe('1,234.5');
    expect(cutShare('7')).toBe('7');
    expect(() => cutShare('7%')).toThrow('is not a decimal');
  });
});
