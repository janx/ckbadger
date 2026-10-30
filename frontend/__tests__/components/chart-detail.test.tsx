import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { act, createTestQueryClient, render } from '../utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getHashRateChart: vi.fn(),
    getNominalApcChart: vi.fn(),
    getMinerAddressDistributionChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({
  ChartSmallMultiple: () => <div>Related preview</div>,
}));
vi.mock('@/components/ui/chart', () => ({
  Chart: ({
    title,
    data,
    yScale,
    reading,
  }: {
    title: string;
    data: unknown[];
    yScale?: string;
    reading?: unknown;
  }) => (
    <div
      role="img"
      aria-label={`${title}: ${data.length} points, ${yScale ?? 'linear'}`}
      data-reading={reading === undefined ? 'last-point' : JSON.stringify(reading)}
    />
  ),
}));

const response = {
  title: 'Hash rate',
  yAxisLabel: 'H/s',
  data: [
    { date: '2024-01-01', value: '1000000000000000' },
    { date: '2024-02-10', value: '9007199254740993' },
  ],
};

describe('shared chart detail', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it("hands nominal APC's API today point to the chart as its main reading (CH-03)", async () => {
    const timeline = {
      title: 'Nominal APC',
      yAxisLabel: 'Nominal APC (%)',
      y2AxisLabel: 'nominal-apc-timeline',
      data: [
        { date: '0', value: '4.2', value2: 'past' },
        { date: '6.75', value: '2.3642', value2: 'current' },
        { date: '20', value: '1.59', value2: 'future' },
      ],
    };
    vi.mocked(api.getNominalApcChart).mockResolvedValue({
      ...timeline,
      today: { blockNumber: 20876568, date: '2026-10-30', year: '6.83', value: '2.3601' },
    });
    const view = render(<ChartDetail slug="nominal-apc" />);
    const chart = await screen.findByRole('img', { name: /^Nominal APC: 3 points/ });
    expect(JSON.parse(chart.getAttribute('data-reading')!)).toEqual({
      label: 'year 6.83',
      values: { primary: '2.3601' },
    });
    view.unmount();
    vi.mocked(api.getNominalApcChart).mockResolvedValue(timeline);
    render(<ChartDetail slug="nominal-apc" />);
    const gap = await screen.findByRole('img', { name: /^Nominal APC: 3 points/ });
    expect(gap.getAttribute('data-reading')).toBe('null');
  });

  it('keeps exact latest value, range, log, source and evidence navigation', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue(response);
    render(<ChartDetail slug="hash-rate" />);
    expect(
      await screen.findByRole('img', { name: 'Hash rate: 2 points, linear' })
    ).toBeInTheDocument();
    expect(screen.getByText(/9.007199254740993 PH\/s/)).toBeInTheDocument();
    expect(screen.getByText('/charts/hash-rate')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Explore Blocks →' })).toHaveAttribute(
      'href',
      '/mainnet/blocks'
    );
    fireEvent.click(screen.getByRole('button', { name: '30D' }));
    expect(screen.getByRole('img', { name: 'Hash rate: 1 points, linear' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Log scale' }));
    expect(screen.getByRole('img', { name: 'Hash rate: 1 points, log' })).toBeInTheDocument();
  });

  it('prints the latest value and last change exactly, thousands grouped (CH-02)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue({
      ...response,
      data: [
        { date: '2024-01-01', value: '1000000000000000' },
        { date: '2024-02-10', value: '1234567890123456789' },
      ],
    });
    render(<ChartDetail slug="hash-rate" />);
    expect(await screen.findByText('2024-02-10 · 1,234.567890123456789 PH/s')).toBeInTheDocument();
    expect(screen.getByText('+1,233.567890123456789 PH/s')).toBeInTheDocument();
  });

  it('puts related charts before the quieter method and evidence tier (CH-20, #11)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue(response);
    render(<ChartDetail slug="hash-rate" />);
    const related = await screen.findByRole('heading', { name: 'Related charts' });
    const method = screen.getByRole('heading', { name: 'How it is computed / Evidence' });
    expect(related.compareDocumentPosition(method) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(related).toHaveClass('text-[15px]');
    expect(method).toHaveClass('text-[13px]', 'text-[#a0a8be]');
    expect(method.closest('[data-tier="2"]')).not.toBeNull();
  });

  it('shows transport error with retry and distinguishes empty indexed data', async () => {
    vi.mocked(api.getHashRateChart)
      .mockRejectedValueOnce(new Error('upstream unavailable'))
      .mockResolvedValueOnce({ ...response, data: [] });
    render(<ChartDetail slug="hash-rate" />);
    expect(await screen.findByRole('alert')).toHaveTextContent('upstream unavailable');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() =>
      expect(screen.getByText('No indexed observations for this chart.')).toBeInTheDocument()
    );
    expect(api.getHashRateChart).toHaveBeenCalledTimes(2);
  });

  it('keeps one title and previously loaded chart after a failed background refresh', async () => {
    const client = createTestQueryClient();
    vi.mocked(api.getHashRateChart).mockResolvedValue(response);
    render(
      <QueryClientProvider client={client}>
        <ChartDetail slug="hash-rate" />
      </QueryClientProvider>
    );
    expect(
      await screen.findByRole('img', { name: 'Hash rate: 2 points, linear' })
    ).toBeInTheDocument();
    vi.mocked(api.getHashRateChart).mockRejectedValue(new Error('secondary read failed'));
    await act(async () => {
      await client.invalidateQueries({ queryKey: ['chart-hash-rate'], exact: true });
    });
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not refresh Hash rate: secondary read failed. Showing previously loaded data.'
    );
    expect(screen.getAllByRole('heading', { level: 1, name: 'Hash rate' })).toHaveLength(1);
    expect(screen.getByRole('img', { name: 'Hash rate: 2 points, linear' })).toBeInTheDocument();
  });

  // CH-04 (round 2): the fact names the miner in the card's head…tail form; the link keeps it all.
  it('names the leading miner in short form in the fact while retaining its address link', async () => {
    const address = `ckb1${'qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq0tpsqq08mkay9ewrfdwlcghv62qw704s93hhsj'}`;
    vi.mocked(api.getMinerAddressDistributionChart).mockResolvedValue({
      title: 'Miner distribution',
      data: [
        {
          minerLockHash: `0x${'a'.repeat(64)}`,
          address,
          minerName: null,
          blocksMined: 41743,
          percentage: '62.7931',
        },
      ],
      totalBlocks: 66477,
      windowDays: 7,
      fromDate: '2026-10-21',
      toDate: '2026-10-27',
    });
    render(<ChartDetail slug="miner-address-distribution" />);
    const label = await screen.findByText('Leading miner: ckb1qzda0c…93hhsj (62.7931%)');
    expect(label).toHaveClass('min-w-0', 'break-words');
    expect(label.parentElement).toHaveClass('grid-cols-1');
    expect(screen.getByRole('link', { name: address })).toHaveAttribute(
      'href',
      `/mainnet/address/${address}`
    );
  });
});
