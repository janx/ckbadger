import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import EpochTimeLengthPage from '@/app/charts/epoch-time-length/page';

const chartPageMock = vi.hoisted(() => vi.fn());
const getHardforksMock = vi.hoisted(() => vi.fn());

vi.mock('@/lib/api', () => ({
  api: {
    getEpochTimeLengthChart: vi.fn(),
    getHardforks: getHardforksMock,
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/charts/chart-page', () => ({
  ChartPage: (props: unknown) => {
    chartPageMock(props);
    const { title, queryKey, markers } = props as {
      title: string;
      queryKey: string;
      markers?: Array<{ label: string; href?: string }>;
    };
    return (
      <div>
        <span>{title}</span>
        <span>{queryKey}</span>
        <span data-testid="markers-count">{markers?.length ?? 0}</span>
      </div>
    );
  },
}));

describe('EpochTimeLengthPage', () => {
  it('passes hardfork markers to ChartPage', async () => {
    getHardforksMock.mockResolvedValue({
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

    expect(screen.getByText('Epoch Time Length')).toBeInTheDocument();
    expect(screen.getByText('chart-epoch-time-length')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByTestId('markers-count')).toHaveTextContent('2');
    });

    const lastProps = chartPageMock.mock.calls.at(-1)?.[0] as {
      markers?: Array<{ x: string; label: string; color: string; href?: string }>;
    };
    expect(lastProps.markers).toEqual([
      { x: '5414', label: 'MIRANA', color: '#f59e0b', href: '/blocks/8775638' },
      { x: '12293', label: 'MEEPO', color: '#f59e0b', href: '/blocks/18430000' },
    ]);
  });
});
