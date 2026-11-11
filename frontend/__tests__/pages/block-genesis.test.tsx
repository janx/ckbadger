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
  useParams: () => ({ id: '0' }),
  usePathname: () => '/blocks/0',
  useRouter: () => ({ push: vi.fn() }),
}));

const page = <T,>(data: T[]) => ({ data, limit: 100, hasMore: false, nextCursor: null });

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
    expect(screen.getByTestId('block-summary')).toHaveTextContent(
      /^2 transactions on 2019-11-16 at 05:09:50 UTC\+8\.$/
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
    const facts = await screen.findByRole('complementary', { name: 'Block facts' });
    const fact = (label: string) => within(facts).getByText(label).parentElement as HTMLElement;
    expect(fact('Miner')).toHaveTextContent(/^MinerGenesis$/);
    expect(fact('Miner reward')).toHaveTextContent(/^Miner rewardNone: genesis has no miner$/);
    expect(facts).not.toHaveTextContent('Not finalized yet');
    expect(fact('Fees')).toHaveTextContent(/^Fees0 CKB$/);
    const reward = (await screen.findByRole('heading', { name: 'Reward breakdown' })).closest(
      'section'
    )!;
    expect(reward).toHaveTextContent('Genesis has no mining reward.');
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
    fireEvent.click(screen.getByRole('button', { name: /^Transactions/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in this block' });
    expect(within(table).getAllByRole('row').length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText('No proposals in this block')).toBeInTheDocument();
    expect(screen.getByText('No uncles in this block.')).toBeInTheDocument();
  });
});
