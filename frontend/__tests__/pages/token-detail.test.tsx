import { tokenProducer } from '@/__tests__/msw/fixtures/asset-producer';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, fireEvent, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
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
const empty = { data: [], limit: 50, hasMore: false, nextCursor: null };
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
    expect(await screen.findByText(`/tokens/${hash}`)).toBeInTheDocument();
    expect(
      screen.getByText('This explorer could not open the token at this address.')
    ).toBeInTheDocument();
  });

  it('shows a token invariant failure as an error', async () => {
    vi.mocked(api.getToken).mockRejectedValue(
      new ApiRequestError(500, 'internal_error', 'token index mismatch')
    );
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('token index mismatch');
    expect(
      screen.queryByText('This explorer could not open the token at this address.')
    ).not.toBeInTheDocument();
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
    expect(
      screen.queryByText('This explorer could not open the token at this address.')
    ).not.toBeInTheDocument();
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
    expect(screen.getByLabelText('Facts')).toHaveTextContent('0.00001000');
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
    expect(screen.getByText(/xUDT · 8 decimals/)).toBeInTheDocument();
    expect(screen.getByText('A test token')).toBeInTheDocument();
    const facts = screen.getByLabelText('Facts');
    expect(facts).toHaveTextContent('Maximum supply: Unknown');
    expect(facts).toHaveTextContent('500,000.00000001');
    expect(facts).toHaveTextContent('Live cells150');
    expect(await screen.findByRole('heading', { name: 'History' })).toBeInTheDocument();
    expect(screen.getByText(/Metadata provenance not recorded/)).toBeInTheDocument();
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
    expect(await screen.findByText('Maximum supply: Unlimited')).toBeInTheDocument();
  });
  it('labels last full activity neutrally and states newest-first exactly once', async () => {
    vi.mocked(api.getTokenActivityFeed).mockResolvedValue({ ...empty, data: [activity] });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText(/^Last transaction involving this token/)).toBeInTheDocument();
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
      expect(api.getTokenTransactions).toHaveBeenCalledWith(hash, { limit: 50, cursor: undefined })
    );
  });
  it('renders unknown decimals as raw amounts, never as zero decimals', async () => {
    vi.mocked(api.getToken).mockResolvedValue({ ...mockToken, decimals: null });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText(/xUDT · Decimals unknown/)).toBeInTheDocument();
    expect(screen.getByLabelText('Facts')).toHaveTextContent('1,000,000,000,000 (raw)');
  });
  it('scales amounts exactly for decimals beyond double precision', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      decimals: 24,
      totalSupply: '123456789012345678901234567890',
    });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByRole('heading', { name: 'TEST' })).toBeInTheDocument();
    expect(screen.getByLabelText('Facts')).toHaveTextContent('123,456.789012345678901234567890');
  });
  it('shows same-symbol collisions, proven issuer and exact supply changes without guessing provenance', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      sameSymbolTokens: [{ typeScriptHash: other, holdersCount: 2 }],
      issuer: { lockHash: other, address: null, lockKind: 'JoyID' },
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
          lockKind: 'JoyID',
        },
      ],
    });
    render(<TokenDetailPage typeHash={hash} />);
    expect(await screen.findByText(/Other tokens also call themselves/)).toHaveTextContent(
      '2 holders'
    );
    expect(screen.getByLabelText('Facts')).toHaveTextContent('JoyID · owner lock proven');
    const details = await screen.findByText('Every change to the supply (1)');
    fireEvent.click(details);
    const table = screen.getByRole('table', { name: 'Mints and burns' });
    expect(table).toHaveTextContent('+0.00000001');
    expect(within(table).getByRole('link', { name: 'tx 2 · sequence 3' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${other}`
    );
  });
  it('shows source errors instead of empty history and supports all-holder cursor paging', async () => {
    vi.mocked(api.getTokenHistory).mockRejectedValue(new Error('History read failed'));
    vi.mocked(api.getTokenHolders).mockImplementation(async (_, p) => ({
      ...empty,
      data: [{ lockScriptHash: hash, address: null, balance: '1', lockKind: null }],
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
      expect(api.getTokenHolders).toHaveBeenCalledWith(hash, { limit: 50, cursor: 'page2' })
    );
  });
});
