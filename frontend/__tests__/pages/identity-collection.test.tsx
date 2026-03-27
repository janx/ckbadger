import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import IdentityCollectionPage from '@/app/identities/[collectionId]/client-page';
import { api } from '@/lib/api';
const DOTBIT_COLLECTION_ID = '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f';
const DID_CKB_COLLECTION_ID = '0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f';

vi.mock('@/lib/api', () => ({
  api: {
    getIdentityCollection: vi.fn(),
    getIdentityCollectionItems: vi.fn(),
    getIdentityCollectionHolders: vi.fn(),
    getIdentityCollectionActivities: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

let mockCollectionId = 'dotbit';
let mockSearchParamsString = '';
const mockReplace = vi.fn();

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ collectionId: mockCollectionId }),
  usePathname: () => `/identities/${mockCollectionId}`,
  useSearchParams: () => new URLSearchParams(mockSearchParamsString),
  useRouter: () => ({ replace: mockReplace, push: vi.fn() }),
}));

const mockIdentityCollection = {
  collectionId: DOTBIT_COLLECTION_ID,
  standard: 'dotbit',
  name: '.bit',
  totalCount: 500,
  liveCount: 320,
  holdersCount: 42,
  activitiesCount: 150,
  ownedCapacity: '0',
  ownedKnowledge: '0',
};

describe('IdentityCollectionPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockCollectionId = 'dotbit';
    mockSearchParamsString = '';
    vi.mocked(api.getIdentityCollectionItems).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getIdentityCollectionHolders).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getIdentityCollectionActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('renders identity collection detail with correct tabs', async () => {
    vi.mocked(api.getIdentityCollection).mockResolvedValue(mockIdentityCollection);

    render(<IdentityCollectionPage collectionId={mockCollectionId} />);

    await waitFor(() => {
      expect(screen.getByText('.bit')).toBeInTheDocument();
    });

    // Identities are now a standalone gallery panel header, not a tab button
    expect(screen.getByText('Identities (500)')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Activities \(150\)$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Holders \(42\)$/ })).toBeInTheDocument();
  });

  it('shows "Total Identities" label', async () => {
    vi.mocked(api.getIdentityCollection).mockResolvedValue(mockIdentityCollection);

    render(<IdentityCollectionPage collectionId={mockCollectionId} />);

    await waitFor(() => {
      expect(screen.getByText('Total Identities')).toBeInTheDocument();
    });

    expect(screen.queryByText('Total NFTs')).not.toBeInTheDocument();
  });

  it('searches identity items by keyword', async () => {
    vi.mocked(api.getIdentityCollection).mockResolvedValue(mockIdentityCollection);

    render(<IdentityCollectionPage collectionId={mockCollectionId} />);

    // Gallery panel is always visible (no tab click needed)
    await waitFor(() => {
      expect(screen.getByText('Identities (500)')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(api.getIdentityCollectionItems).toHaveBeenCalledWith(
        mockCollectionId,
        expect.objectContaining({ limit: 18, search: undefined, status: 'all' })
      );
    });

    fireEvent.change(screen.getByLabelText('Status Filter'), {
      target: { value: 'live' },
    });

    await waitFor(() => {
      expect(api.getIdentityCollectionItems).toHaveBeenCalledWith(
        mockCollectionId,
        expect.objectContaining({ limit: 18, search: undefined, status: 'live' })
      );
    });

    fireEvent.change(screen.getByLabelText('Search .bit'), {
      target: { value: 'alice' },
    });

    await waitFor(() => {
      expect(api.getIdentityCollectionItems).toHaveBeenCalledWith(
        mockCollectionId,
        expect.objectContaining({ limit: 18, search: 'alice', status: 'live' })
      );
    });
  });

  it('shows recycled status for dead items', async () => {
    vi.mocked(api.getIdentityCollection).mockResolvedValue(mockIdentityCollection);
    vi.mocked(api.getIdentityCollectionItems).mockResolvedValue({
      data: [
        {
          nftId: '0x1111',
          name: 'bob.bit',
          standard: 'dotbit',
          ownerLockHash: '0x2222',
          isLive: false,
          createdAtBlock: 100,
          expiredAt: 1800000000,
          txHash: null,
          outputIndex: null,
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<IdentityCollectionPage collectionId={mockCollectionId} />);

    // Gallery panel is always visible (no tab click needed)
    await waitFor(() => {
      expect(screen.getByText('bob.bit')).toBeInTheDocument();
    });

    expect(screen.getAllByText('Recycled').length).toBeGreaterThan(0);
    expect(screen.queryByText(/Cell:/)).not.toBeInTheDocument();
  });

  it('links identity items to correct detail pages', async () => {
    vi.mocked(api.getIdentityCollection).mockResolvedValue(mockIdentityCollection);
    vi.mocked(api.getIdentityCollectionItems).mockResolvedValue({
      data: [
        {
          nftId: '0x1111',
          name: 'alice.bit',
          standard: 'dotbit',
          ownerLockHash: '0x2222',
          isLive: true,
          createdAtBlock: 100,
          txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          outputIndex: 7,
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<IdentityCollectionPage collectionId={mockCollectionId} />);

    // Gallery panel is always visible (no tab click needed)
    await waitFor(() => {
      const link = screen.getByRole('link', { name: 'alice.bit' });
      expect(link).toBeInTheDocument();
      expect(link).toHaveAttribute('href', '/identities/dotbit/0x1111');
    });
  });

  it('hydrates tab from query params', async () => {
    mockSearchParamsString = 'tab=holders';
    vi.mocked(api.getIdentityCollection).mockResolvedValue(mockIdentityCollection);

    render(<IdentityCollectionPage collectionId={mockCollectionId} />);

    await waitFor(
      () => {
        expect(screen.getByText('No holders in this collection')).toBeInTheDocument();
      },
      {
        timeout: 3000,
      }
    );
    expect(api.getIdentityCollectionHolders).toHaveBeenCalledWith(
      mockCollectionId,
      expect.objectContaining({ limit: 50 })
    );
  });

  it('shows "No holders in this collection" when empty holders tab', async () => {
    mockSearchParamsString = 'tab=holders';
    vi.mocked(api.getIdentityCollection).mockResolvedValue(mockIdentityCollection);
    vi.mocked(api.getIdentityCollectionHolders).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<IdentityCollectionPage collectionId={mockCollectionId} />);

    await waitFor(
      () => {
        expect(screen.getByText('No holders in this collection')).toBeInTheDocument();
      },
      {
        timeout: 3000,
      }
    );
  });
});
