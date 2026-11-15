import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-18: the fact row under the lede repeated the head figure 60 px below it
// on eleven pages ("Live cells  2026-11-13 · 1,381,545 cells", "Most common 0.8 · 1.04 %", "Today
// year 7.00 · 2.33 %"), put developer facts in tier 1 ("Frozen phase-1 overlap 92,655,192.17 CKB")
// and hid the interesting fact in a dim label ("Largest script: Default Lock").

vi.mock('@/lib/api', () => ({
  api: {
    getCellCountChart: vi.fn(),
    getBlockTimeDistributionChart: vi.fn(),
    getNominalApcChart: vi.fn(),
    getMostUtilizedScriptsChart: vi.fn(),
    getSecondaryIssuanceChart: vi.fn(),
    getHodlWaveChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));
vi.mock('@/components/charts/blocks-of-day', () => ({ BlocksOfDay: () => null }));
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => <div role="img" aria-label={props.title} />,
}));

/** The fact row under the lede, as `label → value` pairs. */
const facts = () =>
  [...screen.getByRole('banner').querySelectorAll('dl > div')].map((row) => [
    row.querySelector('dt')!.textContent,
    row.querySelector('dd')!.textContent,
  ]);

describe('the fact row under the lede (CH-18)', () => {
  beforeEach(() => {
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it('does not repeat the head figure: live cells keeps only its change', async () => {
    vi.mocked(api.getCellCountChart).mockResolvedValue({
      title: 'Cell Count',
      series: [
        { key: 'allCells', label: 'All Cells', color: '#000' },
        { key: 'liveCells', label: 'Live Cells', color: '#000' },
        { key: 'deadCells', label: 'Dead Cells', color: '#000' },
      ],
      data: [
        { date: '2026-11-12', values: { allCells: '3', liveCells: '1375133', deadCells: '2' } },
        { date: '2026-11-13', values: { allCells: '3', liveCells: '1381545', deadCells: '2' } },
      ],
    });
    render(<ChartDetail slug="cell-count" />);
    await screen.findByRole('group', { name: 'Live cells readings' });
    expect(facts()).toEqual([['Last change', '+6,412 cells']]);
  });

  it('draws no fact row where the only fact was the head figure again', async () => {
    vi.mocked(api.getBlockTimeDistributionChart).mockResolvedValue({
      title: 'Block intervals',
      yAxisLabel: '%',
      data: [
        { date: '0.7', value: '0.9' },
        { date: '0.8', value: '1.04' },
      ],
    });
    const view = render(<ChartDetail slug="block-time-distribution" />);
    await screen.findByRole('group', { name: 'Block intervals readings' });
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
    view.unmount();
    vi.mocked(api.getNominalApcChart).mockResolvedValue({
      title: 'Nominal APC',
      yAxisLabel: 'Nominal APC (%)',
      y2AxisLabel: 'nominal-apc-timeline',
      data: [
        { date: '0', value: '4.2', value2: 'past' },
        { date: '7', value: '2.33', value2: 'current' },
        { date: '20', value: '1.59', value2: 'future' },
      ],
      today: { blockNumber: 1, date: '2026-11-13', year: '7', value: '2.33' },
    });
    render(<ChartDetail slug="nominal-apc" />);
    await screen.findByRole('group', { name: 'Nominal APC readings' });
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
  });

  it("writes a leader's name as the bright value", async () => {
    const share = {
      title: 'Share',
      series: [
        { key: 'top0', label: 'Default Lock', color: '#000', id: 'family:default-lock' },
        { key: 'others', label: 'Others', color: '#000' },
      ],
      data: [
        {
          date: '2026-11-13',
          values: { top0: '5919046400000000', others: '17958953600000000' },
        },
      ],
    };
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts',
      usedShare: share,
      capacityShare: share,
    });
    render(<ChartDetail slug="most-utilized-scripts" />);
    await screen.findByRole('group', { name: 'Scripts readings' });
    expect(facts()).toEqual([['Largest script', 'Default Lock · 59,190,464 CKB · 24.79%']]);
    const dd = screen.getByRole('banner').querySelector('dd')!;
    expect(dd).toHaveClass('text-text-bright');
    expect(within(dd).getByText('Default Lock')).toHaveClass('whitespace-nowrap');
  });

  it('moves the frozen phase-1 overlap to the evidence tier', async () => {
    vi.mocked(api.getSecondaryIssuanceChart).mockResolvedValue({
      title: 'Secondary Issuance',
      series: [
        { key: 'compensation', label: 'DAO', color: '#000' },
        { key: 'mining', label: 'Mining', color: '#000' },
        { key: 'burnt', label: 'Burnt', color: '#000' },
      ],
      data: ['2026-11-12', '2026-11-13'].map((date, i) => ({
        date,
        values: { compensation: '2', mining: '3', burnt: String(1 + i) },
        protocolTotalShannons: String(600000000 + i * 100000000),
        phase1CompensationShannons: '9265519217277960',
      })),
    });
    render(<ChartDetail slug="secondary-issuance" />);
    await screen.findByRole('group', { name: 'Secondary issuance readings' });
    expect(facts()).toEqual([['Last change', '+1 CKB']]);
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier2).getByText(/Frozen phase-1 overlap/)).toHaveTextContent(
      'Frozen phase-1 overlap: 92,655,192.1727796 CKB'
    );
  });

  it('names the largest age band beside its share', async () => {
    const keys = ['gt3y', '1y3y', '6m1y', '3m6m', '1m3m', '1w1m', '1d1w', '24h'];
    vi.mocked(api.getHodlWaveChart).mockResolvedValue({
      title: 'CKB HODL Wave',
      series: keys.map((key) => ({ key, label: key === 'gt3y' ? '> 3y' : key, color: '#000' })),
      data: [
        {
          date: '2026-11-13',
          values: {
            ...Object.fromEntries(keys.map((key) => [key, '10.375'])),
            gt3y: '27.375',
            holderCount: '367704',
          },
        },
      ],
    });
    render(<ChartDetail slug="hodl-wave" />);
    await screen.findByRole('group', { name: 'HODL wave readings' });
    const [[label, value]] = facts();
    expect(label).toBe('Largest age band');
    expect(value).toMatch(/^> 3 ?y · 27\.38%$/);
  });
});
