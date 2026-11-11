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
      'Cycles02,000,000',
    ]);
  });

  // Table-rows R2: the bar heads are the shared ScaleHeader, at the geometry this table has always
  // drawn (an 80 px tick row 2 px under the name), so the lift changes no pixel.
  it('heads its bars with the shared ScaleHeader: 80 px ticks, 2 px under the name', () => {
    render(<TransactionListTable transactions={[scriptCall, transfer, cellbase]} />);
    const heads = within(screen.getByRole('table')).getAllByRole('columnheader');
    for (const [i, end] of [
      [6, '1,000'],
      [7, '2,000,000'],
    ] as const) {
      const ticks = heads[i].querySelector('[data-scale-ticks]') as HTMLElement;
      expect(ticks).toHaveAttribute('aria-hidden', 'true');
      expect(ticks).toHaveClass('text-meta', 'flex', 'justify-between');
      expect(ticks.style.width).toBe('80px');
      expect((ticks.parentElement as HTMLElement).style.gap).toBe('2px');
      expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual(['0', end]);
    }
  });

  it('draws its row bars 8 px tall at their exact width, never widened to a minimum', () => {
    render(<TransactionListTable transactions={[{ ...transfer, cycles: 1000 }, scriptCall]} />);
    const row = within(screen.getByRole('table')).getAllByRole('row')[1];
    // 1,000 of a 2,000,000-cycle scale on an 80 px track: 0.04 px.
    const fill = row.querySelector('td:nth-child(8) [data-segment="value"]') as HTMLElement;
    expect(fill.style.width).toBe('0.04px');
    expect((fill.parentElement as HTMLElement).style.height).toBe('8px');
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
    expect(row).toHaveClass('opacity-[.55]');
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
    expect(base).toHaveClass('opacity-[.55]');
    expect(call).not.toHaveClass('opacity-[.55]');
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
    // a cellbase row is set back, its stone with it (still pressed: it is in a block)
    expect(rows[2]).toHaveClass('opacity-[.55]');
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
      expect(row).toHaveTextContent(width === null ? '0 → 1' : '0 → 1 cells');
      unmount();
    }
  });

  // The stone's footprint (56.5 px: 48.5 and the 8 px gap) is 29.5 px more than the 27 of the seal it replaced.
  // The table had none to spare (its 1,230 px in the widest 1,232 px container; every other column
  // at its longest content), so the hash gave 4 characters: 19 → 15 at 13 px is 31.2 px.
  it('budgets the stone in the Transaction column from the hash, every column and the 1,230 px unchanged', () => {
    render(<TransactionListTable transactions={txs} />);
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    const widths = Array.from(table.querySelectorAll('col')).map(
      (c) => (c as HTMLElement).style.width
    );
    expect(widths).toEqual(['192px', '175px', '220px', '160px', '96px', '62px', '145px', '180px']);
    expect(Number.parseFloat(table.style.minWidth)).toBe(1230);
    expect(ROW_SEAL.tx.footprint).toBe(56.5);
    // "0x" and 6 digits, the ellipsis, 4 digits: 15 characters of JetBrains Mono at 13 px (7.8)
    const [, row] = within(table).getAllByRole('row');
    const shown = within(row).getByTitle(scriptCall.hash);
    expect(shown).toHaveTextContent(/^0x00de30\.\.\.cbbc$/);
    const chars = shown.textContent!.length;
    expect(chars).toBe(15);
    // the stone, the hash and the 16 px gap fit the 192 px; one more character would not
    const used = ROW_SEAL.tx.footprint + chars * 7.8 + 16;
    expect(used).toBeCloseTo(189.5, 6);
    expect(used).toBeLessThanOrEqual(192);
    expect(used + 7.8).toBeGreaterThan(192);
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
    console.info(`50 transaction rows with their stones: ${elapsed.toFixed(1)} ms`);
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
        left: '0x00de304b...25cbbc',
        // Lane P: the block's UTC+8 date; its age is the date's hover.
        right: expect.stringMatching(/^#20,829,775 · \d{4}-\d\d-\d\d$/),
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
        left: '0 CKB · fee 0.00030000 CKB',
        right: '3 → 3 cells · 903 B · 1,704,957 cycles',
      },
    ]);
    expect(call.querySelector('[data-line="what"] .truncate')).toBeNull();
    // A cellbase pays its reward on the what line; its fee is a quiet figure.
    expect(phoneLines(base).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xc89d2d25...9a2cd7', expect.stringMatching(/^#20,829,775 · /)],
      ['what', 'Cellbase pays #20,829,764 · reward 559.61739927 CKB', null],
      ['figures', '0 → 1 cells · 360 B · fee 0 CKB', '0 cycles'],
    ]);
    expect(base.closest('[role="listitem"]')).toHaveClass('opacity-[.55]');
  });

  it('is two lines at 608 px: value and fee join what it did, the figures take the right edge', () => {
    atContainerWidth(608);
    render(<TransactionListTable transactions={[scriptCall]} />);
    const [call] = screen.getAllByTestId('transaction-row-mobile');
    expect(phoneLines(call).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x00de304b...25cbbc', expect.stringMatching(/^#20,829,775 · /)],
      [
        'what',
        'Script call · .bit Time Index State · .bit Time Info · 0 CKB · fee 0.00030000 CKB',
        '3 → 3 cells · 903 B · 1,704,957 cycles',
      ],
    ]);
  });
});
