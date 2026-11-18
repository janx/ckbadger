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
import { ROW_LINE_PX } from '@/lib/type-metrics';
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
      // re-pinned on purpose (the coordinator's ruling on F3-e): the Size ticks in the tick form
      // too (it was "Size, bytes01,000")
      'Size, bytes01k',
      // re-pinned on purpose (readability B5): the Cycles ticks say the scale as a tick does
      'Cycles02M',
    ]);
  });

  // Table-rows R2: the bar heads are the shared ScaleHeader, 2 px under the name, the tick row as
  // long as the bar's track. Old issue C: the Size track is 56 px (was 80); its 24 px went to the
  // transaction's hash. Re-pinned on purpose (readability B5): the Cycles track is 56 px too (was
  // 80), its ticks "0 … 2M"; its 24 px went to What it did, whose note was cut mid-word.
  // Re-pinned on purpose (readability F3-e): each track is its head's widest tick row at 12 px,
  // "0 600,000" (64.8 px; the 56 px track was budgeted at 11 px and 12 px ticks ran past it), and
  // the Cycles head's "0 750M" under the 48 px a bar keeps at least, so 48.
  // Re-pinned on purpose (the coordinator's ruling on F3-e): the Size head writes its ticks in the
  // tick form too, "0 … 1k", so its track is 48 like the Cycles one (renamed: "Size 64.8 and
  // Cycles 48 px ticks").
  it('heads its bars with the shared ScaleHeader: Size and Cycles 48 px ticks, 2 px under the name', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table');
    const heads = within(table).getAllByRole('columnheader');
    for (const [i, end, track] of [
      [6, '1k', '48px'],
      [7, '2M', '48px'],
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
  // Re-pinned on purpose (readability F3-e): the ticks are 12 px since R4, 7.2 px a character:
  // "0" and "600,000" take 57.6 of the 64.8 px track, "0" and "750M" 36 of the 48.
  it('holds the widest tick row of each bar head inside its track', () => {
    render(
      <TransactionListTable
        transactions={[{ ...transfer, txSize: 597000, cycles: 750_000_000 }, scriptCall]}
      />
    );
    const heads = within(screen.getByRole('table')).getAllByRole('columnheader');
    for (const [i, end, used] of [
      // re-pinned on purpose (the coordinator's ruling): "600k" in the tick form, 36 of the 48 px
      [6, '600k', 36],
      [7, '750M', 36],
    ] as const) {
      const ticks = heads[i].querySelector('[data-scale-ticks]') as HTMLElement;
      const text = Array.from(ticks.children).map((t) => t.textContent!);
      expect(text).toEqual(['0', end]);
      expect(text.join('').length * 7.2).toBeCloseTo(used, 6);
      expect(used).toBeLessThan(Number.parseFloat(ticks.style.width));
    }
  });

  it('draws its row bars 8 px tall at their exact width, never widened to a minimum', () => {
    render(<TransactionListTable transactions={[{ ...transfer, cycles: 1000 }, scriptCall]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    // 1,000 of a 2,000,000-cycle scale on a 56 px track (re-pinned on purpose, readability B5: it
    // was 80, 0.04 px): 0.028, drawn to the hundredth, 0.03 px. Re-pinned on purpose (F3-e): the
    // track is 48 px, 0.024, drawn to the hundredth, 0.02 px.
    const fill = row.querySelector('td:nth-child(8) [data-segment="value"]') as HTMLElement;
    expect(fill.style.width).toBe('0.02px');
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
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
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
  // 358 and 328). The page changes both its lists' form at 1,212 px, its pool's sum
  // (transactions-page.test.tsx pins it), so only 1,232 draws the table, What it did taking its 205
  // px minimum and the 20 px beyond; every narrower width draws the phone rows, whose what-line
  // wraps by design. Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 →
  // 16 × 32.3 px gives each list's Transaction column 16 px back, so the pool's sum is 1,212 (was
  // 1,228) and What it did has 225 px at 1,232 (was 209).
  const PAGE_FORM_PX = 1212;
  const PAGE_VIEWPORTS = [1280, 1024, 768, 640, 390, 360] as const;
  /**
   * Block 0's cellbase as the committed list lists it (the live API, `/transactions?cursor=1:0`):
   * the genesis issuance, 33,600,000,000 CKB, and no block whose reward it pays. Its other fields
   * are this fixture's.
   */
  const genesis: Transaction = {
    ...cellbase,
    hash: `0x${'0e'.repeat(32)}`,
    blockNumber: 0,
    outputCapacity: '3360000000000000000',
    txSize: 1235438,
    pays: null,
  };
  const ROWS = [scriptCall, transfer, cellbase, genesis];
  /**
   * Where "a row stands on one line" does not hold: pinned, so the list can only shrink. Each
   * row's line is read from the rendered row (review fb-tests SHOULD-1): the mark's 16 px box, its 8
   * px gap, the headline's and the note's glyphs at 13 px (7.8), the 8 px between them and the cell's
   * 16 px gap; its shortfall is that line less What it did's 225 px at 1,232 (its 205 px minimum and
   * the 20 px the container gives beyond the 1,212). What it did's width moves with the other
   * columns' budgets (a row stone's footprint among them), every shortfall with it: re-pinned on
   * purpose (readability R6), the 16 px the smaller transaction stone gives back lowers each
   * shortfall by 16 (in 209 px they were 221.2, 41.8, 26.2, 41.8); none is cured, the list keeps
   * its four rows.
   */
  const KNOWN_FAULTS = [
    {
      row: 'Script call · .bit Time Index State · .bit Time Info',
      width: 1232,
      line: 430.2,
      shortfall: 205.2,
      lane: 'P3 (the lists pages)',
      why: '16 + 8 + 49 glyphs (11 + 38) × 7.8 + 8 + 16 = 430.2 px in 225 (in 209 beside the 24 px stone: 221.2 short)',
    },
    {
      row: 'CKB Transfer · 8 participants',
      width: 1232,
      line: 250.8,
      shortfall: 25.8,
      lane: 'P3 (the lists pages)',
      why: '16 + 8 + 26 glyphs (12 + 14) × 7.8 + 8 + 16 = 250.8 px in 225 (in 209 beside the 24 px stone: 41.8 short; the literal it replaces, "12 participants", was 27 glyphs: 258.6)',
    },
    {
      row: 'Cellbase · pays #20,829,764',
      width: 1232,
      line: 235.2,
      shortfall: 10.2,
      lane: 'P3 (the lists pages)',
      why: '16 + 8 + 24 glyphs (8 + 16) × 7.8 + 8 + 16 = 235.2 px in 225 (in 209 beside the 24 px stone: 26.2 short; the case the rewrite of b86847b9 dropped, review fb-tests SHOULD-1)',
    },
    {
      row: 'Cellbase · pays no reward yet (block 0)',
      width: 1232,
      line: 250.8,
      shortfall: 25.8,
      lane: 'P3 (the lists pages): cellbases get a tab and words of their own',
      why: '16 + 8 + 26 glyphs (8 + 18) × 7.8 + 8 + 16 = 250.8 px in 225 (in 209 beside the 24 px stone: 41.8 short)',
    },
  ] as const;
  it('keeps each row on one line at the page’s widths, but for its pinned known faults', () => {
    expect(KNOWN_FAULTS).toHaveLength(4);
    const widths = PAGE_VIEWPORTS.map(containerContentPx);
    expect(widths).toEqual([1232, 976, 736, 608, 358, 328]);
    const measured: { row: string; width: number; line: number; shortfall: number }[] = [];
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
        expect([sum, cols[2]]).toEqual([PAGE_FORM_PX, 205]);
        const what = width - (sum - cols[2]);
        // re-pinned on purpose (readability R6): 225 (was 209), the 16 px the stone gave back
        expect(what).toBe(225);
        const trs = Array.from(container.querySelectorAll('tbody tr'));
        expect(trs).toHaveLength(ROWS.length);
        for (const [r, tr] of trs.entries()) {
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
          const shortfall = line - what;
          if (shortfall > 1e-9)
            measured.push({
              row: `${headline.textContent} · ${note.textContent}${ROWS[r].blockNumber === 0 ? ' (block 0)' : ''}`.replace(
                /\s+/g,
                ' '
              ),
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
    expect(measured).toEqual(
      KNOWN_FAULTS.map(({ row, width, line, shortfall }) => ({ row, width, line, shortfall }))
    );
  });

  /**
   * Where a row's figure does not stand inside its column's content box (the column less its 16 px
   * gap) at 1,232 px, the one width the page draws the table: pinned. Review fb-logic 3, the lead's
   * ruling L3: the lists lane gives cellbases a tab and words of their own; here neither the words
   * nor the column change.
   */
  const FIGURE_FAULTS = [
    {
      row: 'block 0',
      column: 'Value, CKB',
      text: 'reward 33,600,000,000.00000000',
      past: 70,
      lane: 'P3 (the lists pages): cellbases get a tab and words of their own',
      why: '30 glyphs at 13 px, 234 px, in Value’s 164 px content box (its 180 px less the 16 px gap; its bound, "reward 9,999.99999999", is 21 glyphs, 163.8, so 70.2 past the bound): it draws over Fee. Genesis’s 33.6 B CKB is the first issuance, not a reward',
    },
  ] as const;
  it('holds every row’s figures inside their columns at 1,232 px, but for its pinned known fault', () => {
    expect(FIGURE_FAULTS).toHaveLength(1);
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
    const names = ['script call', 'transfer', 'cellbase', 'block 0'];
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
    expect(past).toEqual(
      FIGURE_FAULTS.map(({ row, column, text, past }) => ({ row, column, text, past }))
    );
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
      activity: {
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
      },
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
  // Re-pinned with readability R4 (lane F3, step 2): the table text is 13 px (7.8 px a glyph, was 7.2) and its key 14 px (8.4, was 7.8): the table is drawn from 1,286 px (it fitted a 1,248 px container at
  // 1,230), and narrower it takes its row form; it never scrolls.
  // Re-pinned on purpose (readability F3-e): the table is drawn from 1,220 px, and the block and
  // its age are the cell's two lines (they were one, "#… · age"), each whole on its line.
  // Re-pinned on purpose (the coordinator's ruling): 1,203 px on its own (renamed: "1,220").
  // re-pinned on purpose (reviews fb-tests 6 and fb-logic 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13,568 → 9,999" (118 → 126), and Size genesis's cellbase on a 1,500,000 scale, "1,500,000" (129 → 145): 1,227 px (renamed: "1,203").
  // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,211 px (renamed: "1,227").
  it('is drawn from its 1,211 px minimum, keeps the block and its age each on one line and drops the per-cell unit', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(Number.parseFloat(table.style.minWidth)).toBe(1211);
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

  it('draws a zero value quiet, as the board does', () => {
    render(<TransactionListTable transactions={[scriptCall]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    expect(row.querySelector('td:nth-child(4) [data-zero]')).toBeInTheDocument();
  });

  // The Cells column held "3 → 3" but wrapped "10 → 2" onto a second line (a 61 px row): it is
  // 62 px wide and never wraps (coordinator ruling on lane M's report). Re-pinned with readability
  // R4: "10 → 2" at 13 px is 46.8 px, and the gap and 2 spare make the column 65 px.
  // Re-pinned on purpose (readability F3-e): the column holds four digits a side, "9,999 →
  // 9,999" (101.4 px), and the gap: 118 px. Re-pinned on purpose (review fb-tests 6): the most
  // inputs a transaction holds, 13,568 (a block of 597,000 bytes holds that many 44-byte
  // inputs), and four digits of outputs, "13,568 → 9,999" (109.2 px), and the gap: 126 px
  // (renamed: "118 px").
  it('keeps a two-digit cells count on one line (126 px, nowrap)', () => {
    render(
      <TransactionListTable transactions={[{ ...transfer, inputsCount: 10, outputsCount: 2 }]} />
    );
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const heads = within(table).getAllByRole('columnheader');
    const at = heads.findIndex((th) => th.textContent === 'Cells');
    expect((table.querySelectorAll('col')[at] as HTMLElement).style.width).toBe('126px');
    const cell = within(table).getAllByRole('row')[1].querySelectorAll('td')[at];
    expect(cell).toHaveTextContent(/^10 → 2$/);
    expect(cell).toHaveClass('whitespace-nowrap');
    // re-pinned on purpose (the coordinator's ruling): the list is 1,203 px on its own
    // re-pinned on purpose (reviews fb-tests 6 and fb-logic 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13,568 → 9,999" (118 → 126), and Size genesis's cellbase on a 1,500,000 scale, "1,500,000" (129 → 145): 1,227 px
    // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,211 px
    expect(Number.parseFloat(table.style.minWidth)).toBe(1211);
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
      expect(hash).toHaveClass('text-body', 'text-text-bright', 'leading-row');
      expect(hash).not.toHaveClass('text-small');
      expect(hash.closest('a')).toHaveAttribute('href', `/mainnet/tx/${tx.hash}`);
    }
    expect(base).toHaveClass('quiet');
    expect(call).not.toHaveClass('quiet');
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
  const txs = [scriptCall, transfer, cellbase];
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
    expect(rows).toHaveLength(3);
    rows.forEach((tr, i) => {
      const seal = expectOneRowSeal(tr, ids[i]);
      expect(tr.querySelector('td')).toContainElement(seal);
      // (the 20 px line − 16) / 2 clear above and below: the one-line row keeps its height
      expect((ROW_LINE_PX - ROW_SEAL.tx.height) / 2).toBe(2);
      expect(seal).toHaveClass('h-row', 'items-center');
      expect(seal.className).not.toMatch(/-my-/);
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
    expect(widths).toEqual([200, 110, 204, 180, 102, 126, 145, 144]);
    expect(widths.reduce((sum, w) => sum + w, 0)).toBe(1211);
    expect(Number.parseFloat(table.style.minWidth)).toBe(1211);
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
    // Cycles: its 48 px track (F3-e: it was 56), the bar's 10 px gap and the figure of any scale
    // under a billion ("750,000,000", 11 ch at 13 px since R4, 12 before); the last column, no gap
    // after it (F3-e: it counted 16)
    expect(widths[7] - (48 + 10 + 11 * 7.8)).toBeCloseTo(0.2, 6);
    // What it did keeps the longest DAO headline whole beside its 16 px mark and its 8 px gap.
    // Re-pinned on purpose (F3-e): this counted the 8 px between headline and note too, which the
    // headline's line does not take where the note goes under it, as it does at the minimum.
    expect(16 + 8 + 'DAO Withdraw Complete'.length * 7.8 + 16).toBeLessThanOrEqual(widths[2]);
    // Size: its track holds its head's "0 600,000" at 12 px (64.8; F3-e: it was 56, which 12 px
    // ticks ran past), the bar's 10 px gap, the scale's figure "600,000" (7 ch at 13 px; it counted
    // a 5-digit "2,000") and the gap; 0.6 of rounding up to a whole pixel
    // Re-pinned on purpose (the coordinator's ruling): the track is 48 px under its head's tick-form
    // "0 … 600k" (it was 64.8 for the exact "0 600,000"); 0.4 of rounding up to a whole pixel
    // Re-pinned on purpose (review fb-logic 6): the scale's figure is genesis's cellbase's,
    // "1,500,000" (9 ch at 13 px), under "0 … 1.5M"; 0.8 of rounding up to a whole pixel
    expect(widths[6] - (48 + 10 + 9 * 7.8 + 16)).toBeCloseTo(0.8, 6);
  });

  // U-09 (re-pinned; old issue C had the table and the phone row alike at 10 + 6): the size is
  // the form's, the row size in the table and the tight size in the phone row.
  it('shows the hash at the row size in the table and the tight size in the phone row', () => {
    // re-pinned (readability R4): the table is drawn from 1,286 px (was 1,230)
    // re-pinned (readability F3-e): the table is drawn from 1,220 px, so at 1,232 too
    // re-pinned (the coordinator's ruling): on its own from 1,203 px
    // re-pinned (reviews fb-tests 6, fb-logic 6): on its own from 1,227 px
    // re-pinned (readability R6, the transaction stone 16 px): on its own from 1,211 px
    for (const width of [1232, 1211, 1210, 608, 358]) {
      atContainerWidth(width);
      const { unmount } = render(<TransactionListTable transactions={[scriptCall]} />);
      expect(screen.getByTitle(scriptCall.hash)).toHaveTextContent(
        width >= 1211 ? /^0x00de304b…25cbbc$/ : /^0x00de…cbbc$/
      );
      expect(screen.queryByRole('table') === null).toBe(width < 1211);
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
  it('changes form at its 1,211 px minimum', () => {
    for (const [width, form] of [
      [1232, 'table'],
      [1211, 'table'],
      [1210, 'rows'],
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
  /** A head's tick row whole over its track: the ticks at 12 px, one glyph apart, 48 at least. */
  const tickRow = (...ticks: string[]) => Math.max(48, at(ticks.join(' '), 12));
  const budgets = [
    // the stone and its gap, the hash at the row size at 14 px
    ceil(ROW_SEAL.tx.footprint + at('0x00de304b…25cbbc', 14) + 16),
    // "#99,999,999" over "10y 364d ago", the wider of its two lines
    ceil(Math.max(at('#99,999,999', 13), at('10y 364d ago', 13)) + 16),
    // the 16 px mark, its 8 px gap and "DAO Withdraw Complete" whole; its note goes under it
    ceil(16 + 8 + at('DAO Withdraw Complete', 13) + 16),
    // a value under a billion CKB, or a cellbase's "reward" and four whole digits
    ceil(Math.max(at('999,999,999.99999999', 13), at('reward 9,999.99999999', 13)) + 16),
    // a fee under 100 CKB
    ceil(at('99.99999999', 13) + 16),
    // the most inputs a transaction holds, 13,568 (597,000 bytes of 44-byte inputs), and four
    // digits of outputs (re-pinned on purpose, review fb-tests 6: it was four digits a side)
    ceil(at('13,568 → 9,999', 13) + 16),
    // a transaction is no larger than its block, and the largest block is genesis: its 1,235,438
    // byte cellbase on a 1,500,000 scale, "0 1.5M" over the 48 px track (the tick form, the
    // coordinator's ruling), "1,500,000" beside it (re-pinned on purpose, review fb-logic 6: it
    // was the 597,000-byte limit's "0 600k" and "600,000")
    ceil(tickRow('0', '1.5M') + 10 + at('1,500,000', 13) + 16),
    // a scale under a billion cycles: its ticks in the tick form ("1.5k" the widest), the exact
    // "750,000,000" beside the bar; the last column
    ceil(tickRow('0', '1.5k') + 10 + at('750,000,000', 13)),
  ];

  // re-pinned on purpose (the coordinator's ruling): 1,203 px on its own (renamed: "1,220 px")
  // re-pinned on purpose (reviews fb-tests 6 and fb-logic 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13,568 → 9,999" (118 → 126), and Size genesis's cellbase on a 1,500,000 scale, "1,500,000" (129 → 145): 1,227 px (renamed: "1,203 px")
  // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,211 px (renamed: "1,227 px")
  it('pins each column to its content and the table to their sum, 1,211 px', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(budgets).toEqual([200, 110, 204, 180, 102, 126, 145, 144]);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols[2].width).toBe('auto');
    expect(budgets.reduce((sum, w) => sum + w, 0)).toBe(1211);
    expect(Number.parseFloat(table.style.minWidth)).toBe(1211);
    // each bar's track holds its head's widest tick row
    const heads = within(table).getAllByRole('columnheader');
    expect((heads[6].querySelector('[data-scale-ticks]') as HTMLElement).style.width).toBe(
      `${tickRow('0', '1.5M')}px`
    );
    expect((heads[7].querySelector('[data-scale-ticks]') as HTMLElement).style.width).toBe('48px');
  });

  // The coordinator's ruling: a head's ticks are in the tick form, so the Size head writes
  // "0 … 600k" (43.2 px) and its track is the 48 px a bar keeps at least, as the Cycles one is;
  // on its own the list is 1,203 px (the page changes both its lists' form at the larger sum).
  // Re-pinned on purpose (reviews fb-tests 6, fb-logic 6): the column is budgeted for genesis's
  // cellbase, "1,500,000" (145), Cells for 13,568 inputs (126): 1,227 px (renamed: "1,203").
  // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,211 px (renamed: "1,227").
  it('heads Size in the tick form on a 48 px track: 1,211 px on its own', () => {
    render(
      <TransactionListTable
        transactions={[{ ...transfer, txSize: 597000, cycles: 750_000_000 }, scriptCall]}
      />
    );
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const heads = within(table).getAllByRole('columnheader');
    const ticks = heads[6].querySelector('[data-scale-ticks]') as HTMLElement;
    expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual(['0', '600k']);
    expect(ticks.style.width).toBe('48px');
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(ceil(48 + 10 + at('1,500,000', 13) + 16)).toBe(145);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual([
      200, 110, 204, 180, 102, 126, 145, 144,
    ]);
    expect(Number.parseFloat(table.style.minWidth)).toBe(1211);
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
  it('changes form where the sum stops fitting: a table at 1,232 and 1,211, rows below', () => {
    for (const [width, form] of [
      [1232, 'table'],
      [1211, 'table'],
      [1210, 'rows'],
      [608, 'rows'],
      [358, 'rows'],
    ] as const) {
      atContainerWidth(width);
      const { container, unmount } = render(
        <TransactionListTable transactions={[scriptCall, transfer, cellbase]} />
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
        // a phone row's summary form (readability R5, re-pinned on purpose: was "0.00030000"; below
        // 1 CKB, exact)
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
