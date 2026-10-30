// F2 blocks: the Block / BlockTxs boards (BA-12 … BA-20, BA-40) on block #20,827,932's shape.
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import BlockDetailPage from '@/app/blocks/[id]/client-page';
import { resourceScale } from '@/components/blocks/block-visual';
import { api, type GlobalActivity, type Transaction } from '@/lib/api';

vi.mock('@/lib/api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api')>()),
  api: {
    getBlock: vi.fn(),
    getBlocks: vi.fn(),
    getBlockUncle: vi.fn(),
    getBlockProposalFees: vi.fn(),
    getTransactions: vi.fn(),
    getBlockProposals: vi.fn(),
    getHardforks: vi.fn(),
    getGlobalActivities: vi.fn(),
  },
  isNotFoundApiError: () => false,
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ id: '20827932' }),
  usePathname: () => '/blocks/20827932',
  useRouter: () => ({ push: vi.fn() }),
}));

const h = (byte: string) => `0x${byte.repeat(32)}`;
const N = 20_827_932;
const MINER = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgp909qk';
const PARENT_OF_UNCLE = h('55');
const block = {
  ...blockEvidence,
  number: N,
  hash: h('f4'),
  parentHash: h('95'),
  timestamp: '2026-10-25T00:55:37Z',
  parentTimestamp: '2026-10-25T00:55:25.379Z',
  transactionsCount: 4,
  proposalsCount: 2,
  unclesCount: 1,
  uncles: [
    {
      number: N - 1,
      hash: h('cc'),
      parentHash: PARENT_OF_UNCLE,
      timestamp: '2026-10-25T00:55:26.272Z',
      proposalIds: [],
    },
  ],
  difficulty: '961164519177266765',
  epoch: '1002/1472',
  epochNumber: 15_173,
  epochIndex: 1002,
  epochLength: 1472,
  nonce: '0x1',
  transactionsRoot: h('40'),
  minerAddress: MINER,
  minerMessage: '0x302e3231302e30',
  minerMessageText: '0.210.0 (2592ddf 2026-09-17)',
  miningReward: '68432120795',
  miningRewardTxHash: h('c7'),
  rewardBreakdown: {
    primary: '65142942227',
    secondary: '3286129910',
    committed: '3036000',
    proposal: '12658',
    secondaryIssuance: '41691483025',
  },
  cellbasePays: { number: N - 11, hash: h('ec') },
  rewardPaidAtBlock: N + 11,
  rewardFinalizesAt: N + 11,
  totalFees: '5060000',
  size: 3205,
  overheadSize: 883,
  userTransactionsSize: 2322,
  cycles: '5091148',
  transactionSizes: [355, 516, 903, 903],
  rawHeader: {
    epoch: '0x5c003ea003b45',
    dao: '0x28341e52bbf1665b9cb7fbb823dd2a0077f0143eecc3870a00e95a770f4f3407',
    proposalsHash: h('c6'),
    extraHash: h('c0'),
  },
  compactTarget: '0x1913312c',
  version: 0,
  hardforkActivation: null,
};
const neighbour = (number: number, over: Record<string, unknown> = {}) => ({
  ...block,
  number,
  hash: number === N - 2 ? PARENT_OF_UNCLE : h((number % 256).toString(16).padStart(2, '0')),
  timestamp: `2026-10-25T00:5${number - N + 5}:00Z`,
  uncles: [],
  unclesCount: 0,
  size: 641,
  cycles: '0',
  transactionSizes: [355],
  proposalsCount: 0,
  ...over,
});
const neighbours = {
  data: [
    neighbour(N + 2, { size: 3917, transactionSizes: [355, 500, 977] }),
    neighbour(N + 1),
    block,
    neighbour(N - 1),
    neighbour(N - 2, { proposalsCount: 3 }),
  ],
  total: N + 3,
  limit: 50,
  hasMore: true,
  nextCursor: String(N - 2),
};
const activity = (txIndex: number): GlobalActivity => ({
  txHash: `0x${txIndex}fbf9fbc${'0'.repeat(48)}f9199a`.slice(0, 66),
  blockNumber: N,
  txIndex,
  timestamp: block.timestamp,
  isCellbase: txIndex === 0,
  protocolActions: [],
  typeCalls: [],
  lockCalls: [],
  participants: [],
});
const tx = (index: number): Transaction =>
  ({
    hash: h(`${index}a`),
    blockNumber: N,
    blockHash: block.hash,
    index,
    inputsCount: index === 0 ? 1 : 2,
    outputsCount: index === 0 ? 1 : 2,
    fee: index === 0 ? '0' : '5000000',
    value: '0',
    txSize: block.transactionSizes[index],
    cycles: index === 0 ? 0 : 1_645_207,
    cyclesStatus: 'complete',
    outputCapacity: '126697017807',
    proposalOrigin: index === 0 ? null : { blockNumber: N - 2, distance: 2, uncle: null },
    isCellbase: index === 0,
    timestamp: block.timestamp,
  }) as unknown as Transaction;

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getBlock).mockResolvedValue(block);
  vi.mocked(api.getBlockProposalFees).mockResolvedValue({
    number: N,
    hash: block.hash,
    commitWindow: { from: N + 2, to: N + 10 },
    complete: true,
    fees: '31647',
    transactionsCount: 2,
    committedIn: [N + 2],
    transactions: [],
  });
  vi.mocked(api.getBlocks).mockResolvedValue(neighbours);
  vi.mocked(api.getBlockUncle).mockResolvedValue({
    number: N - 1,
    hash: h('cc'),
    parentHash: PARENT_OF_UNCLE,
    timestamp: '2026-10-25T00:55:26.272Z',
    epoch: '1001/1472',
    epochNumber: 15_173,
    epochIndex: 1001,
    epochLength: 1472,
    compactTarget: '0x1913312c',
    difficulty: '961164519177266765',
    nonce: '0x2',
    version: 0,
    transactionsRoot: h('41'),
    proposalsHash: h('42'),
    extraHash: h('43'),
    dao: { raw: block.rawHeader.dao, c: '0', ar: '0', s: '0', u: '0' },
    proposalIds: [],
    includedBy: { number: N, hash: block.hash, index: 0 },
    canonicalAtHeight: { number: N - 1, hash: h('95') },
    sameParentAs: N - 1,
    msAfterSibling: 92,
    rawEpoch: '0x5c003e9003b45',
  });
  // R2b-a 3: `order=asc` answers the block in tx order, cellbase first.
  vi.mocked(api.getGlobalActivities).mockResolvedValue({
    data: [0, 1, 2, 3].map(activity),
    limit: 100,
    hasMore: false,
    nextCursor: null,
  });
  vi.mocked(api.getTransactions).mockResolvedValue({
    data: [0, 1, 2, 3].map(tx),
    limit: 100,
    hasMore: false,
    nextCursor: null,
  });
  vi.mocked(api.getBlockProposals).mockResolvedValue([
    {
      proposalIndex: 0,
      proposalId: '0xd42214475044ac71888b',
      committedTxHash: h('d4'),
      committedBlockNumber: N + 2,
      uncle: null,
    },
    {
      proposalIndex: 1,
      proposalId: '0x6feb58e242396aae0567',
      committedTxHash: h('6f'),
      committedBlockNumber: N + 2,
      uncle: null,
    },
  ]);
});

describe('Block board · first tier', () => {
  it('titles the block by its height between arrow links, hash and summary as the board (BA-16)', async () => {
    render(<BlockDetailPage />);
    const title = await screen.findByRole('heading', { level: 1 });
    expect(title).toHaveTextContent(/^#20,827,932$/);
    expect(screen.getByRole('link', { name: 'Previous block, 20,827,931' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Next block, 20,827,933' })).toBeInTheDocument();
    const summary = screen.getByTestId('block-summary');
    expect(summary).toHaveClass('text-[15px]');
    expect(summary).toHaveTextContent(
      /^4 transactions, mined by .+ on 2026-10-25 at 08:55:37 UTC\+8\. The miner earns 684\.32120795 CKB for this block, paid 11 blocks later\.$/
    );
  });

  it('counts both views in quiet 12 px beside 15 px tab names, and says where the txs came from (BA-40, BA-16)', async () => {
    render(<BlockDetailPage />);
    const views = await screen.findByRole('group', { name: 'Block contents' });
    const [activities, transactions] = within(views).getAllByRole('button');
    for (const [tab, name] of [
      [activities, 'Activities'],
      [transactions, 'Transactions'],
    ] as const) {
      expect(tab).toHaveClass('text-[15px]');
      expect(tab.firstChild).toHaveTextContent(name);
      expect(within(tab).getByText('4')).toHaveClass('text-[12px]');
    }
    expect(
      await screen.findByText("3 committed from #20,827,930's proposals, 2 blocks later")
    ).toBeInTheDocument();
  });

  it('lists activities in block order, cellbase first, with the tx index instead of the height (BA-12, BA-14)', async () => {
    render(<BlockDetailPage />);
    await screen.findByText('Cellbase');
    const rows = Array.from(document.querySelectorAll('[data-feed-row]'));
    expect(rows.map((row) => row.getAttribute('data-feed-row'))).toEqual(
      [0, 1, 2, 3].map((i) => activity(i).txHash)
    );
    expect(within(rows[0] as HTMLElement).getByText('Cellbase')).toBeInTheDocument();
    const evidence = rows[0].querySelector('[data-role="evidence"]')!;
    expect(evidence).toHaveTextContent(/^#0 · 0x0fbf9fbc/);
    expect(evidence).not.toHaveTextContent('20,827,932');
    expect(screen.queryByText('Coinbase')).not.toBeInTheDocument();
  });
});

describe('Block board · second tier', () => {
  it('splits the reward exactly, names the payee and explains the 60 % / 40 % fee shares (BA-13)', async () => {
    render(<BlockDetailPage />);
    const section = (await screen.findByRole('heading', { name: 'Reward breakdown' })).closest(
      'section'
    )!;
    expect(section).toHaveTextContent('paid by the cellbase of #20,827,943');
    expect(within(section).getByTestId('reward-total')).toHaveTextContent(
      /^684\.32120795 CKB to ckb1qzda0c/
    );
    const bar = within(section).getByRole('img', { name: /^Reward shares:/ });
    expect(bar).toHaveAccessibleName(
      'Reward shares: primary issuance 95.19%, secondary issuance 4.80%, committed fees 0.00%, proposal fees 0.00%'
    );
    const parts = bar.querySelectorAll('[data-reward-part]');
    expect(parts).toHaveLength(4);
    for (const part of Array.from(parts)) expect((part as HTMLElement).style.minWidth).toBe('3px');
    expect(section).toHaveTextContent('Committed fees, 60%');
    expect(section).toHaveTextContent('Proposal fees, 40%');
    expect(section).toHaveTextContent('of 0.0506 in fees of the 3 transactions committed here');
    expect(section).toHaveTextContent('the rest goes to DAO depositors or is burnt');
  });

  it('shows the facts without a "Block evidence" title: difficulty, epoch bar, list-scale bars, client (BA-17)', async () => {
    render(<BlockDetailPage />);
    const facts = await screen.findByTestId('block-tier2-facts');
    expect(screen.queryByText('Block evidence')).not.toBeInTheDocument();
    expect(facts).toHaveTextContent('961.16 P');
    expect(facts).toHaveTextContent('961,164,519,177,266,765');
    expect(within(facts).getByRole('img', { name: 'Epoch 15,173: 1,002 of 1,472' })).toBeTruthy();
    const scale = resourceScale(neighbours.data.map((b) => b.size));
    const bar = await within(facts).findByRole('img', {
      name: 'Size, bytes: 3,205, 883 overhead, 2,322 user transactions',
    });
    expect((bar.querySelector('[data-segment="overhead"]') as HTMLElement).style.width).toBe(
      `${Math.round((883 / scale) * 120 * 100) / 100}px`
    );
    expect(facts).toHaveTextContent('0.54% of 597,000 · overhead 883 · transactions 2,322');
    expect(facts).toHaveTextContent('Miner client');
    expect(facts).toHaveTextContent('decoded from the cellbase message');
  });

  it('names the uncle as a sibling with the same parent and its delay; counts committed proposals (BA-18)', async () => {
    render(<BlockDetailPage />);
    const uncles = (await screen.findByRole('heading', { name: 'Uncles' })).closest('section')!;
    expect(uncles).toHaveTextContent('1 · counted toward the NC-Max orphan rate');
    await waitFor(() =>
      expect(uncles).toHaveTextContent(/sibling of #20,827,931 0x.+, same parent #20,827,930/)
    );
    expect(uncles).toHaveTextContent('92 ms after its sibling · 0 proposals');
    const proposals = screen.getByRole('heading', { name: 'Proposals' }).closest('section')!;
    expect(await within(proposals).findByText('2 · both committed')).toBeInTheDocument();
  });

  it('opens the raw header with JSON and Markdown and the decoded epoch (BA-19)', async () => {
    render(<BlockDetailPage />);
    const raw = (await screen.findByText('Raw data')).closest('details')!;
    expect(raw).toHaveAttribute('open');
    expect(within(raw).getByRole('link', { name: 'JSON' }).getAttribute('href')).toMatch(
      /\/blocks\/20827932\.raw$/
    );
    expect(within(raw).getByRole('link', { name: 'Markdown' }).getAttribute('href')).toMatch(
      /\/blocks\/20827932\.md$/
    );
    expect(raw).toHaveTextContent('0x5c003ea003b45 = 15,173 · 1,002 / 1,472');
  });

  it('places the block among its neighbours with times, contents and the uncle forking off #930 (BA-15)', async () => {
    render(<BlockDetailPage />);
    expect(
      await screen.findByText(
        'committed what #20,827,930 proposed, proposed what #20,827,934 committed, and included 1 uncle'
      )
    ).toBeInTheDocument();
    const diagram = await screen.findByRole('region', { name: 'Chain position diagram' });
    await within(diagram).findByLabelText('Block #20,827,934');
    expect(diagram.querySelector(`[data-uncle-fork-from="${N - 2}"]`)).not.toBeNull();
    expect(within(diagram).queryByLabelText('Parent of uncle #20,827,931')).not.toBeInTheDocument();
    expect(diagram.querySelectorAll('[data-tile-time]').length).toBe(5);
    // Visual pass: a 148 px tile has room for the glyph only (the board's tiles carry no count).
    expect(diagram.querySelector('[data-contents-line]')).not.toHaveTextContent('tx');
  });
});

describe('BlockTxs board', () => {
  it('reads the cellbase row as "cellbase", grey, 0 → 1, under 0 … 1,000 / 0 … 2,000,000 scales (BA-20)', async () => {
    render(<BlockDetailPage />);
    fireEvent.click(await screen.findByRole('button', { name: /^Transactions/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in this block' });
    const ths = Array.from(table.querySelectorAll('th'));
    expect(ths.find((th) => th.textContent?.startsWith('Size, bytes'))).toHaveTextContent(
      'Size, bytes01,000'
    );
    expect(ths.find((th) => th.textContent?.startsWith('Cycles'))).toHaveTextContent(
      'Cycles02,000,000'
    );
    const cellbase = table.querySelectorAll('tbody tr')[0] as HTMLElement;
    expect(cellbase).toHaveTextContent('cellbase');
    expect(cellbase).toHaveTextContent('0→1');
    expect((cellbase.querySelector('[data-segment]') as HTMLElement).style.backgroundColor).toBe(
      'rgb(58, 66, 88)'
    );
  });
});

// F2 blocks visual pass + R2b-a wiring (BA-12, BA-13, BA-14, BA-19).
describe('Block board · R2b-a fields', () => {
  it('says which block the cellbase pays (BA-12)', async () => {
    render(<BlockDetailPage />);
    const row = (await screen.findByText('Cellbase')).closest('[data-feed-row]') as HTMLElement;
    expect(row.querySelector('[data-role="headline"]')).toHaveTextContent(
      'Cellbase pays the reward of #20,827,921'
    );
    expect(within(row).getByRole('link', { name: '#20,827,921' }).getAttribute('href')).toMatch(
      new RegExp(`/blocks/${h('ec')}$`)
    );
  });

  it('names the whole secondary issuance and the proposal-fee basis (BA-13)', async () => {
    render(<BlockDetailPage />);
    const section = (await screen.findByRole('heading', { name: 'Reward breakdown' })).closest(
      'section'
    )!;
    expect(section).toHaveTextContent(
      "the miners' share of this block's 416.91483025 secondary issuance; the rest goes to DAO depositors or is burnt"
    );
    await waitFor(() =>
      expect(section).toHaveTextContent(
        'of 0.00031647 in fees of the 2 transactions it proposed, committed in #20,827,934'
      )
    );
    expect(api.getBlockProposalFees).toHaveBeenCalledWith(N);
  });

  it('says the proposal-fee basis is partial while the commit window is still open', async () => {
    vi.mocked(api.getBlockProposalFees).mockResolvedValue({
      number: N,
      hash: block.hash,
      commitWindow: { from: N + 2, to: N + 10 },
      complete: false,
      fees: '30000',
      transactionsCount: 1,
      committedIn: [N + 2],
      transactions: [],
    });
    render(<BlockDetailPage />);
    const section = (await screen.findByRole('heading', { name: 'Reward breakdown' })).closest(
      'section'
    )!;
    await waitFor(() =>
      expect(section).toHaveTextContent(
        'of 0.0003 in fees of the 1 transaction it proposed, committed in #20,827,934 so far; its window runs to #20,827,942'
      )
    );
  });

  it('asks the API for the block in tx order and keeps that order (BA-14)', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [0, 1, 2, 3].map(activity),
      limit: 100,
      hasMore: false,
      nextCursor: null,
    });
    render(<BlockDetailPage />);
    await screen.findByText('Cellbase');
    expect(api.getGlobalActivities).toHaveBeenCalledWith({
      blockNumber: N,
      limit: 100,
      cursor: undefined,
      order: 'asc',
    });
    expect(
      Array.from(document.querySelectorAll('[data-feed-row]')).map((r) =>
        r.getAttribute('data-feed-row')
      )
    ).toEqual([0, 1, 2, 3].map((i) => activity(i).txHash));
  });

  it('shows no pager for a block that fits one page, in either view', async () => {
    render(<BlockDetailPage />);
    await screen.findByText('Cellbase');
    expect(screen.queryByRole('button', { name: 'Previous' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^Transactions/ }));
    await screen.findByRole('table', { name: 'Transactions in this block' });
    expect(screen.queryByRole('button', { name: 'Previous' })).not.toBeInTheDocument();
  });

  it('stacks an uncle row so it reads at 390 px, with short hashes', async () => {
    render(<BlockDetailPage />);
    const uncles = (await screen.findByRole('heading', { name: 'Uncles' })).closest('section')!;
    const row = uncles.querySelector('article')!;
    expect(row).toHaveClass('flex-col');
    await waitFor(() => expect(row).toHaveTextContent(/sibling of #20,827,931 0x9595\.\.\.9595,/));
  });

  it('lists the block bytes in the raw data (BA-19)', async () => {
    render(<BlockDetailPage />);
    const raw = (await screen.findByText('Raw data')).closest('details')!;
    expect(raw).toHaveTextContent('Block bytes3,205 bytes, molecule-serialized');
  });

  it('keeps the "Block" label whole beside the hash', async () => {
    render(<BlockDetailPage />);
    expect(await screen.findByText('Block', { selector: 'span' })).toHaveClass('shrink-0');
  });
});
