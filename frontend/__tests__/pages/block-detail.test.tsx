import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import BlockDetailPage from '@/app/blocks/[id]/client-page';
import { api } from '@/lib/api';

const BLOCK_ID = '8775638';
const BLOCK_HASH = `0x${'aa'.repeat(32)}`;
const pushMock = vi.hoisted(() => vi.fn());
const paramsIdRef = vi.hoisted(() => ({ current: '8775638' }));

vi.mock('@/lib/api', () => ({
  api: {
    getBlock: vi.fn(),
    getTransactions: vi.fn(),
    getBlockFeeStats: vi.fn(),
    getBlockProposals: vi.fn(),
    getHardforks: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ id: paramsIdRef.current }),
  useRouter: () => ({
    push: pushMock,
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    refresh: vi.fn(),
    prefetch: vi.fn(),
  }),
}));

const mockBlock = {
  number: 8_775_638,
  hash: BLOCK_HASH,
  parentHash: `0x${'bb'.repeat(32)}`,
  timestamp: '2026-02-22T00:00:00Z',
  transactionsCount: 1,
  proposalsCount: 0,
  unclesCount: 0,
  difficulty: '123456',
  epoch: '5414(0/1800)',
  epochNumber: 5414,
  epochIndex: 0,
  epochLength: 1800,
  nonce: '0x1',
  transactionsRoot: `0x${'cc'.repeat(32)}`,
  minerAddress: 'ckb1qyqz683v8g0kz6r2gz7g3q6v3c4x4cv8j7ysf30a0g',
  minerMessage: null,
  miningReward: '10000000000',
  miningRewardTxHash: `0x${'dd'.repeat(32)}`,
  compactTarget: '0x1',
  version: 0,
  hardforkActivation: null,
};

describe('BlockDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    pushMock.mockReset();
    paramsIdRef.current = BLOCK_ID;
    vi.mocked(api.getBlock).mockResolvedValue(mockBlock);
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [
        {
          hash: `0x${'11'.repeat(32)}`,
          blockNumber: mockBlock.number,
          blockHash: mockBlock.hash,
          index: 0,
          inputsCount: 1,
          outputsCount: 1,
          fee: '1000',
          isCellbase: false,
          timestamp: mockBlock.timestamp,
        },
      ],
      limit: 100,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getBlockFeeStats).mockResolvedValue({
      blockNumber: mockBlock.number,
      totalSize: 1200,
      totalCycles: 5_000_000,
      avgFeeRate: 1000,
      minFeeRate: 900,
      maxFeeRate: 1100,
      transactionCount: 1,
    });
    vi.mocked(api.getBlockProposals).mockResolvedValue([]);
    vi.mocked(api.getHardforks).mockResolvedValue({
      network: 'mainnet',
      tipEpoch: 13000,
      tipBlock: 19000000,
      events: [],
    });
  });

  it('shows hardfork activation badge on activation block', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({
      ...mockBlock,
      hardforkActivation: {
        id: 'mirana-2021',
        name: 'CKB Edition Mirana',
        shortName: 'Mirana',
        activationEpoch: 5414,
        activationDate: '2022-05-10',
        resources: [
          {
            label: 'CKB2021',
            url: 'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0037-ckb2021/0037-ckb2021.md',
          },
          {
            label: 'Migration Guide',
            url: 'https://github.com/jordanmack/nervos-ckb2021-hard-fork-migration-guide',
          },
        ],
      },
    });

    render(<BlockDetailPage />);

    await waitFor(() => {
      expect(api.getBlock).toHaveBeenCalledWith(BLOCK_ID);
    });

    await waitFor(() => {
      expect(screen.getByText(/HARDFORK ACTIVATION/i)).toBeInTheDocument();
      expect(screen.getByText(/HARDFORK RESOURCES/i)).toBeInTheDocument();
    });

    const resourceLinks = screen.getAllByTestId('block-hardfork-resource-link');
    expect(resourceLinks).toHaveLength(2);
    expect(screen.getByRole('link', { name: 'CKB2021' })).toHaveAttribute(
      'href',
      'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0037-ckb2021/0037-ckb2021.md'
    );
    expect(screen.getByRole('link', { name: 'Migration Guide' })).toHaveAttribute(
      'href',
      'https://github.com/jordanmack/nervos-ckb2021-hard-fork-migration-guide'
    );
  });

  it('hides hardfork banner on regular blocks and supports keyboard navigation', async () => {
    render(<BlockDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Block #8,775,638')).toBeInTheDocument();
    });

    expect(screen.queryByText(/HARDFORK ACTIVATION/i)).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: 'ArrowLeft' });
    fireEvent.keyDown(window, { key: 'h' });
    fireEvent.keyDown(window, { key: 'ArrowRight' });
    fireEvent.keyDown(window, { key: 'l' });

    expect(pushMock).toHaveBeenNthCalledWith(1, `/blocks/${mockBlock.number - 1}`);
    expect(pushMock).toHaveBeenNthCalledWith(2, `/blocks/${mockBlock.number - 1}`);
    expect(pushMock).toHaveBeenNthCalledWith(3, `/blocks/${mockBlock.number + 1}`);
    expect(pushMock).toHaveBeenNthCalledWith(4, `/blocks/${mockBlock.number + 1}`);
  });

  it('loads hardfork resources from timeline when block payload has no resources', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({
      ...mockBlock,
      hardforkActivation: {
        id: 'meepo-2024',
        name: 'CKB Edition Meepo',
        shortName: 'Meepo',
        activationEpoch: 12293,
        activationDate: '2025-07-01',
      },
    });
    vi.mocked(api.getHardforks).mockResolvedValue({
      network: 'mainnet',
      tipEpoch: 13000,
      tipBlock: 19000000,
      events: [
        {
          id: 'meepo-2024',
          name: 'CKB Edition Meepo',
          shortName: 'Meepo',
          editionYear: 2024,
          activationEpoch: 12293,
          activationDate: '2025-07-01',
          activationBlock: 16595590,
          status: 'activated',
          summary: 'CKB-VM v2 activation.',
          resources: [
            {
              label: 'CKB2023',
              url: 'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0051-ckb2023/0051-ckb2023.md',
            },
          ],
        },
      ],
    });

    render(<BlockDetailPage />);

    await waitFor(() => {
      expect(screen.getByRole('link', { name: 'CKB2023' })).toHaveAttribute(
        'href',
        'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0051-ckb2023/0051-ckb2023.md'
      );
    });
  });

  it('tags proposals carried by an uncle with the uncle height', async () => {
    const uncleHash = `0x${'ee'.repeat(32)}`;
    vi.mocked(api.getBlock).mockResolvedValue({ ...mockBlock, proposalsCount: 2 });
    vi.mocked(api.getBlockProposals).mockResolvedValue([
      {
        proposalIndex: 0,
        proposalId: '0x00112233445566778899',
        committedTxHash: null,
        committedBlockNumber: null,
        uncle: null,
      },
      {
        proposalIndex: 1,
        proposalId: '0xaabbccddeeff00112233',
        committedTxHash: null,
        committedBlockNumber: null,
        uncle: { number: 8_775_637, hash: uncleHash },
      },
    ]);

    render(<BlockDetailPage />);
    fireEvent.click(await screen.findByText('Proposals (2)'));

    const uncleRow = (await screen.findByText('0xaabbccddeeff00112233')).closest(
      '[data-testid="block-proposal-row"]'
    ) as HTMLElement;
    expect(uncleRow).toHaveTextContent('uncle #8,775,637');
    const ownRow = screen
      .getByText('0x00112233445566778899')
      .closest('[data-testid="block-proposal-row"]') as HTMLElement;
    expect(ownRow).not.toHaveTextContent(/uncle/);
  });

  it('lists uncle-carried proposals of a block with no proposals of its own', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({ ...mockBlock, proposalsCount: 0, unclesCount: 1 });
    vi.mocked(api.getBlockProposals).mockResolvedValue([
      {
        proposalIndex: 0,
        proposalId: '0xaabbccddeeff00112233',
        committedTxHash: null,
        committedBlockNumber: null,
        uncle: { number: 8_775_637, hash: `0x${'ee'.repeat(32)}` },
      },
    ]);

    render(<BlockDetailPage />);
    fireEvent.click(await screen.findByText('Proposals (0)'));

    const row = (await screen.findByText('0xaabbccddeeff00112233')).closest(
      '[data-testid="block-proposal-row"]'
    ) as HTMLElement;
    expect(row).toHaveTextContent('uncle #8,775,637');
    expect(screen.queryByText('No proposals in this block')).not.toBeInTheDocument();
  });

  it('says there are no proposals when the block and its uncles proposed nothing', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({ ...mockBlock, proposalsCount: 0, unclesCount: 1 });
    vi.mocked(api.getBlockProposals).mockResolvedValue([]);

    render(<BlockDetailPage />);
    fireEvent.click(await screen.findByText('Proposals (0)'));

    expect(await screen.findByText('No proposals in this block')).toBeInTheDocument();
    expect(screen.queryByText('Loading proposals...')).not.toBeInTheDocument();
  });

  it('shows an error row instead of loading forever when the proposals request fails', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({ ...mockBlock, proposalsCount: 3 });
    vi.mocked(api.getBlockProposals).mockRejectedValue(
      new Error('proposals need ckb_db_path to read the node database')
    );

    render(<BlockDetailPage />);
    fireEvent.click(await screen.findByText('Proposals (3)'));

    const row = await screen.findByTestId('block-proposals-error');
    expect(row).toHaveTextContent('ckb_db_path');
    expect(screen.queryByText('Loading proposals...')).not.toBeInTheDocument();
  });

  it('loads block transactions by resolved block number when route id is hash', async () => {
    paramsIdRef.current = BLOCK_HASH;

    render(<BlockDetailPage />);

    await waitFor(() => {
      expect(api.getBlock).toHaveBeenCalledWith(BLOCK_HASH);
    });

    await waitFor(() => {
      expect(api.getTransactions).toHaveBeenCalledWith({
        blockNumber: mockBlock.number,
        limit: 100,
      });
    });
  });
});
