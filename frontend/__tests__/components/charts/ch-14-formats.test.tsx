import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { ChartDetail } from '@/components/charts/chart-detail';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';
import { chartValueAxis } from '@/components/ui/chart';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-14: one quantity in several forms inside the chart pages and cards —
// "24.79%" / "24.79 %" / "67.8320%", "seconds" / "s", "hours" / "h", card figures with no unit
// ("81.05", "845.69", "10.43", "3.95", "0.8", "4.02").

vi.mock('@/lib/api', () => ({
  api: {
    getAverageBlockTimeChart: vi.fn(),
    getEpochTimeLengthChart: vi.fn(),
    getUncleRateChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
    getBlocksByDay: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/blocks-of-day', () => ({ BlocksOfDay: () => null }));
vi.mock('@/components/ui/chart', async (original) => ({
  ...(await original<typeof import('@/components/ui/chart')>()),
  Chart: ({ title }: { title: string }) => <div role="img" aria-label={title} />,
}));

const line = (dates: string[], values: string[]) => ({
  title: 'Chart',
  yAxisLabel: 'Value',
  data: dates.map((date, i) => ({ date, value: values[i] })),
});
const card = async (slug: string, payload: unknown) => {
  const definition = CHART_BY_SLUG.get(slug)!;
  const view = render(
    <ChartSmallMultiple definition={{ ...definition, fetch: async () => payload as never }} />
  );
  const readout = await screen.findByRole('group', { name: `${definition.title} current readout` });
  const text = readout.textContent;
  view.unmount();
  return text;
};

describe('one form per quantity in the chart pages and cards (CH-14)', () => {
  beforeEach(() => {
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it('gives every card figure its unit, a percentage its sign with no space', async () => {
    expect(await card('hash-rate', line(['2026-11-13'], ['845690000000000000']))).toBe(
      '845.69 PH/s'
    );
    expect(await card('average-block-time', line(['2026-11-13'], ['10.43']))).toBe('10.43 s');
    expect(await card('epoch-time-length', line(['15204'], ['3.95']))).toBe('3.95 h');
    expect(await card('transaction-count', line(['2026-11-13'], ['16826']))).toBe(
      '16,826 transactions'
    );
    expect(
      await card('uncle-rate', {
        ...line(['2026-11-13'], ['0.0255']),
        orphanRateTarget: { numerator: 1, denominator: 40 },
      })
    ).toBe('2.55%');
  });

  it('writes the SI symbols s and h, never the words', async () => {
    expect(CHART_BY_SLUG.get('average-block-time')!.unit).toBe('s');
    expect(CHART_BY_SLUG.get('epoch-time-length')!.unit).toBe('h');
    vi.mocked(api.getAverageBlockTimeChart).mockResolvedValue(
      line(['2026-11-12', '2026-11-13'], ['10.1', '10.43'])
    );
    render(<ChartDetail slug="average-block-time" />);
    const readings = await screen.findByRole('group', { name: 'Block time readings' });
    expect(readings).toHaveTextContent('2026-11-1310.43 s');
    expect(document.body).not.toHaveTextContent(/\d (seconds|hours)\b/);
  });

  it('writes a percentage with no space before its sign: legends, targets, ticks, figures', async () => {
    const kind = chartModel(CHART_BY_SLUG.get('common-knowledge-composition')!, {
      title: 'Composition',
      series: ['transfer', 'dao', 'udt', 'nftSpore', 'otherContracts'].map((key) => ({
        key,
        label: key,
        color: '#000',
      })),
      data: [
        {
          date: '2026-11-13',
          values: { transfer: '100', dao: '300', udt: '50', nftSpore: '40', otherContracts: '10' },
        },
      ],
    });
    expect(kind.card!.legend.map((entry) => entry.value)).toEqual([
      '60.00%',
      '20.00%',
      '10.00%',
      '8.00%',
      '2.00%',
    ]);
    const uncle = chartModel(CHART_BY_SLUG.get('uncle-rate')!, {
      ...line(['2026-11-13'], ['0.0255']),
      orphanRateTarget: { numerator: 1, denominator: 40 },
    });
    expect(uncle.referenceLines).toEqual([{ value: 2.5, label: 'target 2.50%' }]);
    const axis = chartValueAxis({
      title: 'Uncle rate',
      data: [{ label: '2026-11-13', values: { primary: '2.55' } }],
      series: [{ key: 'primary', label: 'Uncle rate' }],
      unit: '%',
      referenceLines: uncle.referenceLines,
    });
    for (const label of axis.labels) expect(label).toMatch(/\d%$/);
    vi.mocked(api.getUncleRateChart).mockResolvedValue({
      ...line(['2026-11-12', '2026-11-13'], ['0.025', '0.0255']),
      orphanRateTarget: { numerator: 1, denominator: 40 },
    });
    render(<ChartDetail slug="uncle-rate" />);
    const readings = await screen.findByRole('group', { name: 'Uncle rate readings' });
    expect(readings).toHaveTextContent('2026-11-132.55%');
    expect(screen.getByText('Last change').nextElementSibling).toHaveTextContent('+0.05%');
  });
});
