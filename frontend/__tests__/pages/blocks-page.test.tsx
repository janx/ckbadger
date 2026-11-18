import { CHARTS } from '@/components/charts/chart-catalogue';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { genesisNeighbours, genesisNeighboursPage } from '@/__tests__/msw/fixtures/genesis-block';
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { contentPx, gapPx } from '@/__tests__/utils/tailwind-box';
import { screen, waitFor, within } from '@testing-library/react';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTick } from '@/lib/chart-ticks';
import { render } from '../utils/test-utils';
import BlocksPage from '@/app/blocks/page';
import { api, type Block } from '@/lib/api';
import { ErrorBoundary } from '@/components/error-boundary';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { ROW_LINE_PX } from '@/lib/type-metrics';
import type { SealId } from '@/lib/seal/stones';
import { expectBefore, expectOneRowSeal } from '@/__tests__/utils/seals';
import { cellFaults, faultLines } from '@/__tests__/utils/list-table-cells';
import { CLOCK_BEHIND } from '@/lib/format-age';

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
      screen.getAllByRole('img', { name: /Cellbase, 1\suser\stransaction\b/ }).length
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
    expect(lines[0]).toEqual(['identity', '#0', expect.stringMatching(/^2\stxs · .+ ago$/)]);
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
    // Re-pinned on purpose (the coordinator's ruling on F3-e): the heads' ticks in the tick form
    // (they were the exact scale, "4,000" and "8,000,000")
    const sizeScale = resourceScale([3917, 641]);
    const cyclesScale = resourceScale([6831940, 0]);
    expect(size).toHaveTextContent('Size, bytes0' + formatTick(sizeScale, sizeScale));
    expect(cycles).toHaveTextContent('Cycles0' + formatTick(cyclesScale, cyclesScale));
    expect(size).toHaveClass('text-left');
    // Re-pinned on purpose (readability F3-e): each track is its head's widest tick row, "0" and the
    // widest scale at 12 px a glyph apart ("1,500,000": 79.2 px; "4,000,000,000": 108), not 120;
    // re-pinned again by the coordinator's ruling: in the tick form ("0 1.5M", "0 4B") both rows
    // stand inside the 48 px a bar keeps at least
    const tracks = ['48px', '48px'];
    [size, cycles].forEach((th, i) =>
      expect((th.querySelector('[data-scale-ticks]') as HTMLElement).style.width).toBe(tracks[i])
    );
    const bars = table.querySelectorAll('tbody [data-aligned-bar]');
    expect(bars).toHaveLength(4);
    Array.from(bars).forEach((bar, i) => {
      expect(bar.closest('td')).not.toHaveClass('text-right');
      // Size, then Cycles, on each row: the row's bars on their columns' tracks
      expect((bar.querySelector('[data-track]') as HTMLElement).style.width).toBe(tracks[i % 2]);
    });
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
    const glyph = within(table).getByRole('img', { name: /4\sproposals/ });
    expect(glyph.querySelectorAll('[data-proposal-dot]')).toHaveLength(4);
  });

  // U-09 (re-pinned): the table's unnamed miner at the row size, the one ellipsis; R9 (re-pinned):
  // the row size of an address is `ckb1…` and its last 8 characters
  it('writes the miner on one line, ckb1…4s93hhsj (BA-05)', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const link = within(table).getAllByRole('link', { name: 'ckb1…4s93hhsj' })[0];
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
    expect(within(table).getAllByRole('link', { name: 'ckb1…4s93hhsj' })).toHaveLength(1);
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
    expect(number).toHaveClass('text-body', 'text-text-bright', 'leading-row');
    const hash = within(blockCell).getByTitle(page.data[0].hash);
    expect(hash).toHaveClass('text-meta', 'text-text-dim');
    expect(hash).not.toHaveClass('text-body');
    expect(contentsCell.firstElementChild).toHaveClass('flex');
    expect(contentsCell.querySelector('[data-contents-line]')).toBeInTheDocument();
  });

  // Seals by kind (phase C1): each row leads with its block's stone (16 px since readability R6,
  // its hash and its transaction count), before the height; the height stays the row's link, the
  // seal is an image.
  it('leads every row with its block’s row seal, before the height link', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const rows = Array.from(table.querySelectorAll('tbody tr'));
    expect(rows).toHaveLength(page.data.length);
    rows.forEach((tr, i) => {
      const cell = tr.querySelector('td')!;
      const seal = expectOneRowSeal(tr, sealOf(page.data[i]));
      expect(cell).toContainElement(seal);
      // one 20 px line keeps the row's height: the 16 px stone stands centred on it, 2 px clear
      // above and below (re-pinned on purpose, readability R6: the 26 px stone overhung it by 3 px
      // above and below, `-my-[3px]`)
      expect((ROW_LINE_PX - ROW_SEAL.block.height) / 2).toBe(2);
      expect(seal).toHaveClass('h-row', 'items-center');
      expect(seal.className).not.toMatch(/-my-/);
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
  // Miner 2). Corrected (review fb-tests 3): Height's 2 px of room went in F3-e (34a545c8), as
  // every spare did; its number stays one unit by the table's rule, not by a spare (ListTable: no
  // cell lets a figure break), and its budget is exact to the tenth (the 608 px table's test).
  // re-pinned with readability R4 (lane F3, step 2): the key at 14 px, the table text 13, the hash 12: the derived columns grow with their text and the table needs
  // 1,260 px (was 1,230), so the widest 1,232 px container draws the 608 px table.
  // Re-pinned on purpose (readability F3-e): every column is its content (Block's 13 px spare and
  // the spares of Reward and Fees went; Age, Contents, Size and Cycles, written by hand, hold their
  // widest values; Miner is the flexible column), 1,284 px.
  // Re-pinned on purpose (the coordinator's ruling: the bar heads in the tick form, their tracks
  // 48 px): Size 176 → 145, Cycles 220 → 160, 1,193 px (renamed: "1,284 px").
  // Re-pinned on purpose (readability R6): the block's row stone 26 → 16 px, its footprint 34 → 24,
  // the Block column 230 → 220, the table 1,193 → 1,183 px (renamed: "1,193 px").
  it('budgets the seal in the Block column and keeps the table at 1,183 px', async () => {
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    expect(Number.parseFloat(table.style.minWidth)).toBe(1183);
    const cols = Array.from(table.querySelectorAll('col')).map(
      (c) => (c as HTMLElement).style.minWidth
    );
    expect(cols).toEqual(['220px', '110px', '194px', '118px', '126px', '110px', '145px', '160px']);
    // the Block column: 195.4 px of text and gap rounded up, and the stone's footprint, 16 + 8
    expect(ROW_SEAL.block.footprint).toBe(24);
    expect(cols[0]).toBe(`${196 + ROW_SEAL.block.footprint}px`);
    // what the column with the stone and the column it took from hold, at 0.6 em a character
    // (monospace) and the 16 px gap: Block, the stone, "#20,879,198" at 14 px, a space at 13 px
    // and the tight hash "0xd161…8fef" at 12 px, 11 characters with its one ellipsis (re-pinned,
    // readability R4: the check counted 13, the three-dot form, which 6.6 px a glyph let pass);
    // Fees, "999.99999999" at 13 px (every fee under 1,000 CKB)
    const px = cols.map((w) => Number.parseFloat(w));
    expect(
      ROW_SEAL.block.footprint + chars(11, 14) + chars(1, 13) + chars(11, 12) + 16
    ).toBeLessThanOrEqual(px[0]);
    expect(chars(12, 13) + 16).toBeLessThanOrEqual(px[5]);
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

  // Readability 2026-11-18, lane B5: the limits that give the Size and Cycles bars their meaning
  // ("Fullest of these 50: … of the 597,000-byte limit") stood after the fiftieth row; they stand
  // above the table, and the pager stays under it.
  it('states the block limits above the table, the pager under it (B5)', async () => {
    render(<BlocksPage />);
    const note = await screen.findByText(/^Size and cycles · Fullest of these 2/);
    const table = screen.getByRole('table', { name: 'Latest blocks' });
    expect(note.compareDocumentPosition(table) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const pager = document.querySelector('[data-role="pager-controls"]') as HTMLElement;
    expect(table.compareDocumentPosition(pager) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('reads the footnote figures in bright ink', async () => {
    render(<BlocksPage />);
    const note = await screen.findByText(/^Size and cycles · Fullest of these 2/);
    expect(within(note).getByText('3,917')).toHaveClass('text-text-bright');
    expect(within(note).getByText('6,831,940')).toHaveClass('text-text-bright');
  });

  // Readability F3-e, the coordinator's rulings on the blocks tables: (1) the bar heads write their
  // ticks in the tick form, as every axis on the site does, so each track is the 48 px a bar keeps
  // at least and the wide table is 1,193 px, a table at 1,232; (2) the 608 px table writes a
  // block's transaction count under its age, the second line of one cell headed "Age · txs" (the
  // row already has two lines: the hash under the height, cycles under size), every column at its
  // honest budget. Re-pinned on purpose (readability R6): the block stone's footprint 34 → 24, so
  // the wide table is 1,183 px and the 608 px table 585 (were 1,193 and 595; renamed).
  describe('the rulings: tick-form heads and the 608 px table (readability F3-e)', () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    it('draws the wide table from 1,183 px, its bars on 48 px tracks under tick-form heads', async () => {
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
      const size = ceil(48 + 10 + at('1,500,000', 13) + 16);
      const cycles = ceil(48 + 10 + at('4,000,000,000', 13));
      expect([size, cycles]).toEqual([145, 160]);
      expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual([
        220,
        110,
        194,
        118,
        126,
        110,
        size,
        cycles,
      ]);
      expect(Number.parseFloat(table.style.minWidth)).toBe(1183);
      const ths = Array.from(table.querySelectorAll('th'));
      for (const [head, values] of [
        ['Size, bytes', [3917, 641]],
        ['Cycles', [6831940, 0]],
      ] as const) {
        const th = ths.find((h) => h.textContent?.startsWith(head))!;
        const ticks = th.querySelector('[data-scale-ticks]') as HTMLElement;
        const scale = resourceScale([...values]);
        expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual([
          '0',
          formatTick(scale, scale),
        ]);
        expect(ticks.style.width).toBe('48px');
      }
      for (const track of table.querySelectorAll<HTMLElement>('tbody [data-track]'))
        expect(track.style.width).toBe('48px');
    });

    it('changes form where each table stops fitting: 1,183, then 585, then rows', async () => {
      for (const [width, form] of [
        [1232, 'table'],
        [1183, 'table'],
        [1182, 'narrow-table'],
        [608, 'narrow-table'],
        [585, 'narrow-table'],
        [584, 'rows'],
      ] as const) {
        atContainerWidth(width);
        const { container, unmount } = render(<BlocksPage />);
        await waitFor(() =>
          expect(container.querySelector('[data-list-form]')).toHaveAttribute(
            'data-list-form',
            form
          )
        );
        unmount();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
        vi.mocked(api.getBlocks).mockResolvedValue(page);
      }
    });

    it('writes the count under the age in the 608 px table: five columns, 585 px', async () => {
      atContainerWidth(608);
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      expect(Array.from(table.querySelectorAll('th')).map((th) => th.textContent)).toEqual([
        'Height',
        'Age · txs',
        'Miner',
        'Fees, CKB',
        'Size · cycles',
      ]);
      const budgets = [
        // the stone and its gap beside "#99,999,999" at 14 px (its hash under it is narrower)
        ceil(ROW_SEAL.block.footprint + at('#99,999,999', 14) + 16),
        // "10y 364d ago" over the widest count and proposals, "9,999 · 1,500p" (the count at 13
        // px, the separator, the proposals at 12): 105.6, the wider line
        ceil(Math.max(at('10y 364d ago', 13), at('9,999 · ', 13) + at('1,500p', 12)) + 16),
        // the flexible column: an unnamed miner whole
        ceil(at('ckb1…4s93hhsj', 13) + 16),
        // fees under 1,000 CKB
        ceil(at('999.99999999', 13) + 16),
        // "3,500,000,000" (the cycle limit; genesis's "1,236,271 B" is narrower); the last column
        ceil(at('3,500,000,000', 13)),
      ];
      expect(budgets).toEqual([133, 122, 118, 110, 102]);
      const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
      expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
      expect(cols.map((c) => c.width)).toEqual(['133px', '122px', 'auto', '110px', '102px']);
      expect(Number.parseFloat(table.style.minWidth)).toBe(585);
      // the age, then the count with its proposals: two lines, as the height and hash beside them
      const [first, second] = Array.from(table.querySelectorAll('tbody tr'));
      const lines = (tr: Element) =>
        Array.from(tr.querySelectorAll('td')[1].querySelectorAll(':scope > * > *')).map(
          (l) => l.textContent
        );
      expect(lines(first)).toEqual([expect.stringMatching(/ ago$/), '5 · 4p']);
      expect(lines(second)).toEqual([expect.stringMatching(/ ago$/), '1']);
    });
  });

  // Readability F3-e: every column of the wide table is budgeted from the widest value its
  // formatter writes for the chain (lib/list-bounds.ts), at 0.6 em a glyph (14 px key, 13 px cells,
  // 12 px hash, count and ticks) and the 16 px gap after every column but the last. The bars'
  // tracks hold their heads' tick rows ("0", a glyph, the widest scale) whole. Re-pinned on purpose
  // (the coordinator's ruling): the heads write their ticks in the tick form, so the tracks are 48
  // px and the table needs 1,193 px, a table in the widest container, 1,232 at 1280 (it needed
  // 1,284, and stopped there). Re-pinned on purpose (readability R6): the block stone's footprint
  // 34 → 24, Block 230 → 220, the table 1,183 px (renamed: "1,193 px").
  describe('every wide column budgeted from what it holds (readability F3-e)', () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    /** A column's minimum, as columnPx rounds it: to the hundredth, then up to a whole pixel. */
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    /** A head's tick row whole over its track: the ticks at 12 px, one glyph apart, 48 at least. */
    const tickRow = (...ticks: string[]) => Math.max(48, at(ticks.join(' '), 12));
    const budgets = [
      // the stone and its gap, "#99,999,999" (eight digits) at 14 px, a space at 13, the tight hash
      ceil(
        ROW_SEAL.block.footprint + at('#99,999,999', 14) + chars(1, 13) + at('0x0000…d2de', 12) + 16
      ),
      // the widest age, "10y 364d ago" (two digits of years from genesis's tenth birthday)
      ceil(at('10y 364d ago', 13) + 16),
      // the 112 px glyph, its 8 px gap and the widest count, "9,999 tx" at 12 px
      ceil(112 + 8 + at('9,999 tx', 12) + 16),
      // the flexible column: an unnamed miner whole, "ckb1…4s93hhsj"; a pool name takes the rest
      ceil(at('ckb1…4s93hhsj', 13) + 16),
      // four whole digits of reward, every decimal
      ceil(at('9,999.99999999', 13) + 16),
      // fees under 1,000 CKB
      ceil(at('999.99999999', 13) + 16),
      // genesis's 1,236,271 bytes on a 1,500,000 scale: its tick row in the tick form ("0 1.5M"),
      // the 10 px gap, its figure
      ceil(tickRow('0', '1.5M') + 10 + at('1,500,000', 13) + 16),
      // the cycle limit's 4,000,000,000 scale ("0 4B"), the last column (no gap after it)
      ceil(tickRow('0', '4B') + 10 + at('4,000,000,000', 13)),
    ];

    it('pins each column to its content and the table to their sum, 1,183 px', async () => {
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
      expect(budgets).toEqual([220, 110, 194, 118, 126, 110, 145, 160]);
      expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
      // the Miner column takes what the container gives beyond the sum
      expect(cols.map((c) => c.width)).toEqual([
        '220px',
        '110px',
        '194px',
        'auto',
        '126px',
        '110px',
        '145px',
        '160px',
      ]);
      expect(budgets.reduce((sum, w) => sum + w, 0)).toBe(1183);
      expect(Number.parseFloat(table.style.minWidth)).toBe(1183);
    });

    it('draws each bar on a track its head’s widest tick row fits', async () => {
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      const ths = Array.from(table.querySelectorAll('th'));
      for (const [head, track] of [
        ['Size, bytes', tickRow('0', '1.5M')],
        ['Cycles', tickRow('0', '4B')],
      ] as const) {
        const th = ths.find((h) => h.textContent?.startsWith(head))!;
        expect(
          Number.parseFloat((th.querySelector('[data-scale-ticks]') as HTMLElement).style.width)
        ).toBe(track);
      }
      expect([tickRow('0', '1.5M'), tickRow('0', '4B')]).toEqual([48, 48]);
    });

    it('lets a long pool name end in the one ellipsis, its title the miner', async () => {
      const long = 'A Mining Pool Whose Name Is Longer Than Its Column';
      vi.mocked(api.getBlocks).mockResolvedValue({
        ...page,
        data: [{ ...page.data[0], minerPool: long }, page.data[1]],
      });
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      const pool = within(table).getByRole('link', { name: long });
      expect(pool.closest('td')).toHaveClass('truncate');
      expect(pool).toHaveAttribute('title', miner);
    });

    // re-pinned on purpose (the coordinator's ruling): the table from 1,193 px, so at 1,232 too
    // (renamed: "1,284 the table"); re-pinned on purpose (readability R6): from 1,183 px, the block
    // stone's footprint 34 → 24 (renamed: "1,193 the table")
    it('changes form where the sum stops fitting: 1,183 the table, below it the 608 px table', async () => {
      for (const [width, form] of [
        [1232, 'table'],
        [1183, 'table'],
        [1182, 'narrow-table'],
        [608, 'narrow-table'],
        [358, 'rows'],
      ] as const) {
        atContainerWidth(width);
        const { container, unmount } = render(<BlocksPage />);
        await waitFor(() =>
          expect(container.querySelector('[data-list-form]')).toHaveAttribute(
            'data-list-form',
            form
          )
        );
        unmount();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
        vi.mocked(api.getBlocks).mockResolvedValue(page);
      }
    });
  });

  // A cell holds its content (components/ui/list-table.tsx; review fb-logic 3, the lead's ruling):
  // an age is a figure, one unit; "this device's clock is behind" (29 characters, 226.2 px at 13
  // px) is words and breaks between them in the 94 px Age column and the 106 px Age · txs column,
  // where it stood on one line and drew some 130 px over the next column.
  describe('a cell holds its content (the lead’s rulings on the fb reviews)', () => {
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    it('lets the clock-behind sentence break between its words in both tables', async () => {
      const ahead = new Date(Date.now() + 10 * 60_000).toISOString();
      for (const width of [1232, 608]) {
        atContainerWidth(width);
        vi.mocked(api.getBlocks).mockResolvedValue({
          ...page,
          data: [{ ...page.data[0], timestamp: ahead }, page.data[1]],
        });
        const { unmount } = render(<BlocksPage />);
        const table = await screen.findByRole('table', { name: 'Latest blocks' });
        const [first] = Array.from(table.querySelectorAll('tbody tr'));
        const age = within(first.querySelectorAll('td')[1] as HTMLElement).getByText(CLOCK_BEHIND);
        expect(age.closest('[data-nobreak], .whitespace-nowrap')).toBeNull();
        expect(faultLines(cellFaults(table))).toEqual([]);
        unmount();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
      }
    });

    // Review fb-tests 6 (the lead's ruling): a value one glyph past its bound, and what the table
    // does with it today, for the lists page lane to rule on (no mechanism is built here). The
    // nine-digit height "#100,000,000" (12 glyphs at 14 px, 100.8; the bound's 11 are 92.4) stays
    // one figure on its line, as a figure does in every cell, and runs past its column's content
    // box: in the wide table the Block column's box is 220 − 16 = 204 px, its line the stone (24),
    // the number, a space (7.8) and the tight hash (79.2), 211.8: 7.8 px into its own 16 px gap;
    // in the 608 px table the Height box is 133 − 16 = 117, the stone and the number 124.8: 7.8 px
    // into its gap. Neither reaches the next column (it would from 16 px, a second glyph past).
    // Re-pinned on purpose (readability R6): the stone's footprint 34 → 24 takes 10 px from the
    // line and from the column alike (were 230 / 214 / 221.8 and 143 / 127 / 134.8), so the 7.8
    // stays.
    it('holds a height one glyph past its bound on one line, 7.8 px into its gap', async () => {
      const at = (chars: number, px: number) => (chars * px * 3) / 5;
      const stone = ROW_SEAL.block.footprint;
      expect(stone).toBe(24);
      for (const [width, column, line] of [
        [1232, 220, stone + at(12, 14) + at(1, 13) + at(11, 12)],
        [608, 133, stone + at(12, 14)],
      ] as const) {
        atContainerWidth(width);
        vi.mocked(api.getBlocks).mockResolvedValue({
          ...page,
          data: [
            row(100_000_000, {
              size: 641,
              overheadSize: 641,
              userTransactionsSize: 0,
              cycles: '0',
              transactionSizes: [355],
              proposalsCount: 0,
            }),
            page.data[1],
          ],
        });
        const { unmount } = render(<BlocksPage />);
        const table = await screen.findByRole('table', { name: 'Latest blocks' });
        const cols = Array.from(table.querySelectorAll('col')).map((c) =>
          Number.parseFloat((c as HTMLElement).style.minWidth)
        );
        expect(cols[0]).toBe(column);
        const number = within(table).getByRole('link', { name: '#100,000,000' });
        expect(number.closest('td')).toBe(table.querySelector('tbody td'));
        expect(cellFaults(table).filter((f) => f.rule === 'figure breaks')).toEqual([]);
        const past = line - (column - 16);
        expect(past).toBeCloseTo(7.8, 10);
        expect(past).toBeLessThan(16);
        unmount();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
      }
    });
  });

  // Board L1 (2026-11-01 responsive plan): the phone row is three lines and 640 is a table.
  describe('at phone and 640 widths (board L1)', () => {
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    it('gives each block three lines at 358 px: height, txs and age · miner and fees · the figures', async () => {
      atContainerWidth(358);
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      expect(cards).toHaveLength(2);
      expect(screen.queryByRole('table')).toBeNull();
      expect(phoneLines(cards[0]).map((l) => [l.key, l.left, l.right])).toEqual([
        // the head line: what every reader comes for, the height, the transactions and the age
        ['identity', '#20,828,894', expect.stringMatching(/^5\stxs · \d+[smhd] ago$/)],
        // U-09 (re-pinned): the phone row's unnamed miner at the tight size, 4…6 (the lead's ruling)
        // A phone row's amounts are the summary form, each cut one titled with its exact value
        // (readability R5, re-pinned on purpose: "fees 0.05060000 CKB", "reward 684.32120795 CKB")
        ['miner', 'miner ckb1…93hhsj', 'fees 0.0506 CKB'],
        [
          'figures',
          // Every desktop column: hash, the proposals (consensus detail, after the hash that
          // starts every row's line), size with its overhead, cycles, the reward.
          '0x0000…d2de · 4\u00a0proposals · 3,917 B, 640 B overhead · 6,831,940 cycles · reward 684.32 CKB',
          null,
        ],
      ]);
      // A reward not yet paid says where it will be.
      expect(phoneLines(cards[1])[2].left).toMatch(/· reward pays at #20,828,904$/);
    });

    // Follow-up: the head line carried the proposal and uncle counts too, so at 360 px (294 px of
    // text column, the 13 px height #20,987,284 85.8 px and the 12 px gap leaving 196.2, 27
    // characters at 12 px; re-pinned on purpose, readability R6: the 16 px stone leaves 304 px of
    // text column at 360 and 334 at 390, so 206.2 and 236.2, 28 and 32 characters, where the 26 px
    // stone left 294 and 324) "4 txs · 1 proposal · 13s ago" (28) dropped under the height while
    // "1 tx · 2m ago" stood beside it, and with an uncle it wrapped at 390 too. The head line now
    // holds the height, the transactions and the age; proposals and uncles join the quiet figures,
    // each count one unbroken unit, only when non-zero. jsdom cannot measure: the characters are the
    // measure, 7.8 px each at 13 px, 7.2 at 12, 6.6 at 11.
    it('keeps the head line to the height, the transactions and the age, one line at 360 px', async () => {
      atContainerWidth(358);
      const ago = (ms: number) => new Date(Date.now() - ms).toISOString();
      const busy = {
        ...page.data[0],
        number: 20_987_284,
        hash: `0x${'5e'.repeat(32)}`,
        transactionSizes: Array<number>(999).fill(355),
        transactionsCount: 999,
        proposalsCount: 12,
        unclesCount: 1,
        timestamp: ago(59 * 60_000 + 20_000),
      };
      const plain = {
        ...page.data[1],
        number: 20_987_283,
        hash: `0x${'5d'.repeat(32)}`,
        proposalsCount: 0,
        unclesCount: 0,
      };
      vi.mocked(api.getBlocks).mockResolvedValue({ ...page, data: [busy, plain] });
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      const [head, , figures] = phoneLines(cards[0]);
      // re-pinned (A4 round 2): the count a unit, number and noun joined by a plain space
      expect([head.left, head.right]).toEqual(['#20,987,284', '999 txs · 59m ago']);
      // the consensus detail stands with the quiet figures, after the hash, each count unbroken
      expect(figures.left).toMatch(/^0x5e5e…5e5e · 12\sproposals · 1\suncle · /);
      const quiet = cards[0].querySelector('[data-line="figures"]') as HTMLElement;
      // U-16 (re-pinned): each count and its noun are one unit, a no-break space between them
      for (const count of ['12\u00a0proposals', '1\u00a0uncle'])
        expect(quiet.textContent).toContain(count);
      // a block with neither writes neither
      const [plainHead, , plainFigures] = phoneLines(cards[1]);
      expect(plainHead.right).toMatch(/^1\stx · \d+[smhd] ago$/);
      expect(plainFigures.left).not.toMatch(/proposal|uncle/);
      // the text column beside the stone: the window less the two 16 px gutters and the stone's
      // footprint (re-pinned on purpose, readability R6: 304 and 334 px, were 294 and 324)
      const [at360, at390] = [360, 390].map((w) => w - 32 - ROW_SEAL.block.footprint);
      expect([at360, at390]).toEqual([304, 334]);
      // the worst realistic head: 85.8 + 12 + 17 × 7.2 = 220.2 px, inside 304 (and 334)
      const headWidth = (left: string, right: string) =>
        chars(left.length, 13) + 12 + chars(right.length, 12);
      expect(headWidth(head.left, head.right!)).toBeCloseTo(220.2, 6);
      expect(headWidth(head.left, head.right!)).toBeLessThanOrEqual(at360);
      // the right part holds 28 characters at 360 and 32 at 390 (re-pinned, R6: 27 and 31); the
      // old head of this block was 42
      expect(Math.floor((at360 - chars(11, 13) - 12) / chars(1, 12))).toBe(28);
      expect(Math.floor((at390 - chars(11, 13) - 12) / chars(1, 12))).toBe(32);
      expect(
        headWidth(head.left, '999\u00a0txs · 12\u00a0proposals · 1\u00a0uncle · 59m ago')
      ).toBeGreaterThan(at390);
    });

    // Old issue: at 390 px "miner ckb1qzda0c...93hhsj" (180 px) and "fees 0.00001935 CKB" (134.4)
    // missed the 324 px text column by a few px with the 12 px gap, so the fees dropped to a line
    // of their own and rows took different shapes by their fee. The phone row writes an unnamed
    // miner head-and-tail like every shortened identifier, with a 4-character head (the tight
    // address of U-09, 4…6, the lead's ruling): the address's head after `ckb1` is its lock's code
    // hash, the same for every default-lock miner, and the tail (the checksum of the whole address)
    // tells miners apart. So the miner and any fee below 10 CKB (always eight decimals) hold one
    // line at 390 and at 360 (304 px: 360 less the two 16 px gutters and the block stone's 16 + 8;
    // re-pinned on purpose, readability R6: 294 with the 26 px stone's 26 + 8).
    // jsdom cannot measure: the characters are the measure, 12 px at 7.2 px each, the unit at
    // 11 px. Lane B3's 8da9b2b5 turned this test round (renamed to "at 390 px", the fit at 360
    // asserted as a drop) to pass an 8…6 form; that was wrong and is undone here.
    it('keeps the miner and a fee below 10 CKB on one line at 390 and 360 px', async () => {
      atContainerWidth(358);
      const testnetMiner = 'ckt1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgx3ze7p';
      vi.mocked(api.getBlocks).mockResolvedValue({
        ...page,
        data: [
          { ...page.data[0], totalFees: '999999999' },
          { ...page.data[1], minerPool: 'Example Pool', totalFees: '1935' },
          {
            ...page.data[1],
            number: 20_828_892,
            hash: `0x${'7c'.repeat(32)}`,
            minerAddress: testnetMiner,
          },
        ],
      });
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      const [unnamed, pooled, testnet] = cards.map((card) => phoneLines(card)[1]);
      const width = (text: string) =>
        chars(text.length - ' CKB'.length, 12) + (text.endsWith(' CKB') ? chars(4, 11) : 0);
      const lineWidth = (left: string, right: string) => chars(left.length, 12) + 12 + width(right);
      // the text column beside the stone at 360 and 390 (re-pinned, R6: were 294 and 324)
      const [at360, at390] = [360, 390].map((w) => w - 32 - ROW_SEAL.block.footprint);
      expect([at360, at390]).toEqual([304, 334]);
      // the budget first, as the row writes the line: it failed at 8…6 (297.6 px) and holds at 4…6
      expect(lineWidth(unnamed.left, unnamed.right!)).toBeLessThanOrEqual(at360);
      expect([unnamed.key, unnamed.left, unnamed.right]).toEqual([
        'miner',
        'miner ckb1…93hhsj',
        // the summary form (R5, re-pinned on purpose: was "fees 9.99999999 CKB")
        'fees 9.99 CKB',
      ]);
      expect([pooled.left, pooled.right]).toEqual(['miner Example Pool', 'fees 0.00001935 CKB']);
      expect(testnet.left).toBe('miner ckt1…x3ze7p'); // testnet's prefix, same form
      // the miner stays a link to its address, the full address its title
      for (const [card, name, address] of [
        [cards[0], 'ckb1…93hhsj', miner],
        [cards[1], 'Example Pool', miner],
        [cards[2], 'ckt1…x3ze7p', testnetMiner],
      ] as const) {
        const line = card.querySelector('[data-line="miner"]') as HTMLElement;
        const link = within(line).getByRole('link', { name });
        expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
        expect(link).toHaveAttribute('title', address);
        // one flex line: the miner on the left, the fees as its right part
        expect(link.closest('[data-part="left"]')!.parentElement).toBe(line);
        expect(line.querySelector(':scope > [data-part="right"]')).toHaveTextContent(/^fees /);
      }
      // Readability R5 (re-pinned on purpose): a phone row's fee is the summary form, so a fee of
      // 9.99999999 CKB is "fees 9.99 CKB": 122.4 + 12 + 91.2 = 225.6 px (it was 268.8 px, every
      // decimal). The widest fee is now one under 1 CKB, which keeps its exact value — "fees
      // 0.99999999 CKB" is 268.8 px, as the 8-decimal form was, 35.2 to spare at 360 and 65.2 at
      // 390 (25.2 and 55.2 beside the 26 px stone) — and from 1 CKB up the summary is shorter:
      // 999.99 CKB is 240 px, 1,000,000 CKB 261.6.
      expect(lineWidth(unnamed.left, unnamed.right!)).toBeCloseTo(225.6, 6);
      expect(lineWidth(unnamed.left, unnamed.right!)).toBeLessThanOrEqual(at360);
      expect(lineWidth(unnamed.left, 'fees 0.99999999 CKB')).toBeCloseTo(268.8, 6);
      expect(lineWidth(unnamed.left, 'fees 0.99999999 CKB')).toBeLessThanOrEqual(at360);
      expect(lineWidth(unnamed.left, 'fees 999.99 CKB')).toBeCloseTo(240, 6);
      expect(lineWidth(unnamed.left, 'fees 1,000,000 CKB')).toBeCloseTo(261.6, 6);
      expect(lineWidth(unnamed.left, 'fees 1,000,000 CKB')).toBeLessThanOrEqual(at360);
      // "miner Example Pool" with the fee: 129.6 + 12 + 134.4 = 276 px
      expect(lineWidth(pooled.left, pooled.right!)).toBeCloseTo(276, 6);
      expect(lineWidth(pooled.left, pooled.right!)).toBeLessThanOrEqual(at360);
    });

    // Review rules-tests NOTE-3: two checks here compared literal strings with the 294 / 324 px
    // columns of the 26 px stone, so nothing the page draws could fail them. The short form is R9's
    // rule (an address keeps `ckb1…` and its tail: the head after `ckb1` is its lock's code hash,
    // the same for every default-lock miner, so it tells no two miners apart), no longer the
    // arithmetic's: at the old count above (12 px, the unit 11) lane B3's 8…6, "miner
    // ckb1qzda…93hhsj", with the widest fee was 297.6 px, past the 294 the 26 px stone left at 360
    // and inside the 304 the 16 px one leaves. The live property, on the rows the page draws at
    // 390 and 360 and measured at their own steps (tailwind-box: the line at 13 px, 7.8 a glyph,
    // the unit at 12, 7.2, the 12 px gap): the miner in the tight form, and its line with the
    // widest fee, one under 1 CKB in its exact value, inside the text column beside the stone:
    // "miner ckb1…93hhsj" 132.6 + 12 + "fees 0.99999999" 117 + " CKB" 28.8 = 290.4 px, in 304 at
    // 360 and 334 at 390. (At these steps the 8…6 form would be 321.6 px and the row size's 4…8
    // 306, both past 304.)
    it.each([390, 360] as const)(
      'writes the miner in the tight form at %i px, its line with the widest fee inside the column beside the stone',
      async (viewport) => {
        const content = viewport - 32;
        atContainerWidth(content);
        vi.mocked(api.getBlocks).mockResolvedValue({
          ...page,
          data: [{ ...page.data[0], totalFees: '99999999' }],
        });
        render(<BlocksPage />);
        const [card] = await screen.findAllByTestId('block-row-mobile');
        const line = card.querySelector(':scope > [data-line="miner"]') as HTMLElement;
        const [left, right] = Array.from(line.children) as HTMLElement[];
        expect([left.textContent, right.textContent]).toEqual([
          'miner ckb1…93hhsj',
          'fees 0.99999999 CKB',
        ]);
        const column = content - ROW_SEAL.block.footprint;
        expect(column).toBe(viewport === 390 ? 334 : 304);
        const drawn =
          contentPx(left, viewport) + gapPx(line, viewport) + contentPx(right, viewport);
        expect(drawn).toBeCloseTo(290.4, 6);
        expect(drawn).toBeLessThanOrEqual(column);
      }
    );

    // The review's item H: a block row's age reads the page's one clock, so it moves with no data
    // change (it was written once at render and froze between refetches). Date stands still and
    // moves by hand; the clock's timer is real.
    it('moves a block row’s age with no data change', async () => {
      atContainerWidth(358);
      const T = Date.parse('2026-11-14T12:00:00.000Z');
      vi.useFakeTimers({ toFake: ['Date'], now: T });
      try {
        vi.mocked(api.getBlocks).mockResolvedValue({
          ...page,
          data: [{ ...page.data[0], timestamp: new Date(T - 20_000).toISOString() }],
        });
        render(<BlocksPage />);
        const [card] = await screen.findAllByTestId('block-row-mobile');
        expect(phoneLines(card)[0].right).toMatch(/ · 20s ago$/);
        vi.setSystemTime(T + 1000);
        await waitFor(() => expect(phoneLines(card)[0].right).toMatch(/ · 21s ago$/));
      } finally {
        vi.useRealTimers();
      }
    });

    // Rubbing × Song print, phase 3: the phone row leads with the block's row seal in its own
    // column, before the height; the height is still the row's link. Seals by kind: the column
    // is the stone and its 8 px gap (16 px since readability R6; it was 26).
    it('leads each phone row with its block’s row seal, before the height', async () => {
      atContainerWidth(358);
      render(<BlocksPage />);
      const cards = await screen.findAllByTestId('block-row-mobile');
      cards.forEach((card, i) => {
        const seal = expectOneRowSeal(card, sealOf(page.data[i]));
        expect(card.firstElementChild).toHaveAttribute('data-part', 'mark');
        expect(card.firstElementChild).toContainElement(seal);
        // three lines hold the stone, centred in a row line's box at the top; no overhang
        expect(seal).toHaveClass('h-row', 'items-center');
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
        // two lines (height, hash) hold the stone, centred on the height's line; no overhang
        expect(seal).toHaveClass('h-row', 'items-center');
        expect(seal.className).not.toMatch(/-my-/);
        expectBefore(seal, within(tr as HTMLElement).getAllByRole('link')[0]);
      });
      // R9 (re-pinned): Miner holds a 13-character address, 110 px (it was 154 for 17), so the
      // table's minimum is 564 px (it was 608) and Size · cycles takes the rest; re-pinned with readability R4 (lane F3, step 2): the key at 14 px, the table text 13, the hash 12:
      // 587 px, still inside the 608 px band
      // Re-pinned on purpose (the coordinator's ruling on F3-e): five columns at their honest
      // budgets, the count under the age (Age 76 and Txs 48 → Age · txs 122), Fees 96 → 110,
      // Height's and Size · cycles' 2 px spares gone, Miner the flexible column: 595 px
      // Re-pinned on purpose (readability R6): the block stone 26 → 16 px, its footprint 34 → 24,
      // Height 143 → 133, the table 585 px
      expect(Number.parseFloat(table.style.minWidth)).toBe(585);
      const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
      expect(cols.map((c) => c.width)).toEqual(['133px', '122px', 'auto', '110px', '102px']);
      expect(cols.map((c) => c.minWidth)).toEqual(['133px', '122px', '118px', '110px', '102px']);
      expect(cols[0].width).toBe(`${109 + ROW_SEAL.block.footprint}px`);
      // what each column holds at 0.6 em a character: Height, the stone, "#99,999,999" at 14 px
      // and the gap; Miner, "ckb1…4s93hhsj" (the row size, 13 characters) at 13 px and the gap;
      // Size · cycles, "3,500,000,000" at 13 px (the last column, no gap)
      const px = cols.map((c) => Number.parseFloat(c.minWidth));
      // Height to the tenth (review fb-tests 3: the test, the comment and the budget say one
      // thing): with pl-0 the content box is the column less its 16 px gap, 133 − 16 = 117.0; it
      // holds the stone's 24 and "#99,999,999", 11 × 8.4 = 92.4, 116.4 in all, so 0.6 px stays
      // free; the tight hash under the number, 11 × 7.2 = 79.2, is narrower. The column is that
      // content and its gap rounded up, no spare: ceil(24 + 92.4 + 16) = ceil(132.4) = 133
      // (re-pinned, R6: the stone's 34, 143 − 16 = 127.0, 126.4, ceil(142.4) = 143).
      expect(ROW_SEAL.block.footprint).toBe(24);
      expect(chars(11, 14)).toBeCloseTo(92.4, 10);
      expect(px[0]).toBe(Math.ceil(ROW_SEAL.block.footprint + chars(11, 14) + 16));
      expect(px[0] - 16 - (ROW_SEAL.block.footprint + chars(11, 14))).toBeCloseTo(0.6, 10);
      expect(chars(11, 12)).toBeLessThan(chars(11, 14));
      expect(chars(13, 13) + 16).toBeLessThanOrEqual(px[2]);
      expect(chars(13, 13)).toBeLessThanOrEqual(px[4]);
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

    // Re-pinned on purpose (the coordinator's ruling on F3-e): five columns, the transaction count
    // the age's second line under "Age · txs" (renamed: "the six-column table of board L1").
    it('draws the five-column table of board L1 in the 608 px band, without the reward', async () => {
      atContainerWidth(608);
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      expect(Array.from(table.querySelectorAll('th')).map((th) => th.textContent)).toEqual([
        'Height',
        'Age · txs',
        'Miner',
        'Fees, CKB',
        'Size · cycles',
      ]);
      expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(608);
      const [first] = Array.from(table.querySelectorAll('tbody tr'));
      expect(first.querySelector('td')).toHaveTextContent(/^#20,828,8940x0000…d2de$/);
      // T5 in the 640 table too: the height leads at 13 px bright.
      expect(within(first as HTMLElement).getByRole('link', { name: '#20,828,894' })).toHaveClass(
        'text-body',
        'text-text-bright',
        'leading-row'
      );
      // Regression (the lead's Chrome check, 2026-11-18): a block number is one figure and never
      // breaks inside itself ("#21,045,84" over "5" in a Height cell 0.4 px short). Re-pinned on
      // purpose (the lead's ruling on review fb-tests 5: a rule of the table, not of one link): the
      // link's own whitespace-nowrap went; no cell of a list table lets a word or a figure break
      // where it overflows, so nothing on the number's path to its cell can cut it.
      const height = within(first as HTMLElement).getByRole('link', { name: '#20,828,894' });
      for (let el: HTMLElement | null = height; el && el.tagName !== 'TR'; el = el.parentElement)
        expect(el.className).not.toMatch(/break-words|break-all|overflow-wrap:anywhere/);
      expect(cellFaults(table).filter((f) => f.rule === 'figure breaks')).toEqual([]);
      expect(first).toHaveTextContent('0.05060000');
      expect(first).toHaveTextContent('3,917 B6,831,940');
      expect(screen.queryByTestId('block-row-mobile')).toBeNull();
    });

    // PA-02 (UI review 2026-11-14): the legend under the h1 explains the wide table's Contents
    // glyphs (◆ cellbase, ■ a transaction by its bytes, ● a proposal) and its two-part size bar.
    // The 608 px table and the phone rows write the same facts in figures and draw neither. The
    // glyphs cannot join the phone row's head line: at 360 the text column beside the stone is
    // 292 px, and the height (11 characters at 13 px, 85.8 px), the 112 px glyph and "999 txs ·
    // 59m ago" (17 at 12 px, 122.4 px) with two 12 px gaps make 344.2. So the legend goes with the
    // form that draws what it explains: the wide table, chosen by the list's measured width.
    it('draws the legend only with the wide table, whose glyphs and bars it explains (PA-02)', async () => {
      atContainerWidth(358);
      const phone = render(<BlocksPage />);
      await screen.findAllByTestId('block-row-mobile');
      expect(screen.queryByLabelText('Legend')).toBeNull();
      phone.unmount();
      atContainerWidth(608);
      const narrow = render(<BlocksPage />);
      await screen.findByRole('table', { name: 'Latest blocks' });
      expect(screen.queryByLabelText('Legend')).toBeNull();
      narrow.unmount();
      // the wide table from its 1,193 px minimum (re-pinned, readability R4: 1,232 held it at 1,230;
      // F3-e: 1,260 → 1,284, every column its content; the coordinator's ruling: 1,193, its bar
      // heads in the tick form), so at the widest container, 1,232
      atContainerWidth(1232);
      render(<BlocksPage />);
      const table = await screen.findByRole('table', { name: 'Latest blocks' });
      expect(table.closest('[data-list-form]')).toHaveAttribute('data-list-form', 'table');
      expect(await screen.findByLabelText('Legend')).toHaveTextContent(
        'cellbase' + 'transaction, area ∝ bytes' + 'proposal' + 'block overhead · user transactions'
      );
    });
  });
});

// A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
// awaited; the section says it is loading, never "no …" or "failed", and resumes with the data.
describe('BlocksPage while its read is paused', () => {
  it('says it is loading, not nothing, then lists the blocks once back online', async () => {
    vi.mocked(api.getBlocks).mockResolvedValue(mockBlocksResponse);
    const { resume } = renderPaused(<BlocksPage />);
    expect(await screen.findByText('Loading blocks…')).toHaveAttribute('role', 'status');
    resume();
    expect(await screen.findByRole('table', { name: 'Latest blocks' })).toBeInTheDocument();
    expect(screen.queryByText('Loading blocks…')).toBeNull();
  });
});
