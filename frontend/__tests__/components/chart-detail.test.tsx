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
    getEpochTimeDistributionChart: vi.fn(),
    getEpochTimeLengthChart: vi.fn(),
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
// F2 charts: every prop the detail hands the shared Chart, for the encoding assertions.
const chartProps = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string; data: unknown[]; yScale?: string; reading?: unknown }) => {
    chartProps.push(props);
    const { title, data, yScale, reading } = props;
    return (
      <div
        role="img"
        aria-label={`${title}: ${data.length} points, ${yScale ?? 'linear'}`}
        data-reading={reading === undefined ? 'last-point' : JSON.stringify(reading)}
      />
    );
  },
}));
const lastChart = (title: string) => {
  const props = chartProps.filter((entry) => entry.title === title).at(-1);
  if (!props) throw new Error(`no chart ${title}`);
  return props;
};

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
    chartProps.length = 0;
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
    // The head rounds a rate like the board (9.01); the exact value is its hover title.
    expect(screen.getByTitle('9.007199254740993 PH/s')).toHaveTextContent('9.01PH/s');
    expect(screen.getByText('/charts/hash-rate')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Explore Blocks →' })).toHaveAttribute(
      'href',
      '/mainnet/blocks'
    );
    fireEvent.click(screen.getByRole('button', { name: '30 d' }));
    expect(screen.getByRole('img', { name: 'Hash rate: 1 points, linear' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Log scale' }));
    expect(screen.getByRole('img', { name: 'Hash rate: 1 points, log' })).toBeInTheDocument();
  });

  // CH-02 residual (round 2): rates read at two decimals, thousands grouped; exact on hover.
  it('rounds rate readings like the board, grouped, exact on hover (CH-02)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue({
      ...response,
      data: [
        { date: '2024-01-01', value: '1000000000000000' },
        { date: '2024-02-10', value: '1234567890123456789' },
      ],
    });
    render(<ChartDetail slug="hash-rate" />);
    const readings = await screen.findByRole('group', { name: 'Hash rate readings' });
    expect(readings).toHaveTextContent('2024-02-101,234.57PH/s');
    expect(screen.getByTitle('1,234.567890123456789 PH/s')).toBeInTheDocument();
    expect(screen.getByText('+1,233.57 PH/s')).toBeInTheDocument();
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

  // --- F2 charts group: the ChartDetail board ---
  const mirana = {
    id: 'mirana-2021',
    name: 'CKB Edition Mirana',
    shortName: 'Mirana',
    editionYear: 2021,
    activationEpoch: 5414,
    activationDate: '2022-05-10',
    activationBlock: 7087867,
    status: 'activated' as const,
    summary: '',
    resources: [],
  };
  const meepo = {
    ...mirana,
    id: 'meepo-2024',
    name: 'CKB Edition Meepo',
    shortName: 'Meepo',
    activationEpoch: 12293,
    activationDate: '2025-07-01',
    activationBlock: 16595590,
  };
  const hashRate = {
    title: 'Hash rate',
    yAxisLabel: 'H/s',
    data: [
      { date: '2021-12-31', value: '1000000000000000' },
      { date: '2022-06-01', value: '465311292444713720' },
      { date: '2024-12-27', value: '465311292444713720' },
      { date: '2026-10-29', value: '91340000000000000' },
    ],
    peak: { date: '2022-06-01', value: '465311292444713720' },
    peak2: null,
  };

  it('heads the chart with the latest reading and the API peak at two decimals (CH-21, CH-02)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue(hashRate);
    render(<ChartDetail slug="hash-rate" />);
    const readings = await screen.findByRole('group', { name: 'Hash rate readings' });
    expect(readings).toHaveTextContent('2026-10-2991.34PH/s');
    expect(readings).toHaveTextContent('peak, 2022-06-01465.31PH/s');
    // The exact values stay one hover away.
    expect(screen.getByTitle('465.31129244471372 PH/s')).toBeInTheDocument();
  });

  it('leads with a breadcrumb to its section, plain range labels and a help button (CH-26)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue(hashRate);
    render(<ChartDetail slug="hash-rate" />);
    const crumb = await screen.findByRole('link', { name: 'Charts / Proof of work' });
    expect(crumb).toHaveAttribute('href', '/mainnet/charts#proof-of-work');
    expect(screen.queryByRole('link', { name: /All charts/ })).toBeNull();
    for (const name of ['30 d', '90 d', '1 y', 'All'])
      expect(screen.getByRole('button', { name })).toBeInTheDocument();
    const help = screen.getByRole('button', { name: 'How to read and zoom this chart' });
    expect(help).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(help);
    expect(help).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText(/use ← and → to step through/)).toBeInTheDocument();
  });

  it('draws year ticks, dots and named hardforks, linked from the evidence (CH-22, CH-23, CH-24)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue(hashRate);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [mirana, meepo] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
    render(<ChartDetail slug="hash-rate" />);
    await screen.findByRole('link', {
      name: 'Hardforks: Mirana, epoch 5,414 · Meepo, epoch 12,293 →',
    });
    const props = lastChart('Hash rate');
    expect(props.xTicks).toEqual([
      { index: 1, label: '2022' },
      { index: 2, label: '2024' },
      { index: 3, label: '2026' },
    ]);
    expect(props.pointDots).toBe(true);
    expect(props.hideMarkerLegend).toBe(true);
    expect(props.markers).toEqual([
      { index: 1, label: 'Mirana', plotLabel: 'Mirana', href: '/hardforks' },
      { index: 3, label: 'Meepo', plotLabel: 'Meepo', href: '/hardforks' },
    ]);
    expect(
      screen.getByRole('link', { name: 'Hardforks: Mirana, epoch 5,414 · Meepo, epoch 12,293 →' })
    ).toHaveAttribute('href', '/mainnet/hardforks');
  });

  it('says how hash rate is computed, per complete UTC+8 day (CH-27)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue(hashRate);
    render(<ChartDetail slug="hash-rate" />);
    expect(
      await screen.findByText(
        /For each complete UTC\+8 day: the sum of the difficulty of that day's blocks, divided by the sum of their intervals in seconds/
      )
    ).toBeInTheDocument();
  });

  it('shows four related charts, like the board (CH-28)', async () => {
    vi.mocked(api.getHashRateChart).mockResolvedValue(hashRate);
    render(<ChartDetail slug="hash-rate" />);
    await screen.findByRole('heading', { name: 'Related charts' });
    expect(screen.getAllByText('Related preview')).toHaveLength(4);
  });

  it('reads a distribution at its most common bucket under a Distribution title (CH-30, CH-36)', async () => {
    vi.mocked(api.getEpochTimeDistributionChart).mockResolvedValue({
      title: 'Epoch durations',
      yAxisLabel: 'Epochs',
      data: [
        { date: '3.98', value: '10' },
        { date: '4.02', value: '622' },
        { date: '50.50', value: '1' },
      ],
    });
    render(<ChartDetail slug="epoch-time-distribution" />);
    expect(await screen.findByRole('heading', { name: 'Distribution' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Full history' })).toBeNull();
    const props = lastChart('Epoch durations');
    expect(props.reading).toEqual({ label: '4.02', values: { primary: '622' } });
    expect(props.subtitle).toBe('Completed epochs by hours taken · 3 buckets');
    expect(props.slotBars).toBe(true);
  });

  it('prints epoch numbers with thousands groups (CH-35)', async () => {
    vi.mocked(api.getEpochTimeLengthChart).mockResolvedValue({
      title: 'Epoch length',
      yAxisLabel: 'Hours',
      y2AxisLabel: 'Blocks',
      data: [
        { date: '15201', value: '4.1', value2: '1800' },
        { date: '15202', value: '4.29', value2: '1800' },
      ],
    });
    render(<ChartDetail slug="epoch-time-length" />);
    const readings = await screen.findByRole('group', { name: 'Epoch length readings' });
    expect(readings).toHaveTextContent('#15,2024.29hours');
    const props = lastChart('Epoch length') as { formatLabel: (label: string) => string };
    expect(props.formatLabel('15202')).toBe('epoch #15,202');
  });
});
