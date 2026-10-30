import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import Page from '@/app/identities/dotcell/[identityId]/client-page';
import { api, ApiRequestError, type ItemProtocol, type ItemCellVersions } from '@/lib/api';
import { mockItemReads, dotcellItem, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import { dotcellProtocolProducer as evidence } from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
beforeEach(mockItemReads);
afterEach(() => vi.restoreAllMocks());
it('routes only an actual name 404 to exact endpoint evidence', async () => {
  vi.mocked(api.getDotCellItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'name absent')
  );
  render(<Page identityId="alice.cell" />);
  expect(
    await screen.findByText('/assets/identities/dotcell/items/alice.cell')
  ).toBeInTheDocument();
  expect(
    screen.getByText('This explorer could not open the identity at this address.')
  ).toBeInTheDocument();
});
it('shows current name state, exact protocol, physical cell history and both feed layers', async () => {
  render(<Page identityId="alice.cell" />);
  // The name's own title replaces the loading one once its detail arrives.
  await waitFor(() =>
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('alice.cell')
  );
  expect(screen.getByRole('heading', { level: 1 })).toHaveClass('text-[26px]');
  expect(api.getDotCellItemDetail).toHaveBeenCalledWith('alice.cell');
  expect(await screen.findByRole('region', { name: 'Owner and manager' })).toBeInTheDocument();
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent(
    'Name cell data · layout v3 · 103 bytes'
  );
  expect(screen.getByRole('tab', { name: 'Activities' })).toBeInTheDocument();
  expect(screen.getByRole('tab', { name: 'Transactions' })).toBeInTheDocument();
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
  const rights = await screen.findByRole(
    'region',
    { name: 'Owner and manager' },
    { timeout: 4000 }
  );
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
  expect(await screen.findByRole('heading', { name: 'For sale' })).toBeInTheDocument();
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
  expect(await screen.findByRole('link', { name: 'ckb.cell' }, { timeout: 4000 })).toHaveAttribute(
    'href',
    `/mainnet/identities/dotcell/${next}`
  );
});
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
  fireEvent.click(await screen.findByRole('button', { name: 'Show more sub-names' }));
  await screen.findByRole('link', { name: 'two.alice.cell' });
  fireEvent.click(screen.getByRole('button', { name: 'Show more sub-names' }));
  await screen.findByRole('link', { name: 'three.alice.cell' });
  expect(api.getDotCellItemChildren).toHaveBeenLastCalledWith(item.identityId, {
    limit: 50,
    cursor: 'second',
  });
  expect(screen.getByText('Sub-names · 3')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Show more sub-names' })).not.toBeInTheDocument();
});
it('retains loaded child names when a later page fails and exposes the error', async () => {
  const item = dotcellItem();
  item.children = [{ identityId: '0x' + '81'.repeat(20), name: 'one.alice.cell', label: 'one' }];
  item.childrenHasMore = true;
  item.childrenNextCursor = 'first';
  vi.mocked(api.getDotCellItemDetail).mockResolvedValue(item);
  vi.mocked(api.getDotCellItemChildren).mockRejectedValue(new Error('secondary failed'));
  render(<Page identityId="alice" />);
  fireEvent.click(await screen.findByRole('button', { name: 'Show more sub-names' }));
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
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotcell', evidence.itemId, {
      limit: 50,
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
      limit: 50,
      cursor: undefined,
    })
  );
  unmount();
  vi.mocked(api.getDotCellItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Name not found')
  );
  render(<Page identityId="unknown" />);
  expect(await screen.findByRole('heading', { name: 'An unanswered name.' })).toBeInTheDocument();
  expect(screen.getByText('/assets/identities/dotcell/items/unknown')).toBeInTheDocument();
});

// ── F2 identities (round 2): IdentityDotcell tier 1 (ID-C7 grace, ID-DC5, ID-DC7) ──
it('reads like the .cell board: records named, roles, a held-until bar with grace', async () => {
  render(<Page identityId="alice" />);
  expect(
    await screen.findByRole('heading', { name: 'Held until 2030-03-18' }, { timeout: 4000 })
  ).toBeInTheDocument();
  expect(screen.getByText(/then 30 days of grace/)).toBeInTheDocument();
  expect(document.querySelector('[data-grace]')).not.toBeNull();
  expect(screen.getByRole('region', { name: 'What alice.cell points to' })).toBeInTheDocument();
  const facts = screen.getByRole('complementary', { name: 'Facts' });
  expect(
    Array.from(facts.querySelectorAll(':scope > dl > div > dt')).map((d) => d.textContent)
  ).toEqual([
    'Owner',
    'Manager',
    'Expires',
    'Registered',
    'Records',
    'Sub-names · for sale',
    'CKB held in its cell',
  ]);
  expect(facts).not.toHaveTextContent(/Namespace args|Physical cell owner|Name state/);
});

it("names a .cell version's physical lock as a lock, not its owner (visual pass)", async () => {
  render(<Page identityId="alice" />);
  const versions = await screen.findByLabelText('alice.cell cell versions', {}, { timeout: 4000 });
  expect(versions).toHaveTextContent('Lock ckb1');
  expect(versions).not.toHaveTextContent('Owner ckb1');
});
