import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import SporeDetailPage from '@/app/nfts/[sporeId]/page';
import { api } from '@/lib/api';
import { DOTBIT_COLLECTION_ID } from '@/lib/nft-collections';

vi.mock('@/lib/api', () => ({
  api: {
    getSporeNft: vi.fn(),
    getSporeCluster: vi.fn(),
    getSporeNftDecoded: vi.fn(),
    getSporeNftOccupationChart: vi.fn(),
    getTransactionDetail: vi.fn(),
    getCell: vi.fn(),
    getNftCollection: vi.fn(),
    getNftCollectionOccupationChart: vi.fn(),
    getNftCollectionItems: vi.fn(),
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
    vi.mocked(api.getSporeNftDecoded).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      hash: mockSpore.txHash,
      blockNumber: 123456,
      blockHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      index: 0,
      inputsCount: 1,
      outputsCount: 1,
      fee: '1000',
      isCellbase: false,
      timestamp: '2026-01-01T00:00:00.000Z',
      confirmations: 10,
      inputsCapacity: '100000000000',
      outputsCapacity: '99999999000',
      inputsOccupiedCapacity: '0',
      outputsOccupiedCapacity: '0',
      outputs: [
        {
          capacity: '100000000000',
          occupiedCapacity: 61,
          type: {
            codeHash: '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
            hashType: 'type',
            args: mockSpore.sporeId,
          },
          lock: {
            codeHash: '0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
            hashType: 'type',
            args: '0x01',
          },
        },
      ],
    } as any);
    vi.mocked(api.getCell).mockResolvedValue({
      txHash: mockSpore.txHash,
      outputIndex: mockSpore.outputIndex,
      capacity: '100000000000',
      lockScriptHash: mockSpore.ownerLockHash,
      dataSize: 0,
      createdAtBlock: mockSpore.createdAtBlock,
    } as any);
    vi.mocked(api.getNftCollectionOccupationChart).mockResolvedValue({
      title: 'Test Collection Capacity Occupation',
      data: [],
      series: [],
    });
    vi.mocked(api.getNftCollectionItems).mockResolvedValue({
      data: [],
      total: 0,
      limit: 20,
      hasMore: false,
      nextCursor: null,
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
    vi.mocked(api.getNftCollectionItems).mockResolvedValue({
      data: [
        {
          nftId: '0x1111',
          name: 'alice.bit',
          standard: 'dotbit',
          ownerLockHash: '0x2222',
          isLive: true,
          createdAtBlock: 100,
          expiredAt: 1800000000,
          txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          outputIndex: 7,
        },
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Collection Details')).toBeInTheDocument();
    });

    expect(screen.getByText('Test Collection')).toBeInTheDocument();
    expect(screen.queryByText('Capacity Utilization')).not.toBeInTheDocument();
    expect(screen.getByText(/^Occupied:/)).toBeInTheDocument();
    expect(screen.getByText('Capacity & Occupation')).toBeInTheDocument();
    expect(screen.getByText('Collection NFTs')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText('alice.bit')).toBeInTheDocument();
    });
    expect(screen.getByText('Created at block #100')).toBeInTheDocument();
    expect(screen.queryByLabelText('Search .bit')).not.toBeInTheDocument();
    expect(api.getNftCollectionItems).toHaveBeenCalledWith(
      mockCollection.collectionId,
      expect.objectContaining({ limit: 20 })
    );
  });

  it('searches nft collection items by keyword', async () => {
    mockParams = { sporeId: 'dotbit' };
    vi.mocked(api.getNftCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(api.getNftCollectionItems).toHaveBeenCalledWith(
        mockCollection.collectionId,
        expect.objectContaining({ limit: 20, search: undefined })
      );
    });

    fireEvent.change(screen.getByLabelText('Search .bit'), {
      target: { value: 'alice' },
    });

    await waitFor(() => {
      expect(api.getNftCollectionItems).toHaveBeenCalledWith(
        mockCollection.collectionId,
        expect.objectContaining({ limit: 20, search: 'alice' })
      );
    });
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
