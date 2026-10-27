import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import EpochTimeLengthPage from '@/app/charts/epoch-time-length/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getEpochTimeLengthChart: vi.fn(),
    getHardforks: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('EpochTimeLengthPage', () => {
  it('places activated hardforks on their exact epoch observations', async () => {
    chartMock.mockClear();
    vi.mocked(api.getEpochTimeLengthChart).mockResolvedValue({
      title: 'Epoch Time Length',
      yAxisLabel: 'hours',
      data: [
        { date: '5413', value: '4' },
        { date: '5414', value: '4.1' },
        { date: '12293', value: '3.9' },
      ],
    });
    vi.mocked(api.getHardforks).mockResolvedValue({
      network: 'mainnet',
      tipEpoch: 13000,
      tipBlock: 19000000,
      events: [
        {
          id: 'mirana-2021',
          name: 'CKB Edition Mirana',
          shortName: 'Mirana',
          editionYear: 2021,
          activationEpoch: 5414,
          activationDate: '2022-05-10',
          activationBlock: 8775638,
          status: 'activated',
          summary: 'CKB-VM v1 activation.',
          resources: [],
        },
        {
          id: 'meepo-2024',
          name: 'CKB Edition Meepo',
          shortName: 'Meepo',
          editionYear: 2024,
          activationEpoch: 12293,
          activationDate: '2025-07-01',
          activationBlock: 18430000,
          status: 'activated',
          summary: 'CKB-VM v2 activation.',
          resources: [],
        },
      ],
    });
    render(<EpochTimeLengthPage />);
    expect(await screen.findByText('Epoch length')).toBeInTheDocument();
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls.at(-1)?.[0]).toMatchObject({
      markers: [
        { index: 1, label: 'Mirana', href: '/hardforks' },
        { index: 2, label: 'Meepo', href: '/hardforks' },
      ],
      data: [
        { label: '5413', values: { primary: '4' } },
        { label: '5414', values: { primary: '4.1' } },
        { label: '12293', values: { primary: '3.9' } },
      ],
    });
  });
});
