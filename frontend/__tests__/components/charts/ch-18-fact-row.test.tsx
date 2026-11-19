import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-18: the fact row under the lede repeated the head figure 60 px below it
// on eleven pages ("Live cells  2026-11-13 · 1,381,545 cells", "Most common 0.8 · 1.04 %", "Today
// year 7.00 · 2.33 %"), put developer facts in tier 1 ("Frozen phase-1 overlap 92,655,192.17 CKB")
// and hid the interesting fact in a dim label ("Largest script: Default Lock").
//
// Re-pinned for lane D1 (the chart-hero board, rule 2): the fact row itself goes — one sentence
// under the title says what the series did (its change, its peak), the figure is named in words in
// its eyebrow, and the facts the sentence does not carry stand in the evidence tier. Each test
// keeps its intent: the head figure is said once, a leader is named, developer facts are tier 2.

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

/** The head's sentence, its spaces plain. */
const sentence = () =>
  document.querySelector('[data-role="sentence-words"]')!.textContent!.replace(/\u00a0/g, ' ');

describe('the fact row under the lede (CH-18)', () => {
  beforeEach(() => {
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it('does not repeat the head figure: live cells says only its change, in the sentence', async () => {
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
    expect(await screen.findByTestId('figure-value')).toHaveTextContent('1,381,545 cells');
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
    expect(sentence()).toBe('6,412 cells up on the day before.');
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
    await screen.findByTestId('figure-value');
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
    await screen.findByTestId('figure-value');
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
  });

  it('names the leader in words over its figure, the fact in the evidence tier', async () => {
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
      bands: [
        { key: 'top0', label: 'Default Lock', members: ['top0'] },
        { key: 'others', label: 'Others', members: ['others'] },
      ],
    };
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts',
      usedShare: share,
      capacityShare: share,
    });
    render(<ChartDetail slug="most-utilized-scripts" />);
    expect(await screen.findByTestId('figure-eyebrow')).toHaveTextContent(
      'Default Lock, the largest script · 2026-11-13'
    );
    const figure = screen.getByTestId('figure-value');
    expect(figure).toHaveTextContent('59,190,464 CKB');
    expect(figure).toHaveClass('text-text-bright');
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier2).getByText(/^Largest script:/)).toHaveTextContent(
      'Largest script: Default Lock · 59,190,464 CKB · 24.79%'
    );
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
    await screen.findByTestId('figure-value');
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier2).getByText(/Frozen phase-1 overlap/)).toHaveTextContent(
      'Frozen phase-1 overlap: 92,655,192.1727796 CKB'
    );
  });

  // Re-pinned for lane D1 (rule 3): the figure is the answer the chart is asked, named in words,
  // its share cut, never rounded (27.375 is 27.37; was "Largest age band > 3 y · 27.38%").
  it('names the oldest age band in words over its share', async () => {
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
            '1m6m': '20.75',
            lt1m: '31.125',
            holderCount: '367704',
          },
        },
      ],
      bands: [
        { key: 'gt3y', label: '3 years or more', members: ['gt3y'] },
        { key: '1y3y', label: '1 to 3 years', members: ['1y3y'] },
        { key: '6m1y', label: '6 to 12 months', members: ['6m1y'] },
        { key: '1m6m', label: '1 to 6 months', members: ['3m6m', '1m3m'] },
        { key: 'lt1m', label: 'under 1 month', members: ['1w1m', '1d1w', '24h'] },
      ],
    });
    render(<ChartDetail slug="hodl-wave" />);
    expect(await screen.findByTestId('figure-eyebrow')).toHaveTextContent(
      'unmoved for 3 years or more · 2026-11-13'
    );
    expect(screen.getByTestId('figure-value')).toHaveTextContent('27.37% of all CKB');
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
  });
});
