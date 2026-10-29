import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
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
  timestamp: '2026-10-28T00:00:00Z',
  protocolActions: [
    { protocol: 'dao', protocolName: 'Nervos DAO', action: 'withdraw_request', metadata: {} },
  ],
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
      data: [{ hash: tx, blockNumber: 123, timestamp: '2026-10-28T00:00:00Z', activity }],
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
    expect(await screen.findByText('withdraw request')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /0xaaaaaaaaaa/ })).toHaveAttribute(
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
    fireEvent.click(screen.getByRole('button', { name: 'transactions' }));
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenCalledWith({
        activityType: 'dao',
        limit: 12,
        cursor: undefined,
      })
    );
    expect(await screen.findByText('withdraw request')).toBeInTheDocument();
  });

  it('reports source errors without inventing rows', async () => {
    vi.mocked(api.getGlobalActivities).mockRejectedValue(new Error('canonical index missing'));
    render(<DaoSources />);
    expect(await screen.findByRole('alert')).toHaveTextContent('canonical index missing');
    expect(screen.queryByRole('link', { name: '#123' })).not.toBeInTheDocument();
  });
});
