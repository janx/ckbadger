import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import SporeDetailPage from '@/app/objects/[sporeId]/client-page';
import { api, ApiRequestError } from '@/lib/api';

// Only `api` is stubbed: the error types and error guards are the real ones, so
// tests reject with the exact `ApiRequestError` the fetch layer builds from a
// real API response instead of a hand-rolled `Error('API error: 404')` that
// cannot go stale with the backend.
vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
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
      getSporeItemActivities: vi.fn(),
    },
    isWarmupPendingError: vi.fn(() => false),
    isNetworkInitializingError: vi.fn(() => false),
  };
});

/** What `GET /spore/objects/{id}` answers for a well-formed but absent 32-byte ID. */
const sporeNotFound = () => new ApiRequestError(404, 'not_found', 'Spore not found');
/** What `GET /assets/objects/{id}` answers for an absent collection ID. */
const collectionNotFound = () =>
  new ApiRequestError(404, 'not_found', 'Object collection not found');

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
  composition: {
    tier: 'btc_ckb' as const,
    fullyOnchainCount: 500,
    pureCkbCount: 0,
    decentralizedMixtureCount: 0,
    centralizedMixtureCount: 0,
    unknownCount: 0,
    fullyOnchainRatio: '1.0',
  },
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
    vi.mocked(api.getSporeObjectDecoded).mockRejectedValue(sporeNotFound());
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
    vi.mocked(api.getSporeItemActivities).mockResolvedValue({
      data: [],
      total: 0,
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
      '/mainnet/inventory/objects'
    );
    expect(screen.getByText('Spore Asset (0x1234...cdef)')).toBeInTheDocument();
    expect(screen.getByText('Spore Overview')).toBeInTheDocument();
  });

  it('wraps a long content type so it does not overflow the overview cards', async () => {
    const longContentType = 'image/png;ipfs=QmafNETq4kKGHTuhaZPvfxfe9vY24NgMUmxC6xYkVUGaK8';
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      contentType: longContentType,
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    const contentTypeValue = await screen.findByText(longContentType);
    // break-all lets the unbreakable IPFS token wrap inside its grid column
    // instead of overflowing into the neighbouring "Created" card.
    expect(contentTypeValue).toHaveClass('break-all');
  });

  it('labels the spore composition card as Object Composition', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      mediaProfile: {
        tier: 'btc_ckb',

        issues: [],
        sources: [],
      },
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('Object Composition')).toBeInTheDocument();
    });
  });

  it('renders media source analysis from API profile', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      mediaProfile: {
        tier: 'btc_ckb',

        issues: [],
        sources: [
          {
            uri: 'btcfs://abcdi0',
            scheme: 'btcfs',
            sourceLocation: 'dob_svg',
            dependencyTier: 'btc_ckb',
          },
        ],
      },
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('Media Compositions')).toBeInTheDocument();
      expect(screen.getByText('btcfs://abcdi0')).toBeInTheDocument();
      expect(screen.getAllByText('BTC+CKB').length).toBeGreaterThan(0);
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
    expect(ownerLink).toHaveAttribute(
      'href',
      '/mainnet/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3'
    );
  });

  it('renders decoded traits panel for DOB spores', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      contentType: 'dob/0',
    } as any);
    vi.mocked(api.getSporeObjectDecoded).mockResolvedValue({
      status: 'ok',
      sporeId: mockSpore.sporeId,
      contentType: 'dob/0',
      dnaHex: '0102',
      traits: [{ name: 'Background', value: 'Blue' }],
      media: [],
      issues: [],
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('DOB/0 Details')).toBeInTheDocument();
      expect(screen.getAllByText('Background').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Blue').length).toBeGreaterThan(0);
    });
  });

  it('shows the failure reason when DOB decode failed', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      contentType: 'dob/0',
    } as any);
    vi.mocked(api.getSporeObjectDecoded).mockResolvedValue({
      status: 'failed',
      sporeId: mockSpore.sporeId,
      contentType: 'dob/0',
      dnaHex: null,
      traits: [],
      media: [],
      issues: ['cluster description is not valid JSON: expected value at line 1'],
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    expect(await screen.findByText(/cluster description is not valid JSON/i)).toBeInTheDocument();
    expect(screen.getByText('Undecodable DOB')).toBeInTheDocument();
  });

  it('does not repeat raw media traits in DOB/1 details', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      contentType: 'dob/1',
    } as any);
    vi.mocked(api.getSporeObjectDecoded).mockResolvedValue({
      status: 'decoded',
      sporeId: mockSpore.sporeId,
      contentType: 'dob/1',
      dnaHex: null,
      traits: [
        { name: 'CellNumber', value: '3' },
        { name: 'IMAGE', value: '<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>' },
      ],
      media: [
        {
          mediaType: 'application/json',
          role: null,
          size: 14094,
          hash: 'abc',
          step: 1,
          url: '/spore/objects/0x1/media/abc',
        },
        {
          mediaType: 'image/svg+xml',
          role: 'render',
          size: 0,
          hash: '',
          step: null,
          url: '/spore/objects/0x1/render',
        },
      ],
      issues: [],
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('DOB/1 Details')).toBeInTheDocument();
      expect(screen.getByText('CellNumber')).toBeInTheDocument();
      expect(screen.queryByText('IMAGE')).not.toBeInTheDocument();
    });
  });

  it('renders decoded DOB preview with a fixed-size image frame', async () => {
    vi.mocked(api.getSporeObject).mockResolvedValue({
      ...mockSpore,
      contentType: 'dob/1',
    } as any);
    vi.mocked(api.getSporeObjectDecoded).mockResolvedValue({
      status: 'decoded',
      sporeId: mockSpore.sporeId,
      contentType: 'dob/1',
      dnaHex: null,
      traits: [{ name: 'CellNumber', value: '3' }],
      media: [
        {
          mediaType: 'image/svg+xml',
          role: 'render',
          size: 0,
          hash: '',
          step: null,
          url: '/spore/objects/0x1/render',
        },
      ],
      issues: [],
    } as any);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    const previewImage = await screen.findByAltText('Spore decoded media preview');
    expect(previewImage).toHaveAttribute('src', '/api/mainnet/v1/spore/objects/0x1/render');
    expect(previewImage).toHaveClass('h-80');
    expect(previewImage).toHaveClass('w-80');
  });

  it('renders payload hex+ASCII viewer for text spores', async () => {
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

    // Payload Data hex+ASCII viewer should appear
    await waitFor(() => {
      expect(screen.getByText(/Payload Data/)).toBeInTheDocument();
    });

    // Should show hex offset column
    expect(screen.getByText('0x0000:')).toBeInTheDocument();
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

    // Cluster shown as stat card with name and link
    await waitFor(
      () => {
        expect(api.getSporeCluster).toHaveBeenCalledWith('0xcluster');
        expect(screen.getByText('Cluster')).toBeInTheDocument();
        expect(screen.getByText('Genesis Cluster')).toBeInTheDocument();
      },
      { timeout: 3000 }
    );
  });

  it('falls back to object collection detail when spore lookup returns 404', async () => {
    vi.mocked(api.getSporeObject).mockRejectedValue(sporeNotFound());
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
    expect(screen.getByText('Composition')).toBeInTheDocument();
    expect(screen.getByText('Supply')).toBeInTheDocument();
    expect(screen.getByText('Capacity Statistics')).toBeInTheDocument();
    // Objects are now a standalone gallery panel header, not a tab button
    expect(screen.getByText('Objects (500)')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Activities \(150\)$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Holders \(42\)$/ })).toBeInTheDocument();

    // Gallery panel is always visible (no tab click needed)
    await waitFor(() => {
      expect(screen.getByText('alice.bit')).toBeInTheDocument();
    });
    expect(api.getObjectCollectionItems).toHaveBeenCalledWith(
      mockCollection.collectionId,
      expect.objectContaining({ limit: 18 })
    );
  });

  it('hydrates collection tab from query params', async () => {
    mockSearchParamsString = 'tab=holders';
    vi.mocked(api.getSporeObject).mockRejectedValue(sporeNotFound());
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
    vi.mocked(api.getSporeObject).mockRejectedValue(sporeNotFound());
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('No activities in this collection')).toBeInTheDocument();
    });
  });

  it('updates tab query param when switching collection tabs', async () => {
    vi.mocked(api.getSporeObject).mockRejectedValue(sporeNotFound());
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Holders \(42\)$/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^Holders \(42\)$/ }));

    await waitFor(() => {
      expect(
        mockReplace.mock.calls.some(
          ([href]) =>
            String(href).includes(`/objects/${mockParams.sporeId}`) &&
            String(href).includes('tab=holders')
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

  it('routes a 24-byte mNFT class ID straight to the class page', async () => {
    // An mNFT class ID is 24 bytes and can never be a 32-byte Spore object ID,
    // so /objects/<class id> is decided by the identifier itself — the page must
    // not probe /spore/objects and read the answer's status code. The backend
    // rejects a 24-byte spore ID with 400 (not 404), which is exactly what broke
    // the old probe-then-guess flow.
    const classId = '0x1234567890abcdef1234567890abcdef1234567890abcdef';
    mockParams = { sporeId: classId };
    vi.mocked(api.getSporeObject).mockRejectedValue(
      new ApiRequestError(400, 'bad_request', 'Invalid spore ID: expected 32 bytes, got 24')
    );
    vi.mocked(api.getObjectCollection).mockResolvedValue({
      ...mockCollection,
      collectionId: classId,
      standard: 'm-nft',
    });

    render(<SporeDetailPage sporeId={classId} />);

    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith(`/classes/${classId}`);
    });
    expect(api.getSporeObject).not.toHaveBeenCalled();
    expect(screen.queryByText('Asset not found')).toBeNull();
  });

  it('shows Asset not found for a 32-byte ID that is neither object nor collection', async () => {
    vi.mocked(api.getSporeObject).mockRejectedValue(sporeNotFound());
    vi.mocked(api.getObjectCollection).mockRejectedValue(collectionNotFound());

    render(<SporeDetailPage sporeId={mockParams.sporeId} />);

    await waitFor(() => {
      expect(screen.getByText('Asset not found')).toBeInTheDocument();
    });
    expect(api.getSporeObject).toHaveBeenCalledWith(mockParams.sporeId);
    expect(api.getObjectCollection).toHaveBeenCalledWith(mockParams.sporeId);
  });

  it('shows Asset not found for an identifier of no known object class', async () => {
    // 12 bytes is neither a Spore object ID nor an mNFT class ID: nothing to
    // look up, so the page must fail fast without issuing a request.
    const bogusId = '0x1234567890abcdef12345678';
    mockParams = { sporeId: bogusId };

    render(<SporeDetailPage sporeId={bogusId} />);

    await waitFor(() => {
      expect(screen.getByText('Asset not found')).toBeInTheDocument();
    });
    expect(api.getSporeObject).not.toHaveBeenCalled();
    expect(api.getObjectCollection).not.toHaveBeenCalled();
  });

  it('links mnft collection item to mnft asset detail page', async () => {
    vi.mocked(api.getSporeObject).mockRejectedValue(sporeNotFound());
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

    // Wait for collection view and gallery to load
    await waitFor(
      () => {
        expect(screen.getByText('Objects (500)')).toBeInTheDocument();
      },
      { timeout: 3000 }
    );

    // The items grid is fed by a SEPARATE query from the collection header, so
    // it can still be pending when the header renders — poll for the link.
    await waitFor(
      () => {
        const link = screen
          .getAllByRole('link')
          .find(
            (el) =>
              el.getAttribute('href') === '/mainnet/objects/mnft/0x1111' &&
              el.textContent === '0x1111'
          );
        expect(link).toBeDefined();
      },
      { timeout: 3000 }
    );
  });
});
