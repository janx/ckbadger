// F2 group `objects` (round 2): the Token page against the approved Token board — plain activity
// rows with a lock-kind column and the token mark (IV-14), metadata provenance from the API (IV-26),
// the paged supply events and history line (IV-27, IV-17), holders in the token colour (IV-18), the
// rail with D8's "Not established" sentences (IV-19, D8), header (IV-16) and tier 2 (IV-20).
import { tokenProducer } from '@/__tests__/msw/fixtures/asset-producer';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, fireEvent, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { ledeRestatements } from '@/__tests__/utils/lede-rule';
import TokenDetailPage from '@/app/tokens/[typeHash]/client-page';
import {
  api,
  ApiRequestError,
  type Token,
  type TokenHistory,
  type GlobalActivity,
} from '@/lib/api';
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
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
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

const event = (i: number, kind: 'mint' | 'burn') => ({
  txHash: '0x' + String(i).padStart(2, '0').repeat(32),
  blockNumber: 1000 - i,
  txIndex: 0,
  transferSequence: i,
  timestamp: String(1700000000000 - i * 1000),
  kind,
  amount: '100000000',
  supplyAfter: '1000000000000',
  lockHash: other,
  address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqtest',
  lockKind: 'JoyID',
});
describe('Token board (F2 objects)', () => {
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

  it('shows plain activity rows with the token mark and a lock-kind column (IV-14)', async () => {
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue({
      ...empty,
      data: [
        {
          ...activity,
          typeCalls: [
            {
              typeCodeHash: other,
              typeHashType: 'type',
              typeArgs: '0x',
              scriptHash: hash,
              scriptName: 'xUDT',
            },
          ],
          participants: activity.participants.map((p) => ({
            ...p,
            lockKind: 'UTXOSwap Intent Lock',
          })),
        },
      ],
    });
    const { container } = render(<TokenDetailPage typeHash={hash} />);
    const row = await screen.findByRole('article');
    expect(row.querySelector('[data-role="detail"]')).toBeNull();
    expect(row.querySelector('[data-role="lock-kind"]')).toHaveTextContent('UTXOSwap Intent Lock');
    expect(row.querySelector('[data-role="headline"] [data-class="tokens"]')).not.toBeNull();
    expect(container).toBeTruthy();
  });

  it('states where the name, symbol and decimals come from (IV-26)', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      name: 'RGB++_Jump',
      symbol: 'RGB++_Jump',
      decimals: 18,
      metadata: {
        name: 'RGB++_Jump',
        symbol: 'RGB++_Jump',
        decimals: 18,
        source: 'label',
        chain: { name: 'RGB++ Protocol', symbol: 'RGB++', decimals: 18 },
        conflict: true,
      },
    });
    const { container } = render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'RGB++_Jump' });
    // Consistency plan C1: the tab names the page.
    await waitFor(() => expect(document.title).toBe('RGB++_Jump · token · CKBadger'));
    const tier2 = container.querySelector('[data-tier="2"]')!;
    expect(tier2).toHaveTextContent(
      'from a curated label shipped with CKBadger, not from the chain'
    );
    expect(tier2).toHaveTextContent('Its Unique Cell on chain says RGB++ Protocol · RGB++ · 18.');
    expect(tier2).not.toHaveTextContent('Metadata provenance not recorded');
    expect(screen.getByRole('banner')).toHaveTextContent('On chain it calls itself RGB++');
  });

  it('says plainly when no label or Unique Cell names the token (IV-26)', async () => {
    vi.mocked(api.getToken).mockResolvedValue({ ...mockToken, metadata: null });
    const { container } = render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    expect(container.querySelector('[data-tier="2"]')).toHaveTextContent(
      'Neither a curated label nor a Unique Cell names this token.'
    );
  });

  it('pages the supply events and words the history line (IV-27, IV-17)', async () => {
    vi.mocked(api.getTokenSupplyEvents).mockImplementation(async (_, p) => ({
      ...empty,
      total: 3,
      limit: 2,
      data: p?.cursor ? [event(3, 'burn')] : [event(1, 'mint'), event(2, 'mint')],
      hasMore: !p?.cursor,
      nextCursor: p?.cursor ? null : '999:2',
    }));
    render(<TokenDetailPage typeHash={hash} />);
    const history = await screen.findByRole('region', { name: 'History' });
    expect(history).toHaveTextContent('2 mints · 1 burn · 1,000 transfers');
    // Responsive fix plan 2026-11-01 (A3, report B#18): "H-multiplier ×N" is one unbroken unit.
    const hmul = history.querySelector('[data-role="h-multiplier"]');
    expect(hmul).toHaveTextContent(/^H-multiplier ×\d+\.\d{2}$/);
    expect(hmul).toHaveClass('whitespace-nowrap');
    fireEvent.click(await within(history).findByText('Every change to the supply (3)'));
    const table = within(history).getByRole('table', { name: 'Mints and burns' });
    expect(within(table).getAllByText('Mint')).toHaveLength(2);
    fireEvent.click(within(history).getByRole('button', { name: 'Next' }));
    await waitFor(() =>
      expect(api.getTokenSupplyEvents).toHaveBeenLastCalledWith(hash, {
        limit: 20,
        cursor: '999:2',
      })
    );
    expect(
      await within(within(history).getByRole('table', { name: 'Mints and burns' })).findByText(
        'Burn'
      )
    ).toBeInTheDocument();
  });

  it('draws who holds it in the token colour and names the rest (IV-18)', async () => {
    vi.mocked(api.getTokenDistribution).mockResolvedValue({
      holdersCount: 720,
      totalSupply: '10000',
      largestHolderBalance: '9449',
      nextNineBalance: '381',
      remainingBalance: '170',
      unit: 'token',
      decimals: 0,
      buckets: [{ lowerBound: '0', upperBound: null, holdersCount: 720, balance: '10000' }],
    });
    render(<TokenDetailPage typeHash={hash} />);
    const bar = await screen.findByRole('img', { name: /Largest holder 94.49%/ });
    expect(bar.querySelector('[data-segment]')).toHaveStyle({ backgroundColor: 'var(--c-token)' });
    expect(bar).toHaveAccessibleName(/Other 710 1.70%/);
    expect(screen.queryByText('Remaining holders')).not.toBeInTheDocument();
  });

  it('keeps issuer and cap "Not established" and says why (D8, IV-19)', async () => {
    vi.mocked(api.getToken).mockResolvedValue({ ...mockToken, transfers24h: 0 });
    render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    // C7: the rail is the metrics row; each "not established" keeps its sentence in a title.
    const facts = screen.getByRole('region', { name: 'Metrics' });
    expect(within(facts).getByText('maximum supply not established')).toHaveAttribute(
      'title',
      'Not established. Nothing CKBadger can read in this token’s type script fixes a cap.'
    );
    expect(within(facts).getByText('Not established')).toHaveAttribute(
      'title',
      'Not established. This token’s type script does not name an owner lock CKBadger can prove mints it.'
    );
    expect(facts).toHaveTextContent('· none in 24 h');
    expect(facts).not.toHaveTextContent('Live cells');
  });

  // Consistency round C7: the metrics row replaces the rail (one column), and the lede no longer
  // restates it ("… exist, held by … Its cells hold …" was the rail twice).
  it('opens with one metrics row and a lede that restates none of it (C7)', async () => {
    vi.mocked(api.getTokenDistribution).mockResolvedValue({
      holdersCount: 720,
      totalSupply: '10000',
      largestHolderBalance: '9449',
      nextNineBalance: '381',
      remainingBalance: '170',
      unit: 'token',
      decimals: 0,
      buckets: [{ lowerBound: '0', upperBound: null, holdersCount: 720, balance: '10000' }],
    });
    const { container } = render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    expect(screen.queryByRole('complementary', { name: 'Facts' })).toBeNull();
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(
      within(metrics)
        .getAllByRole('term')
        .map((t) => t.textContent)
    ).toEqual(['Supply', 'Holders', 'Transfers', 'Issuer', 'CKB held in its cells']);
    await waitFor(() => expect(metrics).toHaveTextContent('largest holds 94.49%'));
    expect(screen.getByRole('banner')).not.toHaveTextContent(/exist, held by|Its cells hold/);
    expect(ledeRestatements(container)).toEqual([]);
    // The type hash, which the rail listed, names the token under its title.
    expect(screen.getByRole('banner')).toHaveTextContent(hash);
  });

  it('opens with a 13 px identity line and tells the same-symbol tokens apart (IV-16)', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      sameSymbolTokens: [{ typeScriptHash: other, holdersCount: 2 }],
    });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText(/xUDT · 8 decimals/)).toHaveClass('text-body');
    expect(await screen.findByText(/Another token also calls itself TEST/)).toHaveTextContent(
      'Tell them apart by type hash; this one is 0x12121212…1212.'
    );
  });

  it('explains the type script in tier 2 (IV-20)', async () => {
    const { container } = render(<TokenDetailPage typeHash={hash} />);
    await screen.findByRole('heading', { name: 'TEST' });
    const tier2 = container.querySelector('[data-tier="2"]')!;
    expect(tier2).toHaveTextContent(
      'the token’s type script; its code decides what a valid transfer is'
    );
    expect(tier2).toHaveTextContent('Live cells150');
    // Lane E (C6): the type script drawn as its occupied bytes, one row per part.
    const args = (mockToken.typeArgs.length - 2) / 2;
    expect(tier2).toHaveTextContent(`Script · ${33 + args} occupied bytes in every cell it types`);
    expect(
      [
        ...within(tier2 as HTMLElement).getByRole('group', { name: 'type script byte map' })
          .children,
      ].map((p) => (p as HTMLElement).dataset.segment)
    ).toEqual(['code_hash', 'hash_type', 'args']);
    expect(within(tier2 as HTMLElement).getByLabelText('type script parts')).toHaveTextContent(
      `hash_type${mockToken.typeHashType} (`
    );
  });

  // Responsive gate (2026-11-01): the issuer's full address inside the args note had no break
  // opportunity and pushed the page 76 px past 640 and 326 px past 390.
  it('wraps the issuer address in the args note inside its box', async () => {
    const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq' + 'x'.repeat(40);
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      issuer: { lockHash: other, address, lockKind: 'Default Lock' },
    });
    render(<TokenDetailPage typeHash={hash} />);
    const note = await screen.findByText(/the lock hash of the owner that can mint/);
    expect(note).toHaveTextContent(address);
    expect(note).toHaveClass('[overflow-wrap:anywhere]');
    // Lane L: the issuer's address is a link to its page.
    expect(within(note).getByRole('link', { name: address })).toHaveAttribute(
      'href',
      `/mainnet/address/${address}`
    );
  });

  // Visual pass: the last movement reads as one plain clause.
  it('says when it last moved and what that transaction did', async () => {
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue({ ...empty, data: [activity] });
    render(<TokenDetailPage typeHash={hash} />);
    const line = await screen.findByText(/^It last moved/);
    // Lane P: the day it moved, on the UTC+8 clock; how long ago is the date's hover.
    expect(line.textContent).toMatch(/^It last moved on \d{4}-\d\d-\d\d: .+\.$/);
    expect(line.querySelector('time')).toHaveAttribute('title', expect.stringMatching(/ ago$/));
    expect(line).not.toHaveTextContent('the feed names');
  });
});
