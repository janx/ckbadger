import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import KnowledgeSizePage from '@/app/charts/knowledge-size/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getKnowledgeSizeChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('KnowledgeSizePage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });
  it('separates occupied CKB, utilization percent and signed daily net flow', async () => {
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue({
      title: 'Common Knowledge Size',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Utilization (%)',
      data: [
        { date: '2026-02-18', value: '100', value2: '10' },
        { date: '2026-02-19', value: '130', value2: '13' },
      ],
    });
    render(<KnowledgeSizePage />);
    expect(await screen.findByText('Knowledge size')).toBeInTheDocument();
    await waitFor(() => expect(chartMock).toHaveBeenCalledTimes(3));
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      title: 'Knowledge size',
      unit: 'CKB',
      data: [
        { label: '2026-02-18', values: { primary: '100' } },
        { label: '2026-02-19', values: { primary: '130' } },
      ],
    });
    expect(chartMock.mock.calls[1][0]).toMatchObject({
      title: 'Utilization',
      unit: '%',
      data: [
        { label: '2026-02-18', values: { secondary: '10' } },
        { label: '2026-02-19', values: { secondary: '13' } },
      ],
    });
    expect(chartMock.mock.calls[2][0]).toMatchObject({
      title: 'Net knowledge flow',
      unit: 'CKB/day',
      data: [{ label: '2026-02-19', values: { flow: '30' } }],
    });
    expect(screen.getByText('Calculation scope')).toBeInTheDocument();
    expect(screen.getByText('Evidence path')).toBeInTheDocument();
  });
  it('keeps daily net flow exact above the safe integer range', async () => {
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue({
      title: 'Common Knowledge Size',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Utilization (%)',
      data: [
        { date: '2026-02-18', value: '9007199254740993.00000001', value2: '10' },
        { date: '2026-02-19', value: '9007199254740993.00000003', value2: '10' },
      ],
    });
    render(<KnowledgeSizePage />);
    await waitFor(() => expect(chartMock).toHaveBeenCalledTimes(3));
    expect((chartMock.mock.calls[2][0] as { title: string; data: unknown }).data).toEqual([
      { label: '2026-02-19', values: { flow: '0.00000002' } },
    ]);
  });
});
