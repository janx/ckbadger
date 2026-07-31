import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';

import BitCellItemDetailPage from '@/app/identities/bit-cell/[identityId]/client-page';
import { render } from '@/__tests__/utils/test-utils';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getBitCellItemDetail: vi.fn(),
    getBitCellItemActivities: vi.fn(),
    getAddress: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/src/navigation', () => ({
  usePathname: () => '/identities/bit-cell/0xabc',
  useRouter: () => ({ replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

describe('BitCellItemDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: '0xlock',
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      commonKnowledgeSize: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getBitCellItemActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('renders .bit Cell detail and uses its item endpoints', async () => {
    vi.mocked(api.getBitCellItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit-cell',
      standard: 'bit_cell',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 456,
      expiredAt: 1800000000,
      txHash: '0xtx',
      outputIndex: 3,
    } as any);

    render(<BitCellItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(api.getBitCellItemDetail).toHaveBeenCalledWith('0xabc');
      expect(api.getBitCellItemActivities).toHaveBeenCalledWith('0xabc', { limit: 50 });
    });
    expect(screen.getByText('.BIT CELL')).toBeInTheDocument();
    expect(screen.getByText('.bit Cell ID')).toBeInTheDocument();
    expect(screen.getByText('Expires At')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Back to \.bit Cell Collection/ })).toHaveAttribute(
      'href',
      '/mainnet/identities/bit-cell'
    );
  });
});
