// OB-13 / OB-C9 (Collection / CollectionTxs / ObjectTxs boards): the Size and Cycles headers carry
// their column's shared scale, and a collection's Transactions name the collection in a column of
// its own right after Cells.
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { AssetTransactions } from '@/components/asset/asset-feed';
import type { Transaction } from '@/lib/api';
import {
  atContainerWidth,
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { PHONE_LINE_PX } from '@/lib/type-metrics';
import { activityRead, ITEM_TYPE } from '@/__tests__/utils/activity-read';

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
    // re-pinned on purpose (the coordinator's ruling on F3-e): the tick form, "2k" and "2M" (they
    // were the exact "2,000" and "2,000,000")
    expect(size.querySelector('[data-scale-ticks]')).toHaveTextContent('02k');
    const cycles = within(table).getByRole('columnheader', { name: /Cycles/ });
    expect(cycles.querySelector('[data-scale-ticks]')).toHaveTextContent('02M');
  });

  // The coordinator's ruling (F3-e): a head's tick row is in the tick form and stands whole over
  // its track; the stacked bars drew 80 px under heads with 120 px of ticks. Head and bars share one
  // 48 px track, and each column holds its head's name, its figure and the track: Size the name
  // "Size, bytes" (79.2), Cycles the figure "750,000,000" (85.8), the last column.
  it('draws Size and Cycles on the 48 px track their tick-form heads stand over', () => {
    render(<AssetTransactions rows={[tx(1, 1930, 1_923_713), tx(2, 640, 0)]} />);
    const table = screen.getByRole('table', { name: 'Asset transactions' });
    for (const [name, end] of [
      [/Size/, '2k'],
      [/Cycles/, '2M'],
    ] as const) {
      const ticks = within(table)
        .getByRole('columnheader', { name })
        .querySelector('[data-scale-ticks]') as HTMLElement;
      expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual(['0', end]);
      expect(ticks.style.width).toBe('48px');
    }
    const tracks = table.querySelectorAll<HTMLElement>('tbody [role="img"]');
    expect(tracks.length).toBeGreaterThan(0);
    for (const track of tracks) expect(track.style.width).toBe('48px');
    const at = (text: string, px: number) => (text.length * px * 3) / 5;
    const cols = [...table.querySelectorAll('col')].map((c) =>
      Number.parseFloat((c as HTMLElement).style.minWidth)
    );
    expect(cols.slice(-2)).toEqual([
      Math.ceil(at('Size, bytes', 12) + 16),
      Math.ceil(at('750,000,000', 13)),
    ]);
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
  // columns hold their heads' tick rows on 48 px tracks (re-pinned on purpose, the coordinator's
  // ruling: they held 120 px rows of exact ticks); a feed's own words (a context, an identity's
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
      // re-pinned on purpose (lane p-assets, the page lane notes): a transaction's inputs to
      // MAX_TX_INPUTS, five digits (13,568), its outputs four, as written (it was four a side, 102)
      cells: ceil(at('13568 → 9999', 13) + 16),
      // under a million CKB on one line with its unit; more breaks before the decimals
      output: ceil(at('999,999.99999999', 13) + at(' CKB', 12) + 16),
      // a fee under 100 CKB with its unit
      fee: ceil(at('99.99999999', 13) + at(' CKB', 12) + 16),
      // the head's name, "Size, bytes", wider than the figure ("597,000") and the 48 px track
      size: ceil(Math.max(at('Size, bytes', 12), at('597,000', 13), 48) + 16),
      // the figure, a transaction's cycles under a billion ("750,000,000"); the last column
      cycles: ceil(Math.max(at('Cycles', 12), at('750,000,000', 13), 48)),
    };

    // re-pinned on purpose (the coordinator's ruling): Size 136 → 96, Cycles 120 → 86, 1,027 → 953;
    // then (lane p-assets) Cells 102 → 110, five digits of inputs, 953 → 961 (renamed, it was "953")
    it('in the plain feed: 961 px, a table from it', () => {
      expect(Object.values(base)).toEqual([149, 219, 110, 170, 131, 96, 86]);
      render(<AssetTransactions rows={[tx(1, 1930, 1_923_713), tx(2, 640, 0)]} />);
      expect(minWidths()).toEqual(Object.values(base));
      expect(minWidths().reduce((s, w) => s + w, 0)).toBe(961);
    });

    it('with a token and a collection column: the token’s decimals, a stated context', () => {
      // a token feed's row carries its activity (what moved is read from it): none moved here
      const moved = {
        ...tx(1, 1930, 1_923_713),
        // re-pinned on purpose (lane feed, part 4): the row's stated items (none) are read
        activity: { participants: [], items: [] } as unknown as Transaction['activity'],
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
        // re-pinned on purpose (lane p-assets): the sum is 961 px (was 953)
        [961, 'table'],
        [960, 'rows'],
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
    // the bar head's tick row stripped: "0" and its scale in the tick form (re-pinned on purpose,
    // the coordinator's ruling on F3-e: it was the exact "2,000")
    expect(headers.map((h) => h!.replace(/0[\d.]+[kMBT]?$/, ''))).toEqual([
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
    // Re-pinned on purpose (lane D4 follow-up, the address page's transaction row): the output
    // capacity left line 2's right (it was a bare "500 CKB"), and the bytes, the cycles and the
    // index left the quiet line ("· 1,930 B · 1,923,713 cycles · tx 1 in block"); the table and the
    // cards from 560 px keep them
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      // U-09 (re-pinned): the phone row's hash at the tight size
      ['identity', '0x0101…0101', expect.stringMatching(/^#1,001 · .+ ago$/)],
      ['what', 'Minted 3', null],
      ['figures', '1 → 2\u00a0cells · fee 0.00001 CKB', null],
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
    // re-pinned on purpose (lane D4 follow-up): the context is named by its label as a labelled
    // figure ("Cell after output 1", was "Cell after: output 1"), and the bytes and the index left
    // the quiet line (it read "… · 640 B · tx 1 in block")
    expect(lines[1]).toMatchObject({ left: 'Renewed · Cell after output 1', right: null });
    expect(lines[2].left).toBe('1 → 2\u00a0cells · fee 0.00001 CKB');
  });

  // Re-pinned on purpose (lane D4 follow-up): the card at 608 px is the phone row's parts, its quiet
  // line keeping the output, the bytes, the cycles and the index where no table stands (it was two
  // lines: a bare "500 CKB" with the figures at its right)
  it('keeps the output, bytes, cycles and index on the quiet line of the 608 px card', () => {
    atContainerWidth(608);
    render(<AssetTransactions rows={[tx(1, 1930, null)]} />);
    const [row] = screen.getAllByTestId('asset-tx-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x0101…0101', expect.stringMatching(/ago$/)],
      [
        'figures',
        // a phone row is the summary form (readability R5)
        '1 → 2\u00a0cells · fee 0.00001 CKB · output 500 CKB · 1,930 B · cycles not computed · tx 1 in block',
        null,
      ],
    ]);
  });
});

// Lane D4 follow-up (the coordinator's item 2): an asset's transactions on a phone (a token's page,
// a collection's, an identity's) are the address page's transaction row: the tight hash and the
// block with its age (a date on the identity boards, as their table writes it); what the row did
// on this asset where the feed says (the identity's action, the feed's own column, named); quiet,
// the cells, the token's amount moved with its symbol and the fee, each named. The output capacity,
// the bytes, the cycles and the index in the block leave the phone row for the table and the
// transaction's page; the cards from 560 px keep them at the end of the quiet line.
describe('an asset’s transactions on a phone (lane D4 follow-up)', () => {
  const USDC = `0x${'5d'.repeat(32)}`;
  const moved = (n: number): Transaction => ({
    ...tx(n, 1930, 1_923_713),
    activity: activityRead({
      txHash: `0x${n.toString(16).padStart(2, '0').repeat(32)}`,
      blockNumber: 1000 + n,
      txIndex: 1,
      timestamp: '1700000000000',
      isCellbase: false,
      protocolActions: [],
      typeCalls: [],
      lockCalls: [],
      itemTypes: [ITEM_TYPE.xudt],
      itemTypesUnread: null,
      participants: [
        {
          address: seededAddress('usdc1'),
          lockHash: `0x${'71'.repeat(32)}`,
          lockHashPrefix: null,
          roles: [],
          tags: 0,
          ckbDelta: '-1000',
          usedDelta: '0',
          itemDeltas: [
            {
              kind: 'token',
              typeScriptHash: USDC,
              delta: '2000000000',
              symbol: 'USDC',
              decimals: 6,
            },
          ],
        },
      ],
    }),
  });

  it('is the address row at 358 px: hash and block · age, then cells · amount moved · fee', () => {
    atContainerWidth(358);
    render(
      <AssetTransactions
        rows={[moved(1)]}
        token={{ typeHash: USDC, symbol: 'USDC', decimals: 6 }}
      />
    );
    const [row] = screen.getAllByTestId('asset-tx-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['identity', 'head', '0x0101…0101', expect.stringMatching(/^#1,001 · .+ ago$/)],
      ['figures', 'quiet', '1 → 2\u00a0cells · 2,000 USDC moved · fee 0.00001 CKB', null],
    ]);
    const figures = row.querySelector('[data-line="figures"]')!;
    expect(dottedItems(figures)).toEqual([
      '1 → 2\u00a0cells',
      '2,000 USDC moved',
      'fee 0.00001 CKB',
    ]);
    expect(
      Array.from(figures.querySelectorAll('[data-role="figure-label"]')).map((l) => l.textContent)
    ).toEqual(['USDC moved', 'fee']);
    // the output capacity, the bytes, the cycles and the index leave the phone row
    expect(row.textContent).not.toMatch(/500 CKB|1,930 B|cycles|in block/);
  });

  it('names what the row did on this asset where the feed says, on line 2', () => {
    atContainerWidth(358);
    render(
      <AssetTransactions
        rows={[tx(1, 640, 0)]}
        identity={{ actionLabel: '.bit action', action: () => 'Renewed' }}
        context={{ label: 'Cell after', render: () => 'output 1' }}
      />
    );
    const [row] = screen.getAllByTestId('asset-tx-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0x0101…0101', expect.stringMatching(/^#1,001 · \d{4}-\d{2}-\d{2}$/)],
      ['what', 'Renewed · Cell after output 1', null],
      ['figures', '1 → 2\u00a0cells · fee 0.00001 CKB', null],
    ]);
  });

  // The board's head(): the tight hash at 14 px (92.4), the 12 px between the parts and the block
  // with the oldest age a live list writes at 13 px, "#99,999,999 · 10y 364d ago" (26 characters,
  // 202.8): 307.2 of a 360 px phone's 328 px line (no stone, as the table's).
  it('states its first line, 307.2 of 328 px', () => {
    atContainerWidth(PHONE_LINE_PX[360]);
    render(<AssetTransactions rows={[tx(1, 640, 0)]} />);
    const [row] = screen.getAllByTestId('asset-tx-row-mobile');
    expect(statedHeadPx(row)).toBe(307.2);
    expect(phoneHeadPx(row, 360)).toBeLessThanOrEqual(statedHeadPx(row));
  });
});
