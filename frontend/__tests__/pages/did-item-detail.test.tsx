import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';

import DidCkbItemDetailPage from '@/app/identities/did/[identityId]/client-page';
import { api } from '@/lib/api';
import { render } from '../utils/test-utils';

vi.mock('@/lib/api', () => ({
  api: {
    getDidCkbItemDetail: vi.fn(),
    getDidCkbItemActivities: vi.fn(),
    getAddress: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockReplace = vi.fn();
let mockSearchParams = new URLSearchParams();

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ identityId: '0xabc' }),
  usePathname: () => '/identities/did/0xabc',
  useRouter: () => ({ replace: mockReplace }),
  useSearchParams: () => mockSearchParams,
}));

describe('DidCkbItemDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockReset();
    mockSearchParams = new URLSearchParams();
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: '0xlock',
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      usedCapacity: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getDidCkbItemActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('hydrates activity cursor from URL query', async () => {
    mockSearchParams = new URLSearchParams('activity_cursor=500:0');
    vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'did:alice.ckb',
      standard: 'did_ckb',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      txHash: null,
      outputIndex: null,
    } as any);

    render(<DidCkbItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(api.getDidCkbItemActivities).toHaveBeenCalledWith('0xabc', {
        limit: 50,
        cursor: '500:0',
      });
    });
  });

  it('renders did:ckb wrapper labels and recycled identity state', async () => {
    vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'did:alice.ckb',
      standard: 'did_ckb',
      ownerLockHash: '0xlock',
      isLive: false,
      createdAtBlock: 123,
      txHash: null,
      outputIndex: null,
    } as any);

    render(<DidCkbItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('DID:CKB')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: /Back to did:ckb Collection/ })).toHaveAttribute(
      'href',
      '/identities/did:ckb'
    );
    expect(screen.getByText('did:ckb Name')).toBeInTheDocument();
    expect(screen.getByText('DID ID')).toBeInTheDocument();
    expect(screen.queryByText('Expires At')).not.toBeInTheDocument();
    expect(screen.getAllByText('did:alice.ckb').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Recycled').length).toBeGreaterThan(0);
    expect(screen.getByText('Recycled did:ckb identity has no live cell.')).toBeInTheDocument();
  });

  it('renders activities with burn normalized to recycled', async () => {
    vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'did:alice.ckb',
      standard: 'did_ckb',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      txHash: null,
      outputIndex: null,
    } as any);
    vi.mocked(api.getDidCkbItemActivities).mockResolvedValue({
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

    render(<DidCkbItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(api.getDidCkbItemActivities).toHaveBeenCalledWith('0xabc', { limit: 50 });
    });

    await waitFor(() => {
      expect(screen.getAllByText('recycled, transfer').length).toBeGreaterThan(0);
    });
    expect(screen.getByText(/2023/)).toBeInTheDocument();
  });

  it('applies cursor pagination', async () => {
    vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({
      nftId: '0xabc',
      name: 'did:alice.ckb',
      standard: 'did_ckb',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      txHash: null,
      outputIndex: null,
    } as any);
    vi.mocked(api.getDidCkbItemActivities).mockImplementation(async (_nftId, params) => {
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

    render(<DidCkbItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(api.getDidCkbItemActivities).toHaveBeenCalledWith('0xabc', { limit: 50 });
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Next' })).not.toBeDisabled();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(api.getDidCkbItemActivities).toHaveBeenCalledWith('0xabc', {
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

  it('renders not found panel when item is missing', async () => {
    vi.mocked(api.getDidCkbItemDetail).mockRejectedValue(new Error('API error: 404'));

    render(<DidCkbItemDetailPage identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('did:ckb item not found')).toBeInTheDocument();
    });
  });
});
