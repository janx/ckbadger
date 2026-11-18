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
import { CLASS_COLORS } from '@/lib/design-tokens';
import { expectTicksInGutter } from '@/__tests__/utils/y-ticks';
import {
  allTextLines,
  expectApart,
  expectInside,
  textLines,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';
import { factRows, factsOf } from '@/__tests__/utils/facts';

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

  it('draws deposited large with its name and values inside the plot, the share and the weeks as two 73 px multiples under it', () => {
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
    // re-pinned (readability F3): a multiple is its band and 54 px of data; the band is its top
    // row, the 14 px end value's tight line (17) and 1 px over and under it: 19 + 54 = 73 (was
    // 18 + 54 = 72, where the end value reached 0.2 px under the band)
    expect(MULTIPLE_H).toBe(73);
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
    // the summary form (readability R5, re-pinned on purpose: was "+10.00", padded to two places)
    expect(weekEnd).toHaveTextContent(/^\+10$/);
    expect(weekEnd).toHaveClass('fill-positive');
    // The two multiples sit side by side from sm up and stack below it.
    expect(share.parentElement!.parentElement).toHaveClass('grid-cols-1', 'sm:grid-cols-2');
  });

  // Visual finish V13 (plan 2026-11-04): the gold area is light that fades to the baseline, not a
  // flat paint, and the grid is solid — a dash means a reference, never the grid.
  it('fills deposited with gold fading from .28 to 0 at the baseline, and draws the grids solid', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const main = container.querySelector('svg[data-panel="deposited"]')!;
    const areas = [...main.querySelectorAll('path[fill^="url(#"]')];
    expect(areas).toHaveLength(1);
    const [area] = areas;
    expect(area).not.toHaveClass('fill-gold');
    expect(area).not.toHaveAttribute('fill-opacity');
    const gradient = main.querySelector(
      `linearGradient[id="${area.getAttribute('fill')!.slice(5, -1)}"]`
    )!;
    expect(gradient).toHaveAttribute('x1', '0');
    expect(gradient).toHaveAttribute('y1', '0');
    expect(gradient).toHaveAttribute('x2', '0');
    expect(gradient).toHaveAttribute('y2', '1');
    expect(
      [...gradient.querySelectorAll('stop')].map((stop) => [
        stop.getAttribute('offset'),
        stop.getAttribute('stop-color'),
        stop.getAttribute('stop-opacity'),
      ])
    ).toEqual([
      // the DAO gold of the line over it (stroke-gold), from the design tokens
      ['0', CLASS_COLORS.dao, '0.28'],
      ['1', CLASS_COLORS.dao, '0'],
    ]);
    // the gold line over it is unchanged
    expect(main.querySelector('path.stroke-gold')).toHaveAttribute('fill', 'none');
    // the grid of deposited and the share multiple's half-scale line: solid hairlines
    const share = container.querySelector('svg[data-panel="share"]')!;
    for (const panel of [main, share]) {
      // in the row-rule ink (R5, ruling 4c; re-pinned on purpose: it was the hair)
      const grid = [...panel.querySelectorAll('line.stroke-row-rule')];
      expect(grid.length).toBeGreaterThan(0);
      for (const line of grid) expect(line).not.toHaveAttribute('stroke-dasharray');
    }
  });

  // Readability R5 (re-pinned and renamed on purpose: "..., the week and the peak, exact"): a
  // reading is the summary form, each cut figure titled with its exact value.
  it('reads the last complete day in one row under the picture: deposited, share, depositors, the week and the peak', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const reading = screen.getByRole('status', { name: 'History reading' });
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(reading)).toEqual([
      ['day', date(154)],
      ['deposited', '101,190'],
      ['share', '11.5400%'],
      ['depositors', '254'],
      ['week from', `${date(148)} +10`],
      ['peak', `101,190 on ${date(154)}`],
    ]);
    expect(reading).toHaveTextContent(/tap or hover$/);
    expect(reading).toHaveClass('border-t', 'text-small');
    // Evidence first: the reading follows every panel.
    for (const svg of Array.from(container.querySelectorAll('svg')))
      expect(svg.compareDocumentPosition(reading) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // UI review 2026-11-14 (DY-06, ruling R4) set the reading's fraction digits in the quieter ink
  // of the amount's sign, never the amount's ink at 70 %. Readability R5 (re-pinned and renamed on
  // purpose: "sets the reading’s fraction digits in the sign’s quieter ink, ..."): one ink per
  // amount, the summary form, a cut figure titled with its exact value; still no opacity.
  it('sets each reading amount in one ink, cut and titled, never at an opacity', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const last = deposited.data.length - 1;
    deposited.data[last] = { ...deposited.data[last], value: '101190.25' };
    deposited.peak = { date: date(last), value: '101190.25' };
    render(<Picture deposited={deposited} ratio={ratio} />);
    const reading = screen.getByRole('status', { name: 'History reading' });
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(reading)).toContainEqual(['deposited', '101,190']);
    expect(factsOf(reading)).toContainEqual(['week from', `${date(148)} +10.25`]);
    const amounts = [...reading.querySelectorAll('[data-ckb-amount]')];
    expect(amounts.map((a) => a.textContent)).toEqual(['101,190', '+10.25', '101,190']);
    expect(amounts.map((a) => a.getAttribute('title'))).toEqual([
      '101,190.25 CKB',
      null,
      '101,190.25 CKB',
    ]);
    // deposited and the peak take the reading line's ink, the week's net its sign's
    expect(amounts[1]).toHaveClass('text-positive');
    for (const a of amounts) {
      expect(a.querySelector('[data-fraction]')).toBeNull();
      expect(a.querySelector('.text-text-dim, .text-jade-dim')).toBeNull();
    }
    expect(reading.innerHTML).not.toMatch(/opacity-/);
  });

  // The rules branch's review, logic NOTE 6: hover never moves layout (ReadoutRow). Before the
  // first whole week of a range the reading has no "week from" fact; its row keeps its place,
  // unseen, so the reading is as tall as its fullest form below md and from md.
  it('stands as tall with and without its week fact, below md and from md', () => {
    stubChartWidth(1200);
    stubSvgWidth(1200);
    stubPointerEvents();
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const share = container.querySelector('svg[data-panel="share"]')!;
    const reading = screen.getByRole('status', { name: 'History reading' });
    const full = factRows(reading);
    expect(factsOf(reading).map(([label]) => label)).toContain('week from');
    fireEvent.pointerMove(share, { clientX: 0 });
    expect(factsOf(reading).map(([label]) => label)).not.toContain('week from');
    expect(factRows(reading)).toEqual(full);
    // the row that keeps the place says nothing and is not read out
    const reserved = reading.querySelectorAll('dl > [aria-hidden="true"]');
    expect(reserved).toHaveLength(1);
    expect(reserved[0]).toHaveClass('invisible');
    expect(reserved[0].textContent!.trim()).toBe('');
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
    // the summary form (R5, re-pinned on purpose: was "100,000.00000000")
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(reading).slice(0, 2)).toEqual([
      ['day', date(0)],
      ['deposited', '100,000'],
    ]);
    // Before the first whole week there is no weekly figure to read.
    expect(reading).not.toHaveTextContent('week from');
    // One cursor in each of the three panels.
    expect(container.querySelectorAll('[data-cursor]')).toHaveLength(3);
    fireEvent.pointerLeave(share);
    expect(factsOf(reading)[0]).toEqual(['day', date(154)]);
    // A tap on the main plot pins the day it lands on; leaving keeps it.
    const main = container.querySelector('svg[data-panel="deposited"]')!;
    // Re-pinned with U-19: the plot starts after its 48 px value gutter, so its middle day (77 of
    // 154) is under x = 48 + 1,152 / 2 = 624, no longer 600.
    fireEvent.pointerMove(main, { clientX: 624 });
    fireEvent.click(main, { clientX: 624 });
    fireEvent.pointerLeave(main);
    // re-pinned with readability R4 (lane B7): the reading is labelled facts; "selected" opens the
    // day's value, in the quiet ink, so it shares the date's line (it opened the row of items)
    expect(factsOf(reading)[0]).toEqual(['day', `selected ${date(77)}`]);
    const day = within(reading).getByText(date(77)).closest('dd') as HTMLElement;
    expect(day.firstElementChild).toHaveTextContent(/^selected$/);
    expect(day.firstElementChild).toHaveClass('text-text-dim');
    fireEvent.click(main, { clientX: 624 });
    expect(factsOf(reading)[0]).toEqual(['day', date(154)]);
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
      // the review's item C (re-pinned): the count through formatCount, a no-break space
      'net change per week\u00a0·\u00a0±15 scale, 1\u00a0week cut'
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

  // Readability R5 (re-pinned and renamed on purpose: "... cut to two decimals, never rounded"):
  // the end label is the summary form, which cuts, never rounds, and titles its exact value.
  it('shows the latest week at the weekly end in its summary form, never rounded, in its sign colour', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    // The last week (from day 148) starts at 101,180 CKB and ends at 101,167.65432109: −12.34567891.
    const data = deposited.data.map((p, i) => (i === 154 ? { ...p, value: '101167.65432109' } : p));
    const { container } = render(<Picture deposited={{ ...deposited, data }} ratio={ratio} />);
    const end = container.querySelector('[data-panel="weekly"] [data-role="end-label"]')!;
    expect(end).toHaveTextContent(/^−12\.34$/);
    expect(end).toHaveClass('fill-negative');
    expect(end.parentElement!.querySelector('title')).toHaveTextContent('−12.34567891 CKB');
    // the reading's summary form too (R5, re-pinned on purpose: was "−12.34567891")
    // re-pinned with readability R4 (lane B7): the reading is labelled facts, the detail pages' dl rows (was one line of facts joined by "·")
    expect(factsOf(screen.getByRole('status', { name: 'History reading' }))).toContainEqual([
      'week from',
      '2024-05-28 −12.34',
    ]);
  });

  // Lane E2 (ruling R4) drew the end label's digits after the point in the sign's fraction ink as a
  // fill at full strength, never the amount's ink at fill-opacity .7 (the cinnabar at 2.91–3.16:1
  // by night, the celadon at 2.79–3.12:1 by day). Readability R5 (re-pinned and renamed on purpose:
  // "draws the weekly end label’s fraction in its sign’s fraction ink, ..."): one ink per amount,
  // so the label is one text in its sign's fill, no tspan, still never at a fill opacity; a cut
  // label carries its exact value in a <title>.
  it('draws the weekly end label in its sign’s one fill, never at a fill opacity', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const endOf = (value: string) => {
      const data = deposited.data.map((p, i) => (i === 154 ? { ...p, value } : p));
      const { container, unmount } = render(
        <Picture deposited={{ ...deposited, data }} ratio={ratio} />
      );
      const end = container.querySelector('[data-panel="weekly"] [data-role="end-label"]')!;
      const label = {
        text: end.textContent,
        fill: end
          .getAttribute('class')!
          .split(' ')
          .find((c) => c.startsWith('fill-')),
        fillOpacity: end.getAttribute('fill-opacity'),
        tspans: end.querySelectorAll('tspan').length,
        title: end.parentElement!.querySelector('title')?.textContent ?? null,
      };
      unmount();
      return label;
    };
    // the last week starts at 101,180 CKB: −12.34567891, then +10.25
    expect(endOf('101167.65432109')).toEqual({
      text: '−12.34',
      fill: 'fill-negative',
      fillOpacity: null,
      tspans: 0,
      title: '−12.34567891 CKB',
    });
    expect(endOf('101190.25')).toEqual({
      text: '+10.25',
      fill: 'fill-positive',
      fillOpacity: null,
      tspans: 0,
      title: null,
    });
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

  // Re-pinned with U-19 (UI review 2026-11-14): the values sit in a dim gutter left of the plot
  // (48 px, the charts' least), anchored at their end, no longer ghost values inside it.
  it('starts the deposited scale at a round top near the peak and writes its values in a gutter', () => {
    stubChartWidth(1200);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const ticks = Array.from(
      container.querySelectorAll('[data-panel="deposited"] [data-role="y-tick"]')
    );
    // 101,190 CKB at most: round values up to 150k; 0 is the baseline and 150k the name's band.
    expect(ticks.map((t) => t.textContent)).toEqual(['50k', '100k']);
    for (const t of ticks) {
      expect(t).toHaveAttribute('x', '40');
      expect(t).toHaveAttribute('text-anchor', 'end');
      expect(t).toHaveClass('fill-text-dim');
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
    // Re-pinned on purpose (readability R5): the end label is the summary form, and "+10" (was
    // "+10.00") leaves the name and its note room on one line at 358 px: 43 characters at 11 px ×
    // 0.64 (302.7 px) + 3 at 13 px (25.0) + 24 = 351.7 ≤ 358. A week of −12.34567891 ends in
    // "−12.34", 6 characters (49.9 px): 376.6 > 358, so the note takes its second line.
    const data = deposited.data.map((p, i) => (i === 154 ? { ...p, value: '101167.65432109' } : p));
    const { container } = render(<Picture deposited={{ ...deposited, data }} ratio={ratio} />);
    const note = container.querySelector('[data-panel="weekly"] [data-role="scale-note"]')!;
    expect(note.tagName.toLowerCase()).toBe('tspan');
    // the week of −12.34567891 moves p95, so the scale is ±20 (the note keeps its 21 characters)
    expect(note).toHaveTextContent('±20 scale, 1 week cut');
    expect(note).toHaveAttribute('x', '8');
    // re-pinned (readability F3): the note's line stands under the row's 17 px line, 1 + 17 +
    // 11.4 = 29.4 under the top, 15.6 under the row's baseline at 13.8 (was 12, meeting the name)
    expect(note).toHaveAttribute('dy', '15.6');
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
    // the summary form (R5, re-pinned on purpose: was "+10.00000000")
    const holders = within(reading).getAllByText((_, el) => el?.textContent === '+10');
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

// U-19 (UI review 2026-11-14): the deposited plot wrote its round values inside itself in the ghost
// ink, where a spike crossed them; like every history plot they sit in a dim gutter left of it.
describe('DaoHistory deposited values (U-19)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it.each([358, 1200])('writes them in a dim gutter left of the plot at %i px', (width) => {
    stubChartWidth(width);
    const { deposited, ratio } = series();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    const svg = container.querySelector('[data-panel="deposited"]')!;
    expect(expectTicksInGutter(svg, 'deposited')).toEqual(expect.arrayContaining(['50k', '100k']));
    // the name starts inside the plot, after the gutter
    const name = svg.querySelector('[data-role="chart-name"]')!;
    expect(Number(name.getAttribute('x'))).toBeGreaterThan(
      Number(svg.querySelector('[data-role="y-tick"]')!.getAttribute('x'))
    );
  });
});

// Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every word
// of the three plots stands whole in its band — a name (and the weekly scale note, and a multiple's
// end value) in the band over the data, a deposited value in the plot, a time label under it —
// touches no other word, and stays inside its svg, measured by the type modules (0.6 em a glyph; a
// tight line over its baseline, its descent under it). The band over the data is where the day
// cursor starts (`data-cursor`). The real case is mainnet's: every day since 2019-05-24 to about
// 8 billion CKB, a last week of −12,345,678.12345678 CKB (its end label "−12,345,678"). The
// deposited plot is drawn across the column (1232 at 1280, 608 at 640, 358 on a phone); the two
// multiples side by side from sm (604 and 292 px each) and across a phone (358).
describe('DaoHistory: every word in its band (readability F3)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  function mainnet() {
    const days = 2_000;
    const day0 = Date.UTC(2019, 4, 24);
    const at = (i: number) => new Date(day0 + i * DAY).toISOString().slice(0, 10);
    // about 4 million CKB more each day, to about 8 billion
    const values = Array.from(
      { length: days },
      (_, i) => BigInt(4_000_000 * i + (i % 7) * 1_000_000) * BigInt(1e8)
    );
    // the last whole week nets −12,345,678.12345678 CKB
    values[days - 1] = values[days - 8] - BigInt('1234567812345678');
    const ckb = (v: bigint) => `${v / BigInt(1e8)}.${String(v % BigInt(1e8)).padStart(8, '0')}`;
    const data = values.map((v, i) => ({ date: at(i), value: ckb(v), value2: String(10_000 + i) }));
    return {
      deposited: {
        data,
        title: 'Total Deposit',
        yAxisLabel: 'CKB',
        y2AxisLabel: 'Current depositors',
        peak: null,
      },
      ratio: {
        data: data.map((d) => ({ date: d.date, value: '30.1234' })),
        title: 'Ratio',
        yAxisLabel: '%',
      },
    };
  }

  /** A panel's svg, its band over the data (where the pinned day's cursor starts) and its frame. */
  function panel(container: HTMLElement, name: string) {
    const svg = container.querySelector(`[data-panel="${name}"]`)!;
    const cursor = svg.querySelector('[data-cursor]')!;
    return { svg, band: Number(cursor.getAttribute('y1')), frame: viewBox(svg) };
  }

  // Review F-B (logic finding 1): at a 320 px window the column is 288 px, and the deposited
  // plot and both multiples (stacked below sm) are drawn across it.
  it.each([1232, 608, 604, 358, 292, 288])('at %i px', (width) => {
    stubChartWidth(width);
    stubSvgWidth(width);
    const { deposited, ratio } = mainnet();
    const { container } = render(<Picture deposited={deposited} ratio={ratio} />);
    // a tap pins a day: its cursor is drawn in every panel, from the top of the panel's data
    fireEvent.click(container.querySelector('[data-panel="share"]')!, { clientX: 40 });
    const multiples = [604, 292, 358, 288].includes(width);
    const panels = [
      ...([1232, 608, 358, 288].includes(width) ? ['deposited'] : []),
      ...(multiples ? ['share', 'weekly'] : []),
    ];
    for (const name of panels) {
      const { svg, band, frame } = panel(container, name);
      const over = { ...frame, bottom: band };
      for (const text of [
        ...svg.querySelectorAll('[data-role="chart-name"], [data-role="end-label"]'),
      ])
        for (const line of textLines(text))
          expectInside(line, over, `${name} at ${width}`, 'flush');
      // the deposited plot's foot: its axis line, under its data
      const axis = Number(
        svg.querySelector('line.stroke-base-border')?.getAttribute('y1') ?? frame.bottom
      );
      for (const text of [...svg.querySelectorAll('[data-role="y-tick"]')])
        for (const line of textLines(text))
          expectInside(line, { ...frame, top: band, bottom: axis }, `${name} value`, 'flush');
      for (const text of [...svg.querySelectorAll('[data-role="time-tick"]')])
        for (const line of textLines(text)) {
          expectInside(line, { ...frame, top: axis }, `${name} time`, 'flush');
          expect(line.top, `"${line.text}" clears the axis`).toBeGreaterThan(axis);
        }
      expectApart(allTextLines(svg), `${name} at ${width}`);
    }
    if (multiples)
      expect(
        container.querySelector('[data-panel="weekly"] [data-role="end-label"]')
      ).toHaveTextContent('−12,345,678');
  });
});
