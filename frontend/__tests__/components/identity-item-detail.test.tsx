import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';

import {
  IdentityItemDetail,
  type IdentityItemDetailConfig,
} from '@/components/identity/identity-item-detail';
import { api } from '@/lib/api';
import { render } from '../utils/test-utils';

vi.mock('@/lib/api', () => ({
  api: {
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

const mockFetchDetail = vi.fn();
const mockFetchActivities = vi.fn();

const dotbitConfig: IdentityItemDetailConfig = {
  standard: 'dotbit',
  fetchDetail: mockFetchDetail,
  fetchActivities: mockFetchActivities,
  labels: {
    standardDisplay: 'DOTBIT',
    nameLabel: '.bit Name',
    idLabel: 'Account ID',
    backLabel: 'Back to .bit Collection',
    backHref: '/identities/dotbit',
    defaultTitle: '.bit account',
    notFoundMsg: '.bit item not found',
    recycledMsg: 'Recycled .bit account has no live cell.',
    showExpiry: true,
  },
};

const didCkbConfig: IdentityItemDetailConfig = {
  standard: 'did_ckb',
  fetchDetail: mockFetchDetail,
  fetchActivities: mockFetchActivities,
  labels: {
    standardDisplay: 'DID:CKB',
    nameLabel: 'did:ckb Name',
    idLabel: 'DID ID',
    backLabel: 'Back to did:ckb Collection',
    backHref: '/identities/did:ckb',
    defaultTitle: 'did:ckb identity',
    notFoundMsg: 'did:ckb item not found',
    recycledMsg: 'Recycled did:ckb identity has no live cell.',
    showExpiry: false,
  },
};

describe('IdentityItemDetail', () => {
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
    mockFetchActivities.mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
  });

  it('renders dotbit labels, back link, and live cell link', async () => {
    mockFetchDetail.mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      expiredAt: 1800000000,
      txHash: '0xtx',
      outputIndex: 2,
    });

    render(<IdentityItemDetail config={dotbitConfig} identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('DOTBIT')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: /Back to \.bit Collection/ })).toHaveAttribute(
      'href',
      '/identities/dotbit'
    );
    expect(screen.getByText('.bit Name')).toBeInTheDocument();
    expect(screen.getByText('Account ID')).toBeInTheDocument();
    expect(screen.getByText('Expires At')).toBeInTheDocument();
    expect(
      screen.getAllByRole('link').some((link) => link.getAttribute('href') === '/cell/0xtx-2')
    ).toBe(true);
  });

  it('renders did:ckb standard labels without expiry', async () => {
    mockFetchDetail.mockResolvedValue({
      nftId: '0xabc',
      name: 'did:alice.ckb',
      standard: 'did_ckb',
      ownerLockHash: '0xlock',
      isLive: true,
      createdAtBlock: 123,
      txHash: null,
      outputIndex: null,
    });

    render(<IdentityItemDetail config={didCkbConfig} identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('DID:CKB')).toBeInTheDocument();
    });

    expect(screen.getByText('did:ckb Name')).toBeInTheDocument();
    expect(screen.getByText('DID ID')).toBeInTheDocument();
    expect(screen.queryByText('Expires At')).not.toBeInTheDocument();
  });

  it('shows config-specific not found messages', async () => {
    mockFetchDetail.mockRejectedValue(new Error('API error: 404'));

    const { rerender } = render(<IdentityItemDetail config={dotbitConfig} identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('.bit item not found')).toBeInTheDocument();
    });

    rerender(<IdentityItemDetail config={didCkbConfig} identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('did:ckb item not found')).toBeInTheDocument();
    });
  });

  it('renders recycled status message', async () => {
    mockFetchDetail.mockResolvedValue({
      nftId: '0xabc',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xlock',
      isLive: false,
      createdAtBlock: 123,
      expiredAt: 1800000000,
      txHash: null,
      outputIndex: null,
    });

    render(<IdentityItemDetail config={dotbitConfig} identityId="0xabc" />);

    await waitFor(() => {
      expect(screen.getByText('Recycled .bit account has no live cell.')).toBeInTheDocument();
    });
  });
});
