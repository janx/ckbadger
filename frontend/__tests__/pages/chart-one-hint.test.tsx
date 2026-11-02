import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { sectionsWithManyHints } from '@/__tests__/utils/hints';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// Consistency round C10: a chart page section says "tap or hover" once — the readout row's own
// hint. Its idle words said "Tap or hover the chart for the value of one day." beside it, and the
// help paragraph said it a third time.
vi.mock('@/lib/api', () => ({
  api: {
    getHodlWaveChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
    getBlocksByDay: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
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
const day = (date: string) => ({
  date,
  values: {
    '24h': '5.00',
    '1d1w': '10.00',
    '1w1m': '15.00',
    '1m3m': '10.00',
    '3m6m': '10.00',
    '6m1y': '15.00',
    '1y3y': '20.00',
    gt3y: '15.00',
    holderCount: '1000',
  },
});

describe('chart page hints (C10)', () => {
  beforeEach(() => stubChartWidth(1248));
  afterEach(() => vi.restoreAllMocks());

  it('says "tap or hover" once per section, with the help open too', async () => {
    vi.mocked(api.getHodlWaveChart).mockResolvedValue({
      title: 'CKB HODL Wave',
      data: [day('2024-01-01'), day('2024-01-02')],
      series: bands,
    });
    const { container } = render(<ChartDetail slug="hodl-wave" />);
    const readouts = await screen.findAllByRole('status', { name: /readout$/ });
    expect(readouts.length).toBeGreaterThanOrEqual(2);
    for (const readout of readouts) {
      expect(readout).toHaveTextContent(/^A day on the chart reads its value here\.tap or hover$/);
    }
    expect(sectionsWithManyHints(container)).toEqual([]);
    fireEvent.click(screen.getByRole('button', { name: 'How to read and zoom this chart' }));
    expect(screen.getByText(/select it again to let it go/)).toBeInTheDocument();
    expect(sectionsWithManyHints(container)).toEqual([]);
  });
});
