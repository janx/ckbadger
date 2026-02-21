import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import SporeDetailPage from '@/app/nfts/[sporeId]/page';
import { api } from '@/lib/api';
import { DOTBIT_COLLECTION_ID } from '@/lib/nft-collections';

vi.mock('@/lib/api', () => ({
  api: {
    getSporeNft: vi.fn(),
    getSporeCluster: vi.fn(),
    getSporeNftOccupationChart: vi.fn(),
    getNftCollection: vi.fn(),
    getNftCollectionOccupationChart: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

let mockParams = {
  sporeId: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
};

vi.mock('next/navigation', () => ({
  useParams: () => mockParams,
}));

const mockSpore = {
  sporeId: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
  txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  outputIndex: 0,
  clusterId: null,
  contentType: 'image/png',
  contentSize: 1024,
  ownerLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
  isLive: true,
  createdAtBlock: 123456,
  liveCapacity: '100000000000',
  liveOccupiedCapacity: '61000000000',
};

const mockCollection = {
  collectionId: '0x1234567890abcdef1234567890abcdef1234567890abcdef',
  standard: 'm-nft',
  name: 'Test Collection',
  totalCount: 500,
  liveCount: 320,
  liveCapacity: '800000000000',
  liveOccupiedCapacity: '510000000000',
};

describe('SporeDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockParams = {
      sporeId: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
    };
    vi.mocked(api.getSporeNftOccupationChart).mockResolvedValue({
      title: 'Spore Capacity Occupation',
      data: [],
      series: [],
    });
    vi.mocked(api.getNftCollectionOccupationChart).mockResolvedValue({
      title: 'Test Collection Capacity Occupation',
      data: [],
      series: [],
    });
  });

  it('links back to NFT tab on assets page', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue(mockSpore);

    render(<SporeDetailPage />);

    await waitFor(() => {
      const backLink = screen.getByText('← Back to NFTs');
      expect(backLink).toBeInTheDocument();
      expect(backLink.closest('a')).toHaveAttribute('href', '/assets?type=nft');
    });
  });

  it('renders occupation history panel', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue(mockSpore);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Capacity & Occupation')).toBeInTheDocument();
    });
  });

  it('falls back to NFT collection detail when spore lookup returns 404', async () => {
    vi.mocked(api.getSporeNft).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getNftCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Collection Details')).toBeInTheDocument();
    });

    expect(screen.getByText('Test Collection')).toBeInTheDocument();
    expect(screen.queryByText('Capacity Utilization')).not.toBeInTheDocument();
    expect(screen.getByText(/^Occupied:/)).toBeInTheDocument();
    expect(screen.getByText('Capacity & Occupation')).toBeInTheDocument();
  });

  it('normalizes dotbit slug before querying collection API', async () => {
    mockParams = { sporeId: 'dotbit' };
    vi.mocked(api.getNftCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(api.getNftCollection).toHaveBeenCalledWith(DOTBIT_COLLECTION_ID);
    });
    expect(api.getSporeNft).not.toHaveBeenCalled();
  });

  it('normalizes .bit slug before querying collection API', async () => {
    mockParams = { sporeId: '.bit' };
    vi.mocked(api.getNftCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(api.getNftCollection).toHaveBeenCalledWith(DOTBIT_COLLECTION_ID);
    });
    expect(api.getSporeNft).not.toHaveBeenCalled();
  });
});
