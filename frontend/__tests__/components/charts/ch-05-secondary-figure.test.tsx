import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-05: a secondary chart's 22 px figure was its last point or bucket with
// no label. Cell size read "common knowledge 1,048,617 CKB", the nearly invisible "≥1m CKB" bucket;
// HODL wave "367,704 holders", DAO deposits "16,590 depositors" were undated. Every secondary
// figure now carries the label the head figure carries, and a distribution shows its largest
// bucket by name, never the last.

vi.mock('@/lib/api', () => ({
  api: {
    getCellSizeDistributionChart: vi.fn(),
    getDaoTotalDepositChart: vi.fn(),
    getHodlWaveChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));
const chartProps = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => {
    chartProps.push(props);
    return <div role="img" aria-label={props.title} />;
  },
}));
const lastChart = (title: string) => chartProps.filter((props) => props.title === title).at(-1)!;

describe('a secondary figure says what it reads (CH-05)', () => {
  beforeEach(() => {
    chartProps.length = 0;
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it('reads a distribution at its largest bucket, by name, not at the last', async () => {
    vi.mocked(api.getCellSizeDistributionChart).mockResolvedValue({
      title: 'Cell Size Distribution',
      yAxisLabel: 'Live Cells',
      y2AxisLabel: 'Common Knowledge Size (CKB)',
      data: [
        { date: '<100 CKB', value: '900', value2: '52000000' },
        { date: '100-1k CKB', value: '400', value2: '75000000' },
        { date: '>=1m CKB', value: '1', value2: '1048617' },
      ],
      // the route's wholes (lane D1)
      liveCells: '1301',
      occupiedCapacity: '128048617',
    });
    render(<ChartDetail slug="cell-size-distribution" />);
    const reading = await screen.findByRole('group', { name: 'Common knowledge reading' });
    // The unit after a no-break space (re-pinned for CH-14).
    expect(reading).toHaveTextContent(/^largest, 100.*1k CKB75,000,000 CKB$/);
    expect(reading).not.toHaveTextContent('1,048,617');
    // The bucket in the axis' words (re-pinned for CH-12: was the API's '100-1k CKB').
    expect(lastChart('Common knowledge').reading).toEqual({
      label: '100 – 1k CKB',
      values: { secondary: '75000000' },
    });
  });

  it('dates the depositor count and the holder count', async () => {
    vi.mocked(api.getDaoTotalDepositChart).mockResolvedValue({
      title: 'Total Deposit',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Depositors',
      data: [
        { date: '2026-11-12', value: '100', value2: '16589' },
        { date: '2026-11-13', value: '101', value2: '16590' },
      ],
    });
    render(<ChartDetail slug="total-deposit" />);
    const reading = await screen.findByRole('group', { name: 'Current depositors reading' });
    expect(within(reading).getByText('2026-11-13')).toBeInTheDocument();
    expect(reading).toHaveTextContent('16,590 depositors');
  });

  it('dates the HODL wave holder count', async () => {
    const keys = ['gt3y', '1y3y', '6m1y', '3m6m', '1m3m', '1w1m', '1d1w', '24h'];
    vi.mocked(api.getHodlWaveChart).mockResolvedValue({
      title: 'CKB HODL Wave',
      series: keys.map((key) => ({ key, label: key, color: '#000' })),
      data: [
        {
          date: '2026-11-13',
          values: {
            ...Object.fromEntries(keys.map((key) => [key, '12.5'])),
            // lane D1: the API's merged bands, beside the eight
            '1m6m': '25',
            lt1m: '37.5',
            holderCount: '367704',
          },
        },
      ],
      bands: [
        { key: 'gt3y', label: '3 years or more', members: ['gt3y'] },
        { key: '1y3y', label: '1 to 3 years', members: ['1y3y'] },
        { key: '6m1y', label: '6 to 12 months', members: ['6m1y'] },
        { key: '1m6m', label: '1 to 6 months', members: ['3m6m', '1m3m'] },
        { key: 'lt1m', label: 'under 1 month', members: ['1w1m', '1d1w', '24h'] },
      ],
    });
    render(<ChartDetail slug="hodl-wave" />);
    const reading = await screen.findByRole('group', { name: 'Holder count reading' });
    expect(within(reading).getByText('2026-11-13')).toBeInTheDocument();
    expect(reading).toHaveTextContent('367,704 holders');
  });
});
