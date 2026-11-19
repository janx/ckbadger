import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { tokenProducer } from '@/__tests__/msw/fixtures/asset-producer';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, fireEvent, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import TokenDetailPage from '@/app/tokens/[typeHash]/client-page';
import { expectHeadId } from '@/__tests__/utils/head-id';
import {
  api,
  ApiRequestError,
  type Token,
  type TokenHistory,
  type GlobalActivity,
} from '@/lib/api';
import { fullText } from '@/__tests__/utils/full-text';
import { pagedRead } from '@/__tests__/utils/paged-read';
// re-pinned (lane c2-assets): the feed's tab row is the controls library's Tabs, a command line
// whose names are lowercase ('activities', 'transactions'; were 'Activities', 'Transactions'), and
// its pager is the board's, a list in time ('older →', '← newer'; were 'Next', 'Previous')
vi.mock('@/lib/api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api')>()),
  api: {
    getToken: vi.fn(),
    getTokenHistory: vi.fn(),
    getTokenSupplyEvents: vi.fn(),
    getTokenHolders: vi.fn(),
    getTokenDistribution: vi.fn(),
    getTokenActivityFeed: vi.fn(),
    getTokenTransactions: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));
const hash = '0x' + '12'.repeat(32),
  other = '0x' + '34'.repeat(32);
const mockToken: Token = {
  typeScriptHash: hash,
  typeCodeHash: other,
  typeHashType: 'data1',
  typeArgs: '0x00',
  standard: 'xudt',
  name: 'Test Token',
  symbol: 'TEST',
  decimals: 8,
  description: 'A test token',
  iconUrl: null,
  published: false,
  famous: false,
  tags: null,
  udtType: null,
  manager: null,
  email: null,
  operatorWebsite: null,
  totalSupply: '1000000000000',
  maximumSupply: null,
  maximumSupplyStatus: 'unknown',
  holdersCount: 42,
  transfersCount: 1000,
  transfers24h: 10,
  cellsCount: 150,
  totalCapacity: '50000000000001',
  totalCommonKnowledgeSize: '15300000000000',
  issuer: null,
  sameSymbolTokens: [],
  // The token detail always carries its metadata provenance (contracts INT I3).
  metadata: {
    name: 'Test Token',
    symbol: 'TEST',
    decimals: 8,
    source: 'label',
    chain: null,
    conflict: false,
  },
};
const history: TokenHistory = {
  source: 'indexed_chain_time',
  anchor: { blockNumber: 500, blockHash: other },
  completeThrough: '2024-01-15',
  daily: [
    {
      date: '2024-01-15',
      supply: '1000000000000',
      capacity: '50000000000001',
      occupiedCapacity: '15300000000000',
      freeCapacity: '34700000000001',
    },
  ],
  weeklyTransactions: [{ weekStart: '2024-01-01', transactionsCount: 5 }],
  mintCount: 2,
  burnCount: 1,
  transfersCount: 1000,
};
const empty = pagedRead({ data: [], limit: 20, hasMore: false, nextCursor: null });
const activity: GlobalActivity = {
  txHash: other,
  blockNumber: 12345,
  txIndex: 0,
  timestamp: '1700000000000',
  isCellbase: false,
  participants: [
    {
      lockHash: hash,
      lockHashPrefix: null,
      address: null,
      roles: [],
      tags: 1,
      ckbDelta: '100000001',
      usedDelta: '0',
      itemDeltas: [{ kind: 'token', typeScriptHash: hash, delta: '100000000000' }],
      sent: false,
    },
  ],
  typeCalls: [],
  lockCalls: [],
  recognizedTypes: [],
  protocolActions: [],
};
describe('TokenDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getToken).mockResolvedValue(mockToken);
    vi.mocked(api.getTokenHistory).mockResolvedValue(history);
    vi.mocked(api.getTokenSupplyEvents).mockResolvedValue({ ...empty, total: 0 });
    vi.mocked(api.getTokenHolders).mockResolvedValue({
      ...empty,
      total: 0,
      totalSupply: '0',
      anchor: null,
    });
    vi.mocked(api.getTokenDistribution).mockResolvedValue({
      holdersCount: 0,
      totalSupply: '0',
      largestHolderBalance: '0',
      nextNineBalance: '0',
      remainingBalance: '0',
      unit: 'token',
      decimals: 8,
      buckets: [],
    });
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue(empty);
    vi.mocked(api.getTokenTransactions).mockResolvedValue(empty);
  });

  // Lane B4 fix round (review-rules-logic NOTE 3, the coordinator's item 13): the token that
  // leads its line is the page's token as the API returned it, not the address's spelling of it.
  // A link may spell the hash in capitals (the API reads it either way); the feed's rows carry the
  // API's lowercase type script hash.
  it('leads each line with the page’s token however the address spells its hash', async () => {
    const lettered = `0x${'ab'.repeat(32)}`;
    vi.mocked(api.getToken).mockResolvedValue({ ...mockToken, typeScriptHash: lettered });
    const moved = {
      ...activity,
      participants: [
        {
          ...activity.participants[0],
          itemDeltas: [{ kind: 'token' as const, typeScriptHash: lettered, delta: '100000000000' }],
        },
      ],
    };
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue({ ...empty, data: [moved] });
    render(<TokenDetailPage typeHash={`0x${'AB'.repeat(32)}`} />);
    await screen.findAllByRole('article');
    const line = document.querySelector('[data-role="participant"]')!;
    expect([...line.children].map((e) => e.getAttribute('data-role'))).toEqual([
      'party',
      'item',
      'ckb',
    ]);
  });

  // B4 second fix round (review-rules-fixes-logic NOTE 8, the coordinator's item 14, ruling D7):
  // an issuer whose address spells another lock than it says is refused by the lock decision, and
  // the refusal fails the issuer's note in its place, never the page.
  it('says in the issuer’s note that its lock cannot be read, and draws the page', async () => {
    const joy = seededAddress('issuer', JOYID_CODE_HASH);
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      issuer: {
        lockHash: other,
        address: joy,
        lockCodeHash: DEFAULT_LOCK_CODE_HASH,
        lockKind: 'Default Lock',
      },
    });
    render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { level: 1 });
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    const alert = within(metrics).getByRole('alert');
    // third round (re-pinned on purpose, the coordinator's item 4): said where (the issuer)
    expect(alert).toHaveTextContent(
      `Could not draw the issuer’s lock (issuer ${other}): address ${joy} spells the lock code hash ${JOYID_CODE_HASH}`
    );
    expect(metrics).toHaveTextContent('owner lock proven by token script');
  });

  it('uses entity 404 only for the token read, not a secondary history error', async () => {
    vi.mocked(api.getToken).mockRejectedValue(
      new ApiRequestError(404, 'not_found', 'token absent')
    );
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText('no token with this type hash.')).toBeInTheDocument();
    // U-18: the facts name the checked endpoint with the id shortened, once
    expect(document.querySelector('.error-facts [data-endpoint]')).toHaveTextContent(
      `/tokens/${hash.slice(0, 10)}…${hash.slice(-6)}`
    );
  });

  // Visual finish V8: the page loads as the shared detail skeleton, in the page's main, not the
  // asset shell with a "Loading token…" line.
  it('shows the detail skeleton while the token loads', () => {
    vi.mocked(api.getToken).mockReturnValue(new Promise(() => {}));
    render(<TokenDetailPage typeHash={hash} />);
    const skeleton = screen.getByRole('status', { name: 'Loading …' });
    expect(skeleton.children).toHaveLength(3);
    expect(screen.getByRole('main')).toContainElement(skeleton);
    expect(screen.queryByText('Loading token…')).toBeNull();
    expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
  });

  it('shows a token invariant failure as an error', async () => {
    vi.mocked(api.getToken).mockRejectedValue(
      new ApiRequestError(500, 'internal_error', 'token index mismatch')
    );
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('token index mismatch');
    expect(screen.queryByText('no token with this type hash.')).not.toBeInTheDocument();
  });

  // A4 Chrome F3: the type hash under the title was HexCode's own-box scroller (box 358 px for
  // 436 px of content at 390, on the first screen); it is the head's HeadId in the head's 11 px,
  // on its own line under the standard.
  it('prints the type hash in its head as the head’s full id, never a scroller', async () => {
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('heading', { name: 'TEST' })).toBeInTheDocument();
    expectHeadId(hash);
  });

  it('keeps the token visible and names the distribution endpoint when that secondary read fails', async () => {
    vi.mocked(api.getTokenDistribution).mockRejectedValue(
      new ApiRequestError(404, 'not_found', 'distribution unavailable')
    );
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('heading', { name: 'TEST' })).toBeInTheDocument();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      `Could not load holder distribution from /tokens/${hash}/distribution: API error: 404 - distribution unavailable`
    );
    expect(screen.queryByText('no token with this type hash.')).not.toBeInTheDocument();
  });
  it('consumes unmodified router responses across history, distribution, holders and full transactions', async () => {
    const p = tokenProducer;
    vi.mocked(api.getToken).mockResolvedValue(p.token);
    vi.mocked(api.getTokenHistory).mockResolvedValue(p.history);
    vi.mocked(api.getTokenSupplyEvents).mockResolvedValue(p.supplyEvents);
    vi.mocked(api.getTokenDistribution).mockResolvedValue(p.distribution);
    vi.mocked(api.getTokenHolders).mockResolvedValue(p.holders);
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue(p.activityFeed);
    vi.mocked(api.getTokenTransactions).mockResolvedValue(p.transactions);
    render(<TokenDetailPage typeHash={p.token.typeScriptHash} />);
    expect(await screen.findByRole('heading', { name: 'TOK' })).toBeInTheDocument();
    // a stat tile's summary form: below 1 CKB, the exact value (readability R5, re-pinned on
    // purpose: was "0.00001000")
    expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent('0.00001 CKB');
    expect(await screen.findByRole('table', { name: 'Top 10 holders' })).toHaveTextContent(
      '100.00%'
    );
    fireEvent.click(screen.getByRole('tab', { name: 'transactions' }));
    expect(await screen.findByRole('table', { name: 'Asset transactions' })).toHaveTextContent(
      'Not computed'
    );
    expect(screen.getByRole('table', { name: 'Asset transactions' })).toHaveTextContent('10');
  });
  it('renders token overview, exact capacity partition and tier-one history', async () => {
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('heading', { name: 'TEST' })).toBeInTheDocument();
    expect(screen.getByText(fullText(/xUDT · 8 decimals/))).toBeInTheDocument();
    expect(screen.getByText('A test token')).toBeInTheDocument();
    const facts = screen.getByRole('region', { name: 'Metrics' });
    // F2 objects (D8): not established, with the one sentence that says why (C7: in its title).
    expect(facts).toHaveTextContent('maximum supply not established');
    expect(within(facts).getByText('maximum supply not established')).toHaveAttribute(
      'title',
      'Not established. Nothing CKBadger can read in this token’s type script fixes a cap.'
    );
    // a stat tile's summary form, exact in its title (readability R5, re-pinned on purpose: was
    // "500,000.00000001")
    expect(facts).toHaveTextContent('500,000 CKB');
    expect(within(facts).getByTitle('500,000.00000001 CKB')).toBeInTheDocument();
    expect(await screen.findByRole('heading', { name: 'History' })).toBeInTheDocument();
    // F2 objects (IV-19 / IV-26): live cells and the metadata source live in tier 2.
    expect(screen.getByText(/from a curated label shipped with CKBadger/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: other })).toHaveAttribute(
      'href',
      `/mainnet/script/${other}?hashType=data1&kind=type`
    );
  });
  it('renders unlimited max supply only when the API explicitly establishes it', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      standard: 'sudt',
      maximumSupplyStatus: 'unlimited',
    });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText('maximum supply unlimited')).toBeInTheDocument();
  });
  it('labels last full activity neutrally and states newest-first exactly once', async () => {
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue({ ...empty, data: [activity] });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText(/^It last moved/)).toBeInTheDocument();
    // re-pinned (lane c2-assets): the scope line stands under the tab row, in the section, no
    // longer inside the tablist (a tablist holds its tabs alone)
    expect(
      screen.getByRole('region', { name: 'What happened' }).textContent?.match(/newest first/g)
    ).toHaveLength(1);
    expect(screen.getByRole('tablist', { name: 'What happened' })).not.toHaveTextContent(
      'newest first'
    );
  });
  it('defaults to full participant activities and retains the separate transaction layer', async () => {
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue({ ...empty, data: [activity] });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('tab', { name: 'activities' })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    await waitFor(() =>
      expect(
        screen.getAllByRole('link').some((l) => l.getAttribute('href') === `/mainnet/tx/${other}`)
      ).toBe(true)
    );
    expect(screen.getByRole('link', { name: '#12,345' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/12345'
    );
    fireEvent.click(screen.getByRole('tab', { name: 'transactions' }));
    await waitFor(() =>
      expect(api.getTokenTransactions).toHaveBeenCalledWith(hash, { limit: 20, cursor: undefined })
    );
  });
  it('renders unknown decimals as raw amounts, never as zero decimals', async () => {
    vi.mocked(api.getToken).mockResolvedValue({ ...mockToken, decimals: null });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText(/xUDT · Decimals unknown/)).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent(
      '1,000,000,000,000 (raw)'
    );
  });
  it('scales amounts exactly for decimals beyond double precision', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      decimals: 24,
      totalSupply: '123456789012345678901234567890',
    });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('heading', { name: 'TEST' })).toBeInTheDocument();
    // The supply is a stat tile's summary form; every one of its 24 decimals, scaled in BigInt,
    // is in its title (readability R5, re-pinned on purpose: the tile printed them all).
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(metrics).toHaveTextContent('Supply123,456');
    expect(within(metrics).getByTitle('123,456.78901234567890123456789')).toBeInTheDocument();
  });
  it('shows same-symbol collisions, proven issuer and exact supply changes without guessing provenance', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      sameSymbolTokens: [{ typeScriptHash: other, holdersCount: 2 }],
      issuer: {
        lockHash: other,
        address: null,
        lockCodeHash: JOYID_CODE_HASH,
        lockKind: 'JoyID',
      },
    });
    vi.mocked(api.getTokenSupplyEvents).mockResolvedValue({
      ...empty,
      total: 1,
      data: [
        {
          txHash: other,
          blockNumber: 99,
          txIndex: 2,
          transferSequence: 3,
          timestamp: '1700000000000',
          kind: 'mint',
          amount: '1',
          supplyAfter: '1000000000000',
          lockHash: other,
          address: null,
          lockCodeHash: JOYID_CODE_HASH,
          lockKind: 'JoyID',
        },
      ],
    });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText(/Another token also calls itself/)).toHaveTextContent(
      '2 holders'
    );
    expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent(
      'JoyID · owner lock proven'
    );
    const details = await screen.findByText('Every change to the supply (1)');
    fireEvent.click(details);
    const table = screen.getByRole('table', { name: 'Mints and burns' });
    expect(table).toHaveTextContent('+0.00000001');
    expect(within(table).getByRole('link', { name: 'tx 2 · sequence 3' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${other}`
    );
  });
  // U-16, PB-05 (UI review 2026-11-14): the same-symbol note broke this token's shortened type hash
  // at its ellipsis ("this one is 0x797bfd0b…" / "4080."); the metrics row left a "·" at a line's
  // end ("644,078.845914 · USDC ·") and opened one ("· 3.82% occupied").
  it('keeps the note’s shortened ids whole and every metric note’s dot with its note (U-16, PA-25)', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      sameSymbolTokens: [{ typeScriptHash: other, holdersCount: 2 }],
      issuer: {
        lockHash: other,
        address: null,
        lockCodeHash: JOYID_CODE_HASH,
        lockKind: 'JoyID',
      },
    });
    render(<TokenDetailPage typeHash={hash} />);
    const note = await screen.findByText(/Another token also calls itself/);
    // U-09 (re-pinned): the head's note writes the type hash at the head size, 12…8; R9
    // (re-pinned): a note is prose, the row size, 10…6
    const self = within(note).getByText(`${hash.slice(0, 10)}…${hash.slice(-6)}`);
    expect(self.closest('[data-nobreak]')).not.toBeNull();
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    // every dot of the metrics row belongs to the item after it: none is loose text
    const loose = [...metrics.querySelectorAll('*')].flatMap((el) =>
      [...el.childNodes].filter(
        (n) =>
          n.nodeType === Node.TEXT_NODE &&
          /·/.test(n.textContent ?? '') &&
          !el.hasAttribute('data-dotted-separator')
      )
    );
    expect(loose.map((n) => n.textContent)).toEqual([]);
    expect(metrics).toHaveTextContent('JoyID · owner lock proven by token script');
  });
  // U-15 / lane A4b (UI review 2026-11-14): an issuer whose lock the scripts registry does not name
  // read "Lock kind unavailable · owner lock proven by token script"; the answer now carries the
  // lock's code hash, and the note names the lock by it, linked to that code hash's script page.
  // U-09 (re-pinned): the code hash and the issuer's lock hash take the row size, 10…6.
  it('names an unregistered issuer lock by its code hash, a link to its script page (U-15)', async () => {
    const unregistered = `0x${'43'.repeat(32)}`;
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      issuer: {
        lockHash: other,
        // B4 fix round: a real address on the unregistered lock (the lock is read off it)
        address: seededAddress('test', unregistered),
        lockCodeHash: unregistered,
        lockKind: null,
      },
    });
    render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(metrics).toHaveTextContent('lock 0x43434343…434343 · owner lock proven by token script');
    expect(within(metrics).getByRole('link', { name: '0x43434343…434343' })).toHaveAttribute(
      'href',
      // B4 second fix round (re-pinned on purpose): that lock's page, by the hash type its address
      // spells, as a lock
      expect.stringMatching(new RegExp(`/script/${unregistered}\\?hashType=type&kind=lock$`))
    );
    expect(metrics).not.toHaveTextContent(/lock kind unavailable/i);
  });
  // B4 fix round (review-rules MUST 1): the default lock is silent beside the issuer, and the note
  // leaves that part out: one separator, never "· ·". On the real default lock, by type.
  it('writes one separator before the proof for an issuer on the default lock', async () => {
    const address =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      issuer: {
        lockHash: other,
        address,
        lockCodeHash: DEFAULT_LOCK_CODE_HASH,
        lockKind: 'Default Lock',
      },
    });
    render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(metrics).toHaveTextContent('ckb1…eqxwquc4 · owner lock proven by token script');
    expect(metrics).not.toHaveTextContent(/·\s*·/);
    expect(metrics).not.toHaveTextContent('Default Lock');
  });
  it('writes no lock kind for an issuer lock that is not indexed (U-15)', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      issuer: { lockHash: other, address: null, lockCodeHash: null, lockKind: null },
    });
    render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    // the issuer's lock hash, then the note with no empty part before it
    expect(metrics).toHaveTextContent(/0x34343434…343434 · owner lock proven by token script/);
    expect(metrics.querySelector('[data-lock-kind]')).toBeNull();
    expect(metrics).not.toHaveTextContent(/lock kind unavailable/i);
  });
  // A4b review: an issuer whose lock has no address form (args longer than an address can carry)
  // comes with a null address and its lock's code hash and kind: the metrics name it by its lock
  // hash, linked to the address page that takes a lock hash, and by its lock kind.
  it('names an issuer whose lock has no address form by its lock hash and kind (A4b review)', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      issuer: {
        lockHash: other,
        address: null,
        // B4 fix round: a real other lock (the made-up 0x9b… is no SECP256K1/blake160)
        lockCodeHash: JOYID_CODE_HASH,
        lockKind: 'JoyID',
      },
    });
    render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(within(metrics).getByRole('link', { name: /^0x3434/ })).toHaveAttribute(
      'href',
      expect.stringMatching(new RegExp(`/address/${other}$`))
    );
    expect(metrics).toHaveTextContent(/343434 · JoyID · owner lock proven by token script/);
  });
  it('shows source errors instead of empty history and supports all-holder cursor paging', async () => {
    vi.mocked(api.getTokenHistory).mockRejectedValue(new Error('History read failed'));
    // Re-pinned (lane c2-assets): the pager reads its pages from the response, so the pages agree
    // with it: 21 holders, a full first page of 20 and one on the second (it was one holder a page
    // of 20 with more after it, a count of 2); the way on is the board's "next →" (was "Next").
    const holder = (i: number) => ({
      lockScriptHash: '0x' + i.toString(16).padStart(64, '0'),
      address: null,
      balance: '1',
      lockCodeHash: null,
      lockKind: null,
    });
    vi.mocked(api.getTokenHolders).mockImplementation(async (_, p) =>
      pagedRead({
        ...empty,
        data: p?.cursor ? [holder(21)] : Array.from({ length: 20 }, (_, i) => holder(i + 1)),
        total: 21,
        totalSupply: '21',
        anchor: null,
        hasMore: !p?.cursor,
        nextCursor: p?.cursor ? null : 'page2',
      })
    );
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('History read failed');
    // Re-pinned on purpose (lane p-assets, the placement lane's toggles with the lead's ruling): the
    // short list's form under the top ten, "all 21 holders ↓" (it was "All 21 holders →" in the
    // heading row); its way back "fewer ↑"
    fireEvent.click(await screen.findByRole('button', { name: /^all 21\sholders ↓$/ }));
    await screen.findByRole('table', { name: 'All holders' });
    expect(screen.getByRole('button', { name: 'fewer ↑' })).toBeInTheDocument();
    const pager = screen.getByRole('navigation', { name: 'holders' });
    expect(pager).toHaveTextContent('1–20 of 21');
    fireEvent.click(within(pager).getByRole('button', { name: 'next →' }));
    await waitFor(() =>
      expect(api.getTokenHolders).toHaveBeenCalledWith(hash, { limit: 20, cursor: 'page2' })
    );
  });
});
