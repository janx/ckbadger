import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DaoSources } from '@/components/dao/dao-sources';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
  api: { getGlobalActivities: vi.fn(), getTransactions: vi.fn() },
}));

const tx = '0x' + 'a'.repeat(64);
const activity = {
  txHash: tx,
  txIndex: 2,
  blockNumber: 123,
  // The activity API sends chain time as milliseconds; the transaction API as RFC 3339.
  timestamp: '1793268770452',
  isCellbase: false,
  participants: [],
  typeCalls: [],
  lockCalls: [],
  protocolActions: [
    {
      protocol: 'dao',
      protocolName: 'Nervos DAO',
      action: 'withdraw_request',
      metadata: { capacity: '2605000000000', depositBlock: '19695550' },
    },
  ],
};
const transaction = {
  hash: tx,
  blockNumber: 123,
  blockHash: '0x' + 'b'.repeat(64),
  index: 2,
  inputsCount: 1,
  outputsCount: 1,
  fee: '1000',
  value: '0',
  txSize: 600,
  cycles: 1000,
  cyclesStatus: 'complete',
  isCellbase: false,
  timestamp: '2026-10-29T11:12:50.452+00:00',
  activity,
};

describe('DaoSources', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [activity],
      limit: 12,
      total: null,
      hasMore: true,
      nextCursor: 'next',
    } as any);
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [transaction],
      limit: 12,
      total: null,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('loads canonical DAO-filtered activities and linked source transactions', async () => {
    render(<DaoSources />);
    await waitFor(() =>
      expect(api.getGlobalActivities).toHaveBeenCalledWith({
        filter: 'dao',
        limit: 12,
        cursor: undefined,
      })
    );
    // The shared protocol-action name (DAO-13), on the row or the table and its compact layout.
    expect((await screen.findAllByText(/withdraw request/i)).length).toBeGreaterThan(0);
    expect(screen.getByRole('link', { name: /^0xaaaaaaaa/ })).toHaveAttribute(
      'href',
      `/mainnet/tx/${tx}`
    );
    expect(screen.getByRole('link', { name: '#123' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/123'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Older DAO activities →' }));
    await waitFor(() =>
      expect(api.getGlobalActivities).toHaveBeenCalledWith({
        filter: 'dao',
        limit: 12,
        cursor: 'next',
      })
    );
  });

  it('uses the DAO transaction filter on the Transactions tab', async () => {
    render(<DaoSources />);
    fireEvent.click(screen.getByRole('tab', { name: /transactions/i }));
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenCalledWith({
        activityType: 'dao',
        limit: 12,
        cursor: undefined,
      })
    );
    // The shared protocol-action name (DAO-13), on the row or the table and its compact layout.
    expect((await screen.findAllByText(/withdraw request/i)).length).toBeGreaterThan(0);
  });

  it('lists DAO rows with the shared activity row and transaction table (DAO-13)', async () => {
    const { container } = render(<DaoSources />);
    const row = await screen.findByText(/withdraw request/i);
    const article = row.closest('article[data-feed-row]');
    expect(article).toHaveAttribute('data-feed-row', tx);
    // The vault ring and the action's own amounts come with the shared row.
    expect(article!.querySelector('svg[data-class="dao"]')).not.toBeNull();
    expect(article).toHaveTextContent('26,050');
    expect(container.querySelector('table')).toBeNull();
    // Raw milliseconds never reach the page (DAO-1).
    expect(screen.queryByText(/1793268770452/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('tab', { name: /transactions/i }));
    // The DaoTxs board's own table (DAO-13), on the shared ListTable.
    const table = await screen.findByRole('table', { name: 'DAO transactions' });
    // Same protocol-action name as the Activities tab.
    expect(table).toHaveTextContent(/withdraw request/i);
  });

  it('reports source errors without inventing rows', async () => {
    vi.mocked(api.getGlobalActivities).mockRejectedValue(new Error('canonical index missing'));
    render(<DaoSources />);
    expect(await screen.findByRole('alert')).toHaveTextContent('canonical index missing');
    expect(screen.queryByRole('link', { name: '#123' })).not.toBeInTheDocument();
  });
});

// ---- F2 group dao (round-2 fix) ----
describe('DaoSources board columns (DAO-13, DAO-22)', () => {
  const completion = {
    ...transaction,
    hash: '0x' + 'c'.repeat(64),
    blockNumber: 20874429,
    timestamp: '2026-10-30T03:40:00.000+00:00',
    activity: {
      ...activity,
      txHash: '0x' + 'c'.repeat(64),
      blockNumber: 20874429,
      participants: [
        {
          address:
            'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfzyg3zyg3zyg3zyg3zyg3zyg3zyg3zygssd2hqn',
          lockHash: '0x' + 'd'.repeat(64),
          lockHashPrefix: null,
          roles: [],
          ckbDelta: '781583043832627',
          usedDelta: '0',
          itemDeltas: [],
          tags: 16,
        },
      ],
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'withdraw_complete',
          metadata: { capacity: '781583043832627', compensation: '2572859558410' },
        },
      ],
    },
  };

  beforeEach(() => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [transaction, completion],
      limit: 12,
      total: null,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('gives the Transactions tab the board columns: step, deposit, compensation, parties', async () => {
    render(<DaoSources />);
    fireEvent.click(screen.getByRole('tab', { name: /transactions/i }));
    const table = await screen.findByRole('table', { name: 'DAO transactions' });
    expect(Array.from(table.querySelectorAll('th')).map((th) => th.textContent)).toEqual([
      'Transaction',
      'Block · UTC+8',
      'DAO step',
      'Deposit',
      'Compensation',
      'Parties',
    ]);
    const rows = table.querySelectorAll('tbody tr');
    expect(rows[0]).toHaveTextContent('DAO Withdraw Request');
    expect(rows[0]).toHaveTextContent('26,050.00000000');
    expect(rows[1]).toHaveTextContent('#20,874,429 · 2026-10-30 11:40');
    expect(rows[1]).toHaveTextContent('7,815,830.43832627');
    const comp = rows[1].querySelector('[data-role="compensation"]')!;
    expect(comp).toHaveTextContent('25,728.5955841');
    expect(comp).toHaveClass('text-gold');
    // Gold reaches the digits: nothing inside sets its own text colour (self-check: it was grey).
    expect(comp.querySelector('[class*="text-text"]')).toBeNull();
    expect(rows[1].lastElementChild).toHaveTextContent('1');
  });

  it('marks the tabs as tabs and lets the note wrap below them on phones', () => {
    render(<DaoSources />);
    const tabs = screen.getByRole('tablist', { name: 'What happened' });
    expect(within(tabs).getByRole('tab', { name: /activities/i })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    const note = screen.getByText('deposits, withdrawal requests and withdrawals · newest first');
    expect(note).toHaveClass('basis-full', 'sm:basis-auto');
  });
});
