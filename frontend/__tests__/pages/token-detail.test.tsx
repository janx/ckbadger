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
    },
  ],
  weeklyTransactions: [{ weekStart: '2024-01-01', transactionsCount: 5 }],
  mintCount: 2,
  burnCount: 1,
  transfersCount: 1000,
};
const empty = { data: [], limit: 20, hasMore: false, nextCursor: null };
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
    },
  ],
  typeCalls: [],
  lockCalls: [],
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
    expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent('0.00001000');
    expect(await screen.findByRole('table', { name: 'Top 10 holders' })).toHaveTextContent(
      '100.00%'
    );
    fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
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
    expect(facts).toHaveTextContent('500,000.00000001');
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
    expect(
      screen.getByRole('tablist', { name: 'What happened' }).textContent?.match(/newest first/g)
    ).toHaveLength(1);
  });
  it('defaults to full participant activities and retains the separate transaction layer', async () => {
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue({ ...empty, data: [activity] });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('tab', { name: 'Activities' })).toHaveAttribute(
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
    fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
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
    expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent(
      '123,456.789012345678901234567890'
    );
  });
  it('shows same-symbol collisions, proven issuer and exact supply changes without guessing provenance', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      sameSymbolTokens: [{ typeScriptHash: other, holdersCount: 2 }],
      issuer: {
        lockHash: other,
        address: null,
        lockCodeHash: `0x${'9b'.repeat(32)}`,
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
          lockCodeHash: `0x${'9b'.repeat(32)}`,
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
        lockCodeHash: `0x${'9b'.repeat(32)}`,
        lockKind: 'JoyID',
      },
    });
    render(<TokenDetailPage typeHash={hash} />);
    const note = await screen.findByText(/Another token also calls itself/);
    // U-09 (re-pinned): the head's note writes the type hash at the head size, 12…8
    const self = within(note).getByText(`${hash.slice(0, 12)}…${hash.slice(-8)}`);
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
        address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqtest',
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
      expect.stringMatching(new RegExp(`/script/${unregistered}$`))
    );
    expect(metrics).not.toHaveTextContent(/lock kind unavailable/i);
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
        lockCodeHash: `0x${'9b'.repeat(32)}`,
        lockKind: 'SECP256K1/blake160',
      },
    });
    render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(within(metrics).getByRole('link', { name: /^0x3434/ })).toHaveAttribute(
      'href',
      expect.stringMatching(new RegExp(`/address/${other}$`))
    );
    expect(metrics).toHaveTextContent(
      /343434 · SECP256K1\/blake160 · owner lock proven by token script/
    );
  });
  it('shows source errors instead of empty history and supports all-holder cursor paging', async () => {
    vi.mocked(api.getTokenHistory).mockRejectedValue(new Error('History read failed'));
    vi.mocked(api.getTokenHolders).mockImplementation(async (_, p) => ({
      ...empty,
      data: [
        { lockScriptHash: hash, address: null, balance: '1', lockCodeHash: null, lockKind: null },
      ],
      total: 2,
      totalSupply: '2',
      anchor: null,
      hasMore: !p?.cursor,
      nextCursor: p?.cursor ? null : 'page2',
    }));
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('History read failed');
    fireEvent.click(screen.getByRole('button', { name: /All .*holders/ }));
    await screen.findByRole('table', { name: 'All holders' });
    fireEvent.click(
      within(screen.getByRole('table', { name: 'All holders' }).closest('section')!).getByRole(
        'button',
        { name: 'Next' }
      )
    );
    await waitFor(() =>
      expect(api.getTokenHolders).toHaveBeenCalledWith(hash, { limit: 20, cursor: 'page2' })
    );
  });
});
