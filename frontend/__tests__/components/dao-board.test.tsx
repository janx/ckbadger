import { afterEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
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
    // Board wording (DAO-14; hierarchy v3) for the same readiness partition.
    expect(screen.getByRole('region', { name: 'When deposits can leave' })).toHaveTextContent(
      'waiting for its window 500.00000000 CKB · 1 cell'
    );
    // Hierarchy D5: the epoch-lock caveat is developer evidence (tier 2), not under the bars.
    expect(
      screen.getByText(/does not prove a complete withdraw transaction/).closest('[data-tier="2"]')
    ).not.toBeNull();
    expect(screen.getByRole('region', { name: 'When deposits can leave' })).not.toHaveTextContent(
      /does not prove a complete withdraw transaction/
    );
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
    const reading = screen.getByRole('status', { name: 'History reading' });
    expect(reading).toHaveTextContent('deposited 500.00000000');
    expect(reading).toHaveTextContent('share 1.25%');
  });

  it('keeps the owner link bound to the source address', () => {
    const owner = evidence.concentration.owners[0];
    // The desktop row and the compact card (DAO-22) both link the same address.
    for (const width of [1248, 358]) {
      stubChartWidth(width);
      const { unmount } = render(<DaoBoard statistics={stats} evidence={evidence} />);
      const links = screen.getAllByRole('link', {
        name: `${owner.address.slice(0, 12)}…${owner.address.slice(-7)}`,
      });
      expect(links).toHaveLength(1);
      expect(links[0]).toHaveAttribute('href', `/mainnet/address/${owner.address}`);
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
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
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    // The board legend (hierarchy D3 wording, tier 2 since v3): the weight's 4.50%, not DAO free
    // capacity's 4.00%.
    expect(container.querySelector('[data-tier="2"]')).toHaveTextContent(
      'DAO deposits, free capacity + compensation accrued 4.50% → depositors'
    );
    expect(screen.getByTitle('DAO depositors: 450.00000000 CKB')).toBeInTheDocument();
    expect(screen.queryByText(/free capacity in DAO/)).not.toBeInTheDocument();
  });

  it('writes AR as the exact rate and links anchors to real block routes (DAO-4, #7)', () => {
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(
      screen.getByText(/AR grew from 1\.0000000000000000 at genesis to 1\.0200000000000000/)
    ).toBeInTheDocument();
    const verify = screen.getByRole('link', { name: /Verify statistics anchor block #5/ });
    expect(verify).toHaveAttribute('href', '/mainnet/blocks/5');
    // Hierarchy D3/D8: the anchor link is developer evidence, under "How the Nervos DAO works".
    expect(verify.closest('[data-tier="2"]')).not.toBeNull();
    expect(screen.getByRole('link', { name: 'Open source block →' })).toHaveAttribute(
      'href',
      `/mainnet/blocks/${evidence.header.blockNumber}`
    );
  });

  it('keeps the depositor table at a width where counts and amounts never fuse (DAO-3)', () => {
    stubChartWidth(1248);
    const { unmount } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const table = screen.getByRole('region', { name: 'Who deposits' }).querySelector('table')!;
    // ListTable: an explicit width budget per column; the table is drawn only where its budget
    // fits (R1), so it never scrolls inside a box …
    expect(parseInt(table.style.minWidth, 10)).toBeGreaterThanOrEqual(640);
    expect(table.parentElement).not.toHaveClass('overflow-x-auto');
    const [, , , deposits, amount] = Array.from(table.querySelectorAll('tbody tr')[0].children);
    expect(deposits).toHaveClass('whitespace-nowrap');
    expect(amount).toHaveClass('whitespace-nowrap');
    unmount();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    // … and compact cards where it does not, so no column hides off-screen at 390 px (DAO-22).
    stubChartWidth(358);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(
      screen.getByRole('list', { name: 'Largest depositors, compact' }).parentElement
    ).not.toHaveClass('overflow-x-auto');
    expect(screen.getByRole('region', { name: 'Who deposits' }).querySelector('table')).toBeNull();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
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

  it('opens tier 2 at 96 px with a hairline and the eyebrow, quiet 13 px headings in two columns (DAO-20, R6, D8)', () => {
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
    expect(tier2).toHaveClass('mt-24', 'border-t', 'border-base-border');
    // A space-y-* parent's sibling selector overrides the 96 px margin (measured 32 px).
    expect(tier2.parentElement!.className).not.toMatch(/space-y-/);
    // "How it works ↓" in the hero lands here.
    expect(container.querySelector('#evidence')).toContainElement(tier2);
    expect(tier2.firstElementChild).toHaveTextContent(/^Evidence · for developers$/);
    const headings = Array.from(tier2.querySelectorAll('h2'));
    expect(headings.map((h) => h.textContent)).toEqual([
      'How the Nervos DAO works',
      'The DAO script',
      'The DAO field in every block header · #5',
    ]);
    for (const h of headings) expect(h).toHaveClass('text-body', 'text-[#a0a8be]');
    // Left column: the concept and the script; right column: the header field (board D8).
    const [works, script, header] = headings.map((h) => h.closest('section')!);
    expect(works.parentElement).toBe(script.parentElement);
    expect(header.parentElement).not.toBe(works.parentElement);
    // The explanation column is the wider one (body prose); the header field the narrower.
    expect(works.parentElement!.parentElement).toHaveClass(
      'grid',
      'lg:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]',
      'gap-x-14'
    );
  });

  it('explains the DAO in tier 2: the former hero paragraph and the issuance explanation (D8)', () => {
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const works = within(container.querySelector('[data-tier="2"]') as HTMLElement)
      .getByRole('heading', { name: 'How the Nervos DAO works' })
      .closest('section')!;
    expect(works).toHaveTextContent(
      'CKB locked in the Nervos DAO keeps its share of the supply. Secondary issuance, 1.344 billion CKB a year, is split by how CKB is used: depositors are compensated with the share their deposits make up, miners get the share that is occupied, and the rest is burnt.'
    );
    expect(works).toHaveTextContent(
      'Every block splits its share the same way: the proportions are simply how all CKB issued by that block is used right then.'
    );
    expect(works).toHaveTextContent(
      'So a deposit keeps its share of all CKB through secondary issuance, while free CKB outside the DAO receives nothing.'
    );
    expect(works).not.toHaveTextContent(/interest|earned/i);
    // R1 roles: the explanation is prose (body 13 px); the protocol notes read as small.
    const [concept, notes] = Array.from(works.querySelectorAll('p'));
    expect(concept).toHaveTextContent(/^CKB locked in the Nervos DAO/);
    expect(concept).toHaveClass('text-body', 'text-text-dim');
    expect(notes).toHaveTextContent(/Epoch-lock readiness/);
    expect(notes).toHaveClass('text-small');
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

describe('DaoBoard hero (DAO-9, DAO-10; hierarchy v3 R11, R13)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const hero = () => screen.getByRole('region', { name: 'Nervos DAO' });
  /** The hero block's metric row: the terms after the hero section (tier 2 has its own list). */
  const terms = () => within(hero().parentElement!).getAllByRole('term');
  /** The metric row's value for a label (dt → its dd). */
  const metric = (label: string) =>
    terms().find((t) => t.textContent === label)!.nextElementSibling!;

  it('leads with deposited as the one gold figure and its 24 h change on the same row (R11, R13)', () => {
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={peakHistory}
        circulationHistory={ratio}
      />
    );
    const figure = within(hero()).getByText((_, el) => el?.getAttribute('data-role') === 'figure');
    expect(figure).toHaveTextContent('deposited500.00000000 CKB');
    const amount = within(figure).getByText(
      (_, el) => el?.getAttribute('data-role') === 'deposited'
    );
    expect(amount).toHaveClass('text-gold');
    // The 24 h change sits right of the figure on its baseline row, signed and coloured by sign;
    // the window's own counts follow it (R2b-a 41).
    const change = within(hero()).getByText(
      (_, el) => el?.getAttribute('data-role') === 'change24h'
    );
    expect(change).toHaveTextContent('−2,578,825.48689593 CKB');
    expect(change).toHaveClass('text-negative');
    const row = amount.parentElement!;
    expect(row).toContainElement(change);
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(change.parentElement).toHaveTextContent(
      '−2,578,825.48689593 CKB in 24 h · 12 deposits in, 9 withdrawal requests out'
    );
    expect(change.parentElement).toHaveClass('text-body');
    // The share and the peak moved into the picture's reading row (board v3).
    expect(hero()).not.toHaveTextContent(/of circulating CKB on/);
    expect(screen.getByRole('status', { name: 'History reading' })).toHaveTextContent(
      'share 1.25% · depositors 2 · peak 600.50000000 on 2024-01-14'
    );
    // Developer evidence (AR anchors) is not in tier 1 (DAO-9).
    expect(hero()).not.toHaveTextContent(/anchor/i);
    expect(screen.queryByText(/Observed AR growth/)).not.toBeInTheDocument();
  });

  it('collapses the metrics into one row: a label, a section-size value and an inline note, no sub-lines (R13)', () => {
    const { container } = render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    expect(terms().map((t) => t.textContent)).toEqual([
      'Estimated APC',
      'Compensation, last 365 days',
      'Depositors',
      'Deposits',
    ]);
    const dl = terms()[0].closest('dl')!;
    expect(dl).toHaveClass('grid-cols-2', 'lg:grid-cols-4');
    for (const t of terms()) {
      expect(t).toHaveClass('text-meta', 'text-text-dim');
      // One value per metric, no sub-line under it.
      expect(t.parentElement!.children).toHaveLength(2);
      expect(t.nextElementSibling!.tagName).toBe('DD');
      expect(t.nextElementSibling).toHaveClass('text-section', 'text-text-bright');
    }
    expect(metric('Estimated APC')).toHaveTextContent(/^1\.99%$/);
    expect(metric('Compensation, last 365 days')).toHaveTextContent(/^2\.0000%$/);
    // depositorsChange24h, signed (−3 in the fixture), inline and quiet.
    expect(metric('Depositors')).toHaveTextContent(/^2 · −3 in 24 h$/);
    expect(metric('Deposits')).toHaveTextContent(/^2 · 2 withdrawals requested$/);
    for (const label of ['Depositors', 'Deposits'])
      expect(metric(label).querySelector('span')).toHaveClass('text-small', 'text-text-dim');
    // The former sub-lines are explanation: they moved to tier 2.
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    for (const moved of [
      /at today's issuance and deposits/,
      /on free capacity, from the header's AR/,
    ]) {
      expect(tier1).not.toHaveTextContent(moved);
      expect(tier2).toHaveTextContent(moved);
    }
  });

  it('signs the depositor change and omits it without a 24 h window', () => {
    const { rerender } = render(
      <DaoBoard statistics={{ ...stats, depositorsChange24h: 9 }} evidence={evidence} />
    );
    expect(metric('Depositors')).toHaveTextContent(/^2 · \+9 in 24 h$/);
    rerender(<DaoBoard statistics={{ ...stats, depositorsChange24h: null }} evidence={evidence} />);
    expect(metric('Depositors')).toHaveTextContent(/^2$/);
  });

  it('says the chain is younger than 365 days inline when there is no 365-day window', () => {
    render(
      <DaoBoard
        statistics={stats}
        evidence={{ ...evidence, ar: { ...evidence.ar, year365: null } }}
      />
    );
    expect(metric('Compensation, last 365 days')).toHaveTextContent(
      /^— · the chain is younger than 365 days$/
    );
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

  it('opens with the title row — h1, the charts link and the range buttons — and a short lede (R13)', () => {
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    const title = within(hero()).getByRole('heading', { level: 1, name: 'Nervos DAO' });
    expect(title).toHaveClass('text-index');
    const row = title.parentElement!;
    expect(row).toContainElement(within(hero()).getByRole('group', { name: 'History range' }));
    expect(row).toContainElement(
      within(hero()).getByRole('link', { name: 'Explore DAO charts →' })
    );
    expect(within(hero()).getByRole('link', { name: 'Explore DAO charts →' })).toHaveAttribute(
      'href',
      '/mainnet/charts/total-deposit'
    );
    // No "History" section or heading: the hero is the history.
    expect(screen.queryByRole('heading', { name: 'History' })).toBeNull();
    expect(screen.queryByRole('region', { name: 'History' })).toBeNull();
    const lede = within(hero()).getByText(/^CKB locked in the DAO keeps its share of the supply\./);
    expect(lede.textContent!.length).toBeLessThanOrEqual(90);
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    expect(within(lede).getByRole('link', { name: 'How it works ↓' })).toHaveAttribute(
      'href',
      '#evidence'
    );
    // Title row, lede, figure: in that order.
    const figure = within(hero()).getByText((_, el) => el?.getAttribute('data-role') === 'figure');
    expect(title.compareDocumentPosition(lede) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(lede.compareDocumentPosition(figure) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('draws the picture right under the figure, then its reading and the metric row (R13)', () => {
    stubChartWidth(1200);
    render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    const figure = within(hero()).getByText((_, el) => el?.getAttribute('data-role') === 'figure');
    const main = hero().querySelector('svg[data-panel="deposited"]')!;
    expect(Number(main.getAttribute('height'))).toBeGreaterThanOrEqual(280);
    expect(figure.compareDocumentPosition(main) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(hero().querySelectorAll('svg[data-panel]')).toHaveLength(3);
    expect(hero().querySelector('figcaption')).toBeNull();
    // The hero section is the figure and its picture; the reading and the metrics follow it.
    const reading = screen.getByRole('status', { name: 'History reading' });
    const metrics = terms()[0].closest('dl')!;
    expect(hero()).not.toContainElement(reading);
    expect(hero().compareDocumentPosition(reading) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      reading.compareDocumentPosition(metrics) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(hero().parentElement).toBe(metrics.parentElement);
  });

  it('keeps the title row and says so when the daily history cannot be read', () => {
    render(<DaoBoard statistics={stats} evidence={evidence} historyError />);
    expect(within(hero()).getByRole('alert')).toHaveTextContent(
      'DAO daily history could not be loaded from indexed snapshots.'
    );
    expect(within(hero()).getByRole('link', { name: 'Explore DAO charts →' })).toBeInTheDocument();
    expect(within(hero()).queryByRole('group', { name: 'History range' })).toBeNull();
  });
});

describe('DaoBoard secondary issuance (DAO-7; hierarchy v3 R12)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const issuance = () => screen.getByRole('region', { name: 'Where secondary issuance goes' });
  const tier2 = (container: HTMLElement) =>
    container.querySelector('[data-tier="2"]') as HTMLElement;

  it('splits issuance three ways and only compensation two ways, as an aligned 48 / 48 / 32 px icicle naming each part and its share', () => {
    stubChartWidth(1400);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const svg = within(issuance()).getByRole('img', {
      name: /Secondary issuance since genesis, split three ways/,
    });
    const box = (key: string) => svg.querySelector(`[data-node="${key}"]`)!;
    for (const key of ['total', 'miners', 'compensation', 'burnt', 'paid', 'owed'])
      expect(box(key)).not.toBeNull();
    expect(['total', 'miners', 'paid'].map((k) => box(k).getAttribute('height'))).toEqual([
      '48',
      '48',
      '32',
    ]);
    // compensation's two parts sit exactly under it
    expect(box('paid').getAttribute('x')).toBe(box('compensation').getAttribute('x'));
    // R12: inside each box only the name and the share (1 + 3 + 3 CKB; 1 of 3 paid out).
    const label = (key: string) => svg.querySelector(`[data-label="${key}"]`)!.textContent;
    expect(label('total')).toBe('secondary issuance since genesis 7 CKB');
    expect(label('miners')).toBe('miners 14.29%');
    expect(label('compensation')).toBe('depositors 42.86%');
    expect(label('burnt')).toBe('burnt 42.86% · no cell can ever claim it');
    expect(label('paid')).toBe('paid out 33.3%');
    expect(label('owed')).toBe('still owed 66.7%');
    // No exact amount and no word under the boxes.
    expect(svg.textContent).not.toMatch(/\.\d{8}/);
    expect(svg.querySelector('[data-placeholder]')).toBeNull();
    expect(svg.querySelector('[data-note]')).toBeNull();
    expect(issuance()).not.toHaveTextContent(/interest|earned/i);
  });

  it('gathers every exact amount in one quiet line under the icicle, then one caption', () => {
    stubChartWidth(1400);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const line = within(issuance()).getByText(
      (_, el) => el?.getAttribute('data-role') === 'exact-amounts'
    );
    expect(line).toHaveTextContent(
      'to miners 1.00000000 · for depositors 3.00000000 (paid out 1.00000000 · still owed 2.00000000 to 22,379 live cells) · burnt 3.00000000 CKB'
    );
    expect(line).toHaveClass('text-small', 'text-text-dim');
    const svg = within(issuance()).getByRole('img', { name: /split three ways/ });
    expect(svg.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const caption = within(issuance()).getByText(
      '1.344 billion CKB a year, split in every block by how CKB is used: a deposit keeps its share, free CKB outside the DAO has its share burnt.'
    );
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    expect(line.compareDocumentPosition(caption) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(issuance().lastElementChild).toBe(caption);
  });

  it('moves capacity use, its legend and its line, and the section note to tier 2 (V7)', () => {
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(within(issuance()).queryByLabelText('Current capacity use partition')).toBeNull();
    expect(issuance()).not.toHaveTextContent(
      /is used at block|occupied by cells|exact to the shannon/
    );
    // No note beside the heading.
    expect(
      within(issuance()).getByRole('heading', { level: 2 }).parentElement!.children
    ).toHaveLength(1);
    const works = within(tier2(container))
      .getByRole('heading', { name: 'How the Nervos DAO works' })
      .closest('section')!;
    expect(within(works).getByLabelText('Current capacity use partition')).toBeInTheDocument();
    expect(works).toHaveTextContent(
      'How all 10,000.00000000 CKB issued so far is used at block #5 · every block splits its issuance in these proportions'
    );
    expect(works).toHaveTextContent('occupied by cells 0.01% → miners');
    expect(works).toHaveTextContent(
      'DAO deposits, free capacity + compensation accrued 4.50% → depositors'
    );
    expect(works).toHaveTextContent('everything else 95.49% → burnt');
    expect(works).toHaveTextContent('Statistics are exact to the shannon at block #5');
    // The icicle's former notes, in words.
    expect(works).toHaveTextContent(
      "The miners' share is s × U ÷ C of each block's secondary issuance s, paid in cellbases; the depositors' share accrues via AR and is paid out at withdrawals."
    );
    // The former caption's long form stays in the explanation.
    expect(works).toHaveTextContent(
      'So a deposit keeps its share of all CKB through secondary issuance, while free CKB outside the DAO receives nothing.'
    );
    // The protocol caveats and the anchor link stay in tier 2 (D3, D8).
    for (const caveat of [
      /Protocol occupied \(U\) includes the genesis virtual occupied baseline; it is not the real occupied size of live cells\./,
      /No cell can ever claim the burnt share\. A hardfork could only put future secondary issuance to use; what is burnt stays burnt\./,
      /withdrawal requests take no share/,
    ]) {
      expect(issuance()).not.toHaveTextContent(caveat);
      expect(tier2(container)).toHaveTextContent(caveat);
    }
    expect(
      within(issuance()).queryByRole('link', { name: /Verify statistics anchor block/ })
    ).toBeNull();
  });

  it('draws capacity use as an 18 px bar with each share written inside the segments that fit (R9)', () => {
    stubChartWidth(1400);
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const bar = within(tier2(container)).getByLabelText('Current capacity use partition');
    expect(bar).toHaveClass('h-[18px]');
    const segments = Array.from(bar.children) as HTMLElement[];
    expect(segments.map((s) => s.title)).toEqual([
      'Occupied: 1.00000000 CKB',
      'DAO depositors: 450.00000000 CKB',
      'Everything else: 9,549.00000000 CKB',
    ]);
    // 0.01 % of 1,400 px holds no label; 4.50 % (63 px) holds "4.50%"; the rest says where it goes.
    expect(segments.map((s) => s.textContent)).toEqual([
      '',
      '4.50%',
      'everything else 95.49% → burnt',
    ]);
  });

  it('writes no share inside the capacity bar before its width is known or where a segment is too narrow', () => {
    stubChartWidth(358);
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const bar = within(tier2(container)).getByLabelText('Current capacity use partition');
    // 4.50 % of 358 px is 16 px, too narrow for "4.50%"; the legend still states every share.
    expect(Array.from(bar.children).map((s) => s.textContent)).toEqual([
      '',
      '',
      'everything else 95.49% → burnt',
    ]);
    expect(tier2(container)).toHaveTextContent(
      'DAO deposits, free capacity + compensation accrued 4.50%'
    );
  });

  it('states every part with its share and exact amount at phone width instead of shrinking the drawing', () => {
    stubChartWidth(358);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const svg = within(issuance()).getByRole('img', {
      name: /Secondary issuance since genesis, split three ways/,
    });
    expect(svg.querySelectorAll('text')).toHaveLength(0);
    const legend = within(issuance()).getByRole('list', { name: 'Secondary issuance parts' });
    expect(legend).toHaveTextContent('secondary issuance since genesis 7.00000000 CKB');
    expect(legend).toHaveTextContent('miners 14.29% · 1.00000000');
    expect(legend).toHaveTextContent('depositors 42.86% · 3.00000000');
    expect(legend).toHaveTextContent('paid out 33.3% · 1.00000000');
    expect(legend).toHaveTextContent('still owed 66.7% · 2.00000000 to 22,379 live cells');
    expect(legend).toHaveTextContent('burnt 42.86% · 3.00000000 · no cell can ever claim it');
    // The one line is the desktop form; the list replaces it.
    expect(
      within(issuance()).queryByText((_, el) => el?.getAttribute('data-role') === 'exact-amounts')
    ).toBeNull();
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
      ).toHaveTextContent(/^\+200\.00000000 CKB$/)
    );
    // Board v3: one line, the owed amount white (R11: the figure is the page's one gold text).
    expect(reading).toHaveTextContent(
      '10,000 CKB deposited 2019-11-16 → +200.00000000 CKB compensation by #5tap or hover'
    );
    expect(
      within(reading).getByText((_, el) => el?.getAttribute('data-role') === 'owed')
    ).toHaveClass('text-text-bright');
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
    expect(reading).toHaveTextContent('10,000 CKB deposited 2019-11-18 →');
    await waitFor(() =>
      expect(
        within(reading).getByText((_, el) => el?.getAttribute('data-role') === 'owed')
      ).toHaveTextContent(/^\+98\.01980198 CKB$/)
    );
    expect(api.calculateDaoCompensation).toHaveBeenCalledWith('1000000000000', 3, 5);
    const years = within(grows()).getByRole('img', {
      name: 'Compensation on free capacity in each year since launch',
    });
    // Anniversary to anniversary only: the year in progress (to the tip) is not a bar.
    const bars = years.querySelectorAll('[data-year]');
    expect(bars).toHaveLength(1);
    // Each bar carries its share above it (small) in 72 px; one axis line names the years under it.
    expect(years).toHaveClass('h-[72px]');
    expect(bars[0]).toHaveTextContent(/^1\.00%$/);
    expect(bars[0].querySelector('span')).toHaveClass('text-small');
    expect(bars[0]).toHaveAttribute('title', 'from 1970-01-01: 1.00%');
    const axis = years.nextElementSibling!;
    expect(axis).toHaveTextContent('compensation in each year since launch · 1970-01 →→ 1970-01');
    expect(axis).toHaveClass('text-meta');
    // Evidence first (D4): the chart, its reading and the year bars, then ONE caption at the bottom,
    // one line inside 640 px.
    const caption = within(grows()).getByText(
      'A deposit grows by AR at its withdrawal request ÷ AR at its deposit.'
    );
    expect(chart.compareDocumentPosition(caption) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(years.compareDocumentPosition(caption) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    expect(caption.textContent!.length * 8).toBeLessThanOrEqual(640);
    expect(grows().lastElementChild).toBe(caption);
  });

  it("names AR inside its 200 px plot and writes the tip AR at the line's end, no note beside the heading (V3)", async () => {
    stubChartWidth(600);
    stubCalculator();
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const chart = await within(grows()).findByRole('img', { name: /^AR by date/ });
    expect(chart).toHaveAttribute('height', '200');
    expect(chart.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      'AR, the accumulated rate · 1.0 at genesis'
    );
    // the tip AR cut (not rounded) to four places
    expect(chart.querySelector('[data-role="end-label"]')).toHaveTextContent(/^1\.0150$/);
    expect(grows().querySelector('figcaption')).toBeNull();
    const heading = within(grows()).getByRole('heading', { level: 2 });
    expect(heading).toHaveTextContent('How a deposit grows');
    // The heading stands alone: no note beside it.
    expect(heading.parentElement).toBe(grows());
    expect(grows()).not.toHaveTextContent(/at the tip/);
    // The opening paragraph and the rest of the caption read in the evidence.
    expect(grows()).not.toHaveTextContent(/growth index|since genesis/);
    const tier2 = container.querySelector('[data-tier="2"]');
    expect(tier2).toHaveTextContent("AR, in every block header, is the DAO's growth index.");
    expect(tier2).toHaveTextContent(
      'A deposit grows on the capacity it does not occupy; the curve flattens as more CKB exists.'
    );
    expect(tier2).toHaveTextContent(
      "The deposit reading under “How a deposit grows” is the API calculator's for 10,000 CKB in a standard deposit cell: compensation accrues on its free capacity, the CKB the cell does not occupy."
    );
  });

  it('a tap on the AR chart pins that day in the reading under it; a second tap lets go (R6)', async () => {
    stubChartWidth(600);
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const chart = await within(grows()).findByRole('img', { name: /^AR by date/ });
    const reading = within(grows()).getByRole('status', { name: 'Deposit reading' });
    // the reading is the line under the figure, with the shared hint
    expect(chart.compareDocumentPosition(reading) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(reading).toHaveTextContent('tap or hover');
    // a touch tap on the last day (the plot ends at 590 of 600 px): the pointer leaves first
    fireEvent.pointerLeave(chart);
    fireEvent.click(chart, { clientX: 590 });
    expect(reading).toHaveTextContent(/^tapped 10,000 CKB deposited 2019-11-19 →/);
    await waitFor(() =>
      expect(api.calculateDaoCompensation).toHaveBeenCalledWith('1000000000000', 4, 5)
    );
    fireEvent.pointerLeave(chart);
    expect(reading).toHaveTextContent(/^tapped 10,000 CKB deposited 2019-11-19/);
    fireEvent.click(chart, { clientX: 590 });
    expect(reading).toHaveTextContent(/^10,000 CKB deposited 2019-11-16 →/);
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

describe('DaoBoard when deposits can leave (DAO-14; hierarchy v3 V4)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const windows = () => screen.getByRole('region', { name: 'When deposits can leave' });

  it('draws 200 px bars named inside the plot, one axis line and one reading row (V4)', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const cycles = within(windows()).getByRole('list', { name: 'Phase-0 cycle boundaries' });
    const items = within(cycles).getAllByRole('listitem');
    for (const item of items) {
      expect(item).toHaveClass('h-[200px]');
      // no day number under each bar any more: the axis line names the days
      expect(item.textContent).toBe('');
    }
    const name = within(windows()).getByText('CKB whose 180-epoch cycle ends in the next 30 days');
    expect(name).toHaveClass('text-meta', 'text-text-dim', 'absolute');
    expect(name.parentElement).toContainElement(cycles);
    const axis = cycles.parentElement!.nextElementSibling!;
    expect(Array.from(axis.children).map((c) => c.textContent)).toEqual([
      'in a day',
      'day 10',
      'day 20',
      'day 30',
    ]);
    expect(axis).toHaveClass('text-meta');
    // The next day's bar leads; the rest are quieter (board).
    const bars = windows().querySelectorAll('[data-window-bar]');
    expect(bars).toHaveLength(30);
    expect(bars[0]).toHaveClass('opacity-90');
    expect(bars[1]).toHaveClass('opacity-50');
    // The readout row under the bars (hairline, tap or hover): the next day's cycle end, one line.
    const reading = within(windows()).getByRole('status', { name: 'Cycle reading' });
    expect(reading).toHaveClass('border-t', 'text-small');
    expect(reading).toHaveTextContent(
      /^300\.00000000 CKB in 1 deposit ends its cycle within a daytap or hover$/
    );
    // No note beside the heading, no "days from now" line.
    const heading = within(windows()).getByRole('heading', { level: 2 });
    expect(heading.parentElement!.children).toHaveLength(1);
    expect(windows()).not.toHaveTextContent(/indexed epoch|days from now|6 epochs a day/);
  });

  it('reads any day of the bars on focus or hover, exactly', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    fireEvent.focus(
      screen.getByTitle(/Nominal day 1, epochs \+0 to \+6: 1 deposits, 30000000000 shannons/)
    );
    expect(within(windows()).getByRole('status', { name: 'Cycle reading' })).toHaveTextContent(
      'Nominal day 1 · epochs +0 through +6 · 300.00000000 CKB in 1 deposits'
    );
  });

  it('draws the requested withdrawals as a 28 px bar with the claimable amount inside it and the waiting one under it', () => {
    stubCalculator();
    stubChartWidth(800);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(windows()).toHaveTextContent('withdrawals already requested · 2 cells');
    const bar = within(windows()).getByRole('img', { name: /^Withdrawal requests/ });
    expect(bar).toHaveAccessibleName(
      'Withdrawal requests: past the window 400.00000000 CKB, waiting 500.00000000 CKB'
    );
    expect(bar).toHaveClass('h-[28px]');
    // 400 of 900 is 44 % of 800 px: room for the claimable words inside its segment.
    expect(bar.firstElementChild).toHaveTextContent('claimable now 400.00000000 CKB · 1 cell');
    const waiting = bar.nextElementSibling!;
    expect(waiting).toHaveTextContent(/^waiting for its window 500\.00000000 CKB · 1 cell$/);
    expect(waiting).toHaveClass('text-meta', 'text-right');
  });

  it('writes the claimable amount under the bar where its segment is too narrow for it', () => {
    stubCalculator();
    stubChartWidth(358);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const bar = within(windows()).getByRole('img', { name: /^Withdrawal requests/ });
    expect(bar.firstElementChild!.textContent).toBe('');
    const under = bar.nextElementSibling!;
    expect(under).toHaveTextContent(
      'claimable now 400.00000000 CKB · 1 cellwaiting for its window 500.00000000 CKB · 1 cell'
    );
  });

  it('ends with one one-line caption and keeps the cycle notes, the epoch and the caveats in tier 2', () => {
    stubCalculator();
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const caption = within(windows()).getByText(
      'A withdrawal completes only at the end of a 180-epoch cycle, about 30 days.'
    );
    const bar = within(windows()).getByRole('img', { name: /^Withdrawal requests/ });
    expect(bar.compareDocumentPosition(caption) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    expect(caption.textContent!.length * 8).toBeLessThanOrEqual(640);
    expect(windows().lastElementChild).toBe(caption);
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    for (const moved of [
      /A deposit runs in cycles of 180 epochs, about 30 days, counted from the epoch it was made\./,
      /each bar is the CKB whose current cycle ends that day: six exact epochs from the indexed tip, epoch ranges rather than predicted calendar dates\./,
      /compensation stopped growing at the request/,
      /Epoch-lock readiness alone does not prove a complete withdraw transaction; the required input, headers, witnesses and script execution still apply\./,
      /the indexed epoch is 185\+2\/4/,
    ]) {
      expect(windows()).not.toHaveTextContent(moved);
      expect(tier2).toHaveTextContent(moved);
    }
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

  it('writes the group names and shares inside a 28 px concentration bar, with no legend row (R15)', () => {
    stubCalculator();
    stubChartWidth(1408);
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
    const { container } = render(
      <DaoBoard statistics={{ ...stats, totalDeposited: '10000' }} evidence={many} />
    );
    const bar = within(who()).getByRole('img', { name: /^Share of deposits/ });
    expect(bar).toHaveClass('h-[28px]');
    expect(Array.from(bar.children).map((s) => s.textContent)).toEqual([
      'largest 8.27%',
      'next 9 · 17.93%',
      'next 90 · 31.07%',
      'the other 16,541 · 42.73%',
    ]);
    expect(container.querySelector('[data-role="concentration-legend"]')).toBeNull();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
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
    stubChartWidth(1248);
    const { unmount } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const table = within(who()).getByRole('table', { name: 'Largest depositors' });
    // Board v2: depositor rows read at body (13 px) under quieter column heads; v3 (R15): rows of
    // 32–34 px (6 px above and below a 20.8 px line).
    for (const cell of Array.from(table.querySelectorAll('tbody td')))
      expect(cell).toHaveClass('text-body', 'py-1.5');
    // The Lock column names the lock; its code hash and hash type are the link's title.
    const lock = within(table).getAllByRole('link', { name: /^0x9bd7e06f/ })[0];
    expect(lock.textContent).toBe('0x9bd7e06f…');
    expect(lock).toHaveAttribute(
      'title',
      '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8 (type)'
    );
    unmount();
    vi.unstubAllGlobals();
    stubChartWidth(358);
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    expect(
      within(who()).getByRole('list', { name: 'Largest depositors, compact' })
    ).toHaveTextContent('300.00000000');
    vi.unstubAllGlobals();
    // R2b-a 42: every depositor, ranked, on its own page (board: "All N depositors →").
    expect(within(who()).getByRole('link', { name: 'All 2 depositors →' })).toHaveAttribute(
      'href',
      '/mainnet/dao/depositors'
    );
  });

  it('shows the five largest depositors on the topic page and links the rest (D6, R8)', () => {
    stubCalculator();
    // Seven owners in the evidence; the board lists five, the link goes to all of them.
    const [first] = evidence.concentration.owners;
    const owners = Array.from({ length: 7 }, (_, i) => ({
      ...first,
      rank: i + 1,
      address: `${first.address.slice(0, -7)}owner0${i + 1}`,
      lockScriptHash: '0x' + String(i + 1).repeat(64),
    }));
    const seven = { ...evidence, concentration: { ...evidence.concentration, owners } };
    for (const width of [1248, 358]) {
      stubChartWidth(width);
      const { unmount } = render(<DaoBoard statistics={stats} evidence={seven} />);
      const rows =
        width === 1248
          ? within(who())
              .getByRole('table', { name: 'Largest depositors' })
              .querySelectorAll('tbody tr')
          : within(
              within(who()).getByRole('list', { name: 'Largest depositors, compact' })
            ).getAllByRole('listitem');
      expect(rows).toHaveLength(5);
      expect(within(who()).queryByText(/owner06$/)).toBeNull();
      expect(within(who()).getByRole('link', { name: 'All 2 depositors →' })).toBeInTheDocument();
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
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
    // The feed's former note (hierarchy v3, V6).
    expect(t2).toHaveTextContent(
      '“Recent DAO activity” lists deposits, withdrawal requests and withdrawals, newest first.'
    );
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

  it('sizes the deposited amount so 22 digits fit a 358 px column (390 px overflow)', () => {
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const amount = screen.getByText((_, el) => el?.getAttribute('data-role') === 'deposited');
    // R4: the display step from 640 up, index (26 px) below it: 13 + 9 characters at 26 / 22 px
    // are 322 px, and the unit's space is set at body, so " CKB" ends at 353 px. R11: the page's
    // one gold text, its fraction the dim gold.
    expect(amount).toHaveClass('text-index', 'sm:text-display', 'text-gold');
    // The fraction (its trailing zeros quieter still) is one hero-sized, dim-gold run.
    const fraction = amount.querySelector('.text-hero')!;
    expect(fraction).toHaveTextContent(/^\.00000000$/);
    expect(fraction).toHaveClass('text-gold-dim');
    expect(within(amount).getByText(/CKB$/)).toHaveClass('text-body', 'text-text-dim');
  });

  it('draws AR around its own range, not from zero', async () => {
    stubChartWidth(600);
    stubCalculator();
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const chart = await within(
      screen.getByRole('region', { name: 'How a deposit grows' })
    ).findByRole('img', { name: /^AR by date/ });
    // Round values inside the plot (hierarchy v3), not in a gutter.
    const ticks = Array.from(chart.querySelectorAll('[data-role="y-tick"]')).map((t) =>
      t.textContent!.trim()
    );
    expect(ticks[0]).not.toBe('0');
    // the daily series spans 1.000 … 1.015
    expect(ticks).toContain('1.010');
  });
});

describe('DaoBoard tier-1 rhythm (hierarchy D9, R6)', () => {
  it('spaces the first section 64 px under the hero, the rest 72 px apart, the two columns 56 px apart', () => {
    stubCalculator();
    const { container } = render(<DaoBoard statistics={stats} evidence={evidence} />);
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    // Explicit gaps, no space-y-* (its sibling selector would decide the margins instead).
    expect(tier1.className).not.toMatch(/space-y-/);
    const block = (name: string) => screen.getByRole('region', { name }).parentElement!;
    // The hero block (the hero section, its reading and the metric row) opens tier 1.
    expect(block('Nervos DAO').parentElement).toBe(tier1);
    expect(tier1.firstElementChild).toBe(block('Nervos DAO'));
    expect(block('Where secondary issuance goes')).toHaveClass('mt-16');
    const columns = block('How a deposit grows');
    expect(columns).toBe(block('When deposits can leave'));
    expect(columns).toHaveClass('mt-[72px]', 'lg:grid-cols-2', 'lg:gap-x-14', 'gap-y-[72px]');
    expect(block('Who deposits')).toHaveClass('mt-[72px]');
    expect(screen.getByTestId('sources').parentElement).toHaveClass('mt-[72px]');
    for (const child of Array.from(tier1.children)) expect(child.parentElement).toBe(tier1);
    // No rule between tier-1 sections; their headings are the section step (board).
    const issuance = screen.getByRole('region', { name: 'Where secondary issuance goes' });
    expect(
      within(screen.getByRole('region', { name: 'Who deposits' })).getByRole('link', {
        name: /depositors →$/,
      })
    ).toHaveClass('text-small');
    expect(issuance).not.toHaveClass('border-t');
    expect(within(issuance).getByRole('heading', { level: 2 })).toHaveClass(
      'text-section',
      'font-medium'
    );
  });

  it('keeps one gold text in tier 1: the deposited figure (R11, V8)', async () => {
    stubChartWidth(1400);
    stubCalculator();
    const { container } = render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    const grows = screen.getByRole('region', { name: 'How a deposit grows' });
    await within(grows).findByRole('img', { name: /^AR by date/ });
    await waitFor(() =>
      expect(within(grows).getByText((_, el) => el?.getAttribute('data-role') === 'owed'))
    );
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const deposited = tier1.querySelector('[data-role="deposited"]')!;
    // HTML text: every gold ink class sits on the figure (its fraction is the dim gold).
    const goldInk = Array.from(tier1.querySelectorAll('[class]')).filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => /^text-(gold|warning)(-dim|-bright)?(\/\d+)?$/.test(c))
    );
    expect(goldInk.length).toBeGreaterThan(0);
    for (const el of goldInk) expect(deposited.contains(el)).toBe(true);
    // SVG text: none drawn in gold (the plots' fills, strokes and bars may be gold).
    for (const t of Array.from(tier1.querySelectorAll('svg text, svg tspan'))) {
      expect(t.getAttribute('class') ?? '').not.toMatch(/fill-gold/);
      expect((t.getAttribute('fill') ?? '').toLowerCase()).not.toMatch(/#f2c55c|#d0a840/);
    }
  });

  it('names every tier-1 chart inside its plot and keeps captions and the lede within 640 px (R13, R14)', async () => {
    stubChartWidth(1400);
    stubCalculator();
    const { container } = render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    await within(screen.getByRole('region', { name: 'How a deposit grows' })).findByRole('img', {
      name: /^AR by date/,
    });
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    expect(tier1.querySelector('figcaption')).toBeNull();
    // Captions and the lede (the exact-amounts line is data, one line across the column).
    const prose = Array.from(
      tier1.querySelectorAll('section > p:not([data-role="exact-amounts"])')
    );
    expect(prose.length).toBeGreaterThanOrEqual(4);
    for (const p of prose) expect(p.className).toMatch(/(^|\s)max-w-\[(640|560)px\](\s|$)/);
    // "tap or hover" at most once per section (R14).
    for (const section of Array.from(tier1.querySelectorAll('section')))
      expect(section.querySelectorAll('[data-hint="tap-or-hover"]').length).toBeLessThanOrEqual(1);
  });

  it('keeps tier 1 within the prose budget: at most three caption-length blocks (§0, §4)', async () => {
    stubChartWidth(1400);
    stubCalculator();
    const { container } = render(
      <DaoBoard
        statistics={stats}
        evidence={evidence}
        depositedHistory={history}
        circulationHistory={ratio}
      />
    );
    await within(screen.getByRole('region', { name: 'How a deposit grows' })).findByRole('img', {
      name: /^AR by date/,
    });
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
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
    const hero = () => screen.getByRole('region', { name: 'Nervos DAO' });
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
