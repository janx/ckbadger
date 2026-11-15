import { Component, type ReactNode } from 'react';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
  createTestQueryClient,
} from '@/__tests__/utils/test-utils';
import SporeDetailPage from '@/app/objects/[sporeId]/client-page';
import { expectHeadId } from '@/__tests__/utils/head-id';
import { api, ApiRequestError, type ItemCellVersions } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import { loadUnderStillClock, T0, tick } from '@/__tests__/utils/still-clock';
import {
  sporeDetailProducer,
  sporeCellVersionsProducer,
  objectGeneticsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
const replace = vi.fn();
vi.mock('@/src/navigation', () => ({
  useRouter: () => ({ replace }),
  usePathname: () => '/objects/test',
  useSearchParams: () => new URLSearchParams(),
}));
class Boundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(error: Error) {
    return { error: error.message };
  }
  render() {
    return this.state.error ? <p role="alert">{this.state.error}</p> : this.props.children;
  }
}
const id = sporeDetailProducer.sporeId;
beforeEach(() => {
  mockItemReads();
  replace.mockClear();
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
it('renders exact held/occupied/free capacity and canonical cell versions before whole-life feeds', async () => {
  const { container } = render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByRole('heading', { level: 1, name: 'Genes' })).toHaveClass(
    'text-entity'
  );
  // Consistency plan C1: the tab names the page.
  await waitFor(() => expect(document.title).toBe('Genes · object · CKBadger'));
  expect(await screen.findByLabelText('Genes cell versions')).toHaveTextContent('500.00000000');
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent(
    '229.00000000'
  );
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent(
    '271.00000000'
  );
  expect(container.querySelector('[data-tier="2"]')).toHaveClass(
    'mt-24',
    'border-t',
    'border-base-border'
  );
  expect(api.getItemCellVersions).toHaveBeenCalledWith('spore', id);
});
// A4 Chrome F3: the object id under the title was HexCode's own-box scroller (box 358 px for
// 436 px at 390, the first screen); it is the head's HeadId in the head's 11 px.
it('prints the object id in its head as the head’s full id, never a scroller', async () => {
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByRole('heading', { level: 1, name: 'Genes' })).toBeInTheDocument();
  expectHeadId(id);
});

it('renders exact raw protocol bytes and current gene mapping with separate VM provenance', async () => {
  render(<SporeDetailPage sporeId={id} />);
  // F2 objects (OB-8): the number stays with the DNA; VM provenance is tier-2 DNA evidence.
  // Phase 5 (A13): the number is the last of the DNA command's args, which name the section.
  // U-15 (re-pinned): one byte is "1 byte", the count and its noun one unit.
  expect(
    await screen.findByRole('region', { name: /^DNA \d+\sbytes? · \d+\sgenes? · number \d+$/ })
  ).toHaveTextContent('18446744073709551614');
  expect(screen.getByLabelText('DNA evidence')).toHaveTextContent(
    'original decoder binary hash was not recorded'
  );
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent('spore cell data');
  expect(api.getItemProtocol).toHaveBeenCalledWith('spore', id);
});
it('retains full long content type and never links absent ownership to a null address', async () => {
  const contentType = 'text/' + 'long-subtype-'.repeat(20);
  vi.mocked(api.getSporeObject).mockResolvedValue({
    ...sporeDetailProducer,
    contentType,
    ownerLockHash: null,
    ownerAddress: null,
  });
  const { container } = render(<SporeDetailPage sporeId={id} />);
  // C7: the content type is the Content metric's value, its size the inline note.
  expect(await screen.findByText(contentType)).toHaveClass('break-all');
  expect(screen.getByText(contentType).closest('dd')).toHaveTextContent(
    contentType + ' · 38 bytes'
  );
  expect(screen.getByText('No live owner')).toBeInTheDocument();
  expect(container.querySelector('a[href*="/null"]')).toBeNull();
  expect(api.getObjectGenetics).not.toHaveBeenCalled();
});
it('shows consumed capacity as exact zero and retains its final historical cell', async () => {
  vi.mocked(api.getSporeObject).mockResolvedValue({
    ...sporeDetailProducer,
    isLive: false,
    ownerLockHash: null,
    ownedCapacity: '0',
    ownedKnowledge: '0',
  });
  const history: ItemCellVersions = structuredClone(sporeCellVersionsProducer);
  history.data[0].isLive = false;
  history.data[0].consumedByTx = '0x' + 'aa'.repeat(32);
  history.data[0].consumedAtBlock = 101;
  vi.mocked(api.getItemCellVersions).mockResolvedValue(history);
  render(<SporeDetailPage sporeId={id} />);
  // Consistency plan C12 (old: "Melted" / "this melted object"): the feed's word, Spore's in the title.
  expect(await screen.findByText('Burned')).toBeInTheDocument();
  expect(screen.getByText('No live cell holds capacity for this burned object.')).toHaveAttribute(
    'title',
    'melted, in Spore’s words'
  );
  // Consistency plan C4: the zero is "0 CKB" (was "0.00000000").
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent('0 CKB');
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).not.toHaveTextContent(
    '0.00000000'
  );
  expect(await screen.findByLabelText('Genes cell versions')).toHaveTextContent('500.00000000');
});
// Lane E4: how long ago the object was minted, the life sentence's hover, reads the page's one
// clock, so it moves with no data change; it was written once, from the time the page mounted.
// The clock stops with the page.
it('moves the life sentence’s minted age with no data change (E4)', async () => {
  vi.useFakeTimers({ now: T0 });
  const history: ItemCellVersions = structuredClone(sporeCellVersionsProducer);
  history.data = [{ ...history.data[0], timestamp: String(T0 - 20_000), isLive: true }];
  vi.mocked(api.getItemCellVersions).mockResolvedValue(history);
  const { unmount } = render(<SporeDetailPage sporeId={id} />);
  const ages = () => screen.getByText(/^Minted to /).getAttribute('title');
  await loadUnderStillClock(() => screen.queryByText(/^Minted to /) !== null);
  expect(ages()).toBe('minted 20\u00a0seconds ago');
  tick(1000);
  expect(ages()).toBe('minted 21\u00a0seconds ago');
  tick(38_000);
  expect(ages()).toBe('minted 59\u00a0seconds ago');
  tick(1000);
  expect(ages()).toBe('minted 1\u00a0minute ago');
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});
it('fails with item context when capacity violates the occupied invariant', async () => {
  vi.mocked(api.getSporeObject).mockResolvedValue({ ...sporeDetailProducer, ownedCapacity: '1' });
  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  render(
    <Boundary>
      <SporeDetailPage sporeId={id} />
    </Boundary>
  );
  expect(await screen.findByRole('alert')).toHaveTextContent(
    `${id}: occupied capacity exceeds held capacity`
  );
  error.mockRestore();
});
it('presents BTCFS as link-only media even when a decoded image URL exists', async () => {
  vi.mocked(api.getSporeObject).mockResolvedValue({
    ...sporeDetailProducer,
    mediaProfile: {
      tier: 'btc_ckb',
      sources: [
        {
          uri: `btcfs://${'0bfb'.padEnd(60, '0')}5719i0`,
          scheme: 'btcfs',
          sourceLocation: 'DNA',
          dependencyTier: 'btc_ckb',
        },
      ],
      issues: [],
    },
  });
  const { container } = render(<SporeDetailPage sporeId={id} />);
  expect(
    await screen.findByRole('link', { name: 'Bitcoin tx 0bfb000000…005719 · inscription i0 ↗' })
  ).toHaveAttribute('href', `https://mempool.space/tx/${'0bfb'.padEnd(60, '0')}5719`);
  // Lane U follow-up: the one Omiga link is the media frame's "View image ↗" button; the outlined
  // h1-row button (U5) is gone, so the page has exactly one link to Omiga.
  const omiga = screen.getByRole('link', { name: 'View image ↗' });
  expect(omiga).toHaveAttribute(
    'href',
    `https://omiga.io/info/dobs/${sporeDetailProducer.typeScriptHash}`
  );
  expect(omiga).toHaveClass('bg-jade', 'text-base-bg');
  expect(omiga.closest('[data-hero-media]')).not.toBeNull();
  expect(container.querySelector('[data-role="page-actions"]')).toBeNull();
  expect(screen.queryByRole('link', { name: 'Omiga ↗' })).toBeNull();
  expect(
    screen.getAllByRole('link').filter((a) => a.getAttribute('href')?.includes('omiga.io'))
  ).toHaveLength(1);
  expect(container.querySelector('img')).toBeNull();
});
it('does not repair unavailable raw data with transaction/getCell or client-side protocol parsing', async () => {
  vi.mocked(api.getItemProtocol).mockRejectedValue(new Error('local CKB reader unavailable'));
  const tx = vi.spyOn(api, 'getTransactionDetail'),
    cell = vi.spyOn(api, 'getCell');
  render(<SporeDetailPage sporeId={id} />);
  expect(
    (await screen.findAllByRole('alert')).some((el) =>
      el.textContent?.includes('local CKB reader unavailable')
    )
  ).toBe(true);
  expect(tx).not.toHaveBeenCalled();
  expect(cell).not.toHaveBeenCalled();
});
it('refreshes media classification when pending VM work completes, then stops polling', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const complete = {
    status: 'decoded',
    sporeId: id,
    contentType: 'dob/0',
    dnaHex: '0x01',
    traits: [{ name: 'Face', value: 'Blue' }],
    media: [],
    issues: [],
  };
  vi.mocked(api.getSporeObjectDecoded)
    .mockResolvedValueOnce({ ...complete, status: 'pending' })
    .mockResolvedValue(complete);
  vi.mocked(api.getSporeObject)
    .mockResolvedValueOnce({
      ...sporeDetailProducer,
      mediaProfile: { tier: 'decode_pending', sources: [], issues: [] },
    })
    .mockResolvedValue({
      ...sporeDetailProducer,
      mediaProfile: { tier: 'pure_ckb', sources: [], issues: [] },
    });
  vi.mocked(api.getObjectGenetics).mockResolvedValue({
    ...objectGeneticsProducer,
    cachedVmTraits: {
      ...objectGeneticsProducer.cachedVmTraits,
      status: 'pending',
      traits: [],
      decodedAt: null,
    },
  });
  render(<SporeDetailPage sporeId={id} />);
  expect((await screen.findAllByText(/Decode pending/)).length).toBeGreaterThan(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2100);
  });
  await waitFor(() => expect(api.getSporeObject).toHaveBeenCalledTimes(2));
  const count = vi.mocked(api.getSporeObjectDecoded).mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(api.getSporeObjectDecoded).toHaveBeenCalledTimes(count);
});
it('shows failed VM reason without fabricating traits', async () => {
  vi.mocked(api.getObjectGenetics).mockResolvedValue({
    ...objectGeneticsProducer,
    cachedVmTraits: {
      ...objectGeneticsProducer.cachedVmTraits,
      status: 'failed',
      decodedAt: null,
      traits: [],
      reason: 'VM exit 3',
    },
  });
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByText(/VM exit 3/)).toBeInTheDocument();
  expect(screen.queryByLabelText('Cached VM traits')).not.toBeInTheDocument();
});
it('keeps object data visible with a background refresh error', async () => {
  const client = createTestQueryClient();
  render(
    <QueryClientProvider client={client}>
      <SporeDetailPage sporeId={id} />
    </QueryClientProvider>
  );
  await screen.findByRole('heading', { level: 1, name: 'Genes' });
  vi.mocked(api.getSporeObject).mockRejectedValue(new Error('secondary read failed'));
  await act(async () => {
    await client.invalidateQueries({ queryKey: ['spore', id], exact: true });
  });
  expect(await screen.findByText(/Could not refresh object/)).toHaveTextContent(
    'Showing previously loaded data'
  );
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent(
    '500.00000000'
  );
});
it('switches to real transaction layer and requests its own full scope', async () => {
  vi.mocked(api.getItemTransactions).mockResolvedValue(emptyPage());
  render(<SporeDetailPage sporeId={id} />);
  await screen.findByRole('tab', { name: 'Transactions' });
  fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
  expect(await screen.findByText('No transactions.')).toBeInTheDocument();
  expect(api.getItemTransactions).toHaveBeenCalledWith('spore', id, {
    limit: 20,
    cursor: undefined,
  });
});
it.each(['dotbit', 'did:ckb', 'dotcell', '0x' + 'aa'.repeat(24)])(
  'routes known non-Spore identifier %s without probing object endpoints',
  async (value) => {
    render(<SporeDetailPage sporeId={value} />);
    await waitFor(() => expect(replace).toHaveBeenCalled());
    expect(api.getSporeObject).not.toHaveBeenCalled();
    expect(api.getObjectCollection).not.toHaveBeenCalled();
  }
);
it('only a real object 404 permits probing the collection namespace', async () => {
  vi.mocked(api.getSporeObject).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Spore not found')
  );
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByRole('heading', { level: 1, name: 'Genes' })).toBeInTheDocument();
  expect(api.getObjectCollection).toHaveBeenCalledWith(id);
});
// Visual finish V8: the page loads as the shared detail skeleton — while the object is read, and
// while an absent object's id is tried as a collection — not a "Loading object…" line.
it('shows the detail skeleton while the object loads', () => {
  vi.mocked(api.getSporeObject).mockReturnValue(new Promise(() => {}));
  render(<SporeDetailPage sporeId={id} />);
  const skeleton = screen.getByRole('status', { name: 'Loading …' });
  expect(skeleton.children).toHaveLength(3);
  expect(screen.queryByText('Loading object…')).toBeNull();
  expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
});
it('shows the detail skeleton while an absent object is tried as a collection', async () => {
  vi.mocked(api.getSporeObject).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Spore not found')
  );
  vi.mocked(api.getObjectCollection).mockReturnValue(new Promise(() => {}));
  render(<SporeDetailPage sporeId={id} />);
  await waitFor(() => expect(api.getObjectCollection).toHaveBeenCalledWith(id));
  const skeleton = screen.getByRole('status', { name: 'Loading …' });
  expect(skeleton.children).toHaveLength(3);
  expect(screen.queryByText('Loading object…')).toBeNull();
});

it('does not turn a corrupt object read into an alternative collection lookup', async () => {
  vi.mocked(api.getSporeObject).mockRejectedValue(new Error('corrupt cell'));
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('corrupt cell');
  expect(api.getObjectCollection).not.toHaveBeenCalled();
});
it('renders not found for both absent namespaces or an unrecognized identifier', async () => {
  vi.mocked(api.getSporeObject).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Spore not found')
  );
  vi.mocked(api.getObjectCollection).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Collection not found')
  );
  const { rerender } = render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByText('no object with this id.')).toBeInTheDocument();
  // U-18: one fact line: the lookup that decided (the collection is asked only after the
  // object's 404), the id shortened, once
  expect(document.querySelector('.error-facts [data-endpoint]')).toHaveTextContent(
    `/assets/objects/${id.slice(0, 10)}…${id.slice(-6)}`
  );
  // U-06: an identifier no object route owns is not an id: the same voice page, its own line
  rerender(<SporeDetailPage sporeId="not-an-id" />);
  expect(screen.getByText('that is not an object id.')).toBeInTheDocument();
  expect(screen.queryByRole('heading', { name: 'Invalid object identifier' })).toBeNull();
});
