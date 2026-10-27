import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: { getHashRateChart: vi.fn(), getNetworkStats: vi.fn(), getHardforks: vi.fn() },
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
});
