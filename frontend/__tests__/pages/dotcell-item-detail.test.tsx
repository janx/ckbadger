import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import Page from '@/app/identities/dotcell/[identityId]/client-page';
import { api, ApiRequestError, type ItemProtocol, type ItemCellVersions } from '@/lib/api';
import { mockItemReads, dotcellItem, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import { dotcellProtocolProducer as evidence } from '@/__tests__/msw/fixtures/item-producer';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';
import { fullText } from '@/__tests__/utils/full-text';
import { seededAddress } from '@/__tests__/utils/ckb-address';
// re-pinned (lane c2-assets): the feed's tab row is the controls library's Tabs, a command line
// whose names are lowercase ('activities', 'transactions'; were 'Activities', 'Transactions'), and
// its pager is the board's, a list in time ('older →', '← newer'; were 'Next', 'Previous')
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
beforeEach(mockItemReads);
afterEach(() => vi.restoreAllMocks());
// Lane D2 (PB-22): the four identity pages take one section order — the term, then the records,
// then the roles, then the CKB the cell holds — as the .bit page already did (the .cell page read
// records, roles, then the term).
it('reads its term, then its records, then its roles (lane D2, PB-22)', async () => {
  render(<Page identityId={evidence.itemId} />);
  const tier1 = (await screen.findByRole('heading', { name: /^Term/ })).closest(
    '[data-tier="1"]'
  ) as HTMLElement;
  const term = within(tier1).getByRole('heading', { name: /^Term/ });
  const records = within(tier1).getByRole('heading', { name: /^Records/ });
  const roles = within(tier1).getByRole('heading', { name: /^Roles/ });
  const follows = (a: Element, b: Element) =>
    Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
  expect(follows(term, records)).toBe(true);
  expect(follows(records, roles)).toBe(true);
});
// Lane D2b (PB-22, the owner line D2 left untested): a .cell party's Owner fact is drawn as every
// identity page's owner line is — the address at the row size through `Address`, in the key look
// (the .bit, .bit Cell and did:ckb Owner facts draw it so; a .cell party drew a Hash in an AppLink).
it('draws its Owner fact as every identity page’s owner line: an Address at the row size, in the key look (lane D2b)', async () => {
  const address = seededAddress('dotcell-owner');
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue({
    ...dotcellItem(),
    owner: {
      hashPrefix: '0x' + 'ab'.repeat(20),
      lockHash: '0x' + 'cd'.repeat(32),
      address,
      scriptName: 'Default Lock',
    },
  });
  render(<Page identityId={evidence.itemId} />);
  const facts = await screen.findByRole('region', { name: 'Metrics' });
  const owner = within(facts).getByText('Owner', { selector: 'dt' }).parentElement!;
  const link = within(owner).getByRole('link');
  expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
  expect(link).toHaveTextContent(new RegExp(`^ckb1…${address.slice(-8)}$`));
  // the key look, through the Address renderer: the bright ink, the body step, the mono face
  expect(link).toHaveClass('text-text-bright', 'text-body', 'font-mono');
});
it('routes only an actual name 404 to exact endpoint evidence', async () => {
  vi.mocked(api.getDotCellItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'name absent')
  );
  render(<Page identityId="alice.cell" />);
  expect(
    await screen.findByText('/assets/identities/dotcell/items/alice.cell')
  ).toBeInTheDocument();
  expect(screen.getByText('no identity with this id.')).toBeInTheDocument();
});
// Visual finish V8: the page loads as the shared detail skeleton, not the asset shell with a
// "Loading .cell name…" line.
it('shows the detail skeleton while the name loads', () => {
  vi.mocked(api.getDotCellItemDetail).mockReturnValue(new Promise(() => {}));
  render(<Page identityId="alice.cell" />);
  const skeleton = screen.getByRole('status', { name: 'Loading …' });
  expect(skeleton.children).toHaveLength(3);
  expect(screen.queryByText('Loading .cell name…')).toBeNull();
  expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
});

it('shows current name state, exact protocol, physical cell history and both feed layers', async () => {
  render(<Page identityId="alice.cell" />);
  // The name's own title replaces the loading one once its detail arrives.
  await waitFor(() =>
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('alice.cell')
  );
  expect(screen.getByRole('heading', { level: 1 })).toHaveClass('text-entity');
  expect(api.getDotCellItemDetail).toHaveBeenCalledWith('alice.cell');
  expect(await screen.findByRole('region', { name: 'Owner and manager' })).toBeInTheDocument();
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent(
    'Name cell data · layout v3 · 103 bytes'
  );
  expect(screen.getByRole('tab', { name: 'activities' })).toBeInTheDocument();
  expect(screen.getByRole('tab', { name: 'transactions' })).toBeInTheDocument();
  // The API's name state is the status beside the title (ID-C10).
  expect(screen.getByText('Active')).toHaveAttribute('data-identity-state', 'active');
});
it('links a uniquely resolved protocol owner and retains unresolved manager bytes', async () => {
  const protocol: ItemProtocol = structuredClone(evidence);
  protocol.versions[0].decoded.owner = {
    ...protocol.versions[0].decoded.owner,
    lockHash: '0x' + '22'.repeat(32),
    address: 'ckb1owner',
    scriptName: 'JoyID',
  };
  vi.mocked(api.getItemProtocol).mockResolvedValue(protocol);
  render(<Page identityId="alice" />);
  const rights = await screen.findByRole('region', { name: 'Owner and manager' });
  expect(
    within(within(rights).getByLabelText('Owner · holds the name')).getByRole('link')
  ).toHaveAttribute('href', '/mainnet/address/ckb1owner');
  expect(rights).toHaveTextContent(evidence.versions[0].decoded.manager.hashPrefix);
  expect(rights.querySelector('a[href*="626262"]')).toBeNull();
});
it('shows unresolved named owner as exact prefix and never a fabricated address', async () => {
  render(<Page identityId="alice" />);
  const rights = await screen.findByRole('region', { name: 'Owner and manager' });
  expect(rights).toHaveTextContent(evidence.versions[0].decoded.owner.hashPrefix);
  expect(within(rights).queryByRole('link')).not.toBeInTheDocument();
});
it('renders UTF8 and binary records with exact TTL without dropping unknown keys', async () => {
  const protocol: ItemProtocol = structuredClone(evidence);
  protocol.versions[0].decoded.records = [
    {
      key: 'profile.website',
      label: 'Website',
      valueHex: '0x68747470',
      valueUtf8: 'https://example.com',
      ttl: 300,
      decodedAddress: null,
    },
    {
      key: 'custom.raw',
      label: 'custom.raw',
      valueHex: '0xff0080',
      valueUtf8: null,
      ttl: 4294967295,
      decodedAddress: null,
    },
  ];
  vi.mocked(api.getItemProtocol).mockResolvedValue(protocol);
  render(<Page identityId="alice" />);
  // Tier 1 names them (ID-DC5); tier 2 keeps them exactly as stored, unknown keys included.
  const named = await screen.findByRole('region', { name: 'What alice.cell points to' });
  expect(named).toHaveTextContent('Website');
  expect(named).toHaveTextContent('https://example.com');
  expect(named).toHaveTextContent('custom.raw');
  expect(named).toHaveTextContent('3 bytes, not text');
  const stored = screen.getByRole('region', { name: 'Records as stored · witness' });
  expect(stored).toHaveTextContent('0xff0080');
  expect(stored).toHaveTextContent('ttl 4294967295');
});
it('shows exact sale price, named seller, and the current sale offer only when listed', async () => {
  const item = dotcellItem();
  item.sale = {
    priceShannons: '100000000001',
    seller: item.owner,
    offerOutPoint: { txHash: '0x' + 'ab'.repeat(32), index: 4 },
  };
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue(item);
  const { unmount } = render(<Page identityId="alice" />);
  // Phase 5 (A2): one PromptHeading, no args (the price is the next line).
  expectPrompt(await screen.findByRole('heading', { name: 'For sale' }), 'For sale');
  expect(screen.getByRole('link', { name: 'Sale offer cell' })).toHaveAttribute(
    'href',
    `/mainnet/cell/${item.sale.offerOutPoint!.txHash}-4`
  );
  expect(screen.getByText('1,000')).toBeInTheDocument();
  expect(screen.getByText('.00000001')).toBeInTheDocument();
  unmount();
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue(dotcellItem());
  render(<Page identityId="alice" />);
  await waitFor(() =>
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('alice.cell')
  );
  expect(screen.queryByText('For sale')).not.toBeInTheDocument();
});
it('links the parent, ring successor and sub-names to canonical name routes', async () => {
  const item = dotcellItem();
  item.parent = { identityId: '0x' + '99'.repeat(20), name: 'parent.cell', label: 'parent' };
  item.children = [
    { identityId: '0x' + '88'.repeat(20), name: 'child.alice.cell', label: 'child' },
  ];
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue(item);
  render(<Page identityId="alice" />);
  expect(await screen.findByRole('link', { name: 'parent.cell' })).toHaveAttribute(
    'href',
    `/mainnet/identities/dotcell/${item.parent.identityId}`
  );
  expect(screen.getByRole('link', { name: 'child.alice.cell' })).toHaveAttribute(
    'href',
    `/mainnet/identities/dotcell/${item.children[0].identityId}`
  );
  expect(screen.getByRole('link', { name: /Namespace root cell/ })).toHaveAttribute(
    'href',
    `/mainnet/cell/${'0x' + '99'.repeat(32)}-0`
  );
  // The fixture name is the ring's last (next id zero = end of ring): no successor link.
  expect(screen.getByRole('region', { name: 'Name cell' })).toHaveTextContent("the ring's end");
});
it('names the ring successor and links it by its id (ID-DC6)', async () => {
  const next = '0x' + '3a'.repeat(20);
  const protocol: ItemProtocol = structuredClone(evidence);
  (protocol.versions[0].decoded as { nextId: string }).nextId = next;
  vi.mocked(api.getItemProtocol).mockResolvedValue(protocol);
  vi.mocked(api.getDotCellItemDetail).mockImplementation(async (ref) =>
    ref === next ? { ...dotcellItem(), identityId: next, name: 'ckb.cell' } : dotcellItem()
  );
  render(<Page identityId="alice" />);
  expect(await screen.findByRole('link', { name: 'ckb.cell' })).toHaveAttribute(
    'href',
    `/mainnet/identities/dotcell/${next}`
  );
});
// re-pinned (lane c2-assets): the way on to more sub-names is the controls board's "next 20 ↓"
// (was the boxed "Show more sub-names"), in this test and the failure's below
it('appends every sub-name page and only reports the full child count after completion', async () => {
  const item = dotcellItem();
  item.children = [{ identityId: '0x' + '81'.repeat(20), name: 'one.alice.cell', label: 'one' }];
  item.childrenHasMore = true;
  item.childrenNextCursor = 'first';
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue(item);
  vi.mocked(api.getDotCellItemChildren)
    .mockResolvedValueOnce({
      ...emptyPage([{ identityId: '0x' + '82'.repeat(20), name: 'two.alice.cell', label: 'two' }]),
      hasMore: true,
      nextCursor: 'second',
    })
    .mockResolvedValueOnce(
      emptyPage([{ identityId: '0x' + '83'.repeat(20), name: 'three.alice.cell', label: 'three' }])
    );
  render(<Page identityId="alice" />);
  fireEvent.click(await screen.findByRole('button', { name: 'next 20 ↓' }));
  await screen.findByRole('link', { name: 'two.alice.cell' });
  fireEvent.click(screen.getByRole('button', { name: 'next 20 ↓' }));
  await screen.findByRole('link', { name: 'three.alice.cell' });
  expect(api.getDotCellItemChildren).toHaveBeenLastCalledWith(item.identityId, {
    limit: 20,
    cursor: 'second',
  });
  expect(screen.getByText('Sub-names · 3')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'next 20 ↓' })).not.toBeInTheDocument();
});
// Pinned before the controls lane (C2) replaced the boxed button: a load of sub-names in flight
// cannot be asked for a second time (the button was disabled while it loaded). Re-pinned (lane
// c2-assets): the way on is the controls board's short-list form, "next 20 ↓" (was the boxed
// "Show more sub-names"), drawn only while no load is in flight.
it('asks for the next sub-names once while they load', async () => {
  const item = dotcellItem();
  item.children = [{ identityId: '0x' + '81'.repeat(20), name: 'one.alice.cell', label: 'one' }];
  item.childrenHasMore = true;
  item.childrenNextCursor = 'first';
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue(item);
  vi.mocked(api.getDotCellItemChildren).mockReturnValue(new Promise(() => {}));
  render(<Page identityId="alice" />);
  fireEvent.click(await screen.findByRole('button', { name: 'next 20 ↓' }));
  await waitFor(() => expect(api.getDotCellItemChildren).toHaveBeenCalledTimes(1));
  const again = screen.queryByRole('button', { name: 'next 20 ↓' });
  if (again) fireEvent.click(again);
  expect(api.getDotCellItemChildren).toHaveBeenCalledTimes(1);
});
// C16: a name with no sub-names says so in a line, not under a heading of its own.
it('states "Sub-names · none" as a line, not a heading', async () => {
  render(<Page identityId="alice" />);
  const line = await screen.findByText('Sub-names · none');
  expect(line.tagName).toBe('P');
  expect(screen.queryByRole('heading', { name: /^Sub-names/ })).toBeNull();
});
it('retains loaded child names when a later page fails and exposes the error', async () => {
  const item = dotcellItem();
  item.children = [{ identityId: '0x' + '81'.repeat(20), name: 'one.alice.cell', label: 'one' }];
  item.childrenHasMore = true;
  item.childrenNextCursor = 'first';
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue(item);
  vi.mocked(api.getDotCellItemChildren).mockRejectedValue(new Error('secondary failed'));
  render(<Page identityId="alice" />);
  fireEvent.click(await screen.findByRole('button', { name: 'next 20 ↓' }));
  expect(await screen.findByText('Failed to load more sub-names')).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'one.alice.cell' })).toBeInTheDocument();
});
it('retains recycled name protocol history without a live cell or sale', async () => {
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue({
    ...dotcellItem(),
    isLive: false,
    state: 'recycled',
    liveOutPoint: null,
  });
  const history: ItemCellVersions = {
    data: evidence.versions.map((v) => ({
      ...v.cell,
      isLive: false,
      consumedByTx: '0x' + 'aa'.repeat(32),
      consumedAtBlock: 101,
    })),
    total: 1,
  };
  vi.mocked(api.getItemCellVersions).mockResolvedValue(history);
  render(<Page identityId="alice" />);
  expect(await screen.findByText('Recycled')).toHaveAttribute('data-identity-state', 'recycled');
  expect(await screen.findByText('no live cell')).toBeInTheDocument();
  expect(screen.queryByRole('heading', { name: 'For sale' })).not.toBeInTheDocument();
  expect(await screen.findByLabelText('Raw item evidence')).toBeInTheDocument();
});
it('resets item paging on name navigation and reports an unknown name', async () => {
  vi.mocked(api.getItemActivityFeed).mockResolvedValue({
    ...emptyPage(),
    total: 100,
    hasMore: true,
    nextCursor: '90:0',
  });
  const { rerender, unmount } = render(<Page identityId="alice" />);
  fireEvent.click(await screen.findByRole('button', { name: 'older →' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotcell', evidence.itemId, {
      limit: 20,
      cursor: '90:0',
    })
  );
  const next = '0x' + '77'.repeat(20);
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue({
    ...dotcellItem(),
    identityId: next,
    name: 'bob.cell',
  });
  rerender(<Page identityId="bob" />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotcell', next, {
      limit: 20,
      cursor: undefined,
    })
  );
  unmount();
  vi.mocked(api.getDotCellItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Name not found')
  );
  render(<Page identityId="unknown" />);
  expect(
    await screen.findByRole('heading', {
      level: 1,
      name: "no identity with this id. the local index was asked; it holds none. honey badger don't care.",
    })
  ).toBeInTheDocument();
  expect(screen.getByText('/assets/identities/dotcell/items/unknown')).toBeInTheDocument();
});

// ── F2 identities (round 2): IdentityDotcell tier 1 (ID-C7 grace, ID-DC5, ID-DC7) ──
it('reads like the .cell board: records named, roles, a held-until bar with grace', async () => {
  render(<Page identityId="alice" />);
  expect(
    await screen.findByRole('heading', { name: 'Term held until 2030-03-18' })
  ).toBeInTheDocument();
  expect(screen.getByText(fullText(/then 30 days of grace/))).toBeInTheDocument();
  expect(document.querySelector('[data-grace]')).not.toBeNull();
  expect(screen.getByRole('region', { name: 'What alice.cell points to' })).toBeInTheDocument();
  const facts = screen.getByRole('region', { name: 'Metrics' });
  expect(
    Array.from(facts.querySelectorAll(':scope > dl > div > dt')).map((d) => d.textContent)
  ).toEqual([
    'Owner',
    'Manager',
    'Expires',
    'Registered',
    'Records',
    // re-pinned on purpose (lane P2's follow-up, a standing cause): its own metric, the sale's
    // drawn only for a name for sale (it was "Sub-names · for sale")
    'Sub-names',
    'CKB held in its cell',
  ]);
  expect(facts).not.toHaveTextContent(/Namespace args|Physical cell owner|Name state/);
});

it("names a .cell version's physical lock as a lock, not its owner (visual pass)", async () => {
  render(<Page identityId="alice" />);
  const versions = await screen.findByLabelText('alice.cell cell versions');
  expect(versions).toHaveTextContent('Lock ckb1');
  expect(versions).not.toHaveTextContent('Owner ckb1');
});
