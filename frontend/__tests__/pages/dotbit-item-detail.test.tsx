import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';

import DotbitItemDetailPage from '@/app/identities/dotbit/[identityId]/client-page';
import { api } from '@/lib/api';
import { render } from '../utils/test-utils';

vi.mock('@/lib/api', () => ({
  api: {
    getDotbitItemDetail: vi.fn(),
    getDotbitItemActivities: vi.fn(),
    getAddress: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockReplace = vi.fn();
let mockSearchParams = new URLSearchParams();

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ identityId: '0xabc' }),
  usePathname: () => '/identities/dotbit/0xabc',
  useRouter: () => ({ replace: mockReplace }),
  useSearchParams: () => mockSearchParams,
}));

describe('DotbitItemDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockReset();
    mockSearchParams = new URLSearchParams();
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: '0xlock',
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      commonKnowledgeSize: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getDotbitItemActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('hydrates activity cursor from URL query', async () => {
    mockSearchParams = new URLSearchParams('activity_cursor=500:0');
    vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      expiredAt: 1800000000,
      txHash: '0xtx',
      outputIndex: 2,
    } as any);

    render(<DotbitItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(api.getDotbitItemActivities).toHaveBeenCalledWith('0xabc', {
        limit: 50,
        cursor: '500:0',
      });
    });
  });

  it('renders dotbit labels, back link, and recycled status message', async () => {
    vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xlock',
      isLive: false,
      createdAtBlock: 123,
      expiredAt: 1800000000,
      txHash: null,
      outputIndex: null,
    } as any);

    render(<DotbitItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('Asset Snapshot')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: /Back to \.bit Collection/ })).toHaveAttribute(
      'href',
      '/identities/dotbit'
    );
    expect(screen.getByText('Identity & Ownership')).toBeInTheDocument();
    expect(screen.getByText('Cell Status')).toBeInTheDocument();
    expect(screen.getAllByText('alice.bit').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Recycled').length).toBeGreaterThan(0);
    expect(screen.getByText('Recycled .bit account has no live cell.')).toBeInTheDocument();
  });

  it('renders activities with correct timestamp parsing', async () => {
    vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      expiredAt: 1800000000,
      txHash: '0xtx',
      outputIndex: 2,
    } as any);
    vi.mocked(api.getDotbitItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xacttx',
          blockNumber: 456,
          txIndex: 0,
          timestamp: '1700000300000',
          actions: ['burn', 'transfer'],
        },
      ],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);

    render(<DotbitItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(api.getDotbitItemActivities).toHaveBeenCalledWith('0xabc', { limit: 50 });
    });

    await waitFor(() => {
      expect(screen.getAllByText('recycled, transfer').length).toBeGreaterThan(0);
    });
    expect(screen.getByText(/2023/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '#456' })).toHaveAttribute('href', '/blocks/456');
    expect(
      screen.getAllByRole('link').some((link) => link.getAttribute('href') === '/tx/0xacttx')
    ).toBe(true);
  });

  it('applies cursor pagination', async () => {
    vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      expiredAt: 1800000000,
      txHash: '0xtx',
      outputIndex: 2,
    } as any);
    vi.mocked(api.getDotbitItemActivities).mockImplementation(async (_nftId, params) => {
      if (params?.cursor === '500:0') {
        return {
          data: [
            {
              txHash: '0xpage2',
              blockNumber: 400,
              txIndex: 0,
              timestamp: '1700000400',
              actions: ['mint'],
            },
          ],
          limit: 50,
          hasMore: false,
          nextCursor: null,
        } as any;
      }
      return {
        data: [
          {
            txHash: '0xpage1',
            blockNumber: 500,
            txIndex: 0,
            timestamp: '1700000500',
            actions: ['transfer'],
          },
        ],
        limit: 50,
        hasMore: true,
        nextCursor: '500:0',
      } as any;
    });

    render(<DotbitItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(api.getDotbitItemActivities).toHaveBeenCalledWith('0xabc', { limit: 50 });
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Next' })).not.toBeDisabled();
    });
    const nextButton = screen.getByRole('button', { name: 'Next' });
    fireEvent.click(nextButton);

    await waitFor(() => {
      expect(api.getDotbitItemActivities).toHaveBeenCalledWith('0xabc', {
        limit: 50,
        cursor: '500:0',
      });
    });
    await waitFor(() => {
      expect(
        mockReplace.mock.calls.some((call) => String(call[0]).includes('activity_cursor=500%3A0'))
      ).toBe(true);
    });
  });

  it('renders live cell link when account is live', async () => {
    vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      expiredAt: 1800000000,
      txHash: '0xtx',
      outputIndex: 2,
    } as any);

    render(<DotbitItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      const links = screen.getAllByRole('link');
      expect(links.some((link) => link.getAttribute('href') === '/cell/0xtx-2')).toBe(true);
    });
  });

  it('renders not found panel when item is missing', async () => {
    vi.mocked(api.getDotbitItemDetail).mockRejectedValue(new Error('API error: 404'));

    render(<DotbitItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('.bit item not found')).toBeInTheDocument();
    });
  });
});
