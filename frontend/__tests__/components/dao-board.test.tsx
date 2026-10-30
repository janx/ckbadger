import { describe, expect, it, vi } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DaoBoard } from '@/components/dao/dao-board';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';
import evidence from '@/__tests__/fixtures/dao-evidence.json';

vi.mock('@/components/dao/dao-sources', () => ({
  DaoSources: () => <div data-testid="sources" />,
}));

const stats = {
  ...daoStatistics,
  tipBlockNumber: 5,
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
    expect(screen.getAllByRole('listitem')).toHaveLength(30);
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
    expect(screen.getByText(/Waiting for epoch lock:/)).toBeInTheDocument();
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
    expect(
      screen.getByText((_, el) => el?.textContent === 'Deposited: 500.00000000 CKB')
    ).toBeInTheDocument();
    expect(
      screen.getByText((_, el) => el?.textContent === 'Share of circulating CKB: 1.25%')
    ).toBeInTheDocument();
  });

  it('keeps the owner link bound to the source address', () => {
    render(<DaoBoard statistics={stats} evidence={evidence} />);
    const owner = evidence.concentration.owners[0];
    expect(
      screen.getByRole('link', { name: `${owner.address.slice(0, 12)}…${owner.address.slice(-7)}` })
    ).toHaveAttribute('href', `/mainnet/address/${owner.address}`);
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
    expect(screen.getByText('DAO depositors 4.50%')).toBeInTheDocument();
    expect(screen.getByTitle('DAO depositors: 450.00000000 CKB')).toBeInTheDocument();
    expect(screen.getByText(/phase-0 free capacity plus the compensation/)).toBeInTheDocument();
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
    expect(table).toHaveClass('min-w-[640px]');
    expect(table.parentElement).toHaveClass('overflow-x-auto');
    const [, , , deposits, amount] = Array.from(table.querySelectorAll('tbody tr')[0].children);
    expect(deposits).toHaveClass('whitespace-nowrap');
    expect(amount).toHaveClass('whitespace-nowrap');
  });

  it('reads the 365-day AR anchor as a rate, a UTC time and a span, not raw integers (#11)', () => {
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
      '365-day anchor: block #3 · chain time 2025-10-29 10:32:50 UTC · AR 1.0100000000000000 · observed span 365 d'
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
      'The DAO field in block #5',
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
        }}
        evidence={evidence}
      />
    );
    expect(screen.getByText(/allocation shares are undefined/)).toBeInTheDocument();
    expect(screen.queryByLabelText('Secondary issuance partition')).not.toBeInTheDocument();
  });
});
