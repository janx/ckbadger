// F2 blocks: the Blocks / Block / BlockTxs board pieces, each on its own.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it, onTestFinished, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { inkOf, READING } from '@/__tests__/utils/ink-role';
import { RewardBreakdown, RewardPaidBy } from '@/components/blocks/reward-breakdown';
import { BlockTransactionsTable } from '@/components/blocks/block-transactions-table';
import { positionParts } from '@/components/blocks/block-position';
import { sentenceText } from '@/components/ui/block-link';
import { BlocksLimitsNote, ScaleHeader, resourceScale } from '@/components/blocks/block-visual';
import { formatTick } from '@/lib/chart-ticks';
import { ActivityRow } from '@/components/activity/activity-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { ListTable } from '@/components/ui/list-table';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { scriptLabel } from '@/components/address/address-history';
import type { Transaction } from '@/lib/api';
import {
  atContainerWidth,
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { cellFaults } from '@/__tests__/utils/list-table-cells';
import { containerContentPx, SCREENS } from '@/__tests__/utils/tailwind-box';
import { activityRead } from '@/__tests__/utils/activity-read';

const rewardBlock = {
  number: 20_827_932,
  miningReward: '68432120795',
  miningRewardTxHash: `0x${'c7'.repeat(32)}`,
  rewardBreakdown: {
    primary: '65142942227',
    secondary: '3286129910',
    committed: '3036000',
    proposal: '12658',
    secondaryIssuance: '41691483025',
  },
  rewardPaidAtBlock: 20_827_943,
  rewardFinalizesAt: 20_827_943,
  minerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgp909qk',
  minerPool: null as string | null,
  totalFees: '5060000',
  transactionsCount: 4,
};
const wrap = (ui: React.ReactElement) => render(<MemoryRouter>{ui}</MemoryRouter>);

/**
 * The block page's main column at a window (review fb-tests NOTE-3: read from the page, not a
 * literal). Re-pinned on purpose (lane D2, PA-13): the facts aside that stood beside the block's
 * contents became the head's status strip, so app/blocks/[id]/client-page.tsx sets the
 * transactions in tier 1's one column, in `main.container`, with no grid track, no aside and no
 * inset between (DetailLayout's tier-1 wrapper and the contents' `min-w-0` div add none): the
 * container's content width. (It read the first track of `grid min-w-0 gap-10
 * lg:grid-cols-[minmax(0,1fr)_240px]`: the content width less the 240 px aside and the 40 px gap.)
 */
function blockPageColumn(window: number): number {
  const page = readFileSync(resolve(__dirname, '../../app/blocks/[id]/client-page.tsx'), 'utf8');
  expect(page, 'no grid track beside the block’s contents').not.toMatch(
    /lg:grid-cols-\[[^\]]*px\]/
  );
  expect(page.indexOf('<aside'), 'no aside beside the block’s contents').toBe(-1);
  const table = page.indexOf('<BlockTransactionsTable');
  expect(table, 'the transactions table').toBeGreaterThan(-1);
  expect(SCREENS.find(([name]) => name === 'lg')![1]).toBeLessThanOrEqual(window);
  return containerContentPx(window);
}

describe('RewardBreakdown', () => {
  it('refuses shares that do not add up to the reward', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      wrap(<RewardBreakdown block={{ ...rewardBlock, miningReward: '68432120796' }} />)
    ).toThrow('do not add up');
  });
  it('draws only the non-zero shares, each at least 3 px', () => {
    wrap(
      <RewardBreakdown
        block={{
          ...rewardBlock,
          miningReward: '68429072137',
          rewardBreakdown: { ...rewardBlock.rewardBreakdown, committed: '0', proposal: '0' },
        }}
      />
    );
    const parts = screen.getByRole('img').querySelectorAll('[data-reward-part]');
    expect(Array.from(parts).map((p) => p.getAttribute('data-reward-part'))).toEqual([
      'primary',
      'secondary',
    ]);
    // Re-pinned on purpose (lane P1 follow-up; review chain-a block-quiet): a fee share of 0 is not
    // listed either (it was: "Proposal fees, 40%" stood with nothing in it); the block says once
    // that it earned no fees
    expect(screen.queryByText('Proposal fees, 40%')).toBeNull();
    expect(screen.getByText('This block earned no fees.')).toBeInTheDocument();
  });
  // Readability R4 (lane B7, the fix round): a share's note is a sentence; at 1280 the secondary
  // one ran 108 characters a line across the dl's last column. Each stands on the measure.
  it('sets each share’s note on the measure', () => {
    wrap(<RewardBreakdown block={rewardBlock} />);
    const notes = [
      screen.getByText('the whole primary issuance of this block'),
      screen.getByText(
        /^the miners' share of this block's .* secondary issuance; the rest goes to/
      ),
      screen.getByText(/in fees of the/),
    ];
    for (const note of notes) {
      expect(note.tagName).toBe('DD');
      expect(note).toHaveClass('max-w-measure');
      // lane P1, the ink and size roles: each note is a sentence, the reading ink (it was quiet)
      expect(inkOf(note)).toBe(READING);
    }
  });
  // Lane P1 follow-up (review chain-a block-quiet: "Committed fees, 60% 0 of 0 in fees of the 0
  // transactions committed here"): a fee row with nothing to say is not drawn, and a block whose
  // reward holds no fee says so once.
  it('draws no fee row with nothing to say, and says once that the block earned no fees', () => {
    const quiet = {
      ...rewardBlock,
      miningReward: '68429072137',
      rewardBreakdown: { ...rewardBlock.rewardBreakdown, committed: '0', proposal: '0' },
      totalFees: '0',
      transactionsCount: 1,
    };
    wrap(<RewardBreakdown block={quiet} />);
    expect(screen.queryByText('Committed fees, 60%')).toBeNull();
    expect(screen.queryByText('Proposal fees, 40%')).toBeNull();
    expect(screen.getAllByText('This block earned no fees.')).toHaveLength(1);
    expect(document.body).not.toHaveTextContent('0 of 0');
  });
  it('draws a fee row only where it holds a fee', () => {
    const proposedOnly = {
      ...rewardBlock,
      miningReward: '68429084795',
      rewardBreakdown: { ...rewardBlock.rewardBreakdown, committed: '0' },
      totalFees: '0',
      transactionsCount: 1,
    };
    wrap(<RewardBreakdown block={proposedOnly} />);
    expect(screen.queryByText('Committed fees, 60%')).toBeNull();
    expect(screen.getByText('Proposal fees, 40%')).toBeInTheDocument();
    expect(screen.queryByText('This block earned no fees.')).toBeNull();
  });
  it('says where an unfinalized reward will be paid', () => {
    wrap(<RewardBreakdown block={{ ...rewardBlock, miningReward: null, rewardBreakdown: null }} />);
    expect(screen.getByText(/paid by the cellbase of/)).toHaveTextContent(
      'The reward is paid by the cellbase of #20,827,943; it is not indexed yet.'
    );
  });
  // B3 (readability 2026-11-18): the payee is the block's miner, named as every miner is
  // (MinerLink): by the pool pools.toml names, else by its address at the row size, the address
  // in the title either way.
  it('names the payee by its pool where pools.toml names one, else by its address', () => {
    const pooled = { ...rewardBlock, minerPool: 'Example Pool' };
    const { unmount } = wrap(<RewardBreakdown block={pooled} />);
    const pool = screen.getByRole('link', { name: 'Example Pool' });
    expect(screen.getByTestId('reward-total')).toContainElement(pool);
    expect(pool.getAttribute('href')).toMatch(new RegExp(`/address/${rewardBlock.minerAddress}$`));
    // re-pinned (lane P1 follow-up): the title names the pool whole, then the address
    expect(pool).toHaveAttribute('title', `Example Pool · ${rewardBlock.minerAddress}`);
    // lane P1 follow-up (item 3): the payee is a name inside the line, the inline look, no ink of
    // MinerLink's own
    expect(pool).toHaveClass('underline');
    expect(pool).not.toHaveClass('text-text-bright');
    unmount();
    const unnamed = { ...rewardBlock, minerPool: null };
    wrap(<RewardBreakdown block={unnamed} />);
    expect(screen.getByTestId('reward-total')).toHaveTextContent(/ to ckb1…qgp909qk$/);
    expect(screen.queryByRole('link', { name: 'Example Pool' })).toBeNull();
  });
  it('names the paying cellbase and its tx', () => {
    wrap(<RewardPaidBy block={rewardBlock} />);
    expect(screen.getByRole('link', { name: '#20,827,943' }).getAttribute('href')).toMatch(
      /\/blocks\/20827943$/
    );
  });
});

// The position sentence as text: the parts the page renders, read as one line (lane E3 moved this
// composition here when its last caller in the app went).
const positionSentence = (
  block: Parameters<typeof positionParts>[0],
  links: Parameters<typeof positionParts>[1]
) => sentenceText(positionParts(block, links));

describe('positionSentence', () => {
  const block = { number: 932, proposalsCount: 2, unclesCount: 1 };
  it('reads the incoming and outgoing proposal links and the uncles', () => {
    expect(
      positionSentence(block, [
        { from: 930, to: 932 },
        { from: 932, to: 934 },
      ])
    ).toBe('committed what #930 proposed, proposed what #934 committed, and included 1\u00a0uncle');
  });
  it('says what is missing instead of inventing a link', () => {
    expect(positionSentence({ number: 5, proposalsCount: 0, unclesCount: 0 }, [])).toBe(
      'committed no proposed transactions, proposed nothing, and included no uncle'
    );
    expect(positionSentence({ number: 5, proposalsCount: 3, unclesCount: 2 }, [])).toBe(
      'committed no proposed transactions, proposed 3\u00a0transactions, none committed yet, and included 2\u00a0uncles'
    );
  });
});

describe('BlockTransactionsTable', () => {
  const tx = (index: number, over: Partial<Transaction> = {}): Transaction =>
    ({
      hash: `0x${String(index).repeat(64)}`,
      blockNumber: 7,
      index,
      inputsCount: 1,
      outputsCount: 1,
      fee: '0',
      txSize: 300,
      cycles: 0,
      cyclesStatus: 'complete',
      outputCapacity: '100',
      proposalOrigin: index ? { blockNumber: 5, distance: 2, uncle: null } : null,
      isCellbase: index === 0,
      ...over,
    }) as unknown as Transaction;
  // Lane P1 (the F3-e note: counts written ungrouped): a transaction's inputs are a count, grouped
  // as every count is ("13,568", the most a block holds, MAX_TX_INPUTS).
  it('groups a transaction’s inputs as every count (P1)', () => {
    const { container } = wrap(
      <BlockTransactionsTable
        transactions={[tx(1, { inputsCount: 13568, outputsCount: 2 })]}
        blockNumber={7}
      />
    );
    expect(container).toHaveTextContent('13,568');
    expect(container).not.toHaveTextContent('13568');
  });

  it('fails on a transaction from another block', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      wrap(<BlockTransactionsTable transactions={[tx(0, { blockNumber: 8 })]} blockNumber={7} />)
    ).toThrow('is not in block 7');
  });
  // Readability F3-e, the coordinator's ruling: budgeted honestly with the lane's levers (the bar
  // heads in the tick form on 48 px tracks, no spare, the hash alone at the row size), the table
  // needs 1,086 px; the block page gives it its main column at 1440, so there it is its rows, by
  // ListTable's rule (whether the page gives it its full width is the chain page lane's call).
  // Re-pinned on purpose (review fb-tests 6, the lead's ruling): Cells holds the most inputs a
  // transaction holds, "13568" (106 → 114): 1,094 px (renamed: "1,086 px").
  // Re-pinned on purpose (review fb-tests NOTE-3, the lead's ruling tN3): the main column is read
  // from the page's classes (`blockPageColumn`), 952 px; the literal 968 was 1,248 − 240 − 40, the
  // container before its gutter became 24 px from lg (renamed: "its rows at the 968 px column").
  // Re-pinned on purpose (lane D2; renamed, it was "…, its rows at the page’s main column"): the
  // facts aside went to the head's status strip, so the main column is the page's 1,232 px at
  // 1280 and the table is a table there.
  // Re-pinned on purpose (lane P1 follow-up, item 6; renamed, it was "… 1,094 px …"): the inputs
  // are written grouped (c6f9ea20), "13,568", so Cells 114 → 122 and the table 1,102 px.
  it('budgets every column from what it holds: 1,102 px, a table in the page’s main column', () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      // a block's index, four digits as written
      ceil(at('9999', 13) + 16),
      // the hash at the row size
      ceil(at('0x01010101…010101', 13) + 16),
      // "#99,999,999" and " +10 · uncle" (the proposal window is 2 to 10 blocks)
      ceil(at('#99,999,999 +10 · uncle', 13) + 16),
      // the most inputs a transaction holds (597,000 bytes of 44-byte inputs: 13,568), grouped as
      // written, "13,568", the arrow between its 6 px margins, four digits of outputs, "9,999"
      ceil(at('13,568', 13) + 6 + at('→', 13) + 6 + at('9,999', 13) + 16),
      // the genesis transactions' 33.6 billion CKB: eleven whole digits, every decimal
      ceil(at('99,999,999,999.99999999', 13) + 16),
      // a fee under 100 CKB
      ceil(at('99.99999999', 13) + 16),
      // a 48 px track, the bar's 10 px gap and genesis's scale, "1,500,000"
      ceil(48 + 10 + at('1,500,000', 13) + 16),
      // a 48 px track and a scale under a billion cycles, "750,000,000"; the last column
      ceil(48 + 10 + at('750,000,000', 13)),
    ];
    expect(budgets).toEqual([48, 149, 196, 122, 196, 102, 145, 144]);
    const sum = budgets.reduce((s, w) => s + w, 0);
    expect(sum).toBe(1102);
    const { unmount } = wrap(
      <BlockTransactionsTable
        transactions={[tx(0), tx(1, { cycles: 1000, txSize: 700 })]}
        blockNumber={7}
      />
    );
    const table = screen.getByRole('table');
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(Number.parseFloat(table.style.minWidth)).toBe(sum);
    const heads = Array.from(table.querySelectorAll('thead th'));
    for (const [i, values] of [
      [6, [300, 700]],
      [7, [0, 1000]],
    ] as const) {
      const ticks = heads[i].querySelector('[data-scale-ticks]') as HTMLElement;
      const scale = resourceScale([...values]);
      expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual([
        '0',
        formatTick(scale, scale),
      ]);
      expect(ticks.style.width).toBe('48px');
    }
    unmount();
    for (const [width, form] of [
      [1232, 'table'],
      [1102, 'table'],
      [1101, 'rows'],
      [blockPageColumn(1280), 'table'],
    ] as const) {
      atContainerWidth(width);
      const { container, unmount: drop } = wrap(
        <BlockTransactionsTable transactions={[tx(0)]} blockNumber={7} />
      );
      expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
      drop();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  // Re-pinned on purpose (readability F3-e, the coordinator's ruling on the block page's table): the
  // columns' minimums were literals for 12 / 13 px text (32 + 140 + 120 + 70 + 140 + 104 + 160 +
  // 172 = 938 px, which the 968 px main column at 1440 held); budgeted from what they hold they are
  // 1,086 px, so in that column the table takes its rows, by ListTable's rule, 118 px short
  // (renamed: it was "fits the block page main column at 1440").
  // Restored as the property (review fb-tests 3, the lead's ruling: a property that no longer
  // holds is not rewritten into a number that passes): the table fits the block page's main column
  // at 1440 and at 1280; where it does not, the case runs and pins its shortfall in KNOWN_FAULTS,
  // the chain page lane's (P1) to fix. 1,094 px since this lane counts 13,568 inputs (review
  // fb-tests 6); the table is not re-budgeted here and the page's layout is not this lane's.
  // Re-pinned on purpose (review fb-tests NOTE-3, the lead's ruling tN3): the column was a literal,
  // 968, so a fix on the page's side could not fail the entry; it is read from the page's classes
  // now, 952 px (1,232 − 240 − 40), and the shortfall is 142 (it was 126 against the literal).
  // Lane D2: the known fault is fixed and its entry removed (the list only shrinks: 1 → 0). The
  // facts aside became the head's status strip (PA-13), so the page gives the table its full
  // 1,232 px at 1440 and 1280, 138 px more than its 1,094 (130 more than its 1,102 since the lane
  // P1 follow-up budgets the inputs grouped).
  const BLOCK_PAGE_COLUMN = [1440, 1280].map((window) => ({
    window,
    column: blockPageColumn(window),
  }));
  const KNOWN_FAULTS: readonly {
    case: string;
    column: number;
    shortfall: number;
    lane: string;
    why: string;
  }[] = [];
  it('fits the block page’s main column at 1440 and 1280, but for its pinned known fault', () => {
    expect(KNOWN_FAULTS).toHaveLength(0);
    expect(BLOCK_PAGE_COLUMN.map((c) => c.column)).toEqual([1232, 1232]);
    for (const { column } of BLOCK_PAGE_COLUMN) {
      atContainerWidth(column);
      const { container, unmount } = wrap(
        <BlockTransactionsTable transactions={[tx(0)]} blockNumber={7} />
      );
      const form = container.querySelector('[data-list-form]')!.getAttribute('data-list-form');
      const known = KNOWN_FAULTS.find((f) => f.column === column);
      if (known) {
        // the property fails by exactly the pinned shortfall: the rows, 126 px short
        expect(form).toBe('rows');
        unmount();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
        const drawn = wrap(<BlockTransactionsTable transactions={[tx(0)]} blockNumber={7} />);
        const minWidth = Number.parseFloat(drawn.container.querySelector('table')!.style.minWidth);
        expect(minWidth - column).toBe(known.shortfall);
        drawn.unmount();
      } else {
        expect([column, form]).toEqual([column, 'table']);
        unmount();
      }
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });
  // Table-rows R2: the bar heads are the shared ScaleHeader, at the geometry this table has always
  // drawn (a 96 px tick row 3 px under the name), so the lift changes no pixel.
  // Re-pinned on purpose (readability F3-e, the coordinator's ruling): the ticks in the tick form
  // on the 48 px track a count bar keeps (renamed: "96 px ticks").
  it('heads its bars with the shared ScaleHeader: 48 px ticks, 3 px under the name', () => {
    wrap(
      <BlockTransactionsTable
        transactions={[tx(0), tx(1, { cycles: 1000, txSize: 700 })]}
        blockNumber={7}
      />
    );
    const heads = Array.from(screen.getByRole('table').querySelectorAll('thead th'));
    const sizeScale = resourceScale([300, 700]);
    const cyclesScale = resourceScale([0, 1000]);
    const size = formatTick(sizeScale, sizeScale);
    const cycles = formatTick(cyclesScale, cyclesScale);
    expect(heads.map((h) => h.textContent)).toEqual([
      '#',
      'Transaction',
      'Proposed in',
      'Cells',
      'Output capacity',
      'Fee',
      `Size, bytes0${size}`,
      `Cycles0${cycles}`,
    ]);
    for (const [i, end] of [
      [6, size],
      [7, cycles],
    ] as const) {
      const ticks = heads[i].querySelector('[data-scale-ticks]') as HTMLElement;
      expect(ticks.style.width).toBe('48px');
      expect((ticks.parentElement as HTMLElement).style.gap).toBe('3px');
      expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual(['0', end]);
    }
    // Its row bars: 8 px, exact width, no minimum (300 of the size scale on 48 px).
    const fill = screen
      .getByRole('table')
      .querySelector('tbody tr [data-segment="size"]') as HTMLElement;
    expect(fill.style.width).toBe(
      `${Math.round((300 / resourceScale([300, 700])) * 48 * 100) / 100}px`
    );
    expect((fill.parentElement as HTMLElement).style.height).toBe('8px');
  });
  // Review fb-tests 6 (the lead's ruling): a value one glyph past its bound, and what the table
  // does with it today, for the lists page lane to rule on. A fee of 100 CKB, "100.00000000" (12
  // glyphs at 13 px, 93.6; the bound "99.99999999", 85.8), in the Fee column (102 px, its box 86):
  // one figure on one line, 7.6 px wider than its box, start-aligned past its right edge (CSS Text
  // 3) into its own 16 px gap, short of the Size column.
  it('holds a fee one glyph past its bound on one line, 7.6 px into its gap', () => {
    wrap(<BlockTransactionsTable transactions={[tx(0, { fee: '10000000000' })]} blockNumber={7} />);
    const table = screen.getByRole('table');
    const column = Number.parseFloat(
      (table.querySelectorAll('col')[5] as HTMLElement).style.minWidth
    );
    expect(column).toBe(102);
    const fee = table.querySelectorAll('tbody td')[5] as HTMLElement;
    expect(fee).toHaveTextContent(/^100\.00000000$/);
    expect(fee).toHaveClass('whitespace-nowrap', 'text-right');
    expect(cellFaults(table).filter((f) => f.rule === 'figure breaks')).toEqual([]);
    const past = '100.00000000'.length * 7.8 - (column - 16);
    expect(past).toBeCloseTo(7.6, 10);
    expect(past).toBeLessThan(16);
  });
  it('writes "+distance" beside the proposing block', () => {
    wrap(
      <BlockTransactionsTable transactions={[tx(0), tx(1, { cycles: 1000 })]} blockNumber={7} />
    );
    const rows = screen.getByRole('table').querySelectorAll('tbody tr');
    expect(rows[1]).toHaveTextContent('#5 +2');
    expect(rows[0]).toHaveTextContent('cellbase');
    // lane P1, the ink and size roles: a table cell's content reads in the reading ink, the
    // cellbase's row as every other (the row, its "cellbase" and the distance were quiet)
    expect(rows[0]).not.toHaveClass('text-text-dim');
    expect(inkOf(within(rows[0] as HTMLElement).getByText('cellbase'))).toBe(READING);
    expect(inkOf(within(rows[1] as HTMLElement).getByText(/\+2/))).toBe(READING);
  });
  it('says the genesis block’s origin and a cycles cell with no figure in the reading ink (lane P1)', () => {
    wrap(
      <BlockTransactionsTable
        transactions={[tx(0, { blockNumber: 0, cycles: null, cyclesStatus: 'pending' })]}
        blockNumber={0}
      />
    );
    const [row] = screen.getByRole('table').querySelectorAll('tbody tr');
    expect(inkOf(within(row as HTMLElement).getByText('genesis'))).toBe(READING);
    expect(inkOf(within(row as HTMLElement).getByText('Not yet run'))).toBe(READING);
  });
  it('is a three-line phone row at 358 px: hash and output · proposal · the quiet figures', () => {
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    wrap(
      <BlockTransactionsTable
        transactions={[tx(0), tx(1, { cycles: 1000, fee: '30000', inputsCount: 2 })]}
        blockNumber={7}
      />
    );
    expect(screen.queryByRole('table')).toBeNull();
    const [base, user] = screen.getAllByTestId('block-tx-row-mobile');
    // Re-pinned on purpose (lane D4, the phone-rows board): the output is named and takes line 1's
    // right ("output 0.000001 CKB", was bare on line 2), the proposal has line 2, the index, the
    // cells and the fee are the quiet line; bytes and cycles left the phone row (it was "10x1111…1111"
    // beside "proposed in #5 +2", "0.000001 CKB" beside "fee 0.0003 CKB", "2 → 1 cell" beside "300 B
    // · 1,000 cycles"; a cellbase read "proposed in cellbase").
    expect(phoneLines(user).map((l) => [l.key, l.left, l.right])).toEqual([
      // U-09 (re-pinned): the phone row's hash at the tight size
      ['identity', '0x1111…1111', 'output 0.000001 CKB'],
      ['proposal', 'proposed in #5 +2', null],
      // a phone row is the summary form (readability R5: was padded to 8 decimals)
      ['figures', 'index 1 · 2 → 1\u00a0cell · fee 0.0003 CKB', null],
    ]);
    expect(phoneLines(base)[1].left).toBe('cellbase');
    // re-pinned on purpose (lane P1, the ink and size roles: a table's content is the reading ink,
    // the cellbase's row as every other; its phone row was set in the quiet ink)
    expect(base.closest('[role="listitem"]')).not.toHaveClass('text-text-dim');
  });
  // Lane D4 (the phone-rows board): a block's transaction on a phone is the row's three parts. Line 1
  // the hash and, at the right, its output named ("output N CKB"; the table's Output capacity);
  // line 2 where it was proposed; line 3 quiet, its index in the block, its cells and its fee, each
  // named (the fee left line 2's right, where a long proposal dropped it under and rows took two
  // shapes). Bytes and cycles leave the phone row (the transaction's page, the table, the cards
  // from 560 px). A cellbase says it is one on line 2, and pays no fee. Its first line states the
  // widest output a transaction makes, WIDEST_TX_OUTPUT's eleven whole digits and the unit at the
  // meta step: 297 of 328 px.
  it('is the board’s row at 358 px: hash and output · proposal · index, cells and fee', () => {
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    wrap(
      <BlockTransactionsTable
        transactions={[
          tx(0),
          tx(1, { cycles: 1000, fee: '30000', inputsCount: 2 }),
          tx(2, {
            outputCapacity: '9999999999999999999',
            proposalOrigin: {
              blockNumber: 5,
              distance: 10,
              uncle: { number: 4, hash: '0x' + '9'.repeat(64) },
            },
          }),
        ]}
        blockNumber={7}
      />
    );
    const [base, user, widest] = screen.getAllByTestId('block-tx-row-mobile');
    expect(phoneLines(user).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['identity', 'head', '0x1111…1111', 'output 0.000001 CKB'],
      ['proposal', 'body', 'proposed in #5 +2', null],
      ['figures', 'quiet', 'index 1 · 2 → 1\u00a0cell · fee 0.0003 CKB', null],
    ]);
    expect(dottedItems(user.querySelector('[data-line="figures"]')!)).toEqual([
      'index 1',
      '2 → 1\u00a0cell',
      'fee 0.0003 CKB',
    ]);
    expect(user.textContent).not.toMatch(/300 B|cycles/);
    expect(phoneLines(base).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x0000…0000', 'output 0.000001 CKB'],
      ['proposal', 'cellbase', null],
      ['figures', 'index 0 · 0 → 1\u00a0cell', null],
    ]);
    // re-pinned on purpose (lane P1, the ink and size roles): the cellbase's row is not quiet
    expect(base.closest('[role="listitem"]')).not.toHaveClass('text-text-dim');
    // every figure after its name, the name in the quiet ink
    const labels = Array.from(user.querySelectorAll('[data-role="figure-label"]')).map(
      (l) => l.textContent
    );
    expect(labels).toEqual(['output', 'proposed in', 'index', 'fee']);
    // the widest output the bound claims, whole CKB from 1,000 up: drawn within the stated line
    expect(phoneLines(widest)[0].right).toBe('output 99,999,999,999 CKB');
    expect(phoneLines(widest)[1].left).toBe('proposed in #5 +10 · uncle');
    expect(statedHeadPx(widest)).toBe(297);
    expect(phoneHeadPx(widest, 360)).toBe(statedHeadPx(widest));
  });
  it('keeps the bytes and the cycles on the quiet line of the 608 px card', () => {
    atContainerWidth(608);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    wrap(<BlockTransactionsTable transactions={[tx(1, { cycles: 1000 })]} blockNumber={7} />);
    const [row] = screen.getAllByTestId('block-tx-row-mobile');
    // Re-pinned on purpose (lane D4): the card at 608 px is the phone row's three parts, its quiet
    // line keeping the bytes and the cycles where no table stands (it was two lines: "10x1111…1111"
    // beside a bare "0.000001 CKB", then "proposed in #5 +2 · fee 0 CKB" beside the figures).
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x1111…1111', 'output 0.000001 CKB'],
      ['proposal', 'proposed in #5 +2', null],
      // Consistency plan C4: a zero fee is "0 CKB" (was "0.00000000 CKB").
      ['figures', 'index 1 · 1 → 1\u00a0cell · fee 0 CKB · 300 B · 1,000 cycles', null],
    ]);
  });
});

// Lane D4 follow-up (the coordinator's note): the block a transaction was proposed in is a link
// inside a line, on the phone row and in the table alike: the inline look, underlined.
describe('the block a transaction was proposed in is an inline link (lane D4 follow-up)', () => {
  it('on the phone row', () => {
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    wrap(
      <BlockTransactionsTable
        transactions={[
          {
            hash: `0x${'1'.repeat(64)}`,
            blockNumber: 7,
            index: 1,
            inputsCount: 1,
            outputsCount: 1,
            fee: '0',
            txSize: 300,
            cycles: 0,
            cyclesStatus: 'complete',
            outputCapacity: '100',
            proposalOrigin: { blockNumber: 5, distance: 2, uncle: null },
            isCellbase: false,
          } as unknown as Transaction,
        ]}
        blockNumber={7}
      />
    );
    const link = screen.getByRole('link', { name: '#5' });
    expect(link).toHaveClass('underline');
  });
});

describe('Blocks list pieces', () => {
  // re-pinned on purpose (readability F3-e, the coordinator's ruling): the tick form, "8M" (it was
  // "8,000,000"), on the 48 px track a count bar keeps
  it('heads a bar column with 0 … scale as wide as its track', () => {
    render(<ScaleHeader label="Cycles" scale={8_000_000} trackWidth={48} />);
    expect(screen.getByText('8M').parentElement).toHaveStyle({ width: '48px' });
    expect(() => ScaleHeader({ label: 'x', scale: 0 })).toThrow('invalid column scale');
  });
  it('rejects a page whose blocks disagree on the consensus limits', () => {
    const row = {
      number: 1,
      size: 600,
      cycles: null,
      maxBlockBytes: 597_000,
      maxBlockCycles: '3500000000',
    };
    expect(() =>
      BlocksLimitsNote({ blocks: [row, { ...row, number: 2, maxBlockBytes: 1 }] })
    ).toThrow('consensus limits differ');
    render(<BlocksLimitsNote blocks={[row]} />);
    expect(screen.getByText(/cycles not run yet for these blocks/)).toBeInTheDocument();
  });
});

describe('additive shared props (F2 blocks)', () => {
  it('ActivityRow takes its evidence line from the page', () => {
    wrap(
      <ActivityRow
        activity={activityRead({
          txHash: `0x${'ab'.repeat(32)}`,
          blockNumber: 7,
          timestamp: '1700000000000',
          isCellbase: true,
          protocolActions: [],
          typeCalls: [],
          lockCalls: [],
          itemTypes: [],
          itemTypesUnread: null,
          participants: [],
        })}
        headline="Cellbase"
        evidence={<>#0 · tx</>}
      />
    );
    expect(document.querySelector('[data-role="evidence"]')).toHaveTextContent(/^#0 · tx$/);
  });
  it('CkbAmount drops the unit when the header names it', () => {
    const { container } = render(
      <CkbAmount form="column" shannons={BigInt(150000000)} unit={false} />
    );
    expect(container).toHaveTextContent(/^\+1\.50000000$/);
  });
  it('ListTable heads a column with its rich header', () => {
    render(
      <ListTable
        label="t"
        rows={[1]}
        rowKey={String}
        columns={[{ key: 'a', label: 'A', header: <b>A rich</b>, minWidth: 10, render: String }]}
        mobileRow={String}
      />
    );
    expect(screen.getByRole('columnheader')).toHaveTextContent('A rich');
  });
});

describe('Address board pieces (F2 blocks)', () => {
  it('CapacityPartition keeps small parts visible and leaves a zero part out of the bar', () => {
    render(
      <CapacityPartition
        subject="A"
        total="1000"
        occupied="1"
        free="0"
        daoFree="999"
        minSegmentWidth={3}
        daoFreeNote="free part of 1 deposit"
      />
    );
    const parts = Array.from(document.querySelectorAll('[data-part]')) as HTMLElement[];
    expect(parts.map((p) => p.dataset.part)).toEqual(['occupied', 'DAO free']);
    expect(parts[0].style.minWidth).toBe('3px');
    expect(screen.getByText(/free part of 1 deposit/)).toBeInTheDocument();
    expect(screen.getByText('free')).toBeInTheDocument();
  });
  it('scriptLabel maps the closed label set and refuses an unknown one', () => {
    expect(scriptLabel('NervosDAO', '0x1')).toEqual({ name: 'Nervos DAO', mark: 'dao' });
    expect(() => scriptLabel('Mystery', '0x1')).toThrow('unknown script label "Mystery"');
  });
});
