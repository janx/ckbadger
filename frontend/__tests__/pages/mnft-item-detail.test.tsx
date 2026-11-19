import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
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
import { expectHeadId } from '@/__tests__/utils/head-id';
import { loadUnderStillClock, T0, tick } from '@/__tests__/utils/still-clock';
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
import { expectAnswerUnclipped } from '@/__tests__/utils/copy-answer';
import { pagedRead } from '@/__tests__/utils/paged-read';
// re-pinned (lane c2-assets): the feed's tab row is the controls library's Tabs, a command line
// whose names are lowercase ('activities', 'transactions'; were 'Activities', 'Transactions'), and
// its pager is the board's, a list in time ('older →', '← newer'; were 'Next', 'Previous')

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
  // Lane E4: how long ago the item was issued, the life sentence's hover, reads the page's one
  // clock, so it moves with no data change; it was written once, from the time the page mounted.
  // The clock stops with the page.
  it('moves the life sentence’s issued age with no data change (E4)', async () => {
    vi.useFakeTimers({ now: T0 });
    try {
      const [version] = mnftProtocolProducer.versions;
      vi.mocked(api.getItemProtocol).mockResolvedValue({
        ...mnftProtocolProducer,
        versions: [{ ...version, cell: { ...version.cell, timestamp: String(T0 - 20_000) } }],
      } as ItemProtocol);
      const { unmount } = render(<MnftItemDetailPage objectId={id} />);
      const ages = () => screen.getByText(/^Issued by Issuer A/).getAttribute('title');
      await loadUnderStillClock(() => screen.queryByText(/^Issued by Issuer A/) !== null);
      expect(ages()).toBe('issued 20\u00a0seconds ago');
      tick(1000);
      expect(ages()).toBe('issued 21\u00a0seconds ago');
      tick(38_000);
      expect(ages()).toBe('issued 59\u00a0seconds ago');
      tick(1000);
      expect(ages()).toBe('issued 1\u00a0minute ago');
      unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  // A4 Chrome F3: the token id under the title was HexCode's own-box scroller (break-all with
  // nowrap: box 358 px for its content at 390, the first screen); it is the head's HeadId.
  it('prints the token id in its head as the head’s full id, never a scroller', async () => {
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByRole('heading', { name: 'Class A #99' })).toBeInTheDocument();
    expectHeadId(id);
  });

  // Lane B4 third round (review-rules-fixes2 SHOULD 1, the coordinator's item 3, ruling D7): the
  // lock decision's refusals placed above their parts fail the part they are in, said in its place,
  // never the page: here a cell whose owner address spells another lock (JoyID) than its own.
  it('fails the owner note in place when its lock is refused, the page drawn', async () => {
    const joy = seededAddress('joymn', JOYID_CODE_HASH);
    vi.mocked(api.getItemProtocol).mockResolvedValue({
      ...mnftProtocolProducer,
      versions: mnftProtocolProducer.versions.map((v) => ({
        ...v,
        cell: { ...v.cell, ownerAddress: joy },
      })),
    } as never);
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByRole('heading', { name: 'Class A #99' })).toBeInTheDocument();
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    // re-pinned on purpose (lane P2's follow-up, item 6): the Issued fact's note says the lock the
    // token was issued to through the same decision, so it fails in its place too (one alert was
    // the owner's note); each says the refusal, and the page is drawn
    await waitFor(() => expect(within(metrics).getAllByRole('alert')).toHaveLength(2));
    for (const alert of within(metrics).getAllByRole('alert'))
      expect(alert).toHaveTextContent(new RegExp(`address ${joy} spells the lock`));
  });

  it('renders identity, canonical ownership, class totals, whole-life feeds and raw evidence', async () => {
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByRole('heading', { name: 'Class A #99' })).toBeInTheDocument();
    // Consistency plan C1: the tab names the page.
    await waitFor(() => expect(document.title).toBe('Class A #99 · object · CKBadger'));
    await screen.findByRole('region', { name: 'Raw item evidence' });
    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent('Objects');
    expect(screen.getByRole('link', { name: /^Objects$/ })).toHaveAttribute(
      'href',
      '/mainnet/inventory/objects'
    );
    // OB-M1: /inventory is not a route; the Inventory crumb opens the objects inventory.
    expect(screen.getByRole('link', { name: /^Inventory$/ })).toHaveAttribute(
      'href',
      '/mainnet/inventory/objects'
    );
    expect(screen.getByText(/State 0x03 · claimed · locked/)).toBeInTheDocument();
    // F2 objects (OB-M5): the hero sentence tells who issued it and its life since.
    expect(screen.getByText(/^Issued by Issuer A/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open the class' })).toHaveAttribute(
      'href',
      `/mainnet/classes/${detail.class.classId}`
    );
    expect(screen.getByRole('region', { name: 'In Class A' })).toHaveTextContent(
      '200 issued of 1,000 · 198 live'
    );
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
            `/mainnet/cell/${protocol.cell.txHash}-${protocol.cell.outputIndex}`
        )
    ).toBe(true);
    expect(screen.getByRole('heading', { name: 'm-NFT token ID' })).toBeInTheDocument();
    // lane c2-finish (item 4, and rulings round 1): the issuer's id copies by the icon after it,
    // its answer out of the line's flow with nothing above it to clip it
    expectAnswerUnclipped(screen.getByRole('button', { name: 'copy issuer id' }));
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
    fireEvent.click(screen.getByRole('tab', { name: 'transactions' }));
    expect(await screen.findByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
    expect(api.getItemTransactions).toHaveBeenCalledWith('mnft', id, {
      limit: 20,
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
    vi.mocked(api.getItemActivityFeed).mockResolvedValue(
      pagedRead({
        ...itemActivityFeedProducer,
        hasMore: true,
        nextCursor: '100:2',
      })
    );
    const { rerender } = render(<MnftItemDetailPage objectId={id} />);
    fireEvent.click(await screen.findByRole('button', { name: 'older →' }));
    await waitFor(() =>
      expect(api.getItemActivityFeed).toHaveBeenCalledWith('mnft', id, {
        limit: 20,
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
        limit: 20,
        cursor: undefined,
      })
    );
    expect(api.getItemActivityFeed).not.toHaveBeenCalledWith(
      'mnft',
      nextId,
      expect.objectContaining({ cursor: expect.any(String) })
    );
    // the newest end cannot be taken: quiet words, no control (was a disabled Previous)
    expect(screen.getByRole('navigation', { name: 'activities' })).toHaveTextContent('← newer');
    expect(screen.queryByRole('button', { name: '← newer' })).toBeNull();
  });

  it('shows an absent item only for a real 404 response', async () => {
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(
      new ApiRequestError(404, 'not_found', 'missing')
    );
    render(<MnftItemDetailPage objectId={id} />);
    expect(
      await screen.findByRole('heading', {
        level: 1,
        name: "no object with this id. the local index was asked; it holds none. honey badger don't care.",
      })
    ).toBeInTheDocument();
    // U-18: the facts name the checked endpoint with the id shortened, once
    expect(document.querySelector('.error-facts [data-endpoint]')).toHaveTextContent(
      `/assets/objects/items/${id.slice(0, 10)}…${id.slice(-6)}`
    );
    expect(api.getItemProtocol).not.toHaveBeenCalled();
  });

  // Visual finish V8: the page loads as the shared detail skeleton, not an "m-NFT" h1 and a
  // "Loading m-NFT…" line.
  it('shows the detail skeleton while the item loads', () => {
    vi.mocked(api.getMnftItemDetail).mockReturnValue(new Promise(() => {}));
    render(<MnftItemDetailPage objectId={id} />);
    const skeleton = screen.getByRole('status', { name: 'Loading …' });
    expect(skeleton.children).toHaveLength(3);
    expect(screen.queryByText('Loading m-NFT…')).toBeNull();
    expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
  });

  it('preserves a read error instead of calling the item missing', async () => {
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(
      new ApiRequestError(500, 'internal_error', 'canonical item mismatch')
    );
    render(<MnftItemDetailPage objectId={id} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('canonical item mismatch');
    expect(screen.queryByText('the local index was asked; it holds none.')).not.toBeInTheDocument();
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
    // the owner field says 'No live owner', as every object's owner field does (U-15)
    expect(screen.getByText('No live owner')).toBeInTheDocument();
    expect(screen.getAllByText('No live cell')).toHaveLength(1);
    // lane P2's follow-up (item 11): the spending transaction opens at this cell, the anchor
    // P1's builder writes, so its fold never hides the cell the reader came from
    const cell = consumed.versions[0].cell;
    expect(screen.getByRole('link', { name: 'Consumed in transaction' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${'0x' + '56'.repeat(32)}#${cell.txHash}-${cell.outputIndex}`
    );
    expect(
      screen.getByRole('heading', { name: 'm-nft cell data · version 1' })
    ).toBeInTheDocument();
  });
});
