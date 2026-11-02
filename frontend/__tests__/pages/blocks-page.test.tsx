import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
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
      minerPool: null,
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
      minerPool: null,
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

  it('asks for twenty blocks a page below md (R2)', async () => {
    stubViewportFromMd(false);
    onTestFinished(() => {
      vi.unstubAllGlobals();
    });
    render(<BlocksPage />);
    await waitFor(() =>
      expect(api.getBlocks).toHaveBeenCalledWith({ cursor: undefined, limit: 20 })
    );
    expect(api.getBlocks).not.toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
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
    minerPool: null,
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

  it('names the miner by its pool when pools.toml does, linked to the address (R-12)', async () => {
    vi.mocked(api.getBlocks).mockResolvedValue({
      ...page,
      data: [{ ...page.data[0], minerPool: 'Example Pool' }, page.data[1]],
    });
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const pool = within(table).getByRole('link', { name: 'Example Pool' });
    expect(pool).toHaveAttribute('href', `/mainnet/address/${miner}`);
    expect(pool).toHaveAttribute('title', miner);
    // T5: the block number is the row's one bright thing; a named pool reads in body ink.
    expect(pool).toHaveClass('text-text');
    expect(pool).not.toHaveClass('text-text-bright');
    // The unnamed miner keeps its address tail; nothing is guessed for it.
    expect(within(table).getAllByRole('link', { name: 'ckb1qzda0c...93hhsj' })).toHaveLength(1);
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

  // Table-rows T5: one bright thing per row, the key cell at body; the hash stays an 11 px dim id.
  // T2: the 18 px line keeps the row at one line (13 px at its own 20.8 px line would add 2.8),
  // and the contents glyph sits in a flex box instead of on the text baseline, where its 18 px
  // inline box stretched the line to 23 px and the row to 48.
  it('leads each row with the block number at 13 px bright, the hash 11 px dim (T5, T2)', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const [first] = Array.from(table.querySelectorAll('tbody tr'));
    const [blockCell, , contentsCell] = Array.from(first.querySelectorAll('td'));
    const number = within(blockCell).getByRole('link', { name: '#20,828,894' });
    expect(number).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]');
    const hash = within(blockCell).getByTitle(page.data[0].hash);
    expect(hash).toHaveClass('text-meta', 'text-text-dim');
    expect(hash).not.toHaveClass('text-body');
    expect(contentsCell.firstElementChild).toHaveClass('flex');
    expect(contentsCell.querySelector('[data-contents-line]')).toBeInTheDocument();
  });

  it('reads the footnote figures in bright ink', async () => {
    render(<BlocksPage />);
    const note = await screen.findByText(/^Size and cycles · Fullest of these 2/);
    expect(within(note).getByText('3,917')).toHaveClass('text-text-bright');
    expect(within(note).getByText('6,831,940')).toHaveClass('text-text-bright');
  });

  // Board L1 (2026-11-01 responsive plan): the phone row is three lines and 640 is a table.
  describe('at phone and 640 widths (board L1)', () => {
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    it('gives each block three lines at 358 px: height and contents · miner and fees · the figures', async () => {
      atContainerWidth(358);
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      expect(cards).toHaveLength(2);
      expect(screen.queryByRole('table')).toBeNull();
      expect(phoneLines(cards[0]).map((l) => [l.key, l.left, l.right])).toEqual([
        ['identity', '#20,828,894', expect.stringMatching(/^5 txs · 4 proposals · .+ ago$/)],
        ['miner', 'miner ckb1qzda0c...93hhsj', 'fees 0.05060000 CKB'],
        [
          'figures',
          // Every desktop column: hash, size with its overhead, cycles, the reward.
          '0x0000...d2de · 3,917 B, 640 B overhead · 6,831,940 cycles · reward 684.32120795 CKB',
          null,
        ],
      ]);
      // A reward not yet paid says where it will be.
      expect(phoneLines(cards[1])[2].left).toMatch(/· reward pays at #20,828,904$/);
    });

    it('draws the six-column table of board L1 in the 608 px band, without the reward', async () => {
      atContainerWidth(608);
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      expect(Array.from(table.querySelectorAll('th')).map((th) => th.textContent)).toEqual([
        'Height',
        'Age',
        'Txs',
        'Miner',
        'Fees, CKB',
        'Size · cycles',
      ]);
      expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(608);
      const [first] = Array.from(table.querySelectorAll('tbody tr'));
      expect(first.querySelector('td')).toHaveTextContent(/^#20,828,8940x0000\.\.\.d2de$/);
      // T5 in the 640 table too: the height leads at 13 px bright.
      expect(within(first as HTMLElement).getByRole('link', { name: '#20,828,894' })).toHaveClass(
        'text-body',
        'text-text-bright',
        'leading-[18px]'
      );
      expect(first).toHaveTextContent('0.05060000');
      expect(first).toHaveTextContent('3,917 B6,831,940');
      expect(screen.queryByTestId('block-row-mobile')).toBeNull();
    });
  });
});
