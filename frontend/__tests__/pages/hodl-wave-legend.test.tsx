import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// Responsive review C#4: the full HODL wave drew eight bands with no key; their shares were only
// in the hover readout. The detail page now keys every multi-band chart under the plot.

vi.mock('@/lib/api', () => ({
  api: {
    getHodlWaveChart: vi.fn(),
    getHashRateChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
    getBlocksByDay: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

const bands = [
  { key: '24h', label: '24h', color: '#6366f1' },
  { key: '1d1w', label: '1d-1w', color: '#4ade80' },
  { key: '1w1m', label: '1w-1m', color: '#f87171' },
  { key: '1m3m', label: '1m-3m', color: '#f59e0b' },
  { key: '3m6m', label: '3m-6m', color: '#d4e157' },
  { key: '6m1y', label: '6m-1y', color: '#22c55e' },
  { key: '1y3y', label: '1y-3y', color: '#67e8f9' },
  { key: 'gt3y', label: '> 3y', color: '#a78bfa' },
];
const day = (date: string, gt3y: string, y1y3: string) => ({
  date,
  values: {
    '24h': '5.00',
    '1d1w': '10.00',
    '1w1m': '15.00',
    '1m3m': '10.00',
    '3m6m': '10.00',
    '6m1y': '15.00',
    '1y3y': y1y3,
    gt3y,
    // lane D1: the API's merged bands (10 + 10; 15 + 10 + 5)
    '1m6m': '20.00',
    lt1m: '30.00',
    holderCount: '1000',
  },
});

describe('chart detail key (C#4)', () => {
  beforeEach(() => stubChartWidth(358));
  afterEach(() => vi.restoreAllMocks());

  // Re-pinned for lane D1 (the chart-hero board, rule 5): a phone keys the stack under the plot
  // in the stack's order, top band first, each band the API draws with its latest share (was the
  // eight bands, youngest first, as the legend under every width; the eight now stand in the table
  // under the chart, hodl-wave.test.tsx).
  it('keys the HODL wave under the chart on a phone: its five bands, top first, a swatch and its share', async () => {
    vi.mocked(api.getHodlWaveChart).mockResolvedValue({
      title: 'CKB HODL Wave',
      data: [day('2024-01-01', '10.00', '25.00'), day('2024-01-02', '15.00', '20.00')],
      series: bands,
      bands: [
        { key: 'gt3y', label: '3 years or more', members: ['gt3y'] },
        { key: '1y3y', label: '1 to 3 years', members: ['1y3y'] },
        { key: '6m1y', label: '6 to 12 months', members: ['6m1y'] },
        { key: '1m6m', label: '1 to 6 months', members: ['3m6m', '1m3m'] },
        { key: 'lt1m', label: 'under 1 month', members: ['1w1m', '1d1w', '24h'] },
      ],
    });
    render(<ChartDetail slug="hodl-wave" />);
    const legend = await screen.findByRole('list', { name: 'HODL wave key' });
    const items = within(legend).getAllByRole('listitem');
    expect(items.map((item) => item.querySelector('[data-swatch]') !== null)).toEqual([
      true,
      true,
      true,
      true,
      true,
    ]);
    expect(items.map((item) => item.textContent)).toEqual([
      'under 1 month30.00%',
      '1 to 6 months20.00%',
      '6 to 12 months15.00%',
      '1 to 3 years20.00%',
      '3 years or more15.00%',
    ]);
    // each swatch is its band's colour on the plot
    const swatches = items.map(
      (item) => (item.querySelector('[data-swatch]') as HTMLElement).style.backgroundColor
    );
    expect(new Set(swatches).size).toBe(5);
  });

  it('draws no key under a single-series chart', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue({
      title: 'Hash rate',
      yAxisLabel: 'H/s',
      data: [
        { date: '2024-01-01', value: '1000000000000000' },
        { date: '2024-01-02', value: '2000000000000000' },
      ],
    });
    vi.mocked(api.getBlocksByDay).mockResolvedValue({
      date: '2024-01-02',
      startMs: 0,
      endMs: 86_400_000,
      first: { number: 1, hash: `0x${'01'.repeat(32)}` },
      last: { number: 2, hash: `0x${'02'.repeat(32)}` },
      blocksCount: 2,
      complete: true,
    });
    render(<ChartDetail slug="hash-rate" />);
    await screen.findByRole('img', { name: 'Hash rate, PH/s' });
    expect(screen.queryByRole('list', { name: 'Hash rate legend' })).toBeNull();
  });
});
