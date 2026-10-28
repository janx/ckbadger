import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
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
    getGlobalActivities: vi.fn(),
  },
  isNotFoundApiError: (error: unknown) =>
    typeof error === 'object' && error !== null && 'status' in error && error.status === 404,
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ id: paramsIdRef.current }),
  usePathname: () => `/blocks/${paramsIdRef.current}`,
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
  ...blockEvidence,
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
  minerMessage: '0x636b6220302e3231302e30',
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
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      limit: 100,
      hasMore: false,
      nextCursor: null,
    });
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
          value: '100000000',
          txSize: 226,
          cycles: 5000000,
          cyclesStatus: 'complete',
          outputCapacity: '100000000',
          proposalOrigin: { blockNumber: mockBlock.number - 3, distance: 3, uncle: null },
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
      cyclesStatus: 'complete',
      avgFeeRate: '1000.00',
      minFeeRate: '900.00',
      maxFeeRate: '1100.00',
      transactionCount: 1,
    });
    vi.mocked(api.getBlockProposals).mockResolvedValue([]);
    vi.mocked(api.getHardforks).mockResolvedValue({
      network: 'mainnet',
      consensusFeatures: [],
      scriptDeployments: [],
      tipEpoch: 13000,
      tipBlock: 19000000,
      events: [],
    });
  });

  it('routes only a true block 404 to typed identifier evidence', async () => {
    paramsIdRef.current = BLOCK_HASH;
    vi.mocked(api.getBlock).mockRejectedValue(Object.assign(new Error('absent'), { status: 404 }));
    render(<BlockDetailPage />);
    expect((await screen.findByText('checked')).nextElementSibling).toHaveTextContent(
      `/blocks/${BLOCK_HASH}`
    );
    expect(
      screen.getByText('This explorer could not open the block at this address.')
    ).toBeInTheDocument();
  });

  it('keeps block invariant failures as errors rather than absence evidence', async () => {
    vi.mocked(api.getBlock).mockRejectedValue(
      Object.assign(new Error('missing canonical header'), { status: 500 })
    );
    render(<BlockDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('missing canonical header');
    expect(
      screen.queryByText('This explorer could not open the block at this address.')
    ).not.toBeInTheDocument();
  });

  it('shows exact decoded DAO fields alongside the raw 32-byte header', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({
      ...mockBlock,
      rawHeader: {
        ...mockBlock.rawHeader,
        dao: '0x28341e52bbf1665b9cb7fbb823dd2a0077f0143eecc3870a00e95a770f4f3407',
      },
    });
    render(<BlockDetailPage />);
    await screen.findByText('Position in the chain');
    fireEvent.click(screen.getByText('Raw data'));
    const decoded = screen.getByLabelText('Decoded DAO header');
    expect(decoded).toHaveTextContent('65,862,172,919.14818600');
    expect(decoded).toHaveTextContent('1.2065094518945692');
    expect(decoded).toHaveTextContent('7,587,904,816.56475767');
    expect(decoded).toHaveTextContent('5,191,267,849.00000000');
    expect(decoded).toHaveTextContent('U includes genesis virtual occupied capacity');
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
      consensusFeatures: [],
      scriptDeployments: [],
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
    await screen.findByText('Proposals (2)');

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
    await screen.findByText('Proposals (0)');

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
    await screen.findByText('Proposals (0)');

    expect(await screen.findByText('No proposals in this block')).toBeInTheDocument();
    expect(screen.queryByText('Loading proposals...')).not.toBeInTheDocument();
  });

  it('shows an error row instead of loading forever when the proposals request fails', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({ ...mockBlock, proposalsCount: 3 });
    vi.mocked(api.getBlockProposals).mockRejectedValue(
      new Error('proposals need ckb_db_path to read the node database')
    );

    render(<BlockDetailPage />);
    await screen.findByText('Proposals (3)');

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

  it('shows block activities first and puts exact developer evidence below the shared tier boundary', async () => {
    render(<BlockDetailPage />);
    expect(await screen.findByRole('button', { name: 'Activities' })).toHaveAttribute(
      'aria-pressed',
      'true'
    );
    expect(api.getGlobalActivities).toHaveBeenCalledWith({
      blockNumber: 8775638,
      limit: 100,
      cursor: undefined,
    });
    expect(screen.getByText(/0.20% of 597,000/)).toBeInTheDocument();
    expect(screen.getByText('ckb 0.210.0')).toBeInTheDocument();
    expect(screen.getByText(/10 s after its parent/)).toBeInTheDocument();
    expect(screen.getByText('Block evidence').closest('[data-tier="2"]')).toHaveClass('mt-[72px]');
    expect(
      screen.getByRole('img', { name: 'Size, bytes: 1,200, 974 overhead, 226 user transactions' })
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Transactions (1)' }));
    expect(
      await screen.findByRole('table', { name: 'Transactions in this block' })
    ).toBeInTheDocument();
  });
});
