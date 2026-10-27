import { QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  act,
  createTestQueryClient,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@/__tests__/utils/test-utils';
import MnftItemDetailPage from '@/app/objects/mnft/[objectId]/client-page';
import {
  api,
  ApiRequestError,
  type ItemProtocol,
  type MnftItemDetail,
  type ObjectCollection,
} from '@/lib/api';
import {
  mnftProtocolProducer,
  itemActivityFeedProducer,
  itemTransactionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';

vi.mock('@/lib/api', async (load) => {
  const actual = await load<typeof import('@/lib/api')>();
  return {
    ...actual,
    api: {
      ...actual.api,
      getMnftItemDetail: vi.fn(),
      getObjectCollection: vi.fn(),
      getItemProtocol: vi.fn(),
      getItemCellVersions: vi.fn(),
      getItemActivityFeed: vi.fn(),
      getItemTransactions: vi.fn(),
    },
  };
});
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));

const id = mnftProtocolProducer.itemId;
const protocol = mnftProtocolProducer.versions[0];
const detail: MnftItemDetail = {
  nftId: id,
  standard: 'm-nft',
  isLive: true,
  ownerLockHash: protocol.cell.ownerLockHash,
  ownerAddress: protocol.cell.ownerAddress,
  createdAtBlock: protocol.cell.blockNumber,
  createdAtTx: protocol.cell.txHash,
  tokenIndex: 99,
  characteristicHex: protocol.decoded.characteristicHex,
  configure: protocol.decoded.configure,
  state: protocol.decoded.state,
  permissions: protocol.decoded.permissions,
  claimed: protocol.decoded.claimed,
  locked: protocol.decoded.locked,
  txHash: protocol.cell.txHash,
  outputIndex: protocol.cell.outputIndex,
  class: {
    classId: id.slice(0, 50),
    issuerId: id.slice(0, 42),
    name: 'Class A',
    description: 'Class description recorded on chain',
    renderer: 'https://example.com/renderer.png',
    total: 1000,
    issued: 200,
    configure: protocol.decoded.configure,
    permissions: protocol.decoded.permissions,
  },
  issuer: {
    issuerId: id.slice(0, 42),
    name: 'Issuer A',
    classCount: 2,
    setCount: 3,
    infoHex: '0x7b7d',
  },
  lifecycle: [],
  composition: {
    tier: 'centralized_mixture',
    onchainCount: 0,
    pureCkbCount: 0,
    decentralizedMixtureCount: 0,
    centralizedMixtureCount: 2,
    unknownCount: 0,
    onchainRatio: '0',
  },
};
const collection: ObjectCollection = {
  collectionId: detail.class.classId,
  standard: 'm-nft',
  name: 'Class A',
  totalCount: 200,
  liveCount: 198,
  holdersCount: 150,
  activitiesCount: 221,
  ownedCapacity: '32285294386739',
  ownedKnowledge: '32053400000000',
  composition: detail.composition,
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getMnftItemDetail).mockResolvedValue(detail);
  vi.mocked(api.getItemProtocol).mockResolvedValue(mnftProtocolProducer);
  vi.mocked(api.getObjectCollection).mockResolvedValue(collection);
  vi.mocked(api.getItemActivityFeed).mockResolvedValue(itemActivityFeedProducer);
  vi.mocked(api.getItemTransactions).mockResolvedValue(itemTransactionsProducer);
});

describe('MnftItemDetailPage', () => {
  it('renders identity, canonical ownership, class totals, whole-life feeds and raw evidence', async () => {
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByRole('heading', { name: 'Class A #99' })).toBeInTheDocument();
    await screen.findByRole('region', { name: 'Raw item evidence' });
    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent('Objects');
    expect(screen.getByRole('link', { name: /^Objects$/ })).toHaveAttribute(
      'href',
      '/mainnet/inventory/objects'
    );
    expect(screen.getByText(/State 0x03 · claimed · locked/)).toBeInTheDocument();
    expect(screen.getByText(/Issued in/)).toHaveTextContent('Issuer A');
    expect(screen.getByRole('link', { name: 'Open the class →' })).toHaveAttribute(
      'href',
      `/mainnet/classes/${detail.class.classId}`
    );
    expect(screen.getByText('198 of 200 issued')).toBeInTheDocument();
    expect(
      screen
        .getAllByRole('link')
        .some(
          (link) => link.getAttribute('href') === `/mainnet/blocks/${protocol.cell.blockNumber}`
        )
    ).toBe(true);
    expect(
      screen
        .getAllByRole('link')
        .some(
          (link) =>
            link.getAttribute('href') ===
            `/mainnet/cell/${protocol.cell.txHash}/${protocol.cell.outputIndex}`
        )
    ).toBe(true);
    expect(screen.getByRole('heading', { name: 'm-NFT token ID' })).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { name: 'm-nft cell data · version 1' })
    ).toBeInTheDocument();
    expect(api.getItemCellVersions).not.toHaveBeenCalled(); // Protocol response already carries these same canonical versions.
    const permissions = screen.getByRole('region', { name: 'mNFT permissions' });
    expect(within(permissions).getByText('Destroy after claim').parentElement).toHaveTextContent(
      'Forbidden'
    );
    expect(within(permissions).getByText('Transfer after claim').parentElement).toHaveTextContent(
      'Allowed'
    );
    fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
    expect(await screen.findByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
    expect(api.getItemTransactions).toHaveBeenCalledWith('mnft', id, {
      limit: 50,
      cursor: undefined,
    });
    expect(screen.getAllByText(/cell after/).length).toBeGreaterThan(0);
  });

  it('uses the protocol claim/lock interpretation instead of the old inverted permission labels', async () => {
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      ...detail,
      configure: 3,
      state: 1,
      claimed: true,
      locked: false,
      permissions: { ...detail.permissions, allowClaim: false, allowLock: false },
    });
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByText(/State 0x01 · claimed · unlocked/)).toBeInTheDocument();
    const permissions = screen.getByRole('region', { name: 'mNFT permissions' });
    expect(within(permissions).getByText('Claim').parentElement).toHaveTextContent('Forbidden');
    expect(within(permissions).getByText('Lock').parentElement).toHaveTextContent('Forbidden');
    expect(screen.queryByText('transferable, burnable')).not.toBeInTheDocument();
  });

  it('starts a newly navigated-to token on its first activity page', async () => {
    vi.mocked(api.getItemActivityFeed).mockResolvedValue({
      ...itemActivityFeedProducer,
      hasMore: true,
      nextCursor: '100:2',
    });
    const { rerender } = render(<MnftItemDetailPage objectId={id} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
    await waitFor(() =>
      expect(api.getItemActivityFeed).toHaveBeenCalledWith('mnft', id, {
        limit: 50,
        cursor: '100:2',
      })
    );
    const nextId = '0x' + '34'.repeat(28);
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      ...detail,
      nftId: nextId,
      tokenIndex: 100,
    });
    vi.mocked(api.getItemProtocol).mockResolvedValue({ ...mnftProtocolProducer, itemId: nextId });
    vi.mocked(api.getItemActivityFeed).mockClear();
    rerender(<MnftItemDetailPage objectId={nextId} />);
    await screen.findByRole('heading', { name: 'Class A #100' });
    await waitFor(() =>
      expect(api.getItemActivityFeed).toHaveBeenCalledWith('mnft', nextId, {
        limit: 50,
        cursor: undefined,
      })
    );
    expect(api.getItemActivityFeed).not.toHaveBeenCalledWith(
      'mnft',
      nextId,
      expect.objectContaining({ cursor: expect.any(String) })
    );
    expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled();
  });

  it('shows an absent item only for a real 404 response', async () => {
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(
      new ApiRequestError(404, 'not_found', 'missing')
    );
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByRole('heading', { name: 'mNFT item not found' })).toBeInTheDocument();
    expect(api.getItemProtocol).not.toHaveBeenCalled();
  });

  it('preserves a read error instead of calling the item missing', async () => {
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(
      new ApiRequestError(500, 'internal_error', 'canonical item mismatch')
    );
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('canonical item mismatch');
    expect(screen.queryByText('mNFT item not found')).not.toBeInTheDocument();
  });

  it('retains item content while showing a background read failure', async () => {
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <MnftItemDetailPage objectId={id} />
      </QueryClientProvider>
    );
    await screen.findByRole('heading', { name: 'Class A #99' });
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(new Error('refresh failed'));
    await act(async () => {
      await client.refetchQueries({ queryKey: ['mnft-item-detail', id] });
    });
    expect(await screen.findByRole('alert')).toHaveTextContent('Showing previously loaded data.');
    expect(screen.getByRole('heading', { name: 'Class A #99' })).toBeInTheDocument();
  });

  it('keeps burned cell history and raw bytes without inventing a current owner', async () => {
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      ...detail,
      isLive: false,
      ownerLockHash: null,
      ownerAddress: null,
      txHash: null,
      outputIndex: null,
    });
    const consumed: Extract<ItemProtocol, { kind: 'm-nft' }> =
      structuredClone(mnftProtocolProducer);
    consumed.versions[0].cell.isLive = false;
    consumed.versions[0].cell.consumedByTx = '0x' + '56'.repeat(32);
    consumed.versions[0].cell.consumedAtBlock = 101;
    vi.mocked(api.getItemProtocol).mockResolvedValue(consumed);
    render(<MnftItemDetailPage objectId={id} />);
    await screen.findByRole('region', { name: 'Raw item evidence' });
    expect(screen.getByText('Burned')).toBeInTheDocument();
    expect(screen.getAllByText('No live cell')).toHaveLength(2);
    expect(screen.getByRole('link', { name: 'Consumed in transaction →' })).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { name: 'm-nft cell data · version 1' })
    ).toBeInTheDocument();
  });
});
