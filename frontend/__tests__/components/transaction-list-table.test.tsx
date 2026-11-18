import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { TransactionListTable } from '@/components/transaction/transaction-list-table';
import type { Transaction } from '@/lib/api';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import {
  expectBefore,
  expectOneRowSeal,
  parsed,
  SEAL_INKS,
  sealHash,
} from '@/__tests__/utils/seals';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { drawSeal, type SealId } from '@/lib/seal/stones';

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
  activity: {
    txHash: `0x00de304b${'00'.repeat(25)}25cbbc`,
    blockNumber: 20829775,
    txIndex: 1,
    timestamp: '2026-10-25T00:00:00Z',
    isCellbase: false,
    protocolActions: [],
    lockCalls: [],
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
  },
};
const transfer: Transaction = {
  ...base,
  hash: `0xcfaebc55${'00'.repeat(25)}e6dd61`,
  isCellbase: false,
  value: '76729874184',
  activity: { ...feedSamples.eightParties, typeCalls: [], protocolActions: [] },
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

// UI review 2026-11-14 (DY-02, U-08, ruling R4): "set back" is the dim ink on every word, never
// element opacity (55 % put the hash at 3.6:1 and "reward 559.51…" at 2.1:1 by day, and turned
// the stone pink, an ink the seals do not have); the stone keeps its own state's ink at full
// strength — a cellbase is in force, so it stays the pressed cinnabar.
describe('a cellbase row set back (R4, DY-02)', () => {
  const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('is the quiet variant: every word dim, nothing faded, the stone pressed at full strength', () => {
    render(<TransactionListTable transactions={[scriptCall, cellbase]} />);
    const [, call, base] = within(screen.getByRole('table')).getAllByRole('row');
    expect(base).toHaveClass('quiet');
    expect(call).not.toHaveClass('quiet');
    expect(fading(base)).toEqual([]);
    const stone = base.querySelector('[data-seal-state]') as HTMLElement;
    expect(stone).toHaveAttribute('data-seal-state', 'pressed');
    expect(fading(stone)).toEqual([]);
  });

  it('sets the phone row back the same way', () => {
    atContainerWidth(358);
    render(<TransactionListTable transactions={[cellbase]} />);
    const item = screen.getByTestId('transaction-row-mobile').closest('[role="listitem"]')!;
    expect(item).toHaveClass('quiet');
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
});

describe('TransactionListTable (Transactions board, TX-25 / TX-26 / D5)', () => {
  it('uses the board columns, with the bar scales in the header', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual([
      'Transaction',
      'Block · date',
      'What it did',
      'Value, CKB',
      'Fee, CKB',
      'Cells',
      'Size, bytes01,000',
      // re-pinned on purpose (readability B5): the Cycles ticks say the scale as a tick does
      'Cycles02M',
    ]);
  });

  // Table-rows R2: the bar heads are the shared ScaleHeader, 2 px under the name, the tick row as
  // long as the bar's track. Old issue C: the Size track is 56 px (was 80); its 24 px went to the
  // transaction's hash. Re-pinned on purpose (readability B5): the Cycles track is 56 px too (was
  // 80), its ticks "0 … 2M"; its 24 px went to What it did, whose note was cut mid-word.
  it('heads its bars with the shared ScaleHeader: Size and Cycles 56 px ticks, 2 px under the name', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table');
    const heads = within(table).getAllByRole('columnheader');
    for (const [i, end, track] of [
      [6, '1,000', '56px'],
      [7, '2M', '56px'],
    ] as const) {
      const ticks = heads[i].querySelector('[data-scale-ticks]') as HTMLElement;
      expect(ticks).toHaveAttribute('aria-hidden', 'true');
      expect(ticks).toHaveClass('text-meta', 'flex', 'justify-between');
      expect(ticks.style.width).toBe(track);
      expect((ticks.parentElement as HTMLElement).style.gap).toBe('2px');
      expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual(['0', end]);
      // every row's bar runs on the same track as its head's ticks
      for (const tr of within(table).getAllByRole('row').slice(1))
        expect(
          (tr.querySelectorAll('td')[i].querySelector('[data-track]') as HTMLElement).style.width
        ).toBe(track);
    }
  });

  // Old issue C: each track holds its head's widest tick row (11 px, 6.6 px a character). A
  // transaction is no larger than a block (597,000 bytes), so a page's size scale is at most
  // 600,000: "0" … "600,000" in 52.8 of the 56 px. Any cycles scale under a billion (a page whose
  // largest transaction takes up to 750,000,000 cycles): re-pinned on purpose (readability B5),
  // "0" … "750M" in 33 of the 56 px (it was "750,000,000" in 79.2 of 80).
  it('holds the widest tick row of each bar head inside its track', () => {
    render(
      <TransactionListTable
        transactions={[{ ...transfer, txSize: 597000, cycles: 750_000_000 }, scriptCall]}
      />
    );
    const heads = within(screen.getByRole('table')).getAllByRole('columnheader');
    for (const [i, end, used] of [
      [6, '600,000', 52.8],
      [7, '750M', 33],
    ] as const) {
      const ticks = heads[i].querySelector('[data-scale-ticks]') as HTMLElement;
      const text = Array.from(ticks.children).map((t) => t.textContent!);
      expect(text).toEqual(['0', end]);
      expect(text.join('').length * 6.6).toBeCloseTo(used, 6);
      expect(used).toBeLessThan(Number.parseFloat(ticks.style.width));
    }
  });

  it('draws its row bars 8 px tall at their exact width, never widened to a minimum', () => {
    render(<TransactionListTable transactions={[{ ...transfer, cycles: 1000 }, scriptCall]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    // 1,000 of a 2,000,000-cycle scale on a 56 px track (re-pinned on purpose, readability B5: it
    // was 80, 0.04 px): 0.028, drawn to the hundredth, 0.03 px.
    const fill = row.querySelector('td:nth-child(8) [data-segment="value"]') as HTMLElement;
    expect(fill.style.width).toBe('0.03px');
    expect((fill.parentElement as HTMLElement).style.height).toBe('8px');
  });

  // Readability 2026-11-18, lane B5: "What it did" was 215 px and its note was cut mid-word ("CKB
  // Transfer 2 participa", "Cellbase pays #21,042,6…", capture transactions n-02). The note now
  // wraps under the headline, part by part, and is never cut.
  it('never cuts what a transaction did: the note wraps, whole parts (B5)', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
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
  it('gives What it did the width of a common row on one line, inside the 1,230 px (B5)', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const widths = Array.from(table.querySelectorAll('col')).map((c) =>
      Number.parseFloat((c as HTMLElement).style.width)
    );
    const line = (headline: string, note: string) =>
      16 + 8 + (headline.length + note.length) * 7.2 + 8 + 16;
    expect(line('CKB Transfer', '12 participants')).toBeLessThanOrEqual(widths[2]);
    expect(line('Cellbase', 'pays #21,042,681')).toBeLessThanOrEqual(widths[2]);
    expect(widths.reduce((sum, w) => sum + w, 0)).toBe(1230);
  });

  it('says what each transaction did: kind, mark and a quiet note', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer]} />);
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows[0]).toHaveTextContent('Script call.bit Time Index State · .bit Time Info');
    expect(rows[1]).toHaveTextContent(
      `CKB Transfer${feedSamples.eightParties.participants.length} participants`
    );
  });

  it('sets cellbase rows back and shows the reward they pay, grey, outside every total (D5)', () => {
    render(<TransactionListTable transactions={[cellbase]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    // R4 (re-pinned on purpose): set back is the quiet variant, not 55 % opacity
    expect(row).toHaveClass('quiet');
    expect(row).toHaveTextContent('Cellbase');
    const value = row.querySelector('[data-reward]')!;
    expect(value).toHaveTextContent(/^reward 559\.61739927$/);
    expect(value).toHaveClass('text-text-dim');
    expect(row).toHaveTextContent('0 → 1');
    // R2b-a 35 (TX-25): the block whose reward this cellbase pays.
    expect(row).toHaveTextContent('Cellbasepays #20,829,764');
    // R-6a: the paid block is linked by the store's canonical hash, not by height.
    expect(within(row).getByRole('link', { name: '#20,829,764' })).toHaveAttribute(
      'href',
      `/mainnet/blocks/0x${'64'.repeat(32)}`
    );

    expect(row.querySelector('[data-segment="value"]')).toHaveStyle({
      backgroundColor: 'var(--c-slate-ckb)',
    });
  });

  // Carried over from the retired TransactionTable: a replay not yet run is never a zero bar, and
  // missing byte or cycle evidence fails loudly instead of drawing a guess.
  it('says a pending replay is not yet run and rejects missing size or cycle evidence', () => {
    render(
      <TransactionListTable
        transactions={[{ ...transfer, cycles: null, cyclesStatus: 'pending' }]}
      />
    );
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    expect(row).toHaveTextContent('not yet run');
    expect(screen.queryByRole('img', { name: /^Cycles/ })).not.toBeInTheDocument();
    expect(() => TransactionListTable({ transactions: [{ ...transfer, txSize: null }] })).toThrow(
      'exact serialized size'
    );
    expect(() => TransactionListTable({ transactions: [{ ...transfer, cycles: null }] })).toThrow(
      'inconsistent cycle evidence'
    );
  });
});

describe('TransactionListTable fit (visual pass, 2026-10-30)', () => {
  it('fits the 1,248 px container, keeps block · age on one line and drops the per-cell unit', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(1248);
    const row = within(table).getAllByRole('row')[2];
    expect(row.querySelector('td:nth-child(2) > span')).toHaveClass('whitespace-nowrap');
    expect(row.querySelector('td:nth-child(4)')).toHaveTextContent(/^767\.29874184$/);
    expect(row.querySelector('td:nth-child(5)')).toHaveTextContent(/^0\.00030000$/);
  });

  it('draws a zero value quiet, as the board does', () => {
    render(<TransactionListTable transactions={[scriptCall]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    expect(row.querySelector('td:nth-child(4) [data-zero]')).toBeInTheDocument();
  });

  // The Cells column held "3 → 3" but wrapped "10 → 2" onto a second line (a 61 px row): it is
  // 62 px wide and never wraps (coordinator ruling on lane M's report).
  it('keeps a two-digit cells count on one line (62 px, nowrap)', () => {
    render(
      <TransactionListTable transactions={[{ ...transfer, inputsCount: 10, outputsCount: 2 }]} />
    );
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const heads = within(table).getAllByRole('columnheader');
    const at = heads.findIndex((th) => th.textContent === 'Cells');
    expect((table.querySelectorAll('col')[at] as HTMLElement).style.width).toBe('62px');
    const cell = within(table).getAllByRole('row')[1].querySelectorAll('td')[at];
    expect(cell).toHaveTextContent(/^10 → 2$/);
    expect(cell).toHaveClass('whitespace-nowrap');
    expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(1248);
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
  it('leads each row with its hash at 13 px bright on an 18 px line; a cellbase keeps its dim row (T5)', () => {
    render(<TransactionListTable transactions={[scriptCall, cellbase]} />);
    const [, call, base] = within(screen.getByRole('table')).getAllByRole('row');
    for (const [row, tx] of [
      [call, scriptCall],
      [base, cellbase],
    ] as const) {
      const hash = within(row).getByTitle(tx.hash);
      expect(hash).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]');
      expect(hash).not.toHaveClass('text-small');
      expect(hash.closest('a')).toHaveAttribute('href', `/mainnet/tx/${tx.hash}`);
    }
    expect(base).toHaveClass('quiet');
    expect(call).not.toHaveClass('quiet');
  });
});

// The seals by kind (C2): every row leads with its transaction's row stone, the long slab (24 px
// high, 48.5 wide): rods for the cells it consumed, the glyph in its rim, rods for the cells it
// created. The hash stays the row's link.
describe('TransactionListTable row stones', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const txs = [scriptCall, transfer, cellbase];
  // A cellbase consumes no cell: the API counts its one null input (inputsCount 1), the stone 0.
  const ids: SealId[] = [
    { kind: 'tx', hash: scriptCall.hash, consumed: 3, created: 3 },
    { kind: 'tx', hash: transfer.hash, consumed: 3, created: 3 },
    { kind: 'tx', hash: cellbase.hash, consumed: 0, created: 1 },
  ];

  it('leads every table row with its stone, overhanging the hash line by 3 px, before the hash link', () => {
    render(<TransactionListTable transactions={txs} />);
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(3);
    rows.forEach((tr, i) => {
      const seal = expectOneRowSeal(tr, ids[i]);
      expect(tr.querySelector('td')).toContainElement(seal);
      // (24 − the 18 px line) / 2: the one-line row keeps its height
      expect(seal).toHaveClass('-my-[3px]');
      expectBefore(seal, within(tr).getByTitle(txs[i].hash).closest('a')!);
    });
    // a cellbase row is set back in the dim ink; its stone keeps its own state, pressed at full
    // strength: it is in a block (R4, re-pinned on purpose: the row was faded to 55 %)
    expect(rows[2]).toHaveClass('quiet');
    expect(rows[2].querySelector('[data-seal-state]')).toHaveAttribute(
      'data-seal-state',
      'pressed'
    );
  });

  it('leads every phone row with its stone in its own column, not overhanging', () => {
    atContainerWidth(358);
    render(<TransactionListTable transactions={txs} />);
    const cards = screen.getAllByTestId('transaction-row-mobile');
    expect(cards).toHaveLength(3);
    cards.forEach((card, i) => {
      const seal = expectOneRowSeal(card, ids[i]);
      expect(card.firstElementChild).toHaveAttribute('data-part', 'mark');
      expect(card.firstElementChild).toContainElement(seal);
      expect(seal).not.toHaveClass('-my-[3px]');
      expectBefore(seal, within(card).getByTitle(txs[i].hash));
    });
  });

  // The API counts a cellbase's one input, the null out point; the row knows a cellbase by
  // `isCellbase` (it sets the row back by it) and writes 0, as its Cells column does.
  it('presses a cellbase’s stone 0 → n, as its Cells column reads, never the counted null input', () => {
    expect(cellbase.inputsCount).toBe(1);
    for (const width of [null, 358] as const) {
      if (width !== null) atContainerWidth(width);
      const { unmount } = render(<TransactionListTable transactions={[cellbase]} />);
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
  it('budgets the stone and the 19-character hash in the Transaction column inside the 1,230 px', () => {
    render(<TransactionListTable transactions={txs} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const widths = Array.from(table.querySelectorAll('col')).map((c) =>
      Number.parseFloat((c as HTMLElement).style.width)
    );
    // Re-pinned on purpose (readability B5): What it did 215 → 246, from the Transaction column's
    // spare (221 → 208) and the Cycles bar (180 → 162, its track 80 → 56); 1,230 px as before.
    expect(widths).toEqual([208, 175, 246, 160, 96, 62, 121, 162]);
    expect(widths.reduce((sum, w) => sum + w, 0)).toBe(1230);
    expect(Number.parseFloat(table.style.minWidth)).toBe(1230);
    expect(ROW_SEAL.tx.footprint).toBe(56.5);
    // U-09 (re-pinned): the row size, "0x" and 8 digits, the one ellipsis, 6 digits: 17
    // characters of JetBrains Mono at 13 px (7.8; it was 19 with three dots)
    const [, row] = within(table).getAllByRole('row');
    const shown = within(row).getByTitle(scriptCall.hash);
    expect(shown).toHaveTextContent(/^0x00de304b…25cbbc$/);
    const chars = shown.textContent!.length;
    expect(chars).toBe(17);
    // the stone, the hash and the 16 px gap fit the column (re-pinned on purpose, B5: 208 px, 2.9
    // px to spare; it was 221 with 15.9) and the table changes form where it did
    const used = ROW_SEAL.tx.footprint + chars * 7.8 + 16;
    expect(used).toBeCloseTo(205.1, 6);
    expect(widths[0] - used).toBeCloseTo(2.9, 6);
    // Cycles: its 56 px track, the bar's 10 px gap, the figure of any scale under a billion
    // ("750,000,000", 11 ch at 12 px) and the 16 px gap
    expect(widths[7] - (56 + 10 + 11 * 7.2 + 16)).toBeCloseTo(0.8, 6);
    // What it did keeps the longest DAO headline whole beside its 16 px mark and two 8 px gaps
    expect(16 + 8 + 'DAO Withdraw Complete'.length * 7.2 + 8 + 16).toBeLessThanOrEqual(widths[2]);
    // Size: its 56 px track, the bar's 10 px gap, a 5-digit scale's figure (5 ch at 12 px) and the
    // 16 px gap, 3 px to spare as at the 80 px track's 145
    expect(widths[6] - (56 + 10 + 5 * 7.2 + 16)).toBeCloseTo(3, 6);
  });

  // U-09 (re-pinned; old issue C had the table and the phone row alike at 10 + 6): the size is
  // the form's, the row size in the table and the tight size in the phone row.
  it('shows the hash at the row size in the table and the tight size in the phone row', () => {
    for (const width of [1232, 608, 358]) {
      atContainerWidth(width);
      const { unmount } = render(<TransactionListTable transactions={[scriptCall]} />);
      expect(screen.getByTitle(scriptCall.hash)).toHaveTextContent(
        width === 1232 ? /^0x00de304b…25cbbc$/ : /^0x00de…cbbc$/
      );
      expect(screen.queryByRole('table') === null).toBe(width < 1230);
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  // The form changes where it did: a table from 1,230 px, the phone rows below.
  it('changes form at the same 1,230 px', () => {
    for (const [width, form] of [
      [1232, 'table'],
      [1230, 'table'],
      [1229, 'rows'],
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

describe('TransactionListTable phone row (board L2)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('is three lines at 358 px: who and when, what it did in full, then value · fee and the figures', () => {
    atContainerWidth(358);
    render(<TransactionListTable transactions={[scriptCall, cellbase]} />);
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
        // Consistency plan C4: a zero value is "0 CKB" (was "0.00000000 CKB").
        // a phone row's summary form (readability R5, re-pinned on purpose: was "0.00030000"; below 1 CKB, exact)
        left: '0 CKB · fee 0.0003 CKB',
        right: '3 → 3\u00a0cells · 903 B · 1,704,957 cycles',
      },
    ]);
    expect(call.querySelector('[data-line="what"] .truncate')).toBeNull();
    // A cellbase pays its reward on the what line; its fee is a quiet figure.
    expect(phoneLines(base).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xc89d…2cd7', expect.stringMatching(/^#20,829,775 · /)],
      // a phone row's summary form (readability R5, re-pinned on purpose: was "559.61739927")
      ['what', 'Cellbase pays #20,829,764 · reward 559.61 CKB', null],
      ['figures', '0 → 1\u00a0cell · 360 B · fee 0 CKB', '0 cycles'],
    ]);
    expect(base.closest('[role="listitem"]')).toHaveClass('quiet');
  });

  it('is two lines at 608 px: value and fee join what it did, the figures take the right edge', () => {
    atContainerWidth(608);
    render(<TransactionListTable transactions={[scriptCall]} />);
    const [call] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(call).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x00de…cbbc', expect.stringMatching(/^#20,829,775 · /)],
      [
        'what',
        // a phone row's summary form (readability R5, re-pinned on purpose: was "0.00030000")
        'Script call · .bit Time Index State · .bit Time Info · 0 CKB · fee 0.0003 CKB',
        '3 → 3\u00a0cells · 903 B · 1,704,957 cycles',
      ],
    ]);
  });
});
