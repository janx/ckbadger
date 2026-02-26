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
    getAddress: vi.fn(),
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

function encodeMoleculeBytes(value: Uint8Array): Uint8Array {
  const out = new Uint8Array(4 + value.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, value.length, true);
  out.set(value, 4);
  return out;
}

function encodeSporeData(contentType: string, contentText: string) {
  const contentTypeBytes = new TextEncoder().encode(contentType);
  const contentBytes = new TextEncoder().encode(contentText);

  const ctField = encodeMoleculeBytes(contentTypeBytes);
  const contentField = encodeMoleculeBytes(contentBytes);
  const offsetContentType = 16;
  const offsetContent = offsetContentType + ctField.length;
  const offsetCluster = offsetContent + contentField.length;
  const totalSize = offsetCluster;

  const buffer = new Uint8Array(totalSize);
  const view = new DataView(buffer.buffer);
  view.setUint32(0, totalSize, true);
  view.setUint32(4, offsetContentType, true);
  view.setUint32(8, offsetContent, true);
  view.setUint32(12, offsetCluster, true);
  buffer.set(ctField, offsetContentType);
  buffer.set(contentField, offsetContent);

  return {
    dataHex: `0x${Array.from(buffer)
      .map((item) => item.toString(16).padStart(2, '0'))
      .join('')}`,
    contentTypeStart: offsetContentType + 4,
    contentTypeEnd: offsetContentType + 4 + contentTypeBytes.length,
    contentStart: offsetContent + 4,
    contentEnd: offsetContent + 4 + contentBytes.length,
  };
}

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
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: mockSpore.ownerLockHash,
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      occupiedCapacity: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
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

  it('renders improved spore content panels', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue(mockSpore);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Spore Asset')).toBeInTheDocument();
      expect(screen.getByText('Spore Content Preview')).toBeInTheDocument();
      expect(screen.getByText('Spore Details')).toBeInTheDocument();
      expect(screen.getByText('Rendering Pipeline')).toBeInTheDocument();
      expect(screen.queryByText('How To Read This Spore')).not.toBeInTheDocument();
    });
  });

  it('renders media source analysis from API profile', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue({
      ...mockSpore,
      mediaProfile: {
        tier: 'fully_onchain',
        hasRenderableImage: true,
        issues: [],
        sources: [
          {
            uri: 'btcfs://abcdi0',
            scheme: 'btcfs',
            sourceLocation: 'dob_svg',
            dependencyTier: 'fully_onchain',
          },
        ],
      },
    } as any);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Media Sources')).toBeInTheDocument();
      expect(screen.getByText('btcfs://abcdi0')).toBeInTheDocument();
      expect(screen.getAllByText('Fully On-chain').length).toBeGreaterThan(0);
    });
  });

  it('uses vertical layout for long identity fields', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue(mockSpore);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Spore Details')).toBeInTheDocument();
      expect(screen.getByText('Spore ID')).toBeInTheDocument();
      expect(screen.getByText('Owner Lock Hash')).toBeInTheDocument();
    });

    const sporeIdField = screen.getByText('Spore ID').closest('div')?.parentElement;
    const ownerLockHashField = screen.getByText('Owner Lock Hash').closest('div')?.parentElement;
    expect(sporeIdField).toHaveClass('flex-col');
    expect(ownerLockHashField).toHaveClass('flex-col');
  });

  it('shows owner address resolved from lock hash', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue(mockSpore);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(api.getAddress).toHaveBeenCalledWith(mockSpore.ownerLockHash);
      const ownerLink = screen.getByRole('link', {
        name: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      });
      expect(ownerLink).toBeInTheDocument();
      expect(ownerLink).toHaveAttribute(
        'href',
        '/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3'
      );
    });
  });

  it('renders decoded traits panel for DOB spores', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue({
      ...mockSpore,
      contentType: 'dob/0',
    } as any);
    vi.mocked(api.getSporeNftDecoded).mockResolvedValue({
      sporeId: mockSpore.sporeId,
      contentType: 'dob/0',
      dnaHex: '0102',
      traits: [{ name: 'Background', value: 'Blue' }],
      svgMarkup: null,
      issues: [],
    } as any);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Decoded Traits')).toBeInTheDocument();
      expect(screen.getAllByText('Background').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Blue').length).toBeGreaterThan(0);
    });
  });

  it('renders payload text view for text spores', async () => {
    const encoded = encodeSporeData('text/plain', 'hello from payload text panel');
    vi.mocked(api.getSporeNft).mockResolvedValue({
      ...mockSpore,
      contentType: 'text/plain',
    } as any);
    vi.mocked(api.getCell).mockResolvedValue({
      txHash: mockSpore.txHash,
      outputIndex: mockSpore.outputIndex,
      capacity: '100000000000',
      lockScriptHash: mockSpore.ownerLockHash,
      dataSize: encoded.dataHex.length / 2,
      createdAtBlock: mockSpore.createdAtBlock,
      data: encoded.dataHex,
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cell',
          segments: [
            { label: 'content_type', start: encoded.contentTypeStart, end: encoded.contentTypeEnd },
            { label: 'content', start: encoded.contentStart, end: encoded.contentEnd },
          ],
        },
      },
    } as any);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Payload Text View')).toBeInTheDocument();
      expect(screen.getAllByText('hello from payload text panel').length).toBeGreaterThan(0);
    });

    const payloadTextNodes = screen.getAllByText('hello from payload text panel');
    const payloadPreElements = payloadTextNodes
      .map((node) => node.closest('pre'))
      .filter((element): element is HTMLPreElement => element !== null);
    expect(payloadPreElements.length).toBeGreaterThan(0);
    payloadPreElements.forEach((pre) => {
      expect(pre).toHaveClass('break-all');
      expect(pre).toHaveClass('max-w-full');
    });
  });

  it('renders cluster metadata from JSON description', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue({
      ...mockSpore,
      clusterId: '0xcluster',
    } as any);
    vi.mocked(api.getSporeCluster).mockResolvedValue({
      clusterId: '0xcluster',
      name: 'Genesis Cluster',
      description: JSON.stringify({
        description: 'Metadata-rich cluster',
        version: 3,
      }),
      ownerLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
      ownerAddress: null,
      sporesCount: 42,
      createdAtBlock: 123,
      liveCapacity: '0',
      liveOccupiedCapacity: '0',
    } as any);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Cluster Context')).toBeInTheDocument();
      expect(screen.getByText('Metadata-rich cluster')).toBeInTheDocument();
      const versionLabel = screen.getByText('Version');
      expect(versionLabel).toBeInTheDocument();
      expect(versionLabel.parentElement?.textContent).toContain('3');
      expect(screen.getByText('View Raw Cluster Metadata JSON')).toBeInTheDocument();
    });

    const nameField = screen.getByText('Name').closest('div')?.parentElement;
    const descriptionField = screen.getByText('Description').closest('div')?.parentElement;
    expect(nameField).not.toHaveClass('sm:flex-row');
    expect(descriptionField).not.toHaveClass('sm:flex-row');
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
        expect.objectContaining({ limit: 20, search: undefined, status: 'all' })
      );
    });

    fireEvent.change(screen.getByLabelText('Status Filter'), {
      target: { value: 'live' },
    });

    await waitFor(() => {
      expect(api.getNftCollectionItems).toHaveBeenCalledWith(
        mockCollection.collectionId,
        expect.objectContaining({ limit: 20, search: undefined, status: 'live' })
      );
    });

    fireEvent.change(screen.getByLabelText('Search .bit'), {
      target: { value: 'alice' },
    });

    await waitFor(() => {
      expect(api.getNftCollectionItems).toHaveBeenCalledWith(
        mockCollection.collectionId,
        expect.objectContaining({ limit: 20, search: 'alice', status: 'live' })
      );
    });
  });

  it('shows recycled status without cell text and links to dotbit detail page', async () => {
    mockParams = { sporeId: 'dotbit' };
    vi.mocked(api.getNftCollection).mockResolvedValue({
      ...mockCollection,
      standard: 'dotbit',
      name: '.bit',
    } as any);
    vi.mocked(api.getNftCollectionItems).mockResolvedValue({
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
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('bob.bit')).toBeInTheDocument();
    });

    expect(screen.getAllByText('Recycled').length).toBeGreaterThan(0);
    expect(screen.queryByText(/Cell:/)).not.toBeInTheDocument();

    const detailLink = screen.getByRole('link', { name: 'bob.bit' });
    expect(detailLink).toHaveAttribute('href', '/nfts/dotbit/0x1111');
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

  it('links mnft collection item to mnft asset detail page', async () => {
    vi.mocked(api.getSporeNft).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getNftCollection).mockResolvedValue(mockCollection);
    vi.mocked(api.getNftCollectionItems).mockResolvedValue({
      data: [
        {
          nftId: '0x1111',
          name: null,
          standard: 'm-nft',
          ownerLockHash: '0x2222',
          isLive: true,
          createdAtBlock: 100,
        },
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<SporeDetailPage />);

    await waitFor(() => {
      const link = screen.getByRole('link', { name: '0x1111' });
      expect(link).toBeInTheDocument();
      expect(link).toHaveAttribute('href', '/nfts/mnft/0x1111');
    });
  });
});
