import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { TransactionListTable } from '@/components/transaction/transaction-list-table';
import type { Transaction } from '@/lib/api';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';

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
  hash: `0x00de304b${'00'.repeat(26)}25cbbc`,
  isCellbase: false,
  activity: {
    txHash: `0x00de304b${'00'.repeat(26)}25cbbc`,
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
  hash: `0xcfaebc55${'00'.repeat(26)}e6dd61`,
  isCellbase: false,
  value: '76729874184',
  activity: { ...feedSamples.eightParties, typeCalls: [], protocolActions: [] },
};
const cellbase: Transaction = {
  ...base,
  hash: `0xc89d2d25${'00'.repeat(26)}9a2cd7`,
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
      'Block · age',
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
      backgroundColor: 'rgb(58, 66, 88)',
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
        right: expect.stringMatching(/^#20,829,775 · .+ ago$/),
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
        left: '0.00000000 CKB · fee 0.00030000 CKB',
        right: '3 → 3 cells · 903 B · 1,704,957 cycles',
      },
    ]);
    expect(call.querySelector('[data-line="what"] .truncate')).toBeNull();
    // A cellbase pays its reward on the what line; its fee is a quiet figure.
    expect(phoneLines(base).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xc89d2d25...9a2cd7', expect.stringMatching(/^#20,829,775 · /)],
      ['what', 'Cellbase pays #20,829,764 · reward 559.61739927 CKB', null],
      ['figures', '0 → 1 cells · 360 B · fee 0.00000000 CKB', '0 cycles'],
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
        'Script call · .bit Time Index State · .bit Time Info · 0.00000000 CKB · fee 0.00030000 CKB',
        '3 → 3 cells · 903 B · 1,704,957 cycles',
      ],
    ]);
  });
});
