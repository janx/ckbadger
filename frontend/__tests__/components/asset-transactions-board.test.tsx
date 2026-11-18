// OB-13 / OB-C9 (Collection / CollectionTxs / ObjectTxs boards): the Size and Cycles headers carry
// their column's shared scale, and a collection's Transactions name the collection in a column of
// its own right after Cells.
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { AssetTransactions } from '@/components/asset/asset-feed';
import type { Transaction } from '@/lib/api';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';

const tx = (n: number, size: number, cycles: number | null): Transaction => ({
  hash: `0x${n.toString(16).padStart(2, '0').repeat(32)}`,
  blockNumber: 1000 + n,
  blockHash: `0x${'aa'.repeat(32)}`,
  index: 1,
  inputsCount: 1,
  outputsCount: 2,
  fee: '1000',
  value: '0',
  outputCapacity: '50000000000',
  txSize: size,
  cycles,
  cyclesStatus: cycles === null ? 'pending' : 'complete',
  isCellbase: false,
  timestamp: '1700000000000',
});

describe('AssetTransactions board columns', () => {
  it('heads Size and Cycles with the scale every bar in the column shares (OB-13)', () => {
    render(<AssetTransactions rows={[tx(1, 1930, 1_923_713), tx(2, 640, 0)]} />);
    const table = screen.getByRole('table', { name: 'Asset transactions' });
    const size = within(table).getByRole('columnheader', { name: /Size/ });
    expect(size.querySelector('[data-scale-ticks]')).toHaveTextContent('02,000');
    const cycles = within(table).getByRole('columnheader', { name: /Cycles/ });
    expect(cycles.querySelector('[data-scale-ticks]')).toHaveTextContent('02,000,000');
  });

  it('puts the collection column, titled by the collection, right after Cells (OB-C9)', () => {
    render(
      <AssetTransactions
        rows={[tx(1, 1930, 1_923_713)]}
        context={{ label: 'Nervapes', placement: 'after-cells', render: () => '3 minted' }}
      />
    );
    const headers = screen.getAllByRole('columnheader').map((h) => h.textContent);
    expect(headers.slice(0, 4)).toEqual(['Transaction', 'Block · age', 'Cells', 'Nervapes']);
  });

  // Readability F3-e: every column budgeted from what it holds, at 0.6 em a glyph (13 px cells,
  // 12 px notes, units and ticks) and the 16 px gap after every column but the last. The bar
  // columns hold their heads' 120 px tick rows; a feed's own words (a context, an identity's
  // action) hold 12 characters at their minimum and wrap.
  describe('budgets every column from what it holds (F3-e)', () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5;
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const minWidths = () =>
      [...screen.getByRole('table', { name: 'Asset transactions' }).querySelectorAll('col')].map(
        (c) => Number.parseFloat((c as HTMLElement).style.minWidth)
      );
    const base = {
      // the hash at the row size
      tx: ceil(at('0x01010101…010101', 13) + 16),
      // "#99,999,999 · 10y 364d ago" on its line, "tx 9999 in block" under it
      block: ceil(Math.max(at('#99,999,999 · 10y 364d ago', 13), at('tx 9999 in block', 12)) + 16),
      // four digits a side, as written
      cells: ceil(at('9999 → 9999', 13) + 16),
      // under a million CKB on one line with its unit; more breaks before the decimals
      output: ceil(at('999,999.99999999', 13) + at(' CKB', 12) + 16),
      // a fee under 100 CKB with its unit
      fee: ceil(at('99.99999999', 13) + at(' CKB', 12) + 16),
      // the head's 120 px tick row ("0 600,000" in it), wider than the bar's 80 and its count
      size: 120 + 16,
      // the head's 120 px tick row ("0 4,000,000,000", 108 px, in it); the last column
      cycles: 120,
    };

    it('in the plain feed: 1,027 px, a table from it', () => {
      expect(Object.values(base)).toEqual([149, 219, 102, 170, 131, 136, 120]);
      render(<AssetTransactions rows={[tx(1, 1930, 1_923_713), tx(2, 640, 0)]} />);
      expect(minWidths()).toEqual(Object.values(base));
      expect(minWidths().reduce((s, w) => s + w, 0)).toBe(1027);
    });

    it('with a token and a collection column: the token’s decimals, a stated context', () => {
      // a token feed's row carries its activity (what moved is read from it): none moved here
      const moved = {
        ...tx(1, 1930, 1_923_713),
        activity: { participants: [] } as unknown as Transaction['activity'],
      };
      render(
        <AssetTransactions
          rows={[moved]}
          token={{ typeHash: '0xabc', symbol: 'T', decimals: 8 }}
          context={{ label: 'Nervapes', placement: 'after-cells', render: () => '3 minted' }}
        />
      );
      expect(minWidths()).toEqual([
        base.tx,
        base.block,
        base.cells,
        // the collection's words: 12 characters
        ceil(at('x'.repeat(12), 13) + 16),
        // under a billion whole tokens at the token's 8 decimals, on one line
        ceil(at('999,999,999.99999999', 13) + 16),
        base.output,
        base.fee,
        base.size,
        base.cycles,
      ]);
    });

    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    it('changes form where the plain feed’s sum stops fitting', () => {
      for (const [width, form] of [
        [1232, 'table'],
        [1027, 'table'],
        [1026, 'rows'],
        [608, 'rows'],
        [358, 'rows'],
      ] as const) {
        atContainerWidth(width);
        const { container, unmount } = render(
          <AssetTransactions rows={[tx(1, 1930, 1_923_713)]} />
        );
        expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form);
        unmount();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
      }
    });
  });

  it('keeps an item context at the end of the row (OB-13)', () => {
    render(
      <AssetTransactions
        rows={[tx(1, 1930, null)]}
        context={{ label: 'Object · cell after', render: () => 'to ckb1… (output 1)' }}
      />
    );
    const headers = screen.getAllByRole('columnheader').map((h) => h.textContent);
    expect(headers.at(-1)).toBe('Object · cell after');
  });
});

// ID-C8 (IdentityBit / BitCell / Did / Dotcell boards): an identity's Transactions read
// Transaction · Block · date · <standard> action · Cells · Fee · Size · This name's cell after it —
// no Output capacity and no Cycles columns.
describe('AssetTransactions identity columns (ID-C8)', () => {
  it('uses the identity boards’ columns, the block dated on the UTC+8 clock', () => {
    render(
      <AssetTransactions
        rows={[tx(1, 1930, null)]}
        identity={{ actionLabel: '.bit action', action: () => 'Renewed' }}
        context={{ label: "This name's cell after it", render: () => 'Output 0 · cell after' }}
      />
    );
    const table = screen.getByRole('table', { name: 'Asset transactions' });
    const headers = within(table)
      .getAllByRole('columnheader')
      .map((h) => h.textContent);
    expect(headers.map((h) => h!.replace(/0[\d,]+$/, ''))).toEqual([
      'Transaction',
      'Block · date',
      '.bit action',
      'Cells',
      'Fee',
      'Size, bytes',
      "This name's cell after it",
    ]);
    const row = within(table).getAllByRole('row')[1];
    // 1700000000000 ms = 2023-11-15 06:13:20 UTC+8
    expect(row).toHaveTextContent('#1,001 · 2023-11-15');
    expect(row).toHaveTextContent('Renewed');
  });
});

describe('AssetTransactions phone row (board L2 rules)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('is three lines at 358 px: hash and block · what it did on the asset and the CKB out · the figures', () => {
    atContainerWidth(358);
    render(
      <AssetTransactions
        rows={[tx(1, 1930, 1_923_713)]}
        context={{ label: 'Nervapes', placement: 'after-cells', render: () => 'Minted 3' }}
      />
    );
    expect(screen.queryByRole('table')).toBeNull();
    const [row] = screen.getAllByTestId('asset-tx-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      // U-09 (re-pinned): the phone row's hash at the tight size
      ['identity', '0x0101…0101', expect.stringMatching(/^#1,001 · .+ ago$/)],
      // a phone row is the summary form (readability R5, re-pinned on purpose: was padded to 8
      // decimals)
      ['what', 'Minted 3', '500 CKB'],
      [
        'figures',
        '1 → 2\u00a0cells · fee 0.00001 CKB · 1,930 B · 1,923,713 cycles · tx 1 in block',
        null,
      ],
    ]);
  });

  it('keeps an item context under its label and leaves cycles off an identity row', () => {
    atContainerWidth(358);
    render(
      <AssetTransactions
        rows={[tx(1, 640, 0)]}
        identity={{ actionLabel: '.bit action', action: () => 'Renewed' }}
        context={{ label: 'Cell after', render: () => 'output 1' }}
      />
    );
    const [row] = screen.getAllByTestId('asset-tx-row-mobile');
    const lines = phoneLines(row);
    expect(lines[0].right).toMatch(/^#1,001 · \d{4}-\d{2}-\d{2}$/);
    expect(lines[1]).toMatchObject({ left: 'Renewed · Cell after: output 1', right: null });
    // a phone row is the summary form (readability R5, re-pinned on purpose: was padded to 8
    // decimals)
    expect(lines[2].left).toBe('1 → 2\u00a0cells · fee 0.00001 CKB · 640 B · tx 1 in block');
  });

  it('folds into two lines at 608 px', () => {
    atContainerWidth(608);
    render(<AssetTransactions rows={[tx(1, 1930, null)]} />);
    const [row] = screen.getAllByTestId('asset-tx-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x0101…0101', expect.stringMatching(/ago$/)],
      [
        'what',
        // a phone row is the summary form (readability R5, re-pinned on purpose: was padded to 8
        // decimals)
        '500 CKB',
        '1 → 2\u00a0cells · fee 0.00001 CKB · 1,930 B · cycles not computed · tx 1 in block',
      ],
    ]);
  });
});
