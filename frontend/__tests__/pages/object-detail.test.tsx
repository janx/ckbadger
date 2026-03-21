import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import SporeDetailPage from '@/app/objects/[sporeId]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getSporeObject: vi.fn(),
    getSporeCluster: vi.fn(),
    getSporeObjectDecoded: vi.fn(),
    getSporeObjectCapacityChart: vi.fn(),
    getAddress: vi.fn(),
    getTransactionDetail: vi.fn(),
    getCell: vi.fn(),
    getObjectCollection: vi.fn(),
    getObjectCollectionCapacityChart: vi.fn(),
    getObjectCollectionItems: vi.fn(),
    getObjectCollectionHolders: vi.fn(),
    getObjectCollectionActivities: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

let mockParams = {
  sporeId: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
};
let mockSearchParamsString = '';
const mockReplace = vi.fn();

vi.mock('@/src/navigation', () => ({
  useParams: () => mockParams,
  usePathname: () => `/objects/${mockParams.sporeId}`,
  useSearchParams: () => new URLSearchParams(mockSearchParamsString),
  useRouter: () => ({ replace: mockReplace, push: vi.fn() }),
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
  ownedCapacity: '100000000000',
  ownedKnowledge: '61000000000',
};

const mockCollection = {
  collectionId: '0x1234567890abcdef1234567890abcdef1234567890abcdef',
  standard: 'spore',
  name: 'Test Collection',
  totalCount: 500,
  liveCount: 320,
  holdersCount: 42,
  activitiesCount: 150,
  ownedCapacity: '800000000000',
  ownedKnowledge: '510000000000',
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
    mockSearchParamsString = '';
    vi.mocked(api.getSporeObjectCapacityChart).mockResolvedValue({
      title: 'Spore Capacity History',
      data: [],
      series: [],
    });
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: mockSpore.ownerLockHash,
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      commonKnowledgeSize: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getSporeObjectDecoded).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      hash: mockSpore.txHash,
      status: 'committed',
      pendingSince: null,
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
      inputsCommonKnowledgeSize: '0',
      outputsCommonKnowledgeSize: '0',
      outputs: [
        {
          capacity: '100000000000',
          commonKnowledgeSize: 61,
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
    vi.mocked(api.getObjectCollectionCapacityChart).mockResolvedValue({
      title: 'Test Collection Capacity History',
      data: [],
      series: [],
    });
    vi.mocked(api.getObjectCollectionItems).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getObjectCollectionHolders).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getObjectCollectionActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('renders spore overview panels and back link', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue(mockSpore);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('Spore Overview')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: '← Back to Objects' })).toHaveAttribute(
      'href',
      '/assets?type=object'
    );
    expect(screen.getByText('Spore Asset (0x1234...cdef)')).toBeInTheDocument();
    expect(screen.getByText('Spore Overview')).toBeInTheDocument();
  });

  it('renders media source analysis from API profile', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      mediaProfile: {
        tier: 'fully_on_ckb_and_btc',
        hasRenderableImage: true,
        issues: [],
        sources: [
          {
            uri: 'btcfs://abcdi0',
            scheme: 'btcfs',
            sourceLocation: 'dob_svg',
            dependencyTier: 'fully_on_ckb_and_btc',
          },
        ],
      },
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('Media Sources')).toBeInTheDocument();
      expect(screen.getByText('btcfs://abcdi0')).toBeInTheDocument();
      expect(screen.getAllByText('Fully on BTC+CKB').length).toBeGreaterThan(0);
    });
  });

  it('shows owner address resolved from lock hash', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue(mockSpore);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(
      () => {
        expect(api.getAddress).toHaveBeenCalledWith(mockSpore.ownerLockHash);
      },
      { timeout: 3000 }
    );

    const ownerLink = await screen.findByRole(
      'link',
      {
        name: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      },
      { timeout: 3000 }
    );
    expect(ownerLink).toBeInTheDocument();
    expect(ownerLink).toHaveAttribute('href', '/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3');
  });

  it('renders decoded traits panel for DOB spores', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      contentType: 'dob/0',
    } as any);
    vi.mocked(api.getSporeObjectDecoded).mockResolvedValue({
      sporeId: mockSpore.sporeId,
      contentType: 'dob/0',
      dnaHex: '0102',
      traits: [{ name: 'Background', value: 'Blue' }],
      svgMarkup: null,
      issues: [],
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('Decoded Traits')).toBeInTheDocument();
      expect(screen.getAllByText('Background').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Blue').length).toBeGreaterThan(0);
    });
  });

  it('renders text content in preview for text spores', async () => {
    const encoded = encodeSporeData('text/plain', 'hello from payload text panel');
    vi.mocked(api.getSporeObject).mockResolvedValue({
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

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    // Text content is shown directly in the preview (no separate Payload Text View for text/*)
    await waitFor(() => {
      expect(screen.getAllByText('hello from payload text panel').length).toBeGreaterThan(0);
    });

    // Verify it renders in a <pre> element in the preview
    const payloadTextNodes = screen.getAllByText('hello from payload text panel');
    const payloadPreElements = payloadTextNodes
      .map((node) => node.closest('pre'))
      .filter((element): element is HTMLPreElement => element !== null);
    expect(payloadPreElements.length).toBeGreaterThan(0);

    // Payload Text View should NOT appear for text/* (preview already shows it)
    expect(screen.queryByText('Payload Text View')).not.toBeInTheDocument();
  });

  it('renders cluster metadata from JSON description', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
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
      ownedCapacity: '0',
      ownedKnowledge: '0',
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(
      () => {
        expect(api.getSporeCluster).toHaveBeenCalledWith('0xcluster');
        expect(screen.getByText('Metadata-rich cluster')).toBeInTheDocument();
      },
      {
        timeout: 3000,
      }
    );

    // Cluster context is inline in overview panel
    expect(screen.getByText('Genesis Cluster')).toBeInTheDocument();
    const versionLabel = screen.getByText('Version');
    expect(versionLabel).toBeInTheDocument();
    expect(versionLabel.parentElement?.textContent).toContain('3');
    expect(screen.getByText('View Raw Cluster Metadata JSON')).toBeInTheDocument();
  });

  it('falls back to object collection detail when spore lookup returns 404', async () => {
    vi.mocked(api.getSporeObject).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);
    vi.mocked(api.getObjectCollectionItems).mockResolvedValue({
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
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('Collection Overview')).toBeInTheDocument();
    });

    expect(screen.getByText('Test Collection')).toBeInTheDocument();
    expect(screen.getByText('Supply')).toBeInTheDocument();
    expect(screen.getByText('Capacity Statistics')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Activities \(150\)$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Objects \(500\)$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Holders \(42\)$/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /^Objects \(500\)$/ }));

    await waitFor(() => {
      expect(screen.getByText('alice.bit')).toBeInTheDocument();
    });
    expect(screen.getByText('Created at block #100')).toBeInTheDocument();
    expect(screen.queryByLabelText('Search .bit')).not.toBeInTheDocument();
    expect(api.getObjectCollectionItems).toHaveBeenCalledWith(
      mockCollection.collectionId,
      expect.objectContaining({ limit: 50 })
    );
  });

  it('hydrates collection tab from query params', async () => {
    mockSearchParamsString = 'tab=holders';
    vi.mocked(api.getSporeObject).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('No holders in this collection')).toBeInTheDocument();
    });
    expect(api.getObjectCollectionHolders).toHaveBeenCalledWith(
      mockCollection.collectionId,
      expect.objectContaining({ limit: 50 })
    );
  });

  it('falls back to Activities tab when tab query is invalid', async () => {
    mockSearchParamsString = 'tab=invalid';
    vi.mocked(api.getSporeObject).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('No activities in this collection')).toBeInTheDocument();
    });
  });

  it('updates tab query param when switching collection tabs', async () => {
    vi.mocked(api.getSporeObject).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Objects \(500\)$/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^Objects \(500\)$/ }));

    await waitFor(() => {
      expect(
        mockReplace.mock.calls.some(
          ([href]) =>
            String(href).includes(`/objects/${mockParams.sporeId}`) &&
            String(href).includes('tab=objects')
        )
      ).toBe(true);
    });

    fireEvent.click(screen.getByRole('button', { name: /^Activities \(150\)$/ }));

    await waitFor(() => {
      expect(
        mockReplace.mock.calls.some(
          ([href]) =>
            String(href).includes(`/objects/${mockParams.sporeId}`) &&
            !String(href).includes('tab=')
        )
      ).toBe(true);
    });
  });

  it('redirects identity collection aliases to their canonical collection pages', async () => {
    const assertAliasRedirect = async (alias: string, target: string) => {
      mockParams = { sporeId: alias };
      mockReplace.mockClear();
      const view = render(<SporeDetailPage sporeId={mockParams.sporeId} />);

      await waitFor(() => {
        expect(mockReplace).toHaveBeenCalledWith(target);
      });

      view.unmount();
    };

    await assertAliasRedirect('dotbit', '/identities/dotbit');
    await assertAliasRedirect('.bit', '/identities/dotbit');
    await assertAliasRedirect('did:ckb', '/identities/did:ckb');
  });

  it('links mnft collection item to mnft asset detail page', async () => {
    vi.mocked(api.getSporeObject).mockRejectedValue(new Error('API error: 404'));
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);
    vi.mocked(api.getObjectCollectionItems).mockResolvedValue({
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
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Objects/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^Objects/ }));

    await waitFor(() => {
      const link = screen.getByRole('link', { name: '0x1111' });
      expect(link).toBeInTheDocument();
      expect(link).toHaveAttribute('href', '/objects/mnft/0x1111');
    });
  });
});
