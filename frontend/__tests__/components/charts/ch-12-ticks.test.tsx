import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { placeXTicks } from '@/lib/chart-ticks';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-12: Nominal APC's x axis skipped "year 10" (on a phone "year 0, year 5,
// year 12.5, year 17.5"); Epoch durations printed evenly spaced ticks "2.05, 3.45, 3.73 … 4.90,
// 50.50" with no unit; Block intervals "0.0, 7.1, 14.3 … 50.0"; Difficulty "6k P" beside the figure
// "5,229.34 P"; Cell size "100k-1m CKB", "≥1m CKB" beside "1M cells".

vi.mock('@/lib/api', () => ({
  api: {
    getNominalApcChart: vi.fn(),
    getEpochTimeDistributionChart: vi.fn(),
    getBlockTimeDistributionChart: vi.fn(),
    getDifficultyChart: vi.fn(),
    getCellSizeDistributionChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));
vi.mock('@/components/charts/blocks-of-day', () => ({ BlocksOfDay: () => null }));
const chartProps = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => {
    chartProps.push(props);
    return <div role="img" aria-label={props.title} />;
  },
}));
type Tick = { index: number; label: string };
const ticksOf = (title: string) =>
  chartProps.filter((props) => props.title === title).at(-1)!.xTicks as Tick[];

/** The ticks a plot `width` px wide keeps, for a line (`slot` false) or slot bars. */
function kept(ticks: Tick[], points: number, width: number, slot: boolean): Tick[] {
  const left = 56;
  const right = width - 10;
  const at = (index: number) =>
    left + (slot ? (index + 0.5) / points : index / (points - 1)) * (right - left);
  return placeXTicks(
    ticks.map((tick) => ({ ...tick, at: at(tick.index) })),
    { left, right, width, categorical: false }
  ).ticks;
}
/** Numbers of round labels, which must step evenly: no value of the sequence left out. */
function assertRegular(labels: string[]): void {
  const values = labels.map((label) => Number(label.replace(/^year /, '').replace(/ \w+$/, '')));
  expect(values.every(Number.isFinite)).toBe(true);
  const steps = values.slice(1).map((value, i) => +(value - values[i]).toFixed(9));
  expect(new Set(steps).size).toBeLessThanOrEqual(1);
}

describe('axis ticks (CH-12)', () => {
  beforeEach(() => {
    chartProps.length = 0;
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it('keeps year 10 on the model axis, a regular sequence at every width', async () => {
    const data = Array.from({ length: 81 }, (_, i) => ({
      date: String(i / 4),
      value: String(4.2 - i / 40),
      value2: i < 28 ? 'past' : i === 28 ? 'current' : 'future',
    }));
    vi.mocked(api.getNominalApcChart).mockResolvedValue({
      title: 'Nominal APC',
      yAxisLabel: 'Nominal APC (%)',
      y2AxisLabel: 'nominal-apc-timeline',
      data,
    });
    render(<ChartDetail slug="nominal-apc" />);
    await screen.findByRole('img', { name: 'Nominal APC' });
    const ticks = ticksOf('Nominal APC');
    expect(ticks.map((tick) => tick.label)).toContain('year 10');
    assertRegular(ticks.map((tick) => tick.label));
    for (const width of [1232, 608, 358, 326])
      assertRegular(kept(ticks, 81, width, false).map((tick) => tick.label));
  });

  it('labels Epoch durations at round hours, the unit once, its outliers open-ended', async () => {
    const minutes = [123, ...Array.from({ length: 100 }, (_, i) => 207 + i), 3030];
    vi.mocked(api.getEpochTimeDistributionChart).mockResolvedValue({
      title: 'Epoch Time Distribution',
      yAxisLabel: 'Epochs',
      data: minutes.map((m) => ({ date: (m / 60).toFixed(2), value: '3' })),
    });
    render(<ChartDetail slug="epoch-time-distribution" />);
    await screen.findByRole('img', { name: 'Epoch durations' });
    const labels = ticksOf('Epoch durations').map((tick) => tick.label);
    expect(labels[0]).toBe('≤ 2.05');
    expect(labels.at(-1)).toBe('≥ 50.50 h');
    expect(labels.filter((label) => label.includes('h'))).toHaveLength(1);
    const round = labels.slice(1, -1);
    expect(round.length).toBeGreaterThan(2);
    for (const label of round) expect(Number(label) % 0.25).toBe(0);
    assertRegular(round);
  });

  it('labels Block intervals at round seconds, the unit once', async () => {
    vi.mocked(api.getBlockTimeDistributionChart).mockResolvedValue({
      title: 'Block Time Distribution',
      yAxisLabel: 'Block Ratio (%)',
      data: Array.from({ length: 501 }, (_, i) => ({
        date: `${Math.floor(i / 10)}.${i % 10}`,
        value: '0.199',
      })),
    });
    render(<ChartDetail slug="block-time-distribution" />);
    await screen.findByRole('img', { name: 'Block intervals' });
    expect(ticksOf('Block intervals').map((tick) => tick.label)).toEqual([
      '0',
      '10',
      '20',
      '30',
      '40',
      '50 s',
    ]);
  });

  it('writes one prefix per difficulty tick', async () => {
    vi.mocked(api.getDifficultyChart).mockResolvedValue({
      title: 'Difficulty',
      yAxisLabel: 'Difficulty',
      data: [
        { date: '2026-11-12', value: '5229340000000000000' },
        { date: '2026-11-13', value: '5729340000000000000' },
      ],
    });
    render(<ChartDetail slug="difficulty" />);
    await screen.findByRole('img', { name: 'Difficulty' });
    const props = chartProps.filter((entry) => entry.title === 'Difficulty').at(-1)!;
    const formatAxis = props.formatAxis as ((value: number, step: number) => string) | undefined;
    expect(formatAxis?.(6000, 1000)).toBe('6,000');
    expect(formatAxis?.(5500, 500)).toBe('5,500');
  });

  it('names the Cell size buckets with one prefix form and the unit once', async () => {
    vi.mocked(api.getCellSizeDistributionChart).mockResolvedValue({
      title: 'Cell Size Distribution',
      yAxisLabel: 'Live Cells',
      y2AxisLabel: 'Common Knowledge Size (CKB)',
      data: ['<100 CKB', '100-1k CKB', '1k-10k CKB', '10k-100k CKB', '100k-1m CKB', '>=1m CKB'].map(
        (date, i) => ({ date, value: String(600 - i * 100), value2: '1' })
      ),
    });
    render(<ChartDetail slug="cell-size-distribution" />);
    await screen.findByRole('img', { name: 'Cell size' });
    expect(ticksOf('Cell size').map((tick) => tick.label)).toEqual([
      '< 100',
      '100 – 1k',
      '1k – 10k',
      '10k – 100k',
      '100k – 1M',
      '≥ 1M CKB',
    ]);
    expect(screen.getByRole('group', { name: 'Cell size readings' })).toHaveTextContent(
      'most common, < 100 CKB'
    );
  });
});
