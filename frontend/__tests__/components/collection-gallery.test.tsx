import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { CollectionItems } from '@/components/asset/collection-gallery';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage, identityItem } from '@/__tests__/utils/item-read-fixtures';
import { collectionGalleryFilteredProducer } from '@/__tests__/msw/fixtures/item-producer';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { phoneLines, statedHeadPx } from '@/__tests__/utils/phone-row';
import { shortenAddress } from '@/lib/shorten';
import { PHONE_LINE_PX } from '@/lib/type-metrics';
beforeEach(mockItemReads);
// F2 objects (round 2): the 24-card CollectionGallery was replaced by the Newest list; its two
// tests moved to __tests__/components/collection-objects.test.tsx.
// Re-pinned (lane c2-assets): the name search is the controls board's search field (a searchbox,
// was a textbox) and the status is its filter chips, a radio group named "status" whose radios
// are "all", "live", "inactive" (was a select "Item status" with options All, Live, Inactive).
// Re-pinned (lane c2-finish, 1a): the gallery's pager is the library's, its next 'next →' (was
// 'Next'); the page counts 75 items (was 1, a count the library's pager refuses beside "more":
// page 2 of 1).
it('resets server pagination when search or status changes and preserves item-specific links', async () => {
  vi.mocked(api.getIdentityCollectionItems).mockResolvedValue({
    ...emptyPage([identityItem('bit-cell')]),
    total: 75,
    hasMore: true,
    nextCursor: 'later',
  });
  render(<CollectionItems id="bit-cell" kind="identity" />);
  const item = identityItem('bit-cell');
  expect(await screen.findByRole('link', { name: item.name! })).toHaveAttribute(
    'href',
    `/mainnet/identities/bit-cell/${item.nftId}`
  );
  fireEvent.click(screen.getByRole('button', { name: 'next →' }));
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith(
      'bit-cell',
      expect.objectContaining({ cursor: 'later' })
    )
  );
  fireEvent.change(screen.getByRole('searchbox', { name: 'Search collection' }), {
    target: { value: 'alice' },
  });
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith('bit-cell', {
      limit: 24,
      cursor: undefined,
      search: 'alice',
      status: 'live',
    })
  );
  fireEvent.click(
    within(screen.getByRole('radiogroup', { name: 'status' })).getByRole('radio', {
      name: 'inactive',
    })
  );
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith('bit-cell', {
      limit: 24,
      cursor: undefined,
      search: 'alice',
      status: 'recycled',
    })
  );
});

// Review 2026-11-03, item 3: a .bit collection is 97 % recycled names, so an 'all' default opened
// on a screen of names with no live owner. Live cells are the state: the list opens on live items.
it('opens on live items, and keeps All and Inactive one choice away', async () => {
  vi.mocked(api.getIdentityCollectionItems).mockResolvedValue(emptyPage([identityItem('dotbit')]));
  render(<CollectionItems id="dotbit" kind="identity" />);
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenCalledWith('dotbit', {
      limit: 24,
      cursor: undefined,
      search: undefined,
      status: 'live',
    })
  );
  const status = screen.getByRole('radiogroup', { name: 'status' });
  expect(within(status).getByRole('radio', { name: 'live' })).toHaveAttribute(
    'aria-checked',
    'true'
  );
  expect(
    within(status)
      .getAllByRole('radio')
      .map((r) => r.textContent)
  ).toEqual(['all', 'live', 'inactive']);
  fireEvent.click(within(status).getByRole('radio', { name: 'all' }));
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith(
      'dotbit',
      expect.objectContaining({ status: 'all', cursor: undefined })
    )
  );
});

// Rubbing × Song print, phase 5 (A12): the heading runs on the count the chosen status holds and
// that status, in the chips' own words; a search page has no total, so the status stands alone.
it('runs the items command on the total and the chosen status, the controls beside it', async () => {
  vi.mocked(api.getIdentityCollectionItems).mockImplementation(async (_id, params) =>
    params?.search
      ? { ...emptyPage([identityItem('dotbit')]), total: undefined }
      : { ...emptyPage([identityItem('dotbit')]), total: params?.status === 'live' ? 1204 : 37 }
  );
  render(<CollectionItems id="dotbit" kind="identity" />);
  const heading = await screen.findByRole('heading', { name: 'Identities 1,204 live' });
  expectPrompt(heading, 'Identities', '1,204 live');
  // the search and the status chips stay the row's actions, outside the heading
  const row = heading.parentElement!;
  const status = screen.getByRole('radiogroup', { name: 'status' });
  expect(row).toContainElement(status);
  expect(heading).not.toContainElement(
    screen.getByRole('searchbox', { name: 'Search collection' })
  );
  fireEvent.click(within(status).getByRole('radio', { name: 'inactive' }));
  expect(
    await screen.findByRole('heading', { name: 'Identities 37 inactive' })
  ).toBeInTheDocument();
  fireEvent.click(within(status).getByRole('radio', { name: 'all' }));
  expect(await screen.findByRole('heading', { name: 'Identities 37 all' })).toBeInTheDocument();
  fireEvent.change(screen.getByRole('searchbox', { name: 'Search collection' }), {
    target: { value: 'alice' },
  });
  await waitFor(() =>
    expectPrompt(screen.getByRole('heading', { name: 'Identities all' }), 'Identities', 'all')
  );
});
it('names an object collection by its objects', async () => {
  vi.mocked(api.getObjectCollectionItems).mockResolvedValue({
    ...emptyPage([identityItem('dotbit')]),
    total: 8,
  });
  render(<CollectionItems id="0xabc" kind="object" />);
  expect(await screen.findByRole('heading', { name: 'Objects 8 live' })).toBeInTheDocument();
});

// Round 3 (contracts R-A · R-1, OB-K3): an m-NFT class row reads "<class name> #<token index>".
it('names m-NFT class rows by the class name and the token index', async () => {
  const mnft = (tokenIndex: number, name: string | null) => ({
    nftId: `0x8f67${'00'.repeat(24)}${String(tokenIndex).padStart(8, '0')}`,
    name,
    standard: 'm-nft',
    ownerLockHash: `0x${'12'.repeat(32)}`,
    ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4s93hhsj',
    tokenIndex,
    isLive: true,
    createdAtBlock: 7_000_000,
  });
  vi.mocked(api.getObjectCollectionItems).mockResolvedValue(
    emptyPage([mnft(0, 'Santa Claus'), mnft(1, null)])
  );
  render(<CollectionItems id="class" kind="object" />);
  expect(await screen.findByRole('link', { name: 'Santa Claus #0' })).toHaveAttribute(
    'href',
    `/mainnet/objects/mnft/${mnft(0, null).nftId}`
  );
  // A class without an on-chain name: the index alone, no invented name.
  expect(screen.getByRole('link', { name: '#1' })).toBeInTheDocument();
});

it('refuses an m-NFT row without its token index', async () => {
  vi.mocked(api.getObjectCollectionItems).mockResolvedValue(
    emptyPage([
      {
        nftId: `0x8f67${'00'.repeat(28)}`,
        name: 'Santa Claus',
        standard: 'm-nft',
        ownerLockHash: null,
        ownerAddress: null,
        isLive: false,
        createdAtBlock: 7_000_000,
      },
    ])
  );
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  render(<CollectionItems id="class" kind="object" />);
  expect(await screen.findByText(/m-NFT item 0x8f67.* has no tokenIndex/)).toBeInTheDocument();
  log.mockRestore();
});

// Lane D4 (the phone-rows board, its collection items pane; PB-03): a class's and an identity
// collection's items on a phone were four stacked values, 135 px a row, no composition chip and the
// minting block bare. Each is the board's row: the name has line 1 to itself (it may wrap, it is
// never cut), who holds it is line 2 with, on a class's row, the composition chip at its right
// edge (27 characters of a name at 14 px and the chip are 357 px, past a 360 px phone's 328 px
// line, so the chip moved from the name's line to the holder's); line 3 quiet, where it was minted,
// named, and its state where it is not live.
describe('collection items on a phone (lane D4, PB-03)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const owner = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4s93hhsj';
  const mnft = (tokenIndex: number, isLive = true) => ({
    nftId: `0x8f67${'00'.repeat(24)}${String(tokenIndex).padStart(8, '0')}`,
    name: 'Non-Fungible Santa Claus',
    standard: 'm-nft',
    ownerLockHash: isLive ? `0x${'12'.repeat(32)}` : null,
    ownerAddress: isLive ? owner : null,
    tokenIndex,
    isLive,
    createdAtBlock: 6_140_203,
  });

  it('writes a class’s item as name · holder and composition · where it was minted', async () => {
    atViewportWidth(390);
    vi.mocked(api.getObjectCollectionItems).mockResolvedValue(emptyPage([mnft(0), mnft(2, false)]));
    render(<CollectionItems id="class" kind="object" tier="centralized_mixture" />);
    const [live, gone] = await screen.findAllByTestId('collection-item-row-mobile');
    expect(phoneLines(live).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['name', 'head', 'Non-Fungible Santa Claus #0', null],
      ['holder', 'body', `held by ${shortenAddress(owner, 'row')}`, 'Centralized mix'],
      ['minted', 'quiet', 'minted at #6,140,203', null],
    ]);
    // the chip holds the holder's line's right edge, whatever the holder's length
    // re-pinned on purpose (lane p-assets, review of C and D logic SHOULD 4): a held line wraps by
    // its own rule, the chip under the left only where it and the left's widest item cannot share
    // the line (its left part's basis nothing, its least width its min-content; it was flex-nowrap)
    expect(live.querySelector('[data-line="holder"] > [data-part="left"]')).toHaveClass(
      'basis-0',
      'min-w-min'
    );
    expect(phoneLines(gone).map((l) => [l.key, l.left, l.right])).toEqual([
      ['name', 'Non-Fungible Santa Claus #2', null],
      ['holder', 'No live owner', 'Centralized mix'],
      ['minted', 'minted at #6,140,203 · inactive', null],
    ]);
    // the name is the row's key: never cut, it wraps between its words
    const name = live.querySelector('[data-line="name"] [data-part="left"] a')!;
    expect(name.className).not.toMatch(/truncate/);
    expect(statedHeadPx(live)).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
  });

  it('writes an identity collection’s item with its holder and no chip of an object', async () => {
    atViewportWidth(390);
    const item = identityItem('dotbit');
    vi.mocked(api.getIdentityCollectionItems).mockResolvedValue(emptyPage([item]));
    render(<CollectionItems id="dotbit" kind="identity" />);
    const [row] = await screen.findAllByTestId('collection-item-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['name', item.name, null],
      [
        'holder',
        item.ownerAddress ? `held by ${shortenAddress(item.ownerAddress, 'row')}` : 'No live owner',
        null,
      ],
      ['minted', `minted at #${item.createdAtBlock.toLocaleString('en-US')}`, null],
    ]);
  });
});

// Lane c2-finish, item 1a: the gallery pages with the library's pager, counted only where its own
// response counts the items (the chosen status's total), and as an ordered list with no count on
// a search page (the API gives a search no total): its rows, its size, 'page 2', the next by the
// response's "more".
it('pages counted by its own total, and a search page as a list with no count', async () => {
  const items = (cursor: string | undefined) =>
    Array.from({ length: 24 }, (_, i) => {
      const item = identityItem('dotbit');
      return { ...item, nftId: `${item.nftId}${cursor ?? ''}${i}` };
    });
  vi.mocked(api.getIdentityCollectionItems).mockImplementation(async (_id, params) => ({
    data: items(params?.cursor),
    limit: 24,
    total: params?.search ? undefined : 250,
    hasMore: true,
    nextCursor: `${params?.cursor ?? ''}n`,
  }));
  render(<CollectionItems id="dotbit" kind="identity" />);
  const counted = await screen.findByRole('navigation', { name: 'identities' });
  expect(counted).toHaveTextContent(/^1–24 of 250 · 24 a page/);
  expect(within(counted).getByText('page 1 of 11')).toBeInTheDocument();
  fireEvent.change(screen.getByRole('searchbox', { name: 'Search collection' }), {
    target: { value: 'alice' },
  });
  await waitFor(() =>
    expect(screen.getByRole('navigation', { name: 'identities' })).toHaveTextContent(
      /^1–24 · 24 a page/
    )
  );
  const uncounted = screen.getByRole('navigation', { name: 'identities' });
  expect(within(uncounted).getByText('page 1')).toBeInTheDocument();
  expect(uncounted).not.toHaveTextContent(/ of /);
  fireEvent.click(within(uncounted).getByRole('button', { name: 'next →' }));
  await waitFor(() =>
    expect(screen.getByRole('navigation', { name: 'identities' })).toHaveTextContent(
      /^25–48 · 24 a page/
    )
  );
  expect(
    within(screen.getByRole('navigation', { name: 'identities' })).getByText('page 2')
  ).toBeInTheDocument();
  expect(screen.queryByText(/per page/)).toBeNull();
});
