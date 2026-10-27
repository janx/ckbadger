import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { act, createTestQueryClient, render } from '../utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getHashRateChart: vi.fn(),
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
  Chart: ({ title, data, yScale }: { title: string; data: unknown[]; yScale?: string }) => (
    <div role="img" aria-label={`${title}: ${data.length} points, ${yScale ?? 'linear'}`} />
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

  it('wraps a full miner identity in the fact label while retaining its address link', async () => {
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
    const label = await screen.findByText(`Leading miner: ${address} (62.7931%)`);
    expect(label).toHaveClass('min-w-0', 'break-words');
    expect(label.parentElement).toHaveClass('grid-cols-1');
    expect(screen.getByRole('link', { name: address })).toHaveAttribute(
      'href',
      `/mainnet/address/${address}`
    );
  });
});
