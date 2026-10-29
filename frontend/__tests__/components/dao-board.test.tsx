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
  daoFreeCapacity: '40000000000',
  depositorAllocationWeight: '40000000000',
  tipTotalIssuance: '1000000000000000',
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
