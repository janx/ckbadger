import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { resourceScale } from '@/components/blocks/block-visual';
import { render } from '../utils/test-utils';
import BlocksPage from '@/app/blocks/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getBlocks: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockBlocksResponse = {
  data: [
    {
      ...blockEvidence,
      number: 8_775_639,
      hash: `0x${'22'.repeat(32)}`,
      parentHash: `0x${'11'.repeat(32)}`,
      timestamp: '2026-02-22T00:00:10Z',
      transactionsCount: 2,
      proposalsCount: 0,
      unclesCount: 0,
      difficulty: '0',
      epoch: '5414/1800',
      epochNumber: 5414,
      epochIndex: 8,
      epochLength: 1800,
      nonce: '0x0',
      transactionsRoot: `0x${'33'.repeat(32)}`,
      minerAddress: null,
      minerMessage: null,
      miningReward: null,
      miningRewardTxHash: null,
      hardforkActivation: null,
      compactTarget: '0x0',
      version: 0,
    },
    {
      ...blockEvidence,
      number: 8_775_638,
      hash: `0x${'44'.repeat(32)}`,
      parentHash: `0x${'22'.repeat(32)}`,
      timestamp: '2026-02-22T00:00:00Z',
      transactionsCount: 1,
      proposalsCount: 0,
      unclesCount: 0,
      difficulty: '0',
      epoch: '5414/1800',
      epochNumber: 5414,
      epochIndex: 7,
      epochLength: 1800,
      nonce: '0x0',
      transactionsRoot: `0x${'55'.repeat(32)}`,
      minerAddress: null,
      minerMessage: null,
      miningReward: null,
      miningRewardTxHash: null,
      hardforkActivation: {
        id: 'mirana-2021',
        name: 'CKB Edition Mirana',
        shortName: 'Mirana',
        activationEpoch: 5414,
        activationDate: '2022-05-10',
      },
      compactTarget: '0x0',
      version: 0,
    },
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

describe('BlocksPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getBlocks).mockResolvedValue(mockBlocksResponse);
  });

  it('renders hardfork badge for activation block in block list', async () => {
    render(<BlocksPage />);

    await waitFor(() => {
      expect(api.getBlocks).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
    });
    await waitFor(() => {
      expect(screen.getAllByText('HF · MIRANA').length).toBeGreaterThan(0);
    });

    expect(screen.getByRole('table', { name: 'Latest blocks' })).toHaveTextContent('0.00012345');
    expect(
      screen.getAllByRole('img', { name: /Cellbase, 1 user transactions/ }).length
    ).toBeGreaterThan(0);
    expect(screen.queryByText('Browse all blocks on the CKB network')).not.toBeInTheDocument();
    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Blocks')).toBeInTheDocument();
    expect(screen.getAllByText('#8,775,638').length).toBeGreaterThan(0);
    expect(screen.getAllByText('HF · MIRANA')[0]).toBeInTheDocument();
  });
});

// F2 blocks (BA-01/02/03/04/05/06/46): the approved Blocks v3 board.
describe('BlocksPage · Blocks v3 board', () => {
  const miner = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4s93hhsj';
  const row = (number: number, over: Record<string, unknown>) => ({
    ...mockBlocksResponse.data[0],
    number,
    hash: `0x${number.toString(16).padStart(64, '0')}`,
    hardforkActivation: null,
    minerAddress: miner,
    miningReward: '68432120795',
    totalFees: '5060000',
    ...over,
  });
  const page = {
    data: [
      row(20_828_894, {
        size: 3917,
        overheadSize: 640,
        userTransactionsSize: 3277,
        cycles: '6831940',
        transactionSizes: [355, 500, 900, 900, 977],
        proposalsCount: 4,
      }),
      row(20_828_893, {
        size: 641,
        overheadSize: 641,
        userTransactionsSize: 0,
        cycles: '0',
        transactionSizes: [355],
        proposalsCount: 0,
        miningReward: null,
        rewardFinalizesAt: 20_828_904,
      }),
    ],
    total: 20_828_902,
    limit: 50,
    hasMore: true,
    nextCursor: '20828892',
  };
  beforeEach(() => {
    vi.mocked(api.getBlocks).mockResolvedValue(page);
  });

  it('puts the unit in the Reward and Fees headers, not in every row (BA-46)', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const headers = Array.from(table.querySelectorAll('th')).map((th) => th.textContent);
    expect(headers).toContain('Reward, CKB');
    expect(headers).toContain('Fees, CKB');
    expect(table.querySelector('tbody')!.textContent).not.toContain('CKB');
    expect(table).toHaveTextContent('684.32120795');
  });

  it('draws Size and Cycles on one left baseline under 0 … scale ticks (BA-01, BA-04)', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const ths = Array.from(table.querySelectorAll('th'));
    const size = ths.find((th) => th.textContent?.startsWith('Size, bytes'))!;
    const cycles = ths.find((th) => th.textContent?.startsWith('Cycles'))!;
    expect(size).toHaveTextContent(
      'Size, bytes0' + resourceScale([3917, 641]).toLocaleString('en-US')
    );
    expect(cycles).toHaveTextContent(
      'Cycles0' + resourceScale([6831940, 0]).toLocaleString('en-US')
    );
    expect(size).toHaveClass('text-left');
    for (const th of [size, cycles])
      expect((th.querySelector('[data-scale-ticks]') as HTMLElement).style.width).toBe('120px');
    const bars = table.querySelectorAll('tbody [data-aligned-bar]');
    expect(bars).toHaveLength(4);
    for (const bar of Array.from(bars)) {
      expect(bar.closest('td')).not.toHaveClass('text-right');
      expect((bar.querySelector('[data-track]') as HTMLElement).style.width).toBe('120px');
    }
  });

  it('shows the legend for block overhead and user transactions (BA-04)', async () => {
    render(<BlocksPage />);
    const legend = await screen.findByLabelText('Legend');
    expect(legend).toHaveTextContent('cellbase');
    expect(legend).toHaveTextContent('transaction, area ∝ bytes');
    expect(legend).toHaveTextContent('proposal');
    expect(legend).toHaveTextContent('block overhead · user transactions');
  });

  it('states the fullest block and the most work against the consensus limits (BA-02)', async () => {
    render(<BlocksPage />);
    expect(await screen.findByText(/^Size and cycles · Fullest of these 2/)).toHaveTextContent(
      /^Size and cycles · Fullest of these 2: 3,917 bytes, 0\.66% of the 597,000-byte limit · most work: 6,831,940 cycles, 0\.20% of the 3,500,000,000-cycle limit$/
    );
  });

  it('draws one gold dot per proposal (BA-03)', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const glyph = within(table).getByRole('img', { name: /4 proposals/ });
    expect(glyph.querySelectorAll('[data-proposal-dot]')).toHaveLength(4);
  });

  it('writes the miner on one line, ckb1qzda0c...93hhsj (BA-05)', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const link = within(table).getAllByRole('link', { name: 'ckb1qzda0c...93hhsj' })[0];
    expect(link.closest('td')).toHaveClass('whitespace-nowrap');
  });

  it('pages as newest first: "2 newest of 20,828,902", Newer and Older (BA-06)', async () => {
    render(<BlocksPage />);
    expect(await screen.findByText('2 newest of 20,828,902')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Newer' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Older →' })).toBeEnabled();
    expect(screen.queryByText(/Page 1 of/)).not.toBeInTheDocument();
  });

  // F2 blocks visual pass (2026-10-30, live 1440 / 390): one-line rows and compact cards.
  it('writes the hash beside the height and keeps the glyph with its count on one line', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const [first] = Array.from(table.querySelectorAll('tbody tr'));
    const [blockCell, , contentsCell] = Array.from(first.querySelectorAll('td'));
    expect(blockCell.firstElementChild!.firstElementChild).toHaveClass('whitespace-nowrap');
    expect(blockCell).toHaveTextContent(/^#20,828,894 0x/);
    expect(contentsCell.querySelector('[data-contents-line]')).toHaveClass('whitespace-nowrap');
  });

  it('reads the footnote figures in bright ink', async () => {
    render(<BlocksPage />);
    const note = await screen.findByText(/^Size and cycles · Fullest of these 2/);
    expect(within(note).getByText('3,917')).toHaveClass('text-text-bright');
    expect(within(note).getByText('6,831,940')).toHaveClass('text-text-bright');
  });

  it('gives each block a compact card at narrow widths: height, age, glyph, miner, reward, bars', async () => {
    render(<BlocksPage />);
    const cards = await screen.findAllByTestId('block-row-mobile');
    expect(cards).toHaveLength(2);
    expect(cards[0]).toHaveTextContent('#20,828,894');
    expect(cards[0].querySelectorAll('[data-aligned-bar]')).toHaveLength(2);
    expect(within(cards[0]).getByRole('img', { name: /4 proposals/ })).toBeInTheDocument();
    expect(cards[1]).toHaveTextContent('in #20,828,904');
  });
});
