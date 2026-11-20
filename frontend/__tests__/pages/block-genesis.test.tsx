// The genesis block's page (/blocks/0) on what the API serves for it (fixtures/genesis-block.ts):
// it went straight to the error page, "epoch index 0 is outside its length 0".
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import {
  genesisActivities,
  genesisBlock,
  genesisNeighbours,
  genesisNeighboursPage,
  genesisProposalFees,
  genesisTransactions,
} from '@/__tests__/msw/fixtures/genesis-block';
import BlockDetailPage from '@/app/blocks/[id]/client-page';
import { api } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';

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
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ id: '0' }),
  usePathname: () => '/blocks/0',
  useRouter: () => ({ push: vi.fn() }),
}));

const page = <T,>(data: T[]) => pagedRead({ data, limit: 100, hasMore: false, nextCursor: null });

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getBlock).mockResolvedValue(genesisBlock);
  vi.mocked(api.getBlocks).mockResolvedValue({
    ...genesisNeighboursPage,
    data: genesisNeighbours,
  });
  vi.mocked(api.getTransactions).mockResolvedValue(page(genesisTransactions));
  vi.mocked(api.getGlobalActivities).mockResolvedValue(page(genesisActivities));
  vi.mocked(api.getBlockProposals).mockResolvedValue([]);
  vi.mocked(api.getBlockProposalFees).mockResolvedValue(genesisProposalFees);
  vi.mocked(api.getBlockUncle).mockRejectedValue(new Error('genesis has no uncles'));
  vi.mocked(api.getHardforks).mockRejectedValue(new Error('genesis activates no hardfork'));
});

describe('the genesis block page', () => {
  it('renders block 0 with no previous link, its summary and no reward sentence', async () => {
    render(<BlockDetailPage />);
    const title = await screen.findByRole('heading', { level: 1 });
    expect(title).toHaveTextContent(/^#0$/);
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryAllByRole('link', { name: /^Previous block,/, hidden: true })).toEqual([]);
    expect(screen.getByRole('link', { name: 'Next block, 1', hidden: true })).toBeInTheDocument();
    // Re-pinned on purpose (lane P1, the plan: the genesis lede says what was issued, from fields
    // the API already sends; review chain-a block-genesis): the header's total issuance, its DAO
    // field's C, 33,600,001,452.384882 CKB, in the summary form, exact in its title; the time is
    // the status strip's. It read "2 transactions on 2019-11-16 at 05:09:50 UTC+8."
    const summary = screen.getByTestId('block-summary');
    expect(summary).toHaveTextContent(
      /^2 transactions, which issued the chain’s first 33,600,001,452 CKB\.$/
    );
    expect(summary.querySelector('[data-ckb-amount]')).toHaveAttribute(
      'title',
      '33,600,001,452.384882 CKB'
    );
  });

  // CKB builds the genesis header with the zero epoch (ckb-chain-spec build_genesis:
  // EpochNumberWithFraction::new_unchecked(0, 0, 0)); block 1 is the first header to state epoch
  // 0's length (1/1743 on mainnet). The page says what the header says and why, and draws no bar
  // for a fraction the header does not state.
  it('states the zero epoch the genesis header carries, says why, and draws no epoch bar', async () => {
    render(<BlockDetailPage />);
    const facts = await screen.findByTestId('block-tier2-facts');
    const epoch = within(facts).getByText('Epoch').parentElement as HTMLElement;
    expect(within(epoch).getByRole('link', { name: '0' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/0'
    );
    expect(epoch).toHaveTextContent('0 / 0');
    expect(within(epoch).queryByRole('img')).toBeNull();
    const why = within(epoch).getByTestId('block-epoch-genesis');
    expect(why).toHaveTextContent(
      /^the genesis header’s epoch field is 0x0, so it states no epoch length; #1 is the first header to state epoch 0’s$/
    );
    expect(within(why).getByRole('link', { name: '#1' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/1'
    );
  });

  it('decodes the raw epoch field 0x0 as 0 · 0 / 0', async () => {
    render(<BlockDetailPage />);
    await screen.findByTestId('block-tier2-facts');
    fireEvent.click(screen.getByText('Raw data'));
    const row = screen.getByText('epoch').parentElement as HTMLElement;
    expect(row).toHaveTextContent(/^epoch0x0 = 0 · 0 \/ 0$/);
  });

  // Genesis is mined by no one and earns no reward: the facts say so instead of "not finalized
  // yet", which is true only of a later block whose reward is still to be paid.
  it('says genesis has no miner and no mining reward', async () => {
    render(<BlockDetailPage />);
    // re-pinned on purpose (lane D2): the miner and the reward are the status strip's facts, their
    // labels lowercase ("miner", "reward"; the aside's were "Miner", "Miner reward"); the fees
    // stand in the evidence tier's facts (they were the aside's last)
    const facts = await screen.findByRole('group', { name: 'Block status' });
    const fact = (label: string) => within(facts).getByText(label).parentElement as HTMLElement;
    expect(fact('miner')).toHaveTextContent(/^minerGenesis$/);
    expect(fact('reward')).toHaveTextContent(/^rewardNone: genesis has no miner$/);
    expect(facts).not.toHaveTextContent('Not finalized yet');
    const tier2 = screen.getByTestId('block-tier2-facts');
    expect(within(tier2).getByText('Fees').parentElement).toHaveTextContent(/^Fees0 CKB$/);
    const reward = (await screen.findByRole('heading', { name: 'Reward breakdown' })).closest(
      'section'
    )!;
    expect(reward).toHaveTextContent('Genesis has no mining reward.');
  });

  // The API serves no cellbase message for block 0 alone (it reads no miner witness for genesis;
  // CKB builds genesis's cellbase witness with the bootstrap lock and an empty message). The page
  // said "Miner client: Genesis" captioned "decoded from the cellbase message", and "Cellbase
  // message bytes: Genesis": nothing was decoded and there are no message bytes.
  it('says genesis has no miner client and no miner message, decoding nothing', async () => {
    render(<BlockDetailPage />);
    const facts = await screen.findByTestId('block-tier2-facts');
    const client = within(facts).getByText('Miner client').parentElement as HTMLElement;
    expect(client).toHaveTextContent(/^Miner clientNone: genesis has no miner$/);
    expect(facts).not.toHaveTextContent('decoded from the cellbase message');
    fireEvent.click(screen.getByText('Raw data'));
    const bytes = screen.getByText('Cellbase message bytes').parentElement as HTMLElement;
    expect(bytes).toHaveTextContent(/^Cellbase message bytesNone: genesis has no miner message$/);
  });

  // 1,236,271 bytes against the 597,000-byte block limit: consensus never checks genesis against
  // it. The share is stated exactly and the bar is on the neighbours' scale, which holds it.
  it('states the genesis size beyond the block limit exactly', async () => {
    render(<BlockDetailPage />);
    expect(
      await screen.findByText(/^207\.08% of 597,000 · overhead 1,235,678/)
    ).toBeInTheDocument();
    expect(
      await screen.findByRole('img', {
        name: 'Size, bytes: 1,236,271, 1,235,678 overhead, 593 user transactions',
      })
    ).toBeInTheDocument();
  });

  it('lists the cellbase first, paying no block’s reward, and both transactions', async () => {
    render(<BlockDetailPage />);
    const cellbase = await screen.findByText('Cellbase');
    expect(cellbase.closest('[data-feed-row]')).toHaveTextContent(/Cellbase/);
    expect(screen.queryByText(/pays the reward of/)).toBeNull();
    const rows = Array.from(document.querySelectorAll('[data-feed-row]'));
    expect(rows.map((row) => row.getAttribute('data-feed-row'))).toEqual(
      genesisTransactions.map((tx) => tx.hash)
    );
    // re-pinned (lane c2-chain): the views are the library's tab row (were pressed buttons)
    fireEvent.click(screen.getByRole('tab', { name: /^transactions/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in this block' });
    expect(within(table).getAllByRole('row').length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText('No proposals in this block.')).toBeInTheDocument();
    expect(screen.getByText('No uncles in this block.')).toBeInTheDocument();
  });

  // The genesis cellbase has 671 outputs and 667 parties (no later cellbase has more than one
  // output); since lane feed part 4 its row draws the five largest of them. The page refused it
  // ("cellbase 0xe2fb…: it draws 5 of its 667 parties") and went to the error page: it read the
  // one-output rule of a cellbase that pays a block's reward into the one that pays none.
  it('draws five of the genesis cellbase’s 667 parties and leaves the other 662 to its page', async () => {
    render(<BlockDetailPage />);
    const cellbase = genesisActivities[0];
    await screen.findByText('Cellbase');
    expect(screen.queryByRole('alert')).toBeNull();
    const row = document.querySelector(`[data-feed-row="${cellbase.txHash}"]`) as HTMLElement;
    // its headline names no one: it pays no block's reward
    const headline = row.querySelector('[data-role="headline"]') as HTMLElement;
    expect(headline).toHaveTextContent(/^Cellbase$/);
    // the five it draws, the burn cell's zero lock among them, then the way to the other 662
    expect(row.querySelectorAll('[data-role="participant"]')).toHaveLength(5);
    expect(within(row).getByTitle(cellbase.participants[0].lockHash!)).toBeInTheDocument();
    const rest = within(row).getByRole('link', {
      name: /^662\smore\sparticipants on the transaction page$/,
    });
    expect(rest).toHaveAttribute('href', expect.stringContaining(`/tx/${cellbase.txHash}`));
  });
});
