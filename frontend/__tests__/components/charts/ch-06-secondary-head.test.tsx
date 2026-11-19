import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-06: each secondary chart opened with its command line, then "Separate
// scale: cells. These values are never combined with cells.", then a bold title and a caption
// repeating the title: the name three times in 90 px and an implementation sentence. The name now
// appears once, as the command line, and one dim line says what is counted.

vi.mock('@/lib/api', () => ({
  api: {
    getCellCountChart: vi.fn(),
    getDaoTotalDepositChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
    getBlocksByDay: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));
vi.mock('@/components/charts/blocks-of-day', () => ({ BlocksOfDay: () => null }));
const chartProps = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => {
    chartProps.push(props);
    return <div role="img" aria-label={props.title} />;
  },
}));
const lastChart = (title: string) => chartProps.filter((props) => props.title === title).at(-1)!;

describe('a secondary chart is named once (CH-06)', () => {
  beforeEach(() => {
    chartProps.length = 0;
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it('heads consumed cells with its command line and one plain line, no scale sentence', async () => {
    vi.mocked(api.getCellCountChart).mockResolvedValue({
      title: 'Cell Count',
      series: [
        { key: 'allCells', label: 'All Cells', color: '#000' },
        { key: 'liveCells', label: 'Live Cells', color: '#000' },
        { key: 'deadCells', label: 'Dead Cells', color: '#000' },
      ],
      data: [
        { date: '2026-11-12', values: { allCells: '30', liveCells: '10', deadCells: '20' } },
        { date: '2026-11-13', values: { allCells: '33', liveCells: '11', deadCells: '22' } },
      ],
    });
    render(<ChartDetail slug="cell-count" />);
    const heading = await screen.findByRole('heading', { name: /^Consumed cells, cumulative/ });
    expect(document.body).not.toHaveTextContent(/Separate scale|never combined with/);
    // The chart's own title and caption stay for assistive technology only.
    expect(lastChart('Consumed cells, cumulative').hideCaption).toBe(true);
    // Re-pinned for lane D1 (R3, the ink-role gate): the line that says what a chart counts is a
    // sentence that explains, in the reading ink (was a quiet paragraph).
    const section = heading.closest('section')!;
    expect(
      within(section).getByText('Cells consumed since genesis, counted at the end of each day.')
    ).toHaveClass('text-text');
  });

  it('says what a DAO depositor count counts', async () => {
    vi.mocked(api.getDaoTotalDepositChart).mockResolvedValue({
      title: 'Total Deposit',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Depositors',
      data: [{ date: '2026-11-13', value: '101', value2: '16590' }],
    });
    render(<ChartDetail slug="total-deposit" />);
    await screen.findByRole('heading', { name: /^Current depositors/ });
    expect(document.body).not.toHaveTextContent(/Separate scale/);
    expect(lastChart('Current depositors').hideCaption).toBe(true);
    // re-pinned for lane D1 (R3): the reading ink (was the quiet ink)
    expect(
      screen.getByText('Addresses holding a live Nervos DAO deposit at the end of each day.')
    ).toHaveClass('text-text');
  });
});
