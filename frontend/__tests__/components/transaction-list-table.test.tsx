import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import {
  CELLBASE_LIST_MIN_PX,
  CellbaseListTable,
  TRANSACTION_LIST_MIN_PX,
  TransactionListTable,
} from '@/components/transaction/transaction-list-table';
import { TRANSACTIONS_FORM_PX } from '@/app/transactions/page';
import type { Transaction } from '@/lib/api';
import {
  atContainerWidth,
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { inkOf } from '@/__tests__/utils/ink-role';
import {
  expectBefore,
  expectOneRowSeal,
  parsed,
  SEAL_INKS,
  sealHash,
} from '@/__tests__/utils/seals';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { PHONE_LINE_PX, ROW_LINE_PX } from '@/lib/type-metrics';
import { drawSeal, type SealId } from '@/lib/seal/stones';
import { cellFaults, faultLines } from '@/__tests__/utils/list-table-cells';
import { CLOCK_BEHIND } from '@/lib/format-age';
import {
  boxPx,
  containerContentPx,
  gapPx,
  insetsPx,
  textRunPx,
} from '@/__tests__/utils/tailwind-box';
import { activityRead } from '@/__tests__/utils/activity-read';

const base: Omit<Transaction, 'hash' | 'activity' | 'isCellbase'> = {
  blockNumber: 20829775,
  blockHash: `0x${'cd'.repeat(32)}`,
  index: 1,
  inputsCount: 3,
  outputsCount: 3,
  fee: '30000',
  value: '0',
  outputCapacity: '100000000000',
  txSize: 903,
  cycles: 1704957,
  cyclesStatus: 'complete',
  timestamp: '2026-10-25T00:00:00Z',
};
const scriptCall: Transaction = {
  ...base,
  hash: `0x00de304b${'00'.repeat(25)}25cbbc`,
  isCellbase: false,
  activity: activityRead({
    txHash: `0x00de304b${'00'.repeat(25)}25cbbc`,
    blockNumber: 20829775,
    txIndex: 1,
    timestamp: '2026-10-25T00:00:00Z',
    isCellbase: false,
    protocolActions: [],
    lockCalls: [],
    recognizedTypes: [],
    typeCalls: [
      {
        typeCodeHash: '0xtime',
        typeHashType: 'type',
        typeArgs: '0x00',
        scriptHash: '0xs1',
        scriptName: '.bit Time Index State',
      },
      {
        typeCodeHash: '0xinfo',
        typeHashType: 'type',
        typeArgs: '0x00',
        scriptHash: '0xs2',
        scriptName: '.bit Time Info',
      },
    ],
    participants: [feedSamples.objectTransfer.participants[2]],
  }),
};
const transfer: Transaction = {
  ...base,
  hash: `0xcfaebc55${'00'.repeat(25)}e6dd61`,
  isCellbase: false,
  value: '76729874184',
  activity: activityRead({ ...feedSamples.eightParties, typeCalls: [], protocolActions: [] }),
};
const cellbase: Transaction = {
  ...base,
  hash: `0xc89d2d25${'00'.repeat(25)}9a2cd7`,
  index: 0,
  isCellbase: true,
  inputsCount: 1,
  outputsCount: 1,
  fee: '0',
  value: '0',
  outputCapacity: '55961739927',
  txSize: 360,
  cycles: 0,
  activity: null,
  pays: { number: 20829764, hash: '0x' + '64'.repeat(32) },
};

/** Every class of an element and its descendants that fades it (`opacity-30`, `opacity-[.55]`). */
function fading(element: Element): string[] {
  return [element, ...element.querySelectorAll('*')].flatMap((e) =>
    [...e.classList].filter((c) => /(^|:)opacity-(?!0$|100$)/.test(c))
  );
}

/**
 * Block 0's cellbase as the list lists it (the live API, `/transactions?cursor=1:0`): the genesis
 * issuance, 33,600,000,000 CKB, and no block whose reward it pays. Its other fields are this
 * fixture's.
 */
const genesis: Transaction = {
  ...cellbase,
  hash: `0x${'0e'.repeat(32)}`,
  blockNumber: 0,
  outputCapacity: '3360000000000000000',
  txSize: 1235438,
  pays: null,
};
/** A cellbase of a chain's first blocks: no reward to pay yet, and no CKB out. */
const early: Transaction = {
  ...cellbase,
  hash: `0x${'0a'.repeat(32)}`,
  blockNumber: 5,
  outputCapacity: '0',
  pays: null,
};

// UI review 2026-11-14 (DY-02, U-08, ruling R4): "set back" is the dim ink on every word, never
// element opacity; the stone keeps its own state's ink at full strength. A cellbase row was set
// back because it stood among the user transactions. Re-pinned on purpose (lists lane P3, plan
// §7.6): the cellbases are a list of their own, the subject of their rows, so a row is not set
// back there; the stone is pressed at full strength as before (renamed: "a cellbase row set back").
describe('a cellbase in its own list (R4, DY-02; lists lane P3)', () => {
  const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('is not set back: no quiet row, nothing faded, the stone pressed at full strength', () => {
    render(<CellbaseListTable transactions={[cellbase, genesis]} />);
    const [, base] = within(screen.getByRole('table', { name: 'Cellbases' })).getAllByRole('row');
    expect(base).not.toHaveClass('quiet');
    expect(fading(base)).toEqual([]);
    const stone = base.querySelector('[data-seal-state]') as HTMLElement;
    expect(stone).toHaveAttribute('data-seal-state', 'pressed');
    expect(fading(stone)).toEqual([]);
  });

  it('draws its phone row the same way', () => {
    atContainerWidth(358);
    render(<CellbaseListTable transactions={[cellbase]} />);
    const item = screen.getByTestId('transaction-row-mobile').closest('[role="listitem"]')!;
    expect(item).not.toHaveClass('quiet');
    expect(fading(item)).toEqual([]);
    expect(item.querySelector('[data-seal-state]')).toHaveAttribute('data-seal-state', 'pressed');
  });

  it('draws the quiet variant in globals.css: the dim ink on every word, the ghost and SVG kept', () => {
    const rule = css.match(/\.quiet,\s*\.quiet :where\(([^)]*\))\)\s*\{([^}]*)\}/);
    expect(rule, '.quiet rule').not.toBeNull();
    // the exclusions: a seal (its SVG paints by attribute) and the padding zeros
    expect(rule![1]).toContain('svg');
    expect(rule![1]).toContain('.text-text-ghost');
    expect(rule![2]).toContain('color: var(--c-text-dim)');
    expect(rule![2]).not.toMatch(/opacity|filter/);
  });

  it('refuses a cellbase among the user transactions, and a user transaction among the cellbases', () => {
    expect(() => TransactionListTable({ transactions: [cellbase] })).toThrow(
      `Transaction ${cellbase.hash} is a cellbase: the cellbases are a list of their own`
    );
    expect(() => CellbaseListTable({ transactions: [transfer] })).toThrow(
      `Transaction ${transfer.hash} is no cellbase`
    );
  });
});

describe('TransactionListTable (Transactions board, TX-25 / TX-26 / D5)', () => {
  // Re-pinned on purpose (lists lane P3): the size and the cycles bars left the list of user
  // transactions for What it did's width (a common row stands on one line at 1,232 px); the block's
  // head says what its two lines hold, the block and its age (it said "date")
  it('uses the board columns: the transaction, its block and age, what it did, value, fee, cells', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Transaction', 'Block · age', 'What it did', 'Value, CKB', 'Fee, CKB', 'Cells']);
  });

  // Re-pinned on purpose (lists lane P3): the bars and their ScaleHeader heads left the list (they
  // held a transaction's size and cycles on 48 px tracks, 2 px under the name); the figures are on
  // each transaction's page and in the cards from 560 px (the two tests below)
  it('heads no bar: no tick row and no track in the list', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const table = screen.getByRole('table');
    expect(table.querySelector('[data-scale-ticks]')).toBeNull();
    expect(table.querySelector('[data-track]')).toBeNull();
    expect(table).not.toHaveTextContent('1,704,957');
  });

  // Re-pinned on purpose (lists lane P3): it held the widest tick row of each bar head inside its
  // track; a transaction's size and cycles, at their widest, stand in the card's quiet line
  it('keeps a transaction’s size and cycles, at their widest, in the cards from 560 px', () => {
    atContainerWidth(608);
    render(
      <TransactionListTable transactions={[{ ...transfer, txSize: 597000, cycles: 750_000_000 }]} />
    );
    const [card] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(card)[2].left).toMatch(/ · 597,000 B · 750,000,000 cycles$/);
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  // Re-pinned on purpose (lists lane P3): it drew a 1,000-cycle bar at its exact width; the card
  // writes the exact figure, never a bar widened to a minimum
  it('writes a transaction’s cycles exactly in the cards, never rounded', () => {
    atContainerWidth(608);
    render(<TransactionListTable transactions={[{ ...transfer, cycles: 1000 }]} />);
    const [card] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(card)[2].left).toMatch(/ · 1,000 cycles$/);
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  // Readability 2026-11-18, lane B5: "What it did" was 215 px and its note was cut mid-word ("CKB
  // Transfer 2 participa", "Cellbase pays #21,042,6…", capture transactions n-02). The note now
  // wraps under the headline, part by part, and is never cut.
  it('never cuts what a transaction did: the note wraps, whole parts (B5)', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    for (const row of within(screen.getByRole('table')).getAllByRole('row').slice(1)) {
      const what = row.querySelectorAll('td')[2];
      expect(what.querySelector('.truncate')).toBeNull();
      // the headline and its note share one wrapping line box
      const line = what.querySelector('.flex-wrap') as HTMLElement;
      expect(line).not.toBeNull();
      expect(line.children).toHaveLength(2);
    }
    const [, call] = within(screen.getByRole('table')).getAllByRole('row');
    const note = call.querySelectorAll('td')[2].querySelector('[data-dotted]') as HTMLElement;
    expect(note).not.toHaveClass('whitespace-nowrap');
    expect(note).toHaveClass('flex-wrap');
  });

  // B5: it takes the width a common row needs from the hash (13 px of its 15.9 spare) and the
  // Cycles bar (18 px: its ticks say "2M", so its track is 56 px like Size's), and the table keeps
  // its 1,230 px. Headline + 8 px + note, beside the 16 px mark and its 8 px gap, and the 16 px
  // gap: "CKB Transfer" and "12 participants", "Cellbase" and "pays #21,042,681", on one line.
  // Re-pinned with readability R4 (lane F3, step 2): the table text is 13 px (7.8 px a glyph, was 7.2) and its key 14 px (8.4, was 7.8), so the table needs 1,286 px (was 1,230) and takes its row form in the
  // widest 1,232 px container; what each column holds is unchanged.
  // Re-pinned on purpose (readability F3-e, the flexible column's minimum): What it did takes what
  // the container gives beyond the table's 1,220 px; at its minimum it holds the longest DAO
  // headline whole and a note goes under its headline, and a common row is one line from a
  // container of 1,275 px (at 1,232, What it did has 216 px).
  // Re-pinned on purpose (the coordinator's ruling): with the Size bar on a 48 px track the list's
  // own sum is 1,203 px, so a common row is one line from a container of 1,258 px, and at 1,232
  // What it did has 233 (renamed: "from 1,275 px, beyond the 1,220").
  // Rewritten (review fb-tests 3, the lead's ruling): that test had become a threshold no page
  // gives (the list is never wider than 1,232) and a bare number. Two tests now: what jsdom can
  // state of the cell (the headline and its note may break, and where), and the property itself,
  // "a common row stands on one line", at every width the page gives the list, with the width
  // where it does not hold pinned as a known fault by its shortfall.
  it('lets a headline break between its parts and its words, and its note go under it', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    for (const row of within(table).getAllByRole('row').slice(1)) {
      const what = row.querySelectorAll('td')[2] as HTMLElement;
      expect(what.className).not.toMatch(/whitespace-nowrap|truncate|break-words/);
      // the headline and its note: two items of one wrapping line (the note goes under the
      // headline where the line is full), neither one unbreakable
      const line = what.querySelector('.flex-wrap') as HTMLElement;
      expect(line).toHaveClass('flex', 'min-w-0', 'flex-wrap');
      const [headline, note] = Array.from(line.children) as HTMLElement[];
      for (const part of [headline, note]) {
        expect(part).toHaveClass('min-w-0');
        expect(part).not.toHaveClass('whitespace-nowrap');
      }
      // a name inside a headline is a user's word with no bound (a .cell label in "ring pointer
      // changed for …"): it breaks anywhere rather than run out of the cell (lists lane P3, the
      // notes' logic N6)
      expect(headline).toHaveClass('[overflow-wrap:anywhere]');
      // the headline's separated parts (a protocol's name and its action) break between them first,
      // the separator with the part after it, then between the words of a part (Dotted's items)
      for (const item of headline.querySelectorAll('[data-dotted-item]'))
        expect(item).toHaveClass('min-w-0');
    }
    expect(faultLines(cellFaults(table))).toEqual([]);
  });

  // The page's real widths (app/transactions/page.tsx draws its lists in `main.container`, no inset
  // of its own between): Tailwind's container at each window the readability plan reads, from the
  // theme (tailwind-box's containerContentPx: 1,232 at 1280, 976, 736, 608, then the window less 32,
  // 358 and 328). The page changes its lists' form at its TRANSACTIONS_FORM_PX, 1,212 px, its pool's
  // sum (read from the page, its one source: review rules-tests NOTE-1), so only 1,232 draws the
  // table; every narrower width draws the phone rows, whose what-line wraps by design.
  // Re-pinned on purpose (lists lane P3): the list of user transactions is 898 px on its own (the
  // size and the cycles bars left it, 289 px; Value's bound is a value's alone, 8 px; Cells is the
  // last column, no gap after it, 16 px), so What it did keeps its 204 px minimum and the 314 px
  // the page's width is over the list's sum, 518, and has 538 px at 1,232 (it had 225). Every row
  // stands on one line: the four known faults went, two by the width (".bit Time" 430.2 px, "CKB
  // Transfer · 8 participants" 250.8) and two with the cellbases, a list of their own.
  const PAGE_FORM_PX = TRANSACTIONS_FORM_PX;
  const PAGE_VIEWPORTS = [1280, 1024, 768, 640, 390, 360] as const;
  const ROWS = [scriptCall, transfer];
  /**
   * Where "a row stands on one line" does not hold: pinned, so the list can only shrink. Each row's
   * line is read from the rendered row (review fb-tests SHOULD-1): the mark's 16 px box, its 8 px
   * gap, the headline's and the note's glyphs at 13 px (7.8), the 8 px between them and the cell's
   * 16 px gap; its shortfall is that line less What it did's width at 1,232. Emptied by the lists
   * lane P3 (it held four rows, 205.2, 25.8, 10.2 and 25.8 px short in 225).
   */
  const KNOWN_FAULTS: readonly { row: string; width: number; line: number; shortfall: number }[] =
    [];
  it('keeps each row on one line at the page’s widths: no known fault', () => {
    expect(KNOWN_FAULTS).toHaveLength(0);
    const widths = PAGE_VIEWPORTS.map(containerContentPx);
    expect(widths).toEqual([1232, 976, 736, 608, 358, 328]);
    const measured: { row: string; width: number; line: number; shortfall: number }[] = [];
    const lines: number[] = [];
    for (const [i, width] of widths.entries()) {
      const viewport = PAGE_VIEWPORTS[i];
      atContainerWidth(width);
      const { container, unmount } = render(
        <TransactionListTable transactions={ROWS} formWidth={PAGE_FORM_PX} />
      );
      const form = container.querySelector('[data-list-form]')!.getAttribute('data-list-form');
      if (width < PAGE_FORM_PX) {
        // the phone rows: the property is the table's
        expect([width, form]).toEqual([width, 'rows']);
      } else {
        expect(form).toBe('table');
        const cols = Array.from(container.querySelectorAll('col')).map((c) =>
          Number.parseFloat((c as HTMLElement).style.minWidth)
        );
        const sum = cols.reduce((s, w) => s + w, 0);
        // the list at the page's width: What it did its own 204 px minimum and what the page's
        // width is over the list's own sum, 1,212 − 898
        expect(TRANSACTION_LIST_MIN_PX).toBe(898);
        expect(cols[2]).toBe(204 + (PAGE_FORM_PX - TRANSACTION_LIST_MIN_PX));
        expect([sum, cols[2]]).toEqual([PAGE_FORM_PX, 518]);
        const what = width - (sum - cols[2]);
        expect(what).toBe(538);
        const trs = Array.from(container.querySelectorAll('tbody tr'));
        expect(trs).toHaveLength(ROWS.length);
        for (const tr of trs) {
          // the rendered row: its What it did cell, the mark's box and the line of headline + note
          const td = tr.querySelectorAll('td')[2];
          const cell = td.firstElementChild as HTMLElement;
          const [mark, both] = Array.from(cell.children) as HTMLElement[];
          const [headline, note] = Array.from(both.children) as HTMLElement[];
          const line =
            boxPx(mark, viewport)! +
            gapPx(cell, viewport) +
            textRunPx(headline.textContent!, headline, viewport) +
            gapPx(both, viewport) +
            textRunPx(note.textContent!, note, viewport) +
            insetsPx(td, viewport).box;
          lines.push(Math.round(line * 100) / 100);
          const shortfall = line - what;
          if (shortfall > 1e-9)
            measured.push({
              row: `${headline.textContent} · ${note.textContent}`.replace(/\s+/g, ' '),
              width,
              line: Math.round(line * 100) / 100,
              shortfall: Math.round(shortfall * 100) / 100,
            });
        }
      }
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
    // the two rows that were faults, each on one line now
    expect(lines).toEqual([430.2, 250.8]);
    expect(measured).toEqual(KNOWN_FAULTS);
  });

  /**
   * Where a row's figure does not stand inside its column's content box (the column less its 16 px
   * gap) at 1,232 px, the one width the page draws the table: pinned. Emptied by the lists lane P3:
   * it held block 0's cellbase, "reward 33,600,000,000.00000000", 70 px past Value; the cellbases
   * are a list of their own, block 0's words "the genesis issuance" and its 33.6 billion CKB in an
   * Output column bounded for it (the cellbases' test below).
   */
  const FIGURE_FAULTS: readonly { row: string; column: string; text: string; past: number }[] = [];
  it('holds every row’s figures inside their columns at 1,232 px: no known fault', () => {
    expect(FIGURE_FAULTS).toHaveLength(0);
    const viewport = 1280;
    atContainerWidth(containerContentPx(viewport));
    render(<TransactionListTable transactions={ROWS} formWidth={PAGE_FORM_PX} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const heads = within(table)
      .getAllByRole('columnheader')
      .map((h) => h.textContent);
    const cols = Array.from(table.querySelectorAll('col')).map((c) =>
      Number.parseFloat((c as HTMLElement).style.minWidth)
    );
    const names = ['script call', 'transfer'];
    expect(names).toHaveLength(ROWS.length);
    const past: { row: string; column: string; text: string; past: number }[] = [];
    for (const [r, tr] of Array.from(table.querySelectorAll('tbody tr')).entries())
      for (const column of ['Value, CKB', 'Fee, CKB', 'Cells']) {
        const at = heads.indexOf(column);
        expect(at, column).toBeGreaterThan(-1);
        const td = tr.querySelectorAll('td')[at] as HTMLElement;
        const text = td.textContent!;
        const over = textRunPx(text, td, viewport) - (cols[at] - insetsPx(td, viewport).box);
        if (over > 1e-9)
          past.push({ row: names[r], column, text, past: Math.round(over * 100) / 100 });
      }
    expect(past).toEqual(FIGURE_FAULTS);
  });

  // A cell holds its content (components/ui/list-table.tsx). The lead's Chrome reading of
  // /transactions at 1280 (2026-11-18): ".bit · recycle expired account", one nowrap run 234 px
  // wide in What it did's 193 px line beside its mark, drew 41 px past its content box and 25 px
  // over Value, on 3 of 20 live rows. The ruling: a headline is words; it breaks first between its
  // separated parts (the " · " belongs to the part after it), then between the words of a part
  // still too long, and the row grows taller.
  it('breaks a headline between its parts, then between its words, never past its cell', () => {
    const recycle: Transaction = {
      ...scriptCall,
      hash: `0x0bb1${'00'.repeat(29)}1e`,
      activity: activityRead({
        ...scriptCall.activity!,
        txHash: `0x0bb1${'00'.repeat(29)}1e`,
        protocolActions: [
          {
            protocol: 'dotbit',
            protocolName: '.bit',
            action: 'recycle_expired_account',
            metadata: { accountId: '0x' + 'da'.repeat(20) },
          },
        ],
      }),
    };
    render(<TransactionListTable transactions={[recycle, transfer]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const [, first, second] = within(table).getAllByRole('row');
    const headline = (row: HTMLElement) =>
      row.querySelectorAll('td')[2].querySelector('.flex-wrap')!.firstElementChild as HTMLElement;
    expect(headline(first)).toHaveTextContent('.bit · recycle expired account');
    expect(
      Array.from(headline(first).querySelectorAll('[data-dotted-item]')).map((p) => p.textContent)
    ).toEqual(['.bit', ' · recycle expired account']);
    expect(headline(second)).toHaveTextContent('CKB Transfer');
    for (const row of [first, second]) {
      expect(headline(row)).not.toHaveClass('whitespace-nowrap');
      expect(
        headline(row).querySelector('.whitespace-nowrap:not([data-dotted-separator])')
      ).toBeNull();
    }
    expect(faultLines(cellFaults(table))).toEqual([]);
  });

  // Review fb-tests 6 (the lead's ruling): a value one glyph past its bound, and what the table
  // does with it today, for the lists page lane to rule on (no mechanism is built here). A fee of
  // 100 CKB, "100.00000000" (12 glyphs at 13 px, 93.6; the bound "99.99999999" is 11, 85.8), in
  // the Fee column (102 px, its content box 102 − 16 = 86): one figure on one line, the cell
  // nowrap, 7.6 px wider than its box. A right-aligned line too long for its box is start-aligned
  // (CSS Text 3, text-align), so the figure stands from the box's left edge and runs 7.6 px into
  // its own 16 px gap, short of the Cells column (it would reach it from a second glyph past).
  it('holds a fee one glyph past its bound on one line, 7.6 px into its gap', () => {
    render(<TransactionListTable transactions={[{ ...transfer, fee: '10000000000' }]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const at = within(table)
      .getAllByRole('columnheader')
      .findIndex((th) => th.textContent === 'Fee, CKB');
    const column = Number.parseFloat(
      (table.querySelectorAll('col')[at] as HTMLElement).style.minWidth
    );
    expect(column).toBe(102);
    const fee = within(table).getAllByRole('row')[1].querySelectorAll('td')[at] as HTMLElement;
    expect(fee).toHaveTextContent(/^100\.00000000$/);
    expect(fee).toHaveClass('whitespace-nowrap', 'text-right');
    expect(cellFaults(table).filter((f) => f.rule === 'figure breaks')).toEqual([]);
    const past = '100.00000000'.length * 7.8 - (column - 16);
    expect(past).toBeCloseTo(7.6, 10);
    expect(past).toBeLessThan(16);
  });

  // Review fb-logic 3: the age under the block is a figure; the clock-behind sentence is words
  // and breaks under the block in Block · date's 110 px, where it stood on one 226 px line.
  it('lets the clock-behind sentence break under its block', () => {
    const ahead = new Date(Date.now() + 10 * 60_000).toISOString();
    render(<TransactionListTable transactions={[{ ...transfer, timestamp: ahead }]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const block = within(table).getAllByRole('row')[1].querySelectorAll('td')[1] as HTMLElement;
    expect(
      within(block).getByText(CLOCK_BEHIND).closest('[data-nobreak], .whitespace-nowrap')
    ).toBeNull();
    expect(faultLines(cellFaults(table))).toEqual([]);
  });

  it('says what each transaction did: kind, mark and a quiet note', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows[0]).toHaveTextContent('Script call.bit Time Index State · .bit Time Info');
    expect(rows[1]).toHaveTextContent(
      `CKB Transfer${feedSamples.eightParties.participants.length} participants`
    );
  });

  // Re-pinned on purpose (lists lane P3): a cellbase is a row of the cellbases' list, not set back
  // among the user transactions with "reward" and its amount grey in Value; it says what it pays in
  // its Pays column and the CKB it outputs in its own column, in the column form (renamed: "sets
  // cellbase rows back and shows the reward they pay, grey, outside every total (D5)")
  it('says what a cellbase pays, in its own words, and the CKB it outputs (D5; lists lane P3)', () => {
    render(<CellbaseListTable transactions={[cellbase]} />);
    const table = screen.getByRole('table', { name: 'Cellbases' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Transaction', 'Block · age', 'Pays', 'Output, CKB', 'Cells']);
    const row = within(table).getAllByRole('row')[1];
    expect(row).not.toHaveClass('quiet');
    const [, , pays, output, cells] = Array.from(row.querySelectorAll('td'));
    // R2b-a 35 (TX-25): the block whose reward this cellbase pays
    expect(pays).toHaveTextContent(/^reward for #20,829,764$/);
    // R-6a: the paid block is linked by the store's canonical hash, not by height
    expect(within(pays).getByRole('link', { name: '#20,829,764' })).toHaveAttribute(
      'href',
      `/mainnet/blocks/0x${'64'.repeat(32)}`
    );
    expect(output).toHaveTextContent(/^559\.61739927$/);
    expect(cells).toHaveTextContent(/^0 → 1$/);
  });

  // The lists lane P3 (the notes: block 0's "reward 33,600,000,000.00000000" is the genesis
  // issuance, not a reward): block 0's cellbase says so, and a first block's cellbase, which pays
  // no reward yet, says that; each figure stands inside its column at 1,232 px, Output bounded by
  // WIDEST_TX_OUTPUT ("99,999,999,999.99999999", 23 glyphs, 179.4 in its 180 px content box)
  it('names block 0’s output the genesis issuance and holds it inside its column', () => {
    atContainerWidth(containerContentPx(1280));
    render(
      <CellbaseListTable
        transactions={[cellbase, early, genesis]}
        formWidth={TRANSACTIONS_FORM_PX}
      />
    );
    const table = screen.getByRole('table', { name: 'Cellbases' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows.map((r) => r.querySelectorAll('td')[2].textContent)).toEqual([
      'reward for #20,829,764',
      'no reward yet',
      'the genesis issuance',
    ]);
    expect(rows[2].querySelectorAll('td')[3]).toHaveTextContent(/^33,600,000,000\.00000000$/);
    const cols = Array.from(table.querySelectorAll('col')).map((c) =>
      Number.parseFloat((c as HTMLElement).style.minWidth)
    );
    // the list at the page's width: Pays its 188 px minimum and the 408 the page's 1,212 is over
    // the list's own 804
    expect(CELLBASE_LIST_MIN_PX).toBe(804);
    expect(cols).toEqual([200, 110, 188 + (TRANSACTIONS_FORM_PX - CELLBASE_LIST_MIN_PX), 196, 110]);
    for (const tr of rows) {
      const td = tr.querySelectorAll('td')[3] as HTMLElement;
      expect(
        textRunPx(td.textContent!, td, 1280) - (cols[3] - insetsPx(td, 1280).box)
      ).toBeLessThanOrEqual(0);
    }
    expect(faultLines(cellFaults(table))).toEqual([]);
  });

  // Carried over from the retired TransactionTable: a replay not yet run is never a zero bar, and
  // missing byte or cycle evidence fails loudly instead of drawing a guess.
  // Re-pinned on purpose (lists lane P3): the cycles left the table; the card from 560 px says a
  // replay not yet run in words, never a zero
  it('says a pending replay is not yet run and rejects missing size or cycle evidence', () => {
    atContainerWidth(608);
    render(
      <TransactionListTable
        transactions={[{ ...transfer, cycles: null, cyclesStatus: 'pending' }]}
      />
    );
    const [card] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(card)[2].left).toMatch(/ · cycles not yet run$/);
    expect(screen.queryByRole('img', { name: /^Cycles/ })).not.toBeInTheDocument();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    expect(() => TransactionListTable({ transactions: [{ ...transfer, txSize: null }] })).toThrow(
      'exact serialized size'
    );
    expect(() => TransactionListTable({ transactions: [{ ...transfer, cycles: null }] })).toThrow(
      'inconsistent cycle evidence'
    );
  });
});

describe('TransactionListTable fit (visual pass, 2026-10-30)', () => {
  // Re-pinned with readability R4 (lane F3, step 2): the table text is 13 px (7.8 px a glyph, was 7.2) and its key 14 px (8.4, was 7.8): the table is drawn from 1,286 px (it fitted a 1,248 px container at
  // 1,230), and narrower it takes its row form; it never scrolls.
  // Re-pinned on purpose (readability F3-e): the table is drawn from 1,220 px, and the block and
  // its age are the cell's two lines (they were one, "#… · age"), each whole on its line.
  // Re-pinned on purpose (the coordinator's ruling): 1,203 px on its own (renamed: "1,220").
  // re-pinned on purpose (reviews fb-tests 6 and fb-logic 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13,568 → 9,999" (118 → 126), and Size genesis's cellbase on a 1,500,000 scale, "1,500,000" (129 → 145): 1,227 px (renamed: "1,203").
  // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,211 px (renamed: "1,227").
  // Re-pinned on purpose (lists lane P3): the size and the cycles bars left the list, 898 px on its
  // own (renamed: "1,211")
  it('is drawn from its 898 px minimum, keeps the block and its age each on one line and drops the per-cell unit', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(Number.parseFloat(table.style.minWidth)).toBe(898);
    const row = within(table).getAllByRole('row')[2];
    const lines = row.querySelectorAll('td:nth-child(2) > span > *');
    expect(lines).toHaveLength(2);
    // Re-pinned on purpose (review fb-logic 3, the lead's ruling): the block is one nowrap line;
    // the age line is not nowrap, its age one unit of its own (Utc8Age's NoBreak), so "this
    // device's clock is behind" in its place breaks between its words
    expect(lines[0]).toHaveClass('whitespace-nowrap');
    expect(lines[1]).not.toHaveClass('whitespace-nowrap');
    expect(lines[1].querySelector('[data-nobreak]')).toHaveTextContent(/^\d+[smhdy].* ago$/);
    expect(row.querySelector('td:nth-child(4)')).toHaveTextContent(/^767\.29874184$/);
    expect(row.querySelector('td:nth-child(5)')).toHaveTextContent(/^0\.00030000$/);
  });

  // Re-pinned on purpose and renamed (BRIEF-ink-size-roles item 1: a figure in a cell is the
  // reading ink, a zero as any other; it was "draws a zero value quiet, as the board does"). The
  // block's age under it is a time, and the names after the headline (the scripts called) names:
  // the reading ink too (both were quiet).
  it('draws a zero value, the age and the names after the headline in the reading ink', () => {
    render(<TransactionListTable transactions={[scriptCall]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    const value = row.querySelector('td:nth-child(4)') as HTMLElement;
    expect(value).toHaveTextContent(/^0$|^0\.00000000$/);
    // no wrapper sets the amount quiet (its own form, the amount rule's, is CkbAmount's)
    const amount = value.querySelector('[data-ckb-amount]') as HTMLElement;
    expect(amount.className).not.toMatch(/text-text-dim/);
    for (
      let el: Element | null = amount.parentElement;
      el && el.tagName !== 'TD';
      el = el.parentElement
    )
      expect(el.className).not.toMatch(/text-text-dim/);
    const age = row.querySelector('td:nth-child(2) time') as HTMLElement;
    for (let el: Element | null = age; el && el.tagName !== 'TD'; el = el.parentElement)
      expect(el.className).not.toMatch(/text-text-dim/);
    const name = within(row).getAllByText(/\.bit Time Index State/)[0];
    for (let el: Element | null = name; el && el.tagName !== 'TD'; el = el.parentElement)
      expect(el.className).not.toMatch(/text-text-dim/);
  });

  // The Cells column held "3 → 3" but wrapped "10 → 2" onto a second line (a 61 px row): it is
  // 62 px wide and never wraps (coordinator ruling on lane M's report). Re-pinned with readability
  // R4: "10 → 2" at 13 px is 46.8 px, and the gap and 2 spare make the column 65 px.
  // Re-pinned on purpose (readability F3-e): the column holds four digits a side, "9,999 →
  // 9,999" (101.4 px), and the gap: 118 px. Re-pinned on purpose (review fb-tests 6): the most
  // inputs a transaction holds, 13,568 (a block of 597,000 bytes holds that many 44-byte
  // inputs), and four digits of outputs, "13,568 → 9,999" (109.2 px), and the gap: 126 px
  // (renamed: "118 px").
  // Re-pinned on purpose (lists lane P3): Cells is the list's last column, no 16 px gap after it:
  // "13,568 → 9,999" (109.2) rounded up, 110 px (renamed: "126 px")
  it('keeps a two-digit cells count on one line (110 px, nowrap)', () => {
    render(
      <TransactionListTable transactions={[{ ...transfer, inputsCount: 10, outputsCount: 2 }]} />
    );
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const heads = within(table).getAllByRole('columnheader');
    const at = heads.findIndex((th) => th.textContent === 'Cells');
    expect((table.querySelectorAll('col')[at] as HTMLElement).style.width).toBe('110px');
    const cell = within(table).getAllByRole('row')[1].querySelectorAll('td')[at];
    expect(cell).toHaveTextContent(/^10 → 2$/);
    expect(cell).toHaveClass('whitespace-nowrap');
    // re-pinned on purpose (the coordinator's ruling): the list is 1,203 px on its own
    // re-pinned on purpose (reviews fb-tests 6 and fb-logic 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13,568 → 9,999" (118 → 126), and Size genesis's cellbase on a 1,500,000 scale, "1,500,000" (129 → 145): 1,227 px
    // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,211 px
    // Re-pinned on purpose (lists lane P3): no bars, Value a value's bound, Cells last: 898 px
    expect(Number.parseFloat(table.style.minWidth)).toBe(898);
  });

  // CH-14 (the thousands hunt): a count of four digits or more is grouped, as every count is
  // ('1234 → 5678' read beside '2,555' elsewhere)
  it('groups a cells count of four digits or more', () => {
    render(
      <TransactionListTable
        transactions={[{ ...transfer, inputsCount: 1234, outputsCount: 5678 }]}
      />
    );
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const heads = within(table).getAllByRole('columnheader');
    const at = heads.findIndex((th) => th.textContent === 'Cells');
    const cell = within(table).getAllByRole('row')[1].querySelectorAll('td')[at];
    expect(cell).toHaveTextContent(/^1,234 → 5,678$/);
  });

  // Table-rows T5: one bright thing per row, the key cell at body. The 18 px line keeps the row
  // at its one-line height (a 13 px line is 20.8 px and would make it 45.8).
  // Re-pinned on purpose (lists lane P3, the link-look gate): the bright ink is the key look's, on
  // the link (it was the Hash's own class, a link ink by hand on a child); a cellbase in its own
  // list is not set back (renamed: "… a cellbase keeps its dim row (T5)")
  it('leads each row with its hash at the body step, the key look’s bright ink, on the row line (T5)', () => {
    render(<TransactionListTable transactions={[scriptCall]} />);
    const { unmount } = render(<CellbaseListTable transactions={[cellbase]} />);
    for (const tx of [scriptCall, cellbase]) {
      const hash = screen.getByTitle(tx.hash);
      expect(hash).toHaveClass('text-body', 'leading-row');
      expect(hash).not.toHaveClass('text-small');
      expect(hash).not.toHaveClass('text-text-bright');
      // it takes the link's ink (Hash's `inherit`): its own reading ink would beat the look's
      expect(hash).not.toHaveClass('text-text');
      const link = hash.closest('a')!;
      expect(link).toHaveAttribute('href', `/mainnet/tx/${tx.hash}`);
      expect(link).toHaveClass('text-text-bright');
      expect(link.closest('tr')).not.toHaveClass('quiet');
    }
    unmount();
  });
});

// The seals by kind (C2): every row leads with its transaction's row stone, the long slab (16 px
// high, 32.3 wide since readability R6; it was 24 and 48.5): rods for the cells it consumed, the
// glyph in its rim, rods for the cells it created. The hash stays the row's link.
describe('TransactionListTable row stones', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  // Re-pinned on purpose (lists lane P3): the user transactions' list holds no cellbase; the
  // cellbase's stone is drawn in its own list (the test after these two)
  const txs = [scriptCall, transfer];
  // A cellbase consumes no cell: the API counts its one null input (inputsCount 1), the stone 0.
  const ids: SealId[] = [
    { kind: 'tx', hash: scriptCall.hash, consumed: 3, created: 3 },
    { kind: 'tx', hash: transfer.hash, consumed: 3, created: 3 },
    { kind: 'tx', hash: cellbase.hash, consumed: 0, created: 1 },
  ];

  // re-pinned on purpose (readability R6): the 16 px stone stands centred on the hash's 20 px line,
  // 2 px clear above and below, where the 24 px one overhung it by 2 px (`-my-[2px]`); renamed:
  // "overhanging the hash line by 3 px"
  it('leads every table row with its stone, centred on the hash line, before the hash link', () => {
    render(<TransactionListTable transactions={txs} />);
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(2);
    rows.forEach((tr, i) => {
      const seal = expectOneRowSeal(tr, ids[i]);
      expect(tr.querySelector('td')).toContainElement(seal);
      // (the 20 px line − 16) / 2 clear above and below: the one-line row keeps its height
      expect((ROW_LINE_PX - ROW_SEAL.tx.height) / 2).toBe(2);
      expect(seal).toHaveClass('h-row', 'items-center');
      expect(seal.className).not.toMatch(/-my-/);
      expectBefore(seal, within(tr).getByTitle(txs[i].hash).closest('a')!);
    });
    // every stone is in force, pressed at full strength: its transaction is in a block
    for (const tr of rows)
      expect(tr.querySelector('[data-seal-state]')).toHaveAttribute('data-seal-state', 'pressed');
  });

  it('leads every phone row with its stone in its own column, not overhanging', () => {
    atContainerWidth(358);
    render(<TransactionListTable transactions={txs} />);
    const cards = screen.getAllByTestId('transaction-row-mobile');
    expect(cards).toHaveLength(2);
    cards.forEach((card, i) => {
      const seal = expectOneRowSeal(card, ids[i]);
      expect(card.firstElementChild).toHaveAttribute('data-part', 'mark');
      expect(card.firstElementChild).toContainElement(seal);
      expect(seal.className).not.toMatch(/-my-/);
      expectBefore(seal, within(card).getByTitle(txs[i].hash));
    });
  });

  // The API counts a cellbase's one input, the null out point; the row knows a cellbase by
  // `isCellbase` (it sets the row back by it) and writes 0, as its Cells column does.
  it('presses a cellbase’s stone 0 → n, as its Cells column reads, never the counted null input', () => {
    expect(cellbase.inputsCount).toBe(1);
    for (const width of [null, 358] as const) {
      if (width !== null) atContainerWidth(width);
      // re-pinned on purpose (lists lane P3): in the cellbases' own list
      const { unmount } = render(<CellbaseListTable transactions={[cellbase]} />);
      const row =
        width === null
          ? within(screen.getByRole('table')).getAllByRole('row')[1]
          : screen.getByTestId('transaction-row-mobile');
      const seal = expectOneRowSeal(row, ids[2]);
      expect(seal.querySelector('svg')).toHaveAttribute(
        'aria-label',
        'Transaction seal 0xc89d2d25, 0 consumed, 1 created'
      );
      // the stone of the raw count is another drawing: the 0 is what is pressed
      const counted = drawSeal(
        { kind: 'tx', hash: cellbase.hash, consumed: 1, created: 1 },
        { form: 'row', height: ROW_SEAL.tx.height, state: 'pressed', ...SEAL_INKS }
      );
      expect(seal.innerHTML).not.toBe(parsed(counted.svg));
      expect(row).toHaveTextContent(width === null ? '0 → 1' : '0 → 1 cell');
      unmount();
    }
  });

  // The stone's footprint (56.5 px: 48.5 and the 8 px gap) is 29.5 px more than the 27 of the seal
  // it replaced. Old issue C: the hash keeps its 19 characters, 10 + 6 as in the phone row; the
  // Transaction column's 29 px come from the Size bar's track (80 → 56, 24 px) and What it did
  // (220 → 215, 5 px), so the table keeps its 1,230 px in the widest 1,232 px container.
  // Readability R6: the stone's footprint is 40.3 px (32.3 and the 8 px gap).
  it('budgets the stone and the 19-character hash in the Transaction column inside the 1,230 px', () => {
    render(<TransactionListTable transactions={txs} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const widths = Array.from(table.querySelectorAll('col')).map((c) =>
      Number.parseFloat((c as HTMLElement).style.minWidth)
    );
    // Re-pinned on purpose (readability B5): What it did 215 → 246, from the Transaction column's
    // spare (221 → 208) and the Cycles bar (180 → 162, its track 80 → 56); 1,230 px as before.
    // Re-pinned with readability R4 (lane F3, step 2): the table text is 13 px (7.8 px a glyph, was 7.2) and its key 14 px (8.4, was 7.8): every derived column grows with its text, 1,286 px.
    // Re-pinned on purpose (readability F3-e): every column its content, the spares gone, the
    // block over its age, What it did the flexible column (its minimum read from minWidth): 1,220.
    // Re-pinned on purpose (the coordinator's ruling): Size on a 48 px track, 146 → 129: 1,203.
    // re-pinned on purpose (reviews fb-tests 6 and fb-logic 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13,568 → 9,999" (118 → 126), and Size genesis's cellbase on a 1,500,000 scale, "1,500,000" (129 → 145): 1,227 px.
    // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,211 px.
    // Re-pinned on purpose (lists lane P3): the size and the cycles bars left (145 and 144 px),
    // Value holds a value alone (180 → 172) and Cells is the last column, no gap (126 → 110): 898
    expect(widths).toEqual([200, 110, 204, 172, 102, 110]);
    expect(widths.reduce((sum, w) => sum + w, 0)).toBe(898);
    expect(Number.parseFloat(table.style.minWidth)).toBe(898);
    expect(ROW_SEAL.tx.footprint).toBe(40.3);
    // U-09 (re-pinned): the row size, "0x" and 8 digits, the one ellipsis, 6 digits: 17
    // characters of JetBrains Mono at the body step, 14 px since R4 (8.4; 7.8 at 13 px; it was 19
    // with three dots)
    const [, row] = within(table).getAllByRole('row');
    const shown = within(row).getByTitle(scriptCall.hash);
    expect(shown).toHaveTextContent(/^0x00de304b…25cbbc$/);
    const chars = shown.textContent!.length;
    expect(chars).toBe(17);
    // the stone, the hash and the 16 px gap fit the column (re-pinned on purpose, B5: 208 px, 2.9
    // px to spare; it was 221 with 15.9; R4: 218 px, the hash at 14 px, 2.7 to spare; F3-e: 216
    // px, the spare gone, 0.7 of rounding up to a whole pixel; R6: 200 px, the stone 16 px less,
    // 199.1 used and 0.9 of rounding up)
    const used = ROW_SEAL.tx.footprint + chars * 8.4 + 16;
    expect(used).toBeCloseTo(199.1, 6);
    expect(widths[0] - used).toBeCloseTo(0.9, 6);
    // Cells, the last column: "13,568 → 9,999" (14 ch at 13 px, 109.2) and no gap; 0.8 of
    // rounding up to a whole pixel (lists lane P3: it was Cycles' track, bar gap and figure)
    expect(widths[5] - 14 * 7.8).toBeCloseTo(0.8, 6);
    // What it did keeps the longest DAO headline whole beside its 16 px mark and its 8 px gap.
    // Re-pinned on purpose (F3-e): this counted the 8 px between headline and note too, which the
    // headline's line does not take where the note goes under it, as it does at the minimum.
    expect(16 + 8 + 'DAO Withdraw Complete'.length * 7.8 + 16).toBeLessThanOrEqual(widths[2]);
    // Value: a value under a billion CKB, "999,999,999.99999999" (20 ch at 13 px, 156), and the gap
    // (lists lane P3: it was Size's track, gap, figure and gap; a cellbase's "reward" left Value)
    expect(widths[3] - (20 * 7.8 + 16)).toBeCloseTo(0, 6);
  });

  // U-09 (re-pinned; old issue C had the table and the phone row alike at 10 + 6): the size is
  // the form's, the row size in the table and the tight size in the phone row.
  it('shows the hash at the row size in the table and the tight size in the phone row', () => {
    // re-pinned (readability R4): the table is drawn from 1,286 px (was 1,230)
    // re-pinned (readability F3-e): the table is drawn from 1,220 px, so at 1,232 too
    // re-pinned (the coordinator's ruling): on its own from 1,203 px
    // re-pinned (reviews fb-tests 6, fb-logic 6): on its own from 1,227 px
    // re-pinned (readability R6, the transaction stone 16 px): on its own from 1,211 px
    // re-pinned (lists lane P3, no bars): on its own from 898 px
    for (const width of [1232, 898, 897, 608, 358]) {
      atContainerWidth(width);
      const { unmount } = render(<TransactionListTable transactions={[scriptCall]} />);
      expect(screen.getByTitle(scriptCall.hash)).toHaveTextContent(
        width >= 898 ? /^0x00de304b…25cbbc$/ : /^0x00de…cbbc$/
      );
      expect(screen.queryByRole('table') === null).toBe(width < 898);
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  // The form changes at the table's minimum: a table from 1,286 px (re-pinned, readability R4:
  // 1,230 at 12 / 13 px text), the phone rows below; the widest content box, 1,232, takes the rows.
  // Re-pinned on purpose (readability F3-e): a table from 1,220 px, so the widest content box,
  // 1,232, draws it.
  // Re-pinned on purpose (the coordinator's ruling): on its own from 1,203 px (renamed: "1,220").
  // Re-pinned on purpose (reviews fb-tests 6, fb-logic 6): from 1,227 px (renamed: "1,203").
  // Re-pinned on purpose (readability R6, the transaction stone 16 px): from 1,211 px (renamed:
  // "1,227").
  // Re-pinned on purpose (lists lane P3): from 898 px (renamed: "1,211")
  it('changes form at its 898 px minimum', () => {
    for (const [width, form] of [
      [1232, 'table'],
      [898, 'table'],
      [897, 'rows'],
    ] as const) {
      atContainerWidth(width);
      const { container, unmount } = render(<TransactionListTable transactions={txs} />);
      expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  it('draws 50 rows with 50 stones, each transaction’s once', () => {
    const many: Transaction[] = Array.from({ length: 50 }, (_, i) => ({
      ...transfer,
      hash: sealHash(`list-${i}`),
      inputsCount: i % 7,
      outputsCount: (i * 3) % 11,
    }));
    const started = performance.now();
    render(<TransactionListTable transactions={many} />);
    const elapsed = performance.now() - started;
    console.info(`50\u00a0transaction rows with their stones: ${elapsed.toFixed(1)} ms`);
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(50);
    rows.forEach((tr, i) =>
      expectOneRowSeal(tr, {
        kind: 'tx',
        hash: many[i].hash,
        consumed: i % 7,
        created: (i * 3) % 11,
      })
    );
    const labels = rows.map((tr) => tr.querySelector('svg')!.getAttribute('aria-label'));
    expect(new Set(labels).size).toBe(50);
  });
});

// Readability F3-e: every column of the committed list is budgeted from the widest value its
// formatter writes for the chain (lib/list-bounds.ts), at 0.6 em a glyph (the hash 14 px, the cells
// 13, the ticks 12) and the 16 px gap after every column but the last. What it did is the flexible
// column: the longest DAO headline whole at its minimum, the rest of the container beyond the sum.
// The block and its age are two data of one cell, on two lines: at the minimum every row is two
// lines already (the note under the headline), and the table is 1,220 px, inside the widest
// container (1,232 at 1280).
describe('TransactionListTable budgets (readability F3-e)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
  /** A column's minimum, as columnPx rounds it: to the hundredth, then up to a whole pixel. */
  const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
  // Re-pinned on purpose (lists lane P3): the size and the cycles bars left the list of user
  // transactions; Value holds a value alone; Cells is the last column. The cellbases' list has
  // its own columns, Pays and Output each from their own bound.
  const budgets = [
    // the stone and its gap, the hash at the row size at 14 px
    ceil(ROW_SEAL.tx.footprint + at('0x00de304b…25cbbc', 14) + 16),
    // "#99,999,999" over "10y 364d ago", the wider of its two lines
    ceil(Math.max(at('#99,999,999', 13), at('10y 364d ago', 13)) + 16),
    // the 16 px mark, its 8 px gap and "DAO Withdraw Complete" whole; its note goes under it
    ceil(16 + 8 + at('DAO Withdraw Complete', 13) + 16),
    // a value under a billion CKB
    ceil(at('999,999,999.99999999', 13) + 16),
    // a fee under 100 CKB
    ceil(at('99.99999999', 13) + 16),
    // the most inputs a transaction holds, 13,568 (597,000 bytes of 44-byte inputs), and four
    // digits of outputs; the last column, no gap
    ceil(at('13,568 → 9,999', 13)),
  ];
  const cellbaseBudgets = [
    budgets[0],
    budgets[1],
    // "reward for #99,999,999" on one line
    ceil(at('reward for #99,999,999', 13) + 16),
    // an output of eleven whole digits, "99,999,999,999.99999999" (block 0's 33.6 billion is 14)
    ceil(at('99,999,999,999.99999999', 13) + 16),
    budgets[5],
  ];

  // Re-pinned on purpose (lists lane P3): 898 px on its own (renamed: "1,211 px")
  it('pins each column to its content and the table to their sum, 898 px', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(budgets).toEqual([200, 110, 204, 172, 102, 110]);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols[2].width).toBe('auto');
    expect(budgets.reduce((sum, w) => sum + w, 0)).toBe(898);
    expect(Number.parseFloat(table.style.minWidth)).toBe(898);
  });

  // Re-pinned on purpose (lists lane P3): it pinned the Size head's tick form on its 48 px track;
  // the cellbases' list, the columns of its own words and bounds, 804 px (renamed: "heads Size in
  // the tick form on a 48 px track: 1,211 px on its own")
  it('pins the cellbases’ columns to their content: 804 px on its own', () => {
    render(<CellbaseListTable transactions={[cellbase, genesis]} />);
    const table = screen.getByRole('table', { name: 'Cellbases' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cellbaseBudgets).toEqual([200, 110, 188, 196, 110]);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(cellbaseBudgets);
    expect(cols[2].width).toBe('auto');
    expect(Number.parseFloat(table.style.minWidth)).toBe(804);
  });

  it('writes the block over its age, two lines with no separator', () => {
    render(<TransactionListTable transactions={[transfer]} />);
    const [, row] = within(screen.getByRole('table')).getAllByRole('row');
    const cell = row.querySelectorAll('td')[1];
    const lines = Array.from(cell.firstElementChild!.children) as HTMLElement[];
    expect(lines).toHaveLength(2);
    expect(lines[0]).toHaveTextContent(/^#20,829,775$/);
    expect(lines[1]).toHaveTextContent(/^\d+[smhdy].* ago$/);
    // Re-pinned on purpose (review fb-logic 3, the lead's ruling): both are block lines; the
    // block number's line is nowrap, the age's is not (its age is one unit, Utc8Age's NoBreak; the
    // clock-behind sentence in its place breaks between its words)
    for (const line of lines) expect(line).toHaveClass('block');
    expect(lines[0]).toHaveClass('whitespace-nowrap');
    expect(lines[1]).not.toHaveClass('whitespace-nowrap');
    expect(lines[1].querySelector('[data-nobreak]')).toHaveTextContent(/^\d+[smhdy].* ago$/);
    expect(cell).not.toHaveTextContent('·');
  });

  // re-pinned on purpose (the coordinator's ruling): on its own from 1,203 px (renamed: "1,220")
  // re-pinned on purpose (reviews fb-tests 6, fb-logic 6): from 1,227 px (renamed: "1,203")
  // re-pinned on purpose (readability R6, the transaction stone 16 px): from 1,211 px (renamed:
  // "1,227")
  // re-pinned on purpose (lists lane P3): on its own from 898 px (renamed: "1,211")
  it('changes form where the sum stops fitting: a table at 1,232 and 898, rows below', () => {
    for (const [width, form] of [
      [1232, 'table'],
      [898, 'table'],
      [897, 'rows'],
      [608, 'rows'],
      [358, 'rows'],
    ] as const) {
      atContainerWidth(width);
      const { container, unmount } = render(
        <TransactionListTable transactions={[scriptCall, transfer]} />
      );
      expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });
});

describe('TransactionListTable phone row (board L2)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  // Re-pinned on purpose (lane D4, the phone-rows board): line 3 names each figure ("value 0 CKB",
  // was a bare "0 CKB", PA-09) and keeps the cells, the value and the fee; bytes and cycles left
  // the phone row (was "3 → 3 cells · 903 B · 1,704,957 cycles" at the right); a cellbase says
  // what it pays on line 2 and its reward on line 3, and no fee (was "Cellbase pays #20,829,764 ·
  // reward 559.61 CKB" over "0 → 1 cell · 360 B · fee 0 CKB" and "0 cycles").
  it('is three lines at 358 px: who and when, what it did in full, then cells · value · fee', () => {
    atContainerWidth(358);
    render(<TransactionListTable transactions={[scriptCall]} />);
    // re-pinned on purpose (lists lane P3): the cellbase's row is its own list's
    render(<CellbaseListTable transactions={[cellbase]} />);
    expect(screen.queryByRole('table')).toBeNull();
    const [call, base] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(call)).toEqual([
      {
        key: 'identity',
        tone: 'head',
        left: '0x00de…cbbc',
        // Lane P: the block's UTC+8 date; its age is the date's hover.
        // PA-22 (re-pinned): the live list's row says its age (it was the date)
        right: expect.stringMatching(/^#20,829,775 · \d+[smhdy] ago$/),
      },
      {
        key: 'what',
        tone: 'body',
        // Never truncated: both type scripts survive (A#7).
        left: 'Script call · .bit Time Index State · .bit Time Info',
        right: null,
      },
      {
        key: 'figures',
        tone: 'quiet',
        // Consistency plan C4: a zero value is "0 CKB" (was "0.00000000 CKB"); readability R5: the
        // summary form, below 1 CKB exact
        left: '3 → 3\u00a0cells · value 0 CKB · fee 0.0003 CKB',
        right: null,
      },
    ]);
    expect(call.querySelector('[data-line="what"] .truncate')).toBeNull();
    // re-pinned on purpose (lists lane P3): "reward for #…" (it was "pays #…"), not set back
    expect(phoneLines(base).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xc89d…2cd7', expect.stringMatching(/^#20,829,775 · /)],
      ['what', 'Cellbase · reward for #20,829,764', null],
      // a phone row's summary form (readability R5: was "559.61739927")
      ['figures', '0 → 1\u00a0cell · reward 559.61 CKB', null],
    ]);
    expect(base.closest('[role="listitem"]')).not.toHaveClass('quiet');
  });

  // The lists lane P3: block 0's cellbase is the genesis issuance, named so on its phone row and
  // its amount named "issued", in the summary form; a first block's says it pays no reward yet
  it('names block 0’s and a first block’s cellbase in their own words at 358 px', () => {
    atContainerWidth(358);
    render(<CellbaseListTable transactions={[genesis, early]} />);
    const [first, next] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(first).map((l) => [l.key, l.left])).toEqual([
      ['identity', '0x0e0e…0e0e'],
      ['what', 'Cellbase · the genesis issuance'],
      ['figures', '0 → 1\u00a0cell · issued 33,600,000,000 CKB'],
    ]);
    expect(
      phoneLines(next)
        .slice(1)
        .map((l) => [l.key, l.left])
    ).toEqual([
      ['what', 'Cellbase · no reward yet'],
      ['figures', '0 → 1\u00a0cell · output 0 CKB'],
    ]);
  });

  // Re-pinned on purpose (lane D4): the card at 608 px is the phone row's three parts, its quiet
  // line keeping the bytes and the cycles where no table stands yet (it was two lines, value and
  // fee after what it did and the figures at the right edge).
  it('is three lines at 608 px: the quiet line keeps the bytes and the cycles', () => {
    atContainerWidth(608);
    render(<TransactionListTable transactions={[scriptCall]} />);
    const [call] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(call).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x00de…cbbc', expect.stringMatching(/^#20,829,775 · /)],
      ['what', 'Script call · .bit Time Index State · .bit Time Info', null],
      [
        'figures',
        // a phone row's summary form (readability R5: was "0.00030000")
        '3 → 3\u00a0cells · value 0 CKB · fee 0.0003 CKB · 903 B · 1,704,957 cycles',
        null,
      ],
    ]);
  });
});

// Lane D4 (the phone-rows board the owner passed at gate D0′, its transactions pane): one row,
// three parts. Line 1 the hash in its tight form and the block with its age; line 2 what it did, in
// the reading ink, wrapping between its parts; line 3 quiet, what every reader uses — the cells,
// the value and the fee, each figure with its name (PA-09: the bare "0 CKB" lost its column's name).
// Bytes and cycles leave the phone row: they are on the transaction's page, in the table from its
// bound and in the cards from 560 px. A cellbase writes what it pays on line 2 and its reward,
// named, on line 3.
describe('the transactions phone row on the phone-rows board (lane D4)', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('is three parts at 358 px: hash and block · age, what it did, then cells · value · fee', () => {
    atContainerWidth(358);
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    // re-pinned on purpose (lists lane P3): the cellbase's row is its own list's
    render(<CellbaseListTable transactions={[cellbase]} />);
    const [call, sent, base] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(call).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['identity', 'head', '0x00de…cbbc', expect.stringMatching(/^#20,829,775 · \d+[smhdy] ago$/)],
      ['what', 'body', 'Script call · .bit Time Index State · .bit Time Info', null],
      ['figures', 'quiet', '3 → 3\u00a0cells · value 0 CKB · fee 0.0003 CKB', null],
    ]);
    const figures = call.querySelector('[data-line="figures"]')!;
    expect(dottedItems(figures)).toEqual(['3 → 3\u00a0cells', 'value 0 CKB', 'fee 0.0003 CKB']);
    // no value without its name: every amount on the row stands after its label, in the quiet ink
    for (const row of [call, sent, base])
      for (const amount of row.querySelectorAll('[data-ckb-amount]')) {
        const item = amount.closest('[data-dotted-item]')!;
        const label = item.querySelector('[data-role="figure-label"]');
        expect(label, `${item.textContent}: an amount without its name`).not.toBeNull();
        expect(inkOf(label!)).toMatch(/text-text-dim|quiet/);
      }
    // bytes and cycles are not on the phone row
    expect(call.textContent).not.toMatch(/903 B|cycles/);
    // line 2 is what it did, in the reading ink: the key on line 1 is the row's one bright thing
    const what = call.querySelector('[data-line="what"]')!;
    expect(what.querySelector('.text-text-bright')).toBeNull();
    expect(phoneLines(sent)[2].left).toBe('3 → 3\u00a0cells · value 767.29 CKB · fee 0.0003 CKB');
    expect(phoneLines(base).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xc89d…2cd7', expect.stringMatching(/^#20,829,775 · /)],
      ['what', 'Cellbase · reward for #20,829,764', null],
      // a cellbase pays no fee: its row names the reward it pays and nothing else
      ['figures', '0 → 1\u00a0cell · reward 559.61 CKB', null],
    ]);
  });

  it('keeps the bytes and the cycles in the cards from 560 px, where no table stands yet', () => {
    atContainerWidth(608);
    render(<TransactionListTable transactions={[scriptCall]} />);
    const [call] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(call).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x00de…cbbc', expect.stringMatching(/^#20,829,775 · /)],
      ['what', 'Script call · .bit Time Index State · .bit Time Info', null],
      [
        'figures',
        '3 → 3\u00a0cells · value 0 CKB · fee 0.0003 CKB · 903 B · 1,704,957 cycles',
        null,
      ],
    ]);
  });

  // The board's head(): the 16 px stone (32.3 wide) and its gap, the tight hash at 14 px, the 12 px
  // between the parts, and the block with its age at 13 px. At its widest under a year,
  // "#99,999,999 · 364d ago" (22 characters, 171.6), it is 316.3 of a 360 px phone's 328 px line.
  // An age of a year or more ("10y 364d ago", 202.8) makes it 347.5: it fits 390's 358 and drops
  // the block and its age under the hash at 360, whole (the finding the lane hands back).
  it('states its first line, 316.3 of 328 px, and draws its widest row under a year in it', async () => {
    atContainerWidth(PHONE_LINE_PX[360]);
    const T = Date.parse('2027-10-25T00:00:00.000Z');
    vi.useFakeTimers({ toFake: ['Date'], now: T });
    const widest: Transaction = {
      ...scriptCall,
      blockNumber: 99_999_999,
      // 364 days and an hour before now: the widest age under a year
      timestamp: new Date(T - (364 * 86_400 + 3_600) * 1000).toISOString(),
    };
    render(<TransactionListTable transactions={[widest]} />);
    const [row] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(row)[0].right).toBe('#99,999,999 · 364d ago');
    expect(statedHeadPx(row)).toBe(316.3);
    expect(phoneHeadPx(row, 360)).toBe(statedHeadPx(row));
  });
});

// Lane D4 follow-up (the coordinator's note): the block a cellbase pays is a link inside a line,
// on the phone row and in the table alike: the inline look (components/ui/link.tsx), underlined in
// its line's ink, no hand-written hover ink.
describe('the block a cellbase pays is an inline link (lane D4 follow-up)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  // re-pinned on purpose (lists lane P3): in the cellbases' own list
  it('on the phone row and in the table', () => {
    atContainerWidth(358);
    const { unmount } = render(<CellbaseListTable transactions={[cellbase]} />);
    const phone = within(screen.getByTestId('transaction-row-mobile')).getByRole('link', {
      name: '#20,829,764',
    });
    expect(phone).toHaveClass('underline');
    expect(phone).not.toHaveClass('hover:text-jade');
    unmount();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    atContainerWidth(1248);
    render(<CellbaseListTable transactions={[cellbase]} />);
    const table = within(screen.getByRole('table')).getByRole('link', { name: '#20,829,764' });
    expect(table).toHaveClass('underline');
  });
});
