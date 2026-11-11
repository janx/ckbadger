import { CHARTS } from '@/components/charts/chart-catalogue';
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { genesisNeighbours, genesisNeighboursPage } from '@/__tests__/msw/fixtures/genesis-block';
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { screen, waitFor, within } from '@testing-library/react';
import { resourceScale } from '@/components/blocks/block-visual';
import { render } from '../utils/test-utils';
import BlocksPage from '@/app/blocks/page';
import { api, type Block } from '@/lib/api';
import { ErrorBoundary } from '@/components/error-boundary';
import { ROW_SEAL } from '@/components/ui/row-seal';
import type { SealId } from '@/lib/seal/stones';
import { expectBefore, expectOneRowSeal } from '@/__tests__/utils/seals';

// Time and record the seals' drawing without changing what it draws (a plain wrapper, not a mock
// function: the phone describe's restoreAllMocks would reset one).
const drawn = vi.hoisted(() => ({ ms: 0, ids: [] as unknown[] }));
vi.mock('@/lib/seal/stones', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/seal/stones')>();
  return {
    ...actual,
    drawSeal: (...args: Parameters<typeof actual.drawSeal>) => {
      const started = performance.now();
      try {
        return actual.drawSeal(...args);
      } finally {
        drawn.ms += performance.now() - started;
        drawn.ids.push(args[0]);
      }
    },
  };
});

/** The width of `n` characters of the monospace face (0.6 em each) at `size` px. */
const chars = (n: number, size: number) => n * 0.6 * size;

/** A block's seal: its hash and its transaction count, as the API serves them. */
const sealOf = (block: { hash: string; transactionsCount: number }): SealId => ({
  kind: 'block',
  hash: block.hash,
  txs: block.transactionsCount,
});

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

  // Lane L (charts ↔ topics): the blocks list links the charts drawn from blocks.
  it('links every chart whose numbers come from blocks (lane L)', async () => {
    render(<BlocksPage />);
    const charts = await screen.findByRole('navigation', { name: 'Charts of these numbers' });
    const hrefs = within(charts)
      .getAllByRole('link')
      .map((a) => a.getAttribute('href'));
    expect(hrefs).toEqual(
      CHARTS.filter((c) => c.evidenceHref === '/blocks').map((c) => `/mainnet/charts/${c.slug}`)
    );
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
    expect(screen.getByText('Blocks')).toBeInTheDocument();
    expect(screen.getAllByText('#8,775,638').length).toBeGreaterThan(0);
    expect(screen.getAllByText('HF · MIRANA')[0]).toBeInTheDocument();
  });
});

// The chain's first page as the API lists it (GET /blocks?cursor=3&limit=50: blocks 2, 1, 0).
// Genesis has no miner and no reward, the zero epoch, and 1,236,271 bytes against the 597,000-byte
// limit; every form of the list writes it as what it is.
describe('BlocksPage · the genesis row', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getBlocks).mockResolvedValue({
      ...genesisNeighboursPage,
      data: genesisNeighbours,
    });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('writes genesis as mined by no one and paid no reward, in the wide table', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const [, , genesis] = Array.from(table.querySelectorAll('tbody tr')) as HTMLElement[];
    expect(within(genesis).getByRole('link', { name: '#0' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/0'
    );
    expect(within(genesis).getAllByText('Genesis')).toHaveLength(2); // the miner and the reward
    expect(within(genesis).getByRole('img', { name: /^Size, bytes: 1,236,271/ })).toBeVisible();
    // consensus never checks genesis against the block limit: its share is stated exactly
    expect(screen.getByText(/^Size and cycles · Fullest of these 3/)).toHaveTextContent(
      /^Size and cycles · Fullest of these 3: 1,236,271 bytes, 207\.08% of the 597,000-byte limit/
    );
  });

  it('writes genesis as mined by no one in the 608 px table and on a phone', async () => {
    atContainerWidth(608);
    const { unmount } = render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const [, , genesis] = Array.from(table.querySelectorAll('tbody tr')) as HTMLElement[];
    expect(genesis).toHaveTextContent('Genesis');
    unmount();
    atContainerWidth(358);
    render(<BlocksPage />);
    const cards = await screen.findAllByTestId('block-row-mobile');
    const lines = phoneLines(cards[2]).map((l) => [l.key, l.left, l.right]);
    expect(lines[0]).toEqual(['identity', '#0', expect.stringMatching(/^2 txs · .+ ago$/)]);
    expect(lines[1]).toEqual(['miner', 'miner Genesis', 'fees 0 CKB']);
    expect(lines[2][1]).toMatch(/· 1,236,271 B, 1,235,678 B overhead · 0 cycles · Genesis$/);
  });
});

// F2 blocks (BA-01/02/03/04/05/06/46): the approved Blocks v3 board.
describe('BlocksPage · Blocks v3 board', () => {
  const miner = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4s93hhsj';
  // The API counts a block's transactions as it lists their sizes (the cellbase first), so a row's
  // `transactionsCount` is its `transactionSizes`' length: the count its seal carries.
  const row = (number: number, over: { transactionSizes: number[] } & Record<string, unknown>) => ({
    ...mockBlocksResponse.data[0],
    number,
    hash: `0x${number.toString(16).padStart(64, '0')}`,
    hardforkActivation: null,
    minerAddress: miner,
    minerPool: null,
    miningReward: '68432120795',
    totalFees: '5060000',
    ...over,
    transactionsCount: over.transactionSizes.length,
  });
  // Blocks of 1, 7 and 12 transactions: a row stone writes 1 and 7 in rods, past nine a full block.
  const counted = [1, 7, 12].map((n, i) =>
    row(20_828_894 - i, {
      size: 641 + 355 * (n - 1),
      overheadSize: 641,
      userTransactionsSize: 355 * (n - 1),
      cycles: String(n * 1000),
      transactionSizes: [355, ...Array<number>(n - 1).fill(355)],
      proposalsCount: 0,
    })
  );
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
    // Rubbing × Song print, phase 3: the row seal leads the cell, the height and hash beside it.
    const [seal, identity] = Array.from(blockCell.firstElementChild!.children);
    expect(seal).toHaveAttribute('data-seal', 'block');
    expect(seal).toHaveAttribute('data-seal-form', 'row');
    expect(identity.firstElementChild).toHaveClass('whitespace-nowrap');
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

  // Seals by kind (phase C1): each row leads with its block's stone (26 px, its hash and its
  // transaction count), before the height; the height stays the row's link, the seal is an image.
  it('leads every row with its block’s row seal, before the height link', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const rows = Array.from(table.querySelectorAll('tbody tr'));
    expect(rows).toHaveLength(page.data.length);
    rows.forEach((tr, i) => {
      const cell = tr.querySelector('td')!;
      const seal = expectOneRowSeal(tr, sealOf(page.data[i]));
      expect(cell).toContainElement(seal);
      // one 18 px line keeps the row's height: the 26 px stone overhangs it by 4 px above and below
      expect(ROW_SEAL.block.overhang).toBe(4);
      expect(seal).toHaveClass('-my-[4px]');
      const height = within(cell).getByRole('link', {
        name: `#${page.data[i].number.toLocaleString('en-US')}`,
      });
      expect(height).toHaveAttribute('href', `/mainnet/blocks/${page.data[i].number}`);
      expectBefore(seal, height);
    });
  });

  // The seal's footprint comes out of other columns' slack, so each table keeps the minimum width
  // it switches form at: the full table 1,230 px (Reward gave the old hash seal 22; Fees gives the
  // block stone's 7 more, 34 against 27), the 608 px table (Size · cycles gave 19 and gives 5,
  // Miner 2; Height keeps its 2 px of room so its number never breaks).
  it('budgets the seal in the Block column and keeps the table at 1,230 px', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    expect(Number.parseFloat(table.style.minWidth)).toBe(1230);
    const cols = Array.from(table.querySelectorAll('col')).map(
      (c) => (c as HTMLElement).style.width
    );
    expect(cols).toEqual(['229px', '70px', '170px', '160px', '118px', '105px', '182px', '196px']);
    // the Block column: 195 px of text and gap, and the stone's footprint, 26 + 8
    expect(ROW_SEAL.block.footprint).toBe(34);
    expect(cols[0]).toBe(`${195 + ROW_SEAL.block.footprint}px`);
    // what the column with the stone and the column it took from hold, at 0.6 em a character
    // (monospace) and the 16 px gap: Block, the stone, "#20,879,198" at 13 px, a space at 12 px
    // and "0xd161...8fef" at 11 px; Fees, "999.99999999" at 12 px (every fee under 1,000 CKB)
    const px = cols.map((w) => Number.parseFloat(w));
    expect(
      ROW_SEAL.block.footprint + chars(11, 13) + chars(1, 12) + chars(13, 11) + 16
    ).toBeLessThanOrEqual(px[0]);
    expect(chars(12, 12) + 16).toBeLessThanOrEqual(px[5]);
  });

  // The stone carries the block's transaction count (`transactionsCount`, as the API serves it):
  // 1 and 7 in rods, past nine a full block; the same stone in the table, the 608 px table and the
  // phone rows.
  it('writes each block’s transaction count on its stone: 1, 7 and 12 transactions', async () => {
    vi.mocked(api.getBlocks).mockResolvedValue({ ...page, data: counted });
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const rows = Array.from(table.querySelectorAll('tbody tr'));
    expect(
      rows.map((tr) => tr.querySelector('[data-seal] svg')!.getAttribute('aria-label'))
    ).toEqual(
      counted.map(
        (b, i) =>
          `Block seal ${b.hash.slice(0, 10)}, ${['1 transaction', '7 transactions', '12 transactions'][i]}`
      )
    );
    rows.forEach((tr, i) => expectOneRowSeal(tr, sealOf(counted[i])));
    // three counts, three stones: the count changes the drawing, not only its name
    expect(new Set(rows.map((tr) => tr.querySelector('[data-seal]')!.innerHTML)).size).toBe(3);
  });

  // No default count: a block the API served without `transactionsCount` is refused by its seal,
  // naming the block, instead of a stone with a guessed count.
  it('refuses a block served without its transaction count, naming the block', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    onTestFinished(() => spy.mockRestore());
    const uncounted = { ...page.data[0], transactionsCount: undefined } as unknown as Block;
    vi.mocked(api.getBlocks).mockResolvedValue({ ...page, data: [uncounted, page.data[1]] });
    render(
      <ErrorBoundary fallbackRender={(error) => <p data-testid="refused">{error.message}</p>}>
        <BlocksPage />
      </ErrorBoundary>
    );
    expect(await screen.findByTestId('refused')).toHaveTextContent(
      `seal: block ${page.data[0].hash}: the transaction count must be a whole number ≥ 0, got undefined`
    );
  });

  // 50 rows, 50 seals: each drawn once (Seal memoises per id), the time printed for the record
  // (phase 3 placement, step D; seals by kind, C1).
  it('draws fifty row seals once each in one render', async () => {
    const fifty = Array.from({ length: 50 }, (_, i) =>
      row(20_828_894 - i, {
        size: 641 + i,
        overheadSize: 641,
        userTransactionsSize: i,
        cycles: String(i * 1000),
        transactionSizes: [355],
        proposalsCount: 0,
      })
    );
    vi.mocked(api.getBlocks).mockResolvedValue({ ...page, data: fifty, hasMore: false });
    drawn.ms = 0;
    drawn.ids = [];
    const started = performance.now();
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const elapsed = performance.now() - started;
    expect(table.querySelectorAll('tbody [data-seal="block"][data-seal-form="row"]')).toHaveLength(
      50
    );
    const byHash = (a: unknown, b: unknown) =>
      (a as { hash: string }).hash.localeCompare((b as { hash: string }).hash);
    expect([...drawn.ids].sort(byHash)).toEqual(fifty.map(sealOf).sort(byHash));
    console.info(
      `[seal timing] blocks list, 50 rows: rendered in ${elapsed.toFixed(0)} ms, ` +
        `${drawn.ms.toFixed(1)} ms of it drawing the 50 row seals`
    );
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
        ['miner', 'miner …93hhsj', 'fees 0.05060000 CKB'],
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

    // Old issue: at 390 px "miner ckb1qzda0c...93hhsj" (180 px) and "fees 0.00001935 CKB" (134.4)
    // missed the 324 px text column by a few px with the 12 px gap, so the fees dropped to a line
    // of their own and rows took different shapes by their fee. The phone row names an unnamed
    // miner by its tail (minerName: the address's head is its lock's code hash, the same for every
    // default-lock miner), so the miner and any fee below 10 CKB (always eight decimals) hold one
    // line at 390 and at 360 (294 px). jsdom cannot measure: the characters are the measure, 12 px
    // Courier Prime at 7.2 px each, the unit at 11 px.
    it('keeps the miner and a fee below 10 CKB on one line at 390 and 360 px', async () => {
      atContainerWidth(358);
      vi.mocked(api.getBlocks).mockResolvedValue({
        ...page,
        data: [
          { ...page.data[0], totalFees: '999999999' },
          { ...page.data[1], minerPool: 'Example Pool', totalFees: '1935' },
        ],
      });
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      const [unnamed, pooled] = cards.map((card) => phoneLines(card)[1]);
      expect([unnamed.key, unnamed.left, unnamed.right]).toEqual([
        'miner',
        'miner …93hhsj',
        'fees 9.99999999 CKB',
      ]);
      expect([pooled.left, pooled.right]).toEqual(['miner Example Pool', 'fees 0.00001935 CKB']);
      // the miner stays a link to its address, the full address its title
      for (const [card, name] of [
        [cards[0], '…93hhsj'],
        [cards[1], 'Example Pool'],
      ] as const) {
        const line = card.querySelector('[data-line="miner"]') as HTMLElement;
        const link = within(line).getByRole('link', { name });
        expect(link).toHaveAttribute('href', `/mainnet/address/${miner}`);
        expect(link).toHaveAttribute('title', miner);
        // one flex line: the miner on the left, the fees as its right part
        expect(link.closest('[data-part="left"]')!.parentElement).toBe(line);
        expect(line.querySelector(':scope > [data-part="right"]')).toHaveTextContent(/^fees /);
      }
      const width = (text: string) =>
        chars(text.length - ' CKB'.length, 12) + (text.endsWith(' CKB') ? chars(4, 11) : 0);
      const lineWidth = (left: string, right: string) => chars(left.length, 12) + 12 + width(right);
      // the widest fee below 10 CKB with the tail: 93.6 + 12 + 134.4 = 240 px
      expect(lineWidth(unnamed.left, unnamed.right!)).toBeCloseTo(240, 6);
      expect(lineWidth(unnamed.left, unnamed.right!)).toBeLessThanOrEqual(294);
      // with the address's head it was 180 + 12 + 134.4 = 326.4 px, past both columns
      expect(lineWidth('miner ckb1qzda0c...93hhsj', unnamed.right!)).toBeGreaterThan(324);
      // "miner Example Pool" with the fee: 129.6 + 12 + 134.4 = 276 px
      expect(lineWidth(pooled.left, pooled.right!)).toBeCloseTo(276, 6);
      expect(lineWidth(pooled.left, pooled.right!)).toBeLessThanOrEqual(294);
    });

    // Rubbing × Song print, phase 3: the phone row leads with the block's row seal in its own
    // column, before the height; the height is still the row's link. Seals by kind: the column
    // is the 26 px stone and its 8 px gap.
    it('leads each phone row with its block’s row seal, before the height', async () => {
      atContainerWidth(358);
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      cards.forEach((card, i) => {
        const seal = expectOneRowSeal(card, sealOf(page.data[i]));
        expect(card.firstElementChild).toHaveAttribute('data-part', 'mark');
        expect(card.firstElementChild).toContainElement(seal);
        // three lines hold the stone; no overhang
        expect(seal.className).not.toMatch(/-my-/);
        expectBefore(seal, within(card).getByRole('link', { name: /^#20,828,89/ }));
      });
    });

    it('writes the transaction count on each phone row’s stone: 1, 7 and 12 transactions', async () => {
      atContainerWidth(358);
      vi.mocked(api.getBlocks).mockResolvedValue({ ...page, data: counted });
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      expect(cards).toHaveLength(3);
      cards.forEach((card, i) => expectOneRowSeal(card, sealOf(counted[i])));
    });

    it('leads each row of the 608 px table with its block’s row seal, beside the height and hash', async () => {
      atContainerWidth(608);
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      Array.from(table.querySelectorAll('tbody tr')).forEach((tr, i) => {
        const seal = expectOneRowSeal(tr, sealOf(page.data[i]));
        expect(tr.querySelector('td')).toContainElement(seal);
        // two lines (height, hash) hold the stone; no overhang
        expect(seal.className).not.toMatch(/-my-/);
        expectBefore(seal, within(tr as HTMLElement).getAllByRole('link')[0]);
      });
      expect(Number.parseFloat(table.style.minWidth)).toBe(608);
      const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
      // Height takes the stone's 7 px more than the old hash seal's; Miner gives 2, Size · cycles 5
      expect(cols.map((c) => c.width)).toEqual(['138px', '76px', '48px', '154px', '96px', 'auto']);
      expect(cols.map((c) => c.minWidth)).toEqual([
        '138px',
        '76px',
        '48px',
        '154px',
        '96px',
        '96px',
      ]);
      expect(cols[0].width).toBe(`${104 + ROW_SEAL.block.footprint}px`);
      // what each column holds at 0.6 em a character: Height, the stone, "#20,879,198" at 13 px
      // and the gap, with 2 px of room; Miner, "ckb1qzda0c...93hhsj" at 12 px and the gap; Size ·
      // cycles, "3,500,000,000" at 12 px (the last column, no gap)
      const px = cols.map((c) => Number.parseFloat(c.minWidth));
      expect(ROW_SEAL.block.footprint + chars(11, 13) + 16 + 2).toBeLessThanOrEqual(px[0]);
      expect(chars(19, 12) + 16).toBeLessThanOrEqual(px[3]);
      expect(chars(13, 12)).toBeLessThanOrEqual(px[5]);
    });

    it('writes the transaction count on each stone of the 608 px table: 1, 7 and 12 transactions', async () => {
      atContainerWidth(608);
      vi.mocked(api.getBlocks).mockResolvedValue({ ...page, data: counted });
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      const rows = Array.from(table.querySelectorAll('tbody tr'));
      expect(rows).toHaveLength(3);
      rows.forEach((tr, i) => expectOneRowSeal(tr, sealOf(counted[i])));
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
