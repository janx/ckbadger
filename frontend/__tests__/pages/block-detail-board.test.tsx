// F2 blocks: the Block / BlockTxs boards (BA-12 … BA-20, BA-40) on block #20,827,932's shape.
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { expectRestingLink } from '@/__tests__/utils/resting-link';
import { expectBefore, expectOnePageSeal } from '@/__tests__/utils/seals';
import BlockDetailPage from '@/app/blocks/[id]/client-page';
import { resourceScale } from '@/components/blocks/block-visual';
import { ErrorBoundary } from '@/components/error-boundary';
import { PAGE_SEAL_HEIGHTS } from '@/components/ui/page-seal';
import { api, type Block, type GlobalActivity, type Transaction } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';

/** A block's seal: its hash and its transaction count, as the API serves them. */
const sealOf = (b: { hash: string; transactionsCount: number }): SealId => ({
  kind: 'block',
  hash: b.hash,
  txs: b.transactionsCount,
});

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
  minerPool: null,
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
    expect(summary).toHaveClass('text-body');
    expect(summary).toHaveTextContent(
      /^4 transactions, mined by .+ on 2026-10-25 at 08:55:37 UTC\+8\. The miner earns 684\.32120795 CKB for this block, paid 11 blocks later\.$/
    );
  });

  // Seals by kind (C7): the block's 80 px stone leaves the identity column 262 px at 390 and 232 px
  // at 360, and the one-line title (‹ 36 px, the 30 px height, › 36 px) needs 283 px, so the page
  // scrolled sideways. Below sm the height takes the first line and the step links follow as a pair
  // on the second; from sm up the links flank the height on one line, as before. One DOM (the same
  // two links in the same order), rearranged by wrapping and order alone; jsdom has no layout, so
  // the classes that make the arrangement are what is pinned.
  it('stands the height on its own line below sm, the step links paired under it, in one DOM', async () => {
    render(<BlockDetailPage />);
    const title = await screen.findByRole('heading', { level: 1 });
    const steps = screen.getAllByRole('link', { name: /^(Previous|Next) block,/, hidden: true });
    expect(steps.map((link) => link.getAttribute('aria-label'))).toEqual([
      'Previous block, 20,827,931',
      'Next block, 20,827,933',
    ]);
    const [previous, next] = steps;
    const height = within(title).getByText('#20,827,932');
    const row = height.parentElement!;
    expect(row.parentElement).toBe(title);
    expect(Array.from(row.children)).toEqual([previous, height, next]);
    expect(row).toHaveClass('flex', 'flex-wrap', 'gap-x-2.5', 'sm:flex-nowrap');
    expect(height).toHaveClass('max-sm:order-first', 'max-sm:basis-full');
    // From sm up nothing moves: the height has no rule there, and the links keep their 36 px
    // targets and their place in the DOM order (no order of their own at any width).
    expect(height.className).not.toMatch(/(^|\s)(sm|md|lg|xl):/);
    for (const link of [previous, next]) {
      expect(link).toHaveClass('h-9', 'w-9', 'shrink-0');
      expect(link.className).not.toMatch(/(^|[\s:])order-/);
    }
  });

  it('puts the genesis block’s lone next link on the second line below sm, under its height', async () => {
    // Genesis as the API serves it: no parent, no miner, no reward, epoch 0's first block.
    const genesis = {
      ...block,
      number: 0,
      hash: h('92'),
      parentHash: h('00'),
      timestamp: '2019-11-15T21:09:50.812Z',
      parentTimestamp: null,
      transactionsCount: 2,
      transactionSizes: [355, 516],
      proposalsCount: 0,
      unclesCount: 0,
      uncles: [],
      epoch: '0/1743',
      epochNumber: 0,
      epochIndex: 0,
      epochLength: 1743,
      minerAddress: null,
      minerMessage: null,
      minerMessageText: null,
      miningReward: null,
      miningRewardTxHash: null,
      rewardBreakdown: null,
      cellbasePays: null,
      rewardPaidAtBlock: null,
      rewardFinalizesAt: null,
    };
    vi.mocked(api.getBlock).mockResolvedValue(genesis);
    vi.mocked(api.getBlocks).mockResolvedValue({
      data: [
        { ...genesis, number: 2, hash: h('02'), timestamp: '2019-11-16T06:12:03.871Z' },
        { ...genesis, number: 1, hash: h('01'), timestamp: '2019-11-16T06:11:53.574Z' },
        genesis,
      ],
      total: 3,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    render(<BlockDetailPage />);
    const title = await screen.findByRole('heading', { level: 1 });
    expect(screen.queryAllByRole('link', { name: /^Previous block,/, hidden: true })).toEqual([]);
    const next = screen.getByRole('link', { name: 'Next block, 1', hidden: true });
    const height = within(title).getByText('#0');
    const row = height.parentElement!;
    expect(row.parentElement).toBe(title);
    expect(Array.from(row.children)).toEqual([height, next]);
    expect(row).toHaveClass('flex', 'flex-wrap', 'sm:flex-nowrap');
    expect(height).toHaveClass('max-sm:order-first', 'max-sm:basis-full');
  });

  // Seals by kind (phase C1): the block's stone (the 104 px page seal, 80 below md, −4°), its hash
  // and its transaction count, pressed beside its height and hash, as a collector's stamp; never
  // inside the content.
  it('presses the block’s seal beside its height and hash, outside the content', async () => {
    const { container } = render(<BlockDetailPage />);
    const title = await screen.findByRole('heading', { level: 1 });
    const seal = expectOnePageSeal(container, sealOf(block));
    expect(PAGE_SEAL_HEIGHTS.block).toEqual({ wide: 104, narrow: 80 });
    expect(seal.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Block seal ${block.hash.slice(0, 10)}, 4 transactions`
    );
    const head = seal.closest('[data-sealed-head]')!;
    expect(head).toContainElement(title);
    expect(head).toHaveTextContent(`Block${block.hash}`);
    expectBefore(seal, title);
    expect(container.querySelector('[data-tier="1"]')).not.toContainElement(seal);
    expect(head).not.toContainElement(screen.getByRole('group', { name: 'Block contents' }));
  });

  // Phase 5 (Gate 3 note): the summary stands beside the seal with the height and hash, the
  // head's last line, instead of opening the content under an empty gap.
  it('reads the summary in the sealed head, after the hash, before the tab row', async () => {
    const { container } = render(<BlockDetailPage />);
    const summary = await screen.findByTestId('block-summary');
    const seal = expectOnePageSeal(container, sealOf(block));
    const head = seal.closest('[data-sealed-head]')!;
    expect(head).toContainElement(summary);
    expect(container.querySelector('[data-tier="1"]')).not.toContainElement(summary);
    const column = seal.nextElementSibling!;
    expect(column.lastElementChild).toContainElement(summary);
    expect(summary.parentElement).toHaveAttribute('data-detail-lede');
    expectBefore(screen.getByText(block.hash), summary);
    expectBefore(summary, screen.getByRole('group', { name: 'Block contents' }));
    // The header keeps the 32 px to the content; the summary carries no margin of its own.
    expect(summary).not.toHaveClass('mb-8');
  });

  // The stone's count is the block's `transactionsCount` as the API serves it: a block of 1, of 7
  // and of 248 transactions (past nine, the page seal writes three digits in rods).
  it.each([1, 7, 248])('writes the block’s %i transaction(s) on its seal', async (count) => {
    const counted = {
      ...block,
      transactionsCount: count,
      transactionSizes: Array<number>(count).fill(355),
    };
    vi.mocked(api.getBlock).mockResolvedValue(counted);
    const { container } = render(<BlockDetailPage />);
    await screen.findByRole('heading', { level: 1 });
    const seal = expectOnePageSeal(container, sealOf(counted));
    expect(seal.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Block seal ${block.hash.slice(0, 10)}, ${count} transaction${count === 1 ? '' : 's'}`
    );
  });

  // No default count: a block served without `transactionsCount` is refused, never pressed with a
  // guessed count. On this page the summary's count ("4 transactions", formatCount) is read before
  // the head draws its seal, so its refusal is the one shown; no stone is drawn.
  it('refuses a block served without its transaction count, and draws no seal', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      vi.mocked(api.getBlock).mockResolvedValue({
        ...block,
        transactionsCount: undefined,
      } as unknown as Block);
      const { container } = render(
        <ErrorBoundary fallbackRender={(error) => <p data-testid="refused">{error.message}</p>}>
          <BlockDetailPage />
        </ErrorBoundary>
      );
      expect(await screen.findByTestId('refused')).toHaveTextContent(
        /^Invalid count undefined for transaction$/
      );
      expect(container.querySelector('[data-seal]')).toBeNull();
    } finally {
      spy.mockRestore();
    }
  });

  it('says which pool mined the block when pools.toml names it (R-12)', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({ ...block, minerPool: 'Example Pool' });
    render(<BlockDetailPage />);
    const summary = await screen.findByTestId('block-summary');
    expect(summary).toHaveTextContent(/^4 transactions, mined by Example Pool on 2026-10-25/);
    expect(within(summary).getByRole('link', { name: 'Example Pool' })).toHaveAttribute(
      'href',
      `/mainnet/address/${block.minerAddress}`
    );
  });

  it('counts both views in quiet 12 px beside 17 px tab names, and says where the txs came from (BA-40, BA-16)', async () => {
    render(<BlockDetailPage />);
    const views = await screen.findByRole('group', { name: 'Block contents' });
    const [activities, transactions] = within(views).getAllByRole('button');
    for (const [tab, name] of [
      [activities, 'Activities'],
      [transactions, 'Transactions'],
    ] as const) {
      expect(tab).toHaveClass('text-section');
      expect(tab.firstChild).toHaveTextContent(name);
      expect(within(tab).getByText('4')).toHaveClass('text-small');
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

  // Visual finish V5: a feed row's tx link rests as ink with the faint aqua underline (was
  // `text-interactive-dim`); the global a:hover lights it jade.
  it("rests each activity row's tx link as ink, not aqua-dim (V5)", async () => {
    render(<BlockDetailPage />);
    await screen.findByText('Cellbase');
    const rows = Array.from(document.querySelectorAll('[data-feed-row]'));
    expect(rows).toHaveLength(4);
    for (const row of rows) {
      const hash = row.getAttribute('data-feed-row')!;
      const link = row.querySelector(`[data-role="evidence"] a[href$="/tx/${hash}"]`);
      expect(link, hash).not.toBeNull();
      expectRestingLink(link as HTMLElement);
    }
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
    // Lane L: the uncle and its sibling share the height; the uncle's links to the uncle page,
    // the sibling's to the canonical block (as its hash beside it does).
    expect(
      within(uncles)
        .getAllByRole('link', { name: '#20,827,931' })
        .map((a) => a.getAttribute('href'))
    ).toEqual(['/mainnet/blocks/20827932/uncles/0', `/mainnet/blocks/0x${'95'.repeat(32)}`]);
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
    // Lane L: the sentence's heights are links to their blocks, so it is one paragraph of text
    // and links (was one text node).
    const sentence = await screen.findByTestId('block-position-sentence');
    expect(sentence).toHaveTextContent(
      /^committed what #20,827,930 proposed, proposed what #20,827,934 committed, and included 1 uncle$/
    );
    expect(within(sentence).getByRole('link', { name: '#20,827,930' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20827930'
    );
    expect(within(sentence).getByRole('link', { name: '#20,827,934' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20827934'
    );
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
      'var(--c-slate-ckb)'
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
    // Lane L: the committing block is a link.
    expect(within(section).getByRole('link', { name: '#20,827,934' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20827934'
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
    expect(within(section).getByRole('link', { name: '#20,827,942' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20827942'
    );
  });

  it('links both ends of a closed commit window that committed nothing it proposed (lane L)', async () => {
    vi.mocked(api.getBlockProposalFees).mockResolvedValue({
      number: N,
      hash: block.hash,
      commitWindow: { from: N + 2, to: N + 10 },
      complete: true,
      fees: '0',
      transactionsCount: 0,
      committedIn: [],
      transactions: [],
    });
    render(<BlockDetailPage />);
    const section = (await screen.findByRole('heading', { name: 'Reward breakdown' })).closest(
      'section'
    )!;
    await waitFor(() =>
      expect(section).toHaveTextContent(
        'no transaction it proposed first was committed in its window, #20,827,934 to #20,827,942'
      )
    );
    for (const name of ['#20,827,934', '#20,827,942'])
      expect(within(section).getByRole('link', { name })).toHaveAttribute(
        'href',
        `/mainnet/blocks/${name.slice(1).replace(/,/g, '')}`
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
