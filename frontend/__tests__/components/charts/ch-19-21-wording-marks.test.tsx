import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ChartDetail } from '@/components/charts/chart-detail';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel, formatChartExact, formatChartReadout } from '@/components/charts/chart-model';
import { Chart } from '@/components/ui/chart';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-19 and CH-21: "full history 81 model samples" over a model curve that runs
// to year 20; Cohort retention headed by the unfinished current month; the HODL wave's youngest
// band "24h" among ranges; legends "Weekly Turnover (%) 1.86 %"; "21. Others beyond the top 20"
// numbered as a rank; the target line hidden in the data and keyed by a "---" that reads as text;
// "-0.00 %"; negatives written with a hyphen.

vi.mock('@/lib/api', () => ({
  api: {
    getNominalApcChart: vi.fn(),
    getAddressCohortRetentionChart: vi.fn(),
    getMostUtilizedScriptsChart: vi.fn(),
    getHashRateChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
    getBlocksByDay: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));
vi.mock('@/components/charts/blocks-of-day', () => ({ BlocksOfDay: () => null }));

const def = (slug: string) => CHART_BY_SLUG.get(slug)!;

describe('chart wording and marks (CH-19, CH-21)', () => {
  beforeEach(() => {
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
    stubChartWidth(1232);
  });

  it('calls a model curve a model of its years, not samples', async () => {
    vi.mocked(api.getNominalApcChart).mockResolvedValue({
      title: 'Nominal APC',
      yAxisLabel: 'Nominal APC (%)',
      y2AxisLabel: 'nominal-apc-timeline',
      data: Array.from({ length: 81 }, (_, i) => ({
        date: String(i / 4),
        value: String(4.2 - i / 40),
        value2: i < 28 ? 'past' : i === 28 ? 'current' : 'future',
      })),
    });
    render(<ChartDetail slug="nominal-apc" />);
    expect(await screen.findByRole('heading', { name: 'Model 20\u00a0years' })).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent('model samples');
  });

  it('heads Cohort retention with the last complete cohort, not the month still filling', async () => {
    const payload = {
      title: 'Address Cohort Retention',
      yAxisLabel: 'Common Knowledge / Balance (%)',
      y2AxisLabel: 'Common Knowledge Size (CKB)',
      data: [
        { date: '2026-09', value: '7.5', value2: '100' },
        { date: '2026-10', value: '3.25', value2: '200' },
        { date: '2026-11', value: '1.5', value2: '300' },
      ],
    };
    expect(chartModel(def('address-cohort-retention'), payload).headline).toEqual({
      pre: '2026-10',
      display: '3.25',
      unit: '%',
    });
    vi.mocked(api.getAddressCohortRetentionChart).mockResolvedValue(payload);
    render(<ChartDetail slug="address-cohort-retention" />);
    const readings = await screen.findByRole('group', { name: 'Cohort retention readings' });
    expect(readings).toHaveTextContent('2026-103.25%');
    expect(screen.getByRole('group', { name: 'Common knowledge reading' })).toHaveTextContent(
      '2026-10200 CKB'
    );
  });

  it('names the HODL bands as ranges, the youngest "< 1 d"', () => {
    const keys = ['gt3y', '1y3y', '6m1y', '3m6m', '1m3m', '1w1m', '1d1w', '24h'];
    const labels = ['> 3y', '1y-3y', '6m-1y', '3m-6m', '1m-3m', '1w-1m', '1d-1w', '24h'];
    const model = chartModel(def('hodl-wave'), {
      title: 'CKB HODL Wave',
      series: keys.map((key, i) => ({ key, label: labels[i], color: '#000' })),
      data: [
        {
          date: '2026-11-13',
          values: { ...Object.fromEntries(keys.map((key) => [key, '12.5'])), holderCount: '1' },
        },
      ],
    });
    expect(model.series.map((series) => series.label)).toEqual([
      '> 3 y',
      '1 y – 3 y',
      '6 m – 1 y',
      '3 m – 6 m',
      '1 m – 3 m',
      '1 w – 1 m',
      '1 d – 1 w',
      '< 1 d',
    ]);
  });

  it("names a companion series by the page's own words, no '(%)'", () => {
    const turnover = chartModel(def('capacity-turnover-ratio'), {
      title: 'Capacity Turnover Ratio',
      yAxisLabel: 'Daily Turnover (%)',
      y2AxisLabel: 'Weekly Turnover (%)',
      data: [{ date: '2026-11-13', value: '0.5', value2: '1.86' }],
    });
    expect(turnover.series.map((series) => series.label)).toEqual([
      'Weekly turnover',
      'Daily turnover',
    ]);
    const inflation = chartModel(def('inflation-rate'), {
      title: 'Inflation Rate',
      yAxisLabel: 'Nominal Inflation (%)',
      y2AxisLabel: 'Real Inflation (%)',
      data: [{ date: '2026-11-13', value: '5.49', value2: '1.2' }],
    });
    expect(inflation.series.map((series) => series.label)).toEqual([
      'Nominal inflation',
      'Real inflation',
    ]);
  });

  it('does not number the remainder of a ranking as a rank', async () => {
    const share = {
      title: 'Share',
      series: [
        { key: 'top0', label: 'Default Lock', color: '#000', id: 'family:default-lock' },
        { key: 'others', label: 'Others', color: '#000' },
      ],
      data: [{ date: '2026-11-13', values: { top0: '300000000', others: '100000000' } }],
    };
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts',
      usedShare: share,
      capacityShare: share,
    });
    render(<ChartDetail slug="most-utilized-scripts" />);
    const ranking = await screen.findByRole('list', { name: 'Scripts ranking' });
    const rows = within(ranking).getAllByRole('listitem');
    expect(rows[0]).toHaveTextContent(/^1\. Default Lock/);
    expect(rows[1]).toHaveTextContent(/^Others beyond the top 1/);
  });

  it('draws the target line over the data, keyed by the same drawn dash', () => {
    const { container } = render(
      <Chart
        title="Uncle rate"
        subtitle="daily"
        data={[
          { label: '2026-11-12', values: { primary: '2.4' } },
          { label: '2026-11-13', values: { primary: '2.6' } },
        ]}
        series={[{ key: 'primary', label: 'Uncle rate' }]}
        unit="%"
        referenceLines={[{ value: 2.5, label: 'target 2.50%' }]}
        formatValue={(value) => value}
      />
    );
    const lines = [...container.querySelectorAll('[data-reference-line] line')];
    // A knock-out in the page's ground under the dash, so the line reads where data crosses it.
    expect(lines).toHaveLength(2);
    expect(lines[0].getAttribute('stroke')).toBe('var(--c-base-bg)');
    const dash = lines[1];
    expect(dash.getAttribute('stroke-opacity')).toBeNull();
    const svg = container.querySelector('svg')!;
    const order = [...svg.children];
    const plotted = order.findIndex((child) => child.querySelector('path'));
    const target = order.findIndex((child) => child.matches('[data-reference-line]'));
    expect(target).toBeGreaterThan(plotted);
    const sample = container.querySelectorAll('[data-reference-key] line');
    expect(sample).toHaveLength(2);
    for (const attribute of ['stroke', 'stroke-width', 'stroke-dasharray'])
      expect(sample[1].getAttribute(attribute)).toBe(dash.getAttribute(attribute));
  });

  it('writes no "-0.00" and writes negatives with the minus sign', async () => {
    expect(formatChartReadout('-0.001', '%')).toBe('0.00');
    expect(formatChartReadout('-1234.5', 'CKB')).toBe('−1.23k');
    expect(formatChartExact('-30', 'CKB/day')).toBe('−30');
    vi.mocked(api.getHashRateChart).mockResolvedValue({
      title: 'Hash rate',
      yAxisLabel: 'H/s',
      data: [
        { date: '2026-11-12', value: '1234567890123456789' },
        { date: '2026-11-13', value: '1000000000000000' },
      ],
    });
    render(<ChartDetail slug="hash-rate" />);
    await screen.findByRole('group', { name: 'Hash rate readings' });
    expect(screen.getByText('Last change').nextElementSibling).toHaveTextContent('−1,233.57 PH/s');
  });
});
