import { afterEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DaoBoard } from '@/components/dao/dao-board';
import { api } from '@/lib/api';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';
import evidence from '@/__tests__/fixtures/dao-evidence.json';

vi.mock('@/components/dao/dao-sources', () => ({
  DaoSources: () => <div data-testid="sources" />,
}));
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return {
    ...actual,
    api: { ...actual.api, calculateDaoCompensation: vi.fn(), getDaoArHistory: vi.fn() },
  };
});

/** R2b-a 43: AR at the close of four complete days; the evidence tip (#5) has AR 1.02. */
const AR_DAYS = [
  { date: '2019-11-16', blockNumber: 1, ar: '10000000000000000', arIndex: '1.0000000000000000' },
  { date: '2019-11-17', blockNumber: 2, ar: '10050000000000000', arIndex: '1.0050000000000000' },
  { date: '2019-11-18', blockNumber: 3, ar: '10100000000000000', arIndex: '1.0100000000000000' },
  { date: '2019-11-19', blockNumber: 4, ar: '10150000000000000', arIndex: '1.0150000000000000' },
];

/** The API calculator for 10,000 CKB: compensation = 100 CKB per 0.01 of AR gained (fixture). */
function stubCalculator() {
  vi.mocked(api.getDaoArHistory).mockResolvedValue({
    anchor: { blockNumber: 4, blockHash: '0x' + '4'.repeat(64) },
    days: AR_DAYS,
  });
  vi.mocked(api.calculateDaoCompensation).mockImplementation(
    async (capacity: string, depositBlock: number, withdrawBlock?: number) => ({
      capacity,
      capacityCkb: '10000',
      depositBlock,
      withdrawBlock: withdrawBlock!,
      estimatedCompensation: depositBlock === 1 ? '20000000000' : '9801980198',
      estimatedCompensationCkb: '',
      totalWithdrawable: '',
      totalWithdrawableCkb: '',
      apc: null,
      occupiedCapacity: '10200000000',
      freeCapacity: '989800000000',
    })
  );
}

const ANCHOR_5 = { blockNumber: 5, blockHash: '0x' + 'a5'.repeat(32) };
const stats = {
  ...daoStatistics,
  tipBlockNumber: 5,
  anchor: ANCHOR_5,
  // Secondary issuance at the evidence header's block #5, whose S (500000000) = owed + burnt.
  miningReward: '100000000',
  depositCompensation: '300000000',
  totalCompensationPaid: '100000000',
  unclaimedCompensation: '200000000',
  burnt: '300000000',
  tipSecondaryPool: '500000000',
  totalDeposited: '50000000000',
  totalDepositedCkb: '500',
  // Free capacity in DAO counts frozen phase-1 cells; the depositors' weight does not, but adds
  // the compensation phase-0 deposits already accrued. Distinct values keep the bar honest.
  daoFreeCapacity: '40000000000',
  depositorAllocationWeight: '45000000000',
  tipTotalIssuance: '1000000000000',
  tipProtocolOccupiedCapacity: '100000000',
};
const history = {
  data: [
    { date: '2024-01-13', value: '100.1', value2: '1' },
    { date: '2024-01-14', value: '400.001', value2: '2' },
    { date: '2024-01-15', value: '500', value2: '2' },
  ],
  title: 'Total Deposit',
  yAxisLabel: 'CKB',
  y2AxisLabel: 'Current depositors',
  // R8: every chart response carries its first greatest point.
  peak: { date: '2024-01-15', value: '500' },
  peak2: { date: '2024-01-14', value: '2' },
};
const ratio = {
  data: [
    { date: '2024-01-13', value: '0.50' },
    { date: '2024-01-14', value: '1.00' },
    { date: '2024-01-15', value: '1.25' },
  ],
  title: 'Ratio',
  yAxisLabel: '%',
};

describe('DaoBoard', () => {
  it('partitions all 30 exact epoch buckets and shows phase-1 lock readiness', () => {
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    const cycles = screen.getByRole('list', { name: 'Phase-0 cycle boundaries' });
    expect(within(cycles).getAllByRole('listitem')).toHaveLength(30);
    expect(
      screen.getByTitle(/Nominal day 1, epochs \+0 to \+6: 1 deposits, 30000000000 shannons/)
    ).toBeInTheDocument();
    expect(
      screen.getByTitle(/Nominal day 30, epochs \+174 to \+180: 1 deposits, 20000000000 shannons/)
    ).toBeInTheDocument();
    fireEvent.focus(
      screen.getByTitle(/Nominal day 1, epochs \+0 to \+6: 1 deposits, 30000000000 shannons/)
    );
    expect(screen.getByRole('region', { name: 'When deposits can leave' })).toHaveTextContent(
      'Nominal day 1 · epochs +0 through +6 · 300.00000000 CKB in 1 deposits'
    );
    // Board wording (DAO-14) for the same readiness partition.
    expect(screen.getByRole('region', { name: 'When deposits can leave' })).toHaveTextContent(
      '500.00000000 CKB in 1 cell waits for its window.'
    );
    expect(screen.getByText(/does not prove a complete withdraw transaction/)).toBeInTheDocument();
    expect(screen.getByTestId('sources')).toBeInTheDocument();
  });

  it('switches complete-day history range without changing exact printed amounts', () => {
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: '1M' }));
    // One History reading (DAO-11) carries the exact amounts of the last complete day.
    const reading = screen.getByLabelText('History reading');
    expect(reading).toHaveTextContent('deposited500.00000000');
    expect(reading).toHaveTextContent('share of circulation1.25%');
  });

  it('keeps the owner link bound to the source address', () => {
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const owner = evidence.concentration.owners[0];
    // The desktop row and the compact card (DAO-22) both link the same address.
    const links = screen.getAllByRole('link', {
      name: `${owner.address.slice(0, 12)}…${owner.address.slice(-7)}`,
    });
    expect(links).toHaveLength(2);
    for (const link of links)
      expect(link).toHaveAttribute('href', `/mainnet/address/${owner.address}`);
  });

  it('prints an exact seven-day net change including a one-shannon loss', () => {
    const days = Array.from({ length: 8 }, (_, index) => ({
      date: `2024-01-${String(index + 1).padStart(2, '0')}`,
      value: index === 7 ? '99.99999999' : '100',
      value2: '1',
    }));
    const shares = days.map(({ date }) => ({ date, value: '1.00' }));
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={{ ...history, data: days }}
        circulationHistory={{ ...ratio, data: shares }}
      />
    );
    const net = screen.getAllByText((_, el) => {
      if (!(el instanceof HTMLElement)) return false;
      return el.className.includes('tabular-nums') && el.textContent === '−0.00000001';
    });
    expect(net.length).toBeGreaterThan(0);
  });

  it("draws the depositors' share from their allocation weight, not DAO free capacity (DAO-2)", () => {
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    // The board legend (D3 wording): the weight's 4.50%, not DAO free capacity's 4.00%.
    expect(screen.getByRole('region', { name: 'Where secondary issuance goes' })).toHaveTextContent(
      'phase-0 free capacity + compensation already accrued 4.50% → depositors'
    );
    expect(screen.getByTitle('DAO depositors: 450.00000000 CKB')).toBeInTheDocument();
    expect(screen.queryByText(/free capacity in DAO/)).not.toBeInTheDocument();
  });

  it('writes AR as the exact rate and links anchors to real block routes (DAO-4, #7)', () => {
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(
      screen.getByText(/AR grew from 1\.0000000000000000 at genesis to 1\.0200000000000000/)
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Verify statistics anchor block #5/ })).toHaveAttribute(
      'href',
      '/mainnet/blocks/5'
    );
    expect(screen.getByRole('link', { name: 'Open source block →' })).toHaveAttribute(
      'href',
      `/mainnet/blocks/${evidence.header.blockNumber}`
    );
  });

  it('keeps the depositor table at a scrollable width so counts and amounts never fuse (DAO-3)', () => {
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const table = screen.getByRole('region', { name: 'Who deposits' }).querySelector('table')!;
    // ListTable: an explicit width budget per column, scrolled inside its own box on desktop …
    expect(parseInt(table.style.minWidth, 10)).toBeGreaterThanOrEqual(640);
    expect(table.parentElement).toHaveClass('overflow-x-auto');
    const [, , , deposits, amount] = Array.from(table.querySelectorAll('tbody tr')[0].children);
    expect(deposits).toHaveClass('whitespace-nowrap');
    expect(amount).toHaveClass('whitespace-nowrap');
    // … and compact cards below md, so no column hides off-screen at 390 px (DAO-22).
    expect(
      screen.getByRole('list', { name: 'Largest depositors, compact' }).parentElement
    ).not.toHaveClass('overflow-x-auto');
  });

  it('reads the 365-day AR anchor as a rate, a UTC+8 time and a span, not raw integers (#11)', () => {
    const year365 = {
      anchorBlockNumber: 3,
      anchorChainTimeMs: '1761733970452',
      anchorAr: '10100000000000000',
      elapsedChainTimeMs: '31536000000',
      observedGrowthPercent: '0.99',
    };
    render(
      <DaoBoard statistics={stats} evidence={{ ...evidence, ar: { ...evidence.ar, year365 } }} />
    );
    const line = screen.getByText(/365-day anchor/);
    expect(line).toHaveTextContent(
      '365-day anchor: block #3 · chain time 2025-10-29 18:32:50 UTC+8 · AR 1.0100000000000000 · observed span 365 d'
    );
    expect(line).not.toHaveTextContent(/ ms|10100000000000000/);
  });

  it('fails fast when a complete deposit day lacks its depositor count (#17)', () => {
    const missing = {
      ...history,
      data: history.data.map((row, i) => (i === 2 ? { ...row, value2: null } : row)),
    };
    expect(() =>
      render(
        <DaoBoard
          statistics={stats}
          evidence={evidence}
          depositedHistory={missing}
          circulationHistory={ratio}
        />
      )
    ).toThrow('2024-01-15 has no current depositor count');
  });

  it('keeps the 72 px tier-2 boundary and quiet 13 px tier-2 headings (DAO-20)', () => {
    const { container } = render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).not.toBeNull();
    expect(tier2).toHaveClass('mt-[72px]');
    // A space-y-* parent's sibling selector overrides the 72 px margin (measured 32 px).
    expect(tier2.parentElement!.className).not.toMatch(/space-y-/);
    const headings = Array.from(tier2.querySelectorAll('h2'));
    expect(headings.map((h) => h.textContent)).toEqual([
      'The DAO field in every block header · #5',
      'The DAO script',
    ]);
    for (const h of headings) expect(h).toHaveClass('text-[13px]', 'text-[#a0a8be]');
  });

  it('renders a zero secondary allocation without dividing by zero', () => {
    render(
      <DaoBoard
        statistics={{
          ...stats,
          miningReward: '0',
          depositCompensation: '0',
          burnt: '0',
          totalCompensationPaid: '0',
          unclaimedCompensation: '0',
          // nothing issued yet: the statistics row's S is 0 too
          tipSecondaryPool: '0',
        }}
        evidence={{ ...evidence, header: { ...evidence.header, s: '0' } }}
      />
    );
    expect(screen.getByText(/allocation shares are undefined/)).toBeInTheDocument();
    expect(screen.queryByLabelText('Secondary issuance partition')).not.toBeInTheDocument();
  });
});

// ---- F2 group dao (round-2 fix) ----
const peakHistory = {
  ...history,
  data: [
    { date: '2024-01-13', value: '100.1', value2: '1' },
    { date: '2024-01-14', value: '600.5', value2: '2' },
    { date: '2024-01-15', value: '500', value2: '2' },
  ],
  peak: { date: '2024-01-14', value: '600.5' },
  peak2: { date: '2024-01-14', value: '2' },
};

describe('DaoBoard hero (DAO-9, DAO-10)', () => {
  it('leads with deposited, its share of circulation and its peak, and the board metric names', () => {
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={peakHistory}
        circulationHistory={ratio}
      />
    );
    const hero = screen.getByRole('region', { name: 'Nervos DAO summary' });
    expect(hero).toHaveTextContent('deposited500.00000000 CKB');
    expect(hero).toHaveTextContent(
      '1.25% of circulating CKB on 2024-01-15 · peak 600.50000000 CKB on 2024-01-14'
    );
    expect(hero).toHaveTextContent(
      "Compensation, last 365 days2.0000%on free capacity, from the header's AR"
    );
    expect(hero).toHaveTextContent('Depositors2');
    expect(hero).toHaveTextContent('Deposits2+ 2 withdrawals requested');
    expect(hero).toHaveTextContent('Estimated APC1.99%');
    // The 24 h net change is signed and coloured by its sign.
    const change = within(hero).getByText((_, el) => el?.getAttribute('data-role') === 'change24h');
    expect(change).toHaveTextContent('−2,578,825.48689593');
    expect(change).toHaveClass('text-negative');
    // Developer evidence (AR anchors) is not in tier 1 (DAO-9).
    expect(hero).not.toHaveTextContent(/anchor/i);
    expect(screen.queryByText(/Observed AR growth/)).not.toBeInTheDocument();
    // R2b-a 41: the 24 h window's own counts (board: "N deposits in, M withdrawal requests out").
    expect(hero).toHaveTextContent('12 deposits in, 9 withdrawal requests out');
  });

  it('fails when the deposit history arrives without its R8 peak', () => {
    const noPeak = { ...history, peak: undefined };
    expect(() =>
      render(
        <DaoBoard
          statistics={stats}
          evidence={evidence}
          depositedHistory={noPeak}
          circulationHistory={ratio}
        />
      )
    ).toThrow('/dao/charts/total-deposit has no peak');
  });
});

describe('DaoBoard History (DAO-11)', () => {
  afterEach(() => vi.restoreAllMocks());
  it('draws History as one figure with one reading, not three charts', () => {
    stubChartWidth(1200);
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    const region = screen.getByRole('region', { name: 'History' });
    expect(region.querySelectorAll('svg')).toHaveLength(1);
    expect(within(region).getAllByLabelText('History reading')).toHaveLength(1);
  });
});

describe('DaoBoard secondary issuance (DAO-7)', () => {
  afterEach(() => vi.restoreAllMocks());
  const issuance = () => screen.getByRole('region', { name: 'Where secondary issuance goes' });

  it('splits issuance three ways and only compensation two ways, as an aligned icicle', () => {
    stubChartWidth(1400);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const svg = within(issuance()).getByRole('img', {
      name: /Secondary issuance since genesis, split three ways/,
    });
    const box = (key: string) => svg.querySelector(`[data-node="${key}"]`)!;
    for (const key of ['total', 'miners', 'compensation', 'burnt', 'paid', 'owed'])
      expect(box(key)).not.toBeNull();
    // compensation's two parts sit exactly under it
    expect(box('paid').getAttribute('x')).toBe(box('compensation').getAttribute('x'));
    expect(svg.querySelector('[data-label="compensation"]')).toHaveTextContent(
      'compensation for depositors'
    );
    expect(svg.querySelector('[data-placeholder="burnt"]')).toHaveTextContent(
      'No cell can ever claim it.'
    );
    expect(svg.querySelector('[data-placeholder="miners"]')).toHaveTextContent('paid in cellbases');
    expect(issuance()).toHaveTextContent('still owed');
    expect(issuance()).not.toHaveTextContent(/interest|earned/i);
  });

  it('says where each share of capacity use goes, and why a deposit keeps its share', () => {
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(issuance()).toHaveTextContent('occupied by cells 0.01% → miners');
    expect(issuance()).toHaveTextContent(
      'phase-0 free capacity + compensation already accrued 4.50% → depositors · withdrawal requests take no share'
    );
    expect(issuance()).toHaveTextContent('everything else 95.49% → burnt');
    expect(issuance()).toHaveTextContent(
      'So a deposit keeps its share of all CKB through secondary issuance.'
    );
  });

  it('states every part in words at phone width instead of shrinking the drawing', () => {
    stubChartWidth(358);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const svg = within(issuance()).getByRole('img', {
      name: /Secondary issuance since genesis, split three ways/,
    });
    expect(svg.querySelectorAll('text')).toHaveLength(0);
    const legend = within(issuance()).getByRole('list', { name: 'Secondary issuance parts' });
    expect(legend).toHaveTextContent('to miners');
    expect(legend).toHaveTextContent('paid out');
    expect(legend).toHaveTextContent('No cell can ever claim it.');
  });
});

describe('DaoBoard how a deposit grows (DAO-8, DAO-9)', () => {
  afterEach(() => vi.restoreAllMocks());
  const grows = () => screen.getByRole('region', { name: 'How a deposit grows' });

  it("reads what 10,000 CKB deposited on a day is owed, from the API's calculator", async () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const reading = await within(grows()).findByLabelText('Deposit reading');
    await waitFor(() =>
      expect(
        within(reading).getByText((_, el) => el?.getAttribute('data-role') === 'owed')
      ).toHaveTextContent('+200.00000000 CKB compensation')
    );
    // R2b-a 43: the calculator says which free capacity it assumed (board: "its 9,898 free CKB").
    expect(reading).toHaveTextContent(
      '10,000 CKB deposited on 2019-11-16 is owed +200.00000000 CKB compensation · 2.0000% on its 9,898 free CKB, by #5'
    );
    // One compensation path: the API's calculator, the day's closing block → the evidence tip.
    expect(api.calculateDaoCompensation).toHaveBeenCalledWith('1000000000000', 1, 5);
    expect(grows()).not.toHaveTextContent(/interest|earned/i);
  });

  it('plots AR for every complete day, moves the reading to any day, and bars each whole year', async () => {
    stubChartWidth(600);
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const chart = await within(grows()).findByRole('img', { name: /^AR by date/ });
    // one point per complete day (the x axis prints the first and last)
    expect(chart).toHaveTextContent('2019-11-16');
    expect(chart).toHaveTextContent('2019-11-19');
    fireEvent.keyDown(chart, { key: 'ArrowLeft' });
    const reading = within(grows()).getByLabelText('Deposit reading');
    expect(reading).toHaveTextContent('10,000 CKB deposited on 2019-11-18');
    expect(reading).toHaveTextContent('0.9901% on its');
    await waitFor(() =>
      expect(
        within(reading).getByText((_, el) => el?.getAttribute('data-role') === 'owed')
      ).toHaveTextContent('+98.01980198 CKB compensation')
    );
    expect(api.calculateDaoCompensation).toHaveBeenCalledWith('1000000000000', 3, 5);
    const years = within(grows()).getByRole('img', {
      name: 'Compensation on free capacity in each year since launch',
    });
    // Anniversary to anniversary only: the year in progress (to the tip) is not a bar.
    expect(years.querySelectorAll('[data-year]')).toHaveLength(1);
    expect(years).toHaveTextContent('1.00%from 1970-01');
  });

  it('refuses a daily AR series with a missing day', async () => {
    stubCalculator();
    vi.mocked(api.getDaoArHistory).mockResolvedValue({
      anchor: { blockNumber: 4, blockHash: '0x' + '4'.repeat(64) },
      days: [AR_DAYS[0], AR_DAYS[2], AR_DAYS[3]],
    });
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(await within(grows()).findByRole('alert')).toHaveTextContent(
      'DAO AR history skips from 2019-11-16 to 2019-11-18'
    );
  });

  it('keeps the AR anchors and the observed span out of tier 1 (DAO-9)', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(grows()).not.toHaveTextContent(/anchor|observed span/);
    expect(screen.getByText(/365-day anchor/).closest('[data-tier="2"]')).not.toBeNull();
  });
});

describe('DaoBoard when deposits can leave (DAO-14)', () => {
  it('says what ends its cycle within a day and what is already requested', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const windows = screen.getByRole('region', { name: 'When deposits can leave' });
    expect(windows).toHaveTextContent('days from now · 6 epochs a day2 deposits');
    expect(windows).toHaveTextContent('300.00000000 CKB in 1 deposit ends its cycle within a day.');
    expect(windows).toHaveTextContent('withdrawals already requested · 2 cells · 900.00000000 CKB');
    expect(windows).toHaveTextContent(
      '400.00000000 CKB in 1 cell is past its window and can be claimed now; its compensation stopped growing at the request. 500.00000000 CKB in 1 cell waits for its window.'
    );
    const ready = within(windows).getByRole('img', { name: /^Withdrawal requests/ });
    expect(ready).toHaveAccessibleName(
      'Withdrawal requests: past the window 400.00000000 CKB, waiting 500.00000000 CKB'
    );
    // The next day's bar leads; the rest are quieter (board).
    const bars = windows.querySelectorAll('[data-window-bar]');
    expect(bars).toHaveLength(30);
    expect(bars[0]).toHaveClass('opacity-90');
    expect(bars[1]).toHaveClass('opacity-50');
  });
});

describe('DaoBoard who deposits (DAO-12, DAO-22)', () => {
  const who = () => screen.getByRole('region', { name: 'Who deposits' });

  it('draws the concentration bar: largest, next 9, next 90, everyone else', () => {
    stubCalculator();
    const many = {
      ...evidence,
      phase0: { capacity: '10000', count: 20000 },
      concentration: {
        ...evidence.concentration,
        currentDepositors: 16641,
        top1Capacity: '827',
        top10Capacity: '2620',
        top100Capacity: '5727',
      },
    };
    render(<DaoBoard statistics={{ ...stats, totalDeposited: '10000' }} evidence={many} />);
    expect(within(who()).getByRole('img', { name: /^Share of deposits/ })).toHaveAccessibleName(
      'Share of deposits: largest 8.27%, next 9 17.93%, next 90 31.07%, the other 16,541 42.73%'
    );
  });

  it('names only the groups that exist when there are few depositors', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(within(who()).getByRole('img', { name: /^Share of deposits/ })).toHaveAccessibleName(
      'Share of deposits: largest 60.00%, next 1 40.00%'
    );
  });

  it('lists the largest depositors as rows on desktop and as compact cards at phone width', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(within(who()).getByRole('table', { name: 'Largest depositors' })).toBeInTheDocument();
    expect(
      within(who()).getByRole('list', { name: 'Largest depositors, compact' })
    ).toHaveTextContent('300.00000000');
    // R2b-a 42: every depositor, ranked, on its own page (board: "All N depositors →").
    expect(within(who()).getByRole('link', { name: 'All 2 depositors →' })).toHaveAttribute(
      'href',
      '/mainnet/dao/depositors'
    );
  });
});

describe('DaoBoard tier 2 (DAO-15, DAO-16)', () => {
  // Statistics at the header's block #5, whose S (500000000) = still owed + burnt.
  const sameBlock = {
    ...stats,
    miningReward: '100000000',
    depositCompensation: '300000000',
    totalCompensationPaid: '100000000',
    unclaimedCompensation: '200000000',
    burnt: '300000000',
  };
  const tier2 = (container: HTMLElement) => container.querySelector('[data-tier="2"]')!;

  it('reads the header field as CKB and AR as a rate, and proves S at one block', () => {
    stubCalculator();
    const { container } = render(<DaoBoard statistics={sameBlock} evidence={evidence} />);
    const t2 = tier2(container);
    expect(t2).toHaveTextContent('ARbytes 8..161.0200000000000000 accumulated rate (genesis 1.0)');
    expect(t2).not.toHaveTextContent('10200000000000000 scaled rate');
    expect(t2).toHaveTextContent('Cbytes 0..810,000,000.00000000 CKB issued in total');
    // contracts W5-3 (DAO-16): the identity names the block it holds at, number and hash.
    expect(t2).toHaveTextContent(
      'At block #5 (0xa5a5a5a5...a5a5a5a5), S (5.00000000 CKB) equals the compensation still owed (2.00000000) plus the burnt share (3.00000000) to the shannon: S only counts the burnt share, it does not hold it for anyone. Compensation paid + still owed equals the depositors’ share.'
    );
    expect(within(t2 as HTMLElement).getByRole('link', { name: '#5' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/5'
    );
    expect(within(t2 as HTMLElement).getByTitle(ANCHOR_5.blockHash)).toHaveTextContent(
      '0xa5a5a5a5...a5a5a5a5'
    );
    expect(t2).toHaveTextContent(
      'paid out: (capacity − occupied) × AR(request) ÷ AR(deposit) + occupied'
    );
    expect(t2).toHaveTextContent('102 bytes occupied with a secp256k1 lock: 8 + 53 + 33 + 8');
  });

  it('fails when the statistics row S is not still owed + burnt', () => {
    stubCalculator();
    expect(() =>
      render(
        <DaoBoard
          statistics={{ ...sameBlock, tipSecondaryPool: '500000001' }}
          evidence={evidence}
        />
      )
    ).toThrow('DAO statistics at #5: S 500000001 is not still owed + burnt 500000000');
  });

  it('fails when the statistics anchor is not the block the row was computed at', () => {
    stubCalculator();
    expect(() =>
      render(
        <DaoBoard
          statistics={{ ...sameBlock, anchor: { ...ANCHOR_5, blockNumber: 4 } }}
          evidence={evidence}
        />
      )
    ).toThrow('DAO statistics at #5: anchor #4 is not the block the row was computed at');
  });

  it('states the identity from the statistics row even when the header shown is another block', () => {
    stubCalculator();
    const { container } = render(
      <DaoBoard
        statistics={{
          ...sameBlock,
          tipBlockNumber: 4,
          anchor: { blockNumber: 4, blockHash: '0x' + 'b4'.repeat(32) },
          tipSecondaryPool: '500000000',
        }}
        evidence={evidence}
      />
    );
    const t2 = tier2(container);
    expect(t2).toHaveTextContent('The DAO field in every block header · #5');
    expect(t2).toHaveTextContent(
      'At block #4 (0xb4b4b4b4...b4b4b4b4), S (5.00000000 CKB) equals the compensation still owed'
    );
    expect(t2).not.toHaveTextContent('not shown across two blocks');
  });
});

describe('DaoBoard layout at 1440 and 390 (self-check findings)', () => {
  afterEach(() => vi.restoreAllMocks());

  it('places the page intro in the hero beside the metrics (board grid)', () => {
    stubCalculator();
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        intro={<h1>Nervos DAO</h1>}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    const hero = screen.getByRole('region', { name: 'Nervos DAO summary' });
    expect(within(hero).getByRole('heading', { name: 'Nervos DAO' })).toBeInTheDocument();
  });

  it('sizes the deposited amount so 22 digits fit a 358 px column (390 px overflow)', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const amount = screen.getByText((_, el) => el?.getAttribute('data-role') === 'deposited');
    expect(amount).toHaveClass('text-[24px]', 'sm:text-[30px]');
  });

  it('draws AR around its own range, not from zero', async () => {
    stubChartWidth(600);
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const chart = await within(
      screen.getByRole('region', { name: 'How a deposit grows' })
    ).findByRole('img', { name: /^AR by date/ });
    const ticks = Array.from(chart.querySelectorAll('text[text-anchor="end"]')).map((t) =>
      t.textContent!.trim()
    );
    expect(ticks[0]).not.toBe('0');
    // the daily series spans 1.000 … 1.015
    expect(ticks).toContain('1.010');
  });
});

describe('DaoBoard tier-1 rhythm (board V63)', () => {
  it('keeps tier-1 sections 48 px apart with no rule between them', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const issuance = screen.getByRole('region', { name: 'Where secondary issuance goes' });
    expect(issuance).not.toHaveClass('border-t');
    expect(issuance.parentElement).toHaveClass('space-y-12');
    expect(within(issuance).getByRole('heading', { level: 2 })).toHaveClass(
      'text-[15px]',
      'font-medium'
    );
  });
});

describe('DaoBoard 24 h counts (DAO-10, R2b-a 41)', () => {
  it('says one deposit and one request in the singular, and nothing without a 24 h window', () => {
    stubCalculator();
    const { rerender } = render(
      <DaoBoard
        statistics={{ ...stats, deposits24h: 1, withdrawalRequests24h: 1 }}
        evidence={evidence}
      />
    );
    const hero = () => screen.getByRole('region', { name: 'Nervos DAO summary' });
    expect(hero()).toHaveTextContent('1 deposit in, 1 withdrawal request out');
    rerender(
      <DaoBoard
        statistics={{
          ...stats,
          deposits24h: null,
          withdrawalRequests24h: null,
          depositChange24h: null,
        }}
        evidence={evidence}
      />
    );
    expect(hero()).not.toHaveTextContent(/ in, .* out/);
  });

  it('refuses a 24 h window with one count and not the other', () => {
    stubCalculator();
    expect(() =>
      render(
        <DaoBoard statistics={{ ...stats, withdrawalRequests24h: null }} evidence={evidence} />
      )
    ).toThrow('DAO statistics at #5: 24 h deposits 12 without its withdrawal-request count');
  });
});
