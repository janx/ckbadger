import { SHEETS } from '@/components/transaction/tx-structure';
import { boxPx, containerContentPx } from '@/__tests__/utils/tailwind-box';
import { ckbAddress } from '@/__tests__/utils/ckb-address';
import { LARGEST_TX_BYTES } from '@/lib/list-bounds';
import { occupiedBytesText } from '@/lib/occupied';
import { describe, it, expect, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import { CellTile } from '@/components/ui/cell-tile';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { TxStructure } from '@/components/transaction/tx-structure';
import type { TransactionDetail, TransactionScriptGroup } from '@/lib/api';
import { drawSeal } from '@/lib/seal/stones';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { OWNER_SEAL } from '@/components/ui/owner-seal';
import { fontPx, textPx } from '@/lib/type-metrics';
import { sealedTile } from '@/__tests__/utils/cell-tile-seal';
import {
  expectBefore,
  expectOneRowSeal,
  expectOwnerSealOf,
  sealHash,
} from '@/__tests__/utils/seals';

/**
 * C11: in a tile the cell's stone and the owner's stand alone in the 18 px mark column, the
 * column's 10 px gap their gap, so neither keeps its own trailing margin.
 */
const IN_MARK_COLUMN = { gap: false };

// The drawing is counted: every seal on a tile is drawn once per id, however often it renders.
vi.mock('@/lib/seal/stones', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/seal/stones')>();
  return { ...actual, drawSeal: vi.fn(actual.drawSeal) };
});

describe('CellTile occupied wording (TX-19)', () => {
  // B4 fix round (re-pinned on purpose, tests MUST-2): "occupied" always carries its unit, as a
  // figure does ("occupied 195 B"); it was "occupied 195"
  it('states occupied capacity as whole bytes with their unit ("occupied 195 B")', () => {
    render(
      <CellTile
        cell={sealedTile({
          id: 'in:0',
          testId: 'tile',
          href: '/cell/0x-0',
          label: 'input 0',
          owner: 'owner',
          lockKind: 'Default Lock',
          kind: 'other',
          content: 'CKBFS file',
          capacity: '19500000000',
          occupiedBytes: '195',
          state: 'consumed',
        })}
      />
    );
    const tile = screen.getByTestId('tile');
    expect(tile).toHaveTextContent('occupied 195 B');
    expect(tile).not.toHaveTextContent('occupied 195.00000000');
  });

  // B4 second fix round (review-rules-fixes-logic NOTE 4, the coordinator's item 10): a cell's
  // occupied bytes reach the tile as the API gives them (commonKnowledgeSize), with no shannons
  // between (the tiles multiplied by 10^8 and the tile divided again)
  it('takes the cell’s occupied bytes as the API gives them, no shannons between', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    expect(outputs[0]).toMatchObject({ occupiedBytes: '61' });
    expect(outputs[0]).not.toHaveProperty('occupied');
  });

  // B4 second fix round (re-pinned on purpose): the tile takes bytes, so a figure that is not a
  // whole number of bytes is refused as one (it took shannons and refused one not a multiple of
  // 10^8); the change that is not whole bytes is refused where shannons are read, lib/occupied
  it('refuses an occupied amount that is not whole bytes', () => {
    expect(() =>
      render(
        <CellTile
          cell={sealedTile({
            id: 'in:0',
            href: '/cell/0x-0',
            label: 'input 0',
            owner: 'owner',
            lockKind: null,
            kind: 'ckb',
            content: 'CKB',
            capacity: '19500000000',
            occupiedBytes: '61.00000001',
            state: 'consumed',
          })}
        />
      )
    ).toThrow(
      'in:0: occupied bytes: expected a non-negative decimal integer, received "61.00000001"'
    );
  });
});

/* ---------- the seals by kind (C3) ---------- */

const HASH = sealHash('c3 tile transaction');
const PREV = sealHash('c3 tile previous transaction');
/** The real script hash of { code_hash 0x9b × 32, hash_type type, args 0x } (blake2b, CKB). */
const LOCK = '0x82cbea9785c59f24117b45294cff296e8ab0ac2f29bbfa78af6b01e4d5325644';
const NEW_OWNER = sealHash('c3 tile new owner');
const TOKEN = sealHash('c3 tile token type');
const lock = { codeHash: `0x${'9b'.repeat(32)}`, hashType: 'type', args: '0x' };
// B4 second fix round (tests MUST-1): an owner's address is the real address of the cell's own
// lock (a tile refuses one that spells another); these failed the bech32m checksum
const ADDRESS = ckbAddress(lock as { codeHash: string; hashType: 'type'; args: string });
const OWNER_ADDRESS = ckbAddress(
  scriptHashToScript(NEW_OWNER) as { codeHash: string; hashType: 'type'; args: string }
);
const token = { codeHash: sealHash('c3 xudt code'), hashType: 'type', args: '0x' };
const tokenDisplay = {
  kind: 'token' as const,
  standard: 'xudt',
  id: TOKEN,
  name: 'USDI',
  collectionId: null,
  collectionName: null,
  symbol: 'USDI',
  decimals: 6,
  amount: '500000000',
  noAmountReason: null,
  compositionTier: null,
};
const group = (
  kind: 'lock' | 'type',
  ran: boolean,
  scriptHash: string,
  inputIndices: number[],
  outputIndices: number[]
): TransactionScriptGroup => ({
  scriptHash,
  kind,
  ran,
  inputIndices,
  outputIndices,
  witnessIndex: ran ? (inputIndices[0] ?? outputIndices[0]) : null,
  script: scriptHashToScript(scriptHash),
  cycles: null,
});
function scriptHashToScript(hash: string) {
  return hash === LOCK ? lock : hash === TOKEN ? token : { ...lock, args: hash.slice(0, 42) };
}
/**
 * A token transfer as the API serves it: input 0 plain and input 1 a USDI cell, both of LOCK;
 * output 0 the plain change back to LOCK, output 1 the USDI cell to a new owner.
 */
function transfer(
  status: 'committed' | 'pending',
  outputStatus: 'live' | 'consumed' | 'pending' = status === 'committed' ? 'live' : 'pending'
): TransactionDetail {
  const plain = { capacity: '6100000000', commonKnowledgeSize: '61', display: null, lock };
  const usdi = {
    capacity: '14200000000',
    commonKnowledgeSize: '142',
    type: token,
    cellType: 'xudt',
    display: tokenDisplay,
  };
  return {
    hash: HASH,
    status,
    isCellbase: false,
    inputs: [
      { ...plain, address: ADDRESS, previousOutput: { txHash: PREV, index: 4 } },
      { ...usdi, lock, address: ADDRESS, previousOutput: { txHash: PREV, index: 5 } },
    ],
    outputs: [
      { ...plain, address: ADDRESS, status: outputStatus, isChange: true },
      {
        ...usdi,
        lock: scriptHashToScript(NEW_OWNER),
        address: OWNER_ADDRESS,
        status: outputStatus,
        isChange: false,
      },
    ],
    scriptGroups: [
      group('lock', true, LOCK, [0, 1], []),
      group('type', true, TOKEN, [1], [1]),
      group('lock', false, LOCK, [], [0]),
      group('lock', false, NEW_OWNER, [], [1]),
    ],
  } as unknown as TransactionDetail;
}

describe('CellTile leads with the cell’s seal (seal kinds, C3)', () => {
  it('leads with one row seal of the cell’s id, an image before the out point, which stays the link', () => {
    const { inputs, outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[1]} />);
    const tile = screen.getByTestId('tx-io-output-1');
    const seal = expectOneRowSeal(
      tile,
      {
        kind: 'cell',
        tx: HASH,
        index: 1,
        lock: NEW_OWNER,
        type: { class: 'tokens', hash: TOKEN },
        capacity: BigInt(14200000000),
      },
      'pressed',
      IN_MARK_COLUMN
    );
    // the 18 px stone stands centred in a box of the 20 px line, 1 px clear above and below, so
    // the first line keeps its height (re-pinned on purpose, readability R6: the 30 px stone
    // overhung the line by 5 px, `-my-[5px]`)
    expect(seal).toHaveClass('h-row', 'items-center');
    expect(seal.className).not.toMatch(/-my-/);
    const outPoint = within(tile).getByRole('link', { name: /^0x[0-9a-f]{4}…[0-9a-f]{4}:1$/ });
    expect(outPoint).toHaveAttribute('href', `/mainnet/cell/${HASH}-1`);
    expectBefore(seal, outPoint);
    // the first thing in the tile
    expect(tile.querySelector('[data-seal]')).toBe(seal);
    // an input's seal is the cell it spends
    expect(inputs[1].seal.id).toEqual({
      kind: 'cell',
      tx: PREV,
      index: 5,
      lock: LOCK,
      type: { class: 'tokens', hash: TOKEN },
      capacity: BigInt(14200000000),
    });
  });

  // C11: the owner's row is the tile's second; its stone stands in the mark column, the address
  // starts at the text edge (was: the stone directly before the address in one span).
  it('stands the owner’s stone of the cell’s lock in the mark column of the owner’s row, outside its link', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[1]} />);
    const tile = screen.getByTestId('tx-io-output-1');
    const holder = tile.querySelector('[data-tile-owner]')!;
    expect(holder).toBe(tile.children[1]);
    const stone = expectOwnerSealOf(holder, NEW_OWNER, IN_MARK_COLUMN);
    const [mark, text] = Array.from(holder.children);
    expect(mark.children).toHaveLength(1);
    expect(mark.firstElementChild).toBe(stone);
    // R9 (re-pinned): the owner at the row size, 4…8 (it was 4…6)
    const address = within(tile).getByRole('link', { name: /^ckb1…[0-9a-z]{8}$/ });
    expect(address).toHaveAttribute('href', `/mainnet/address/${OWNER_ADDRESS}`);
    expect(text).toContainElement(address);
    // one line: re-pinned on purpose (lane D3, Gate B's ruling): the 18 px stone, as high as the
    // cell's, stands centred in a box of the 20 px line, 1 px clear above and below, as the cell's
    // stone does (it was the 20 px stone overhanging the line, `-my-px`)
    expect(stone).toHaveClass('h-row', 'items-center');
    expect(stone.className).not.toMatch(/-my-/);
    // the out point and its seal come first; the owner, then the lock's kind
    expectBefore(within(tile).getByRole('link', { name: /:1$/ }), stone);
  });

  it('inks the cells a committed transaction consumed and the outputs consumed since', () => {
    const { inputs, outputs } = cellTiles(transfer('committed', 'consumed'), undefined);
    for (const cell of [...inputs, ...outputs]) {
      const { unmount } = render(<CellTile cell={cell} />);
      expectOneRowSeal(screen.getByTestId(cell.testId!), cell.seal.id, 'ink', IN_MARK_COLUMN);
      unmount();
    }
    // a live output is pressed
    const live = cellTiles(transfer('committed', 'live'), undefined).outputs[0];
    render(<CellTile cell={live} />);
    expectOneRowSeal(screen.getByTestId('tx-io-output-0'), live.seal.id, 'pressed', IN_MARK_COLUMN);
  });

  it('draws the outputs of a transaction not yet committed dashed, and its inputs still pressed', () => {
    const { inputs, outputs } = cellTiles(transfer('pending'), undefined);
    for (const cell of outputs) {
      const { unmount } = render(<CellTile cell={cell} />);
      const seal = expectOneRowSeal(
        screen.getByTestId(cell.testId!),
        cell.seal.id,
        'unpressed',
        IN_MARK_COLUMN
      );
      expect(seal.querySelector('[stroke-dasharray]')).not.toBeNull();
      unmount();
    }
    for (const cell of inputs) {
      const { unmount } = render(<CellTile cell={cell} />);
      expectOneRowSeal(screen.getByTestId(cell.testId!), cell.seal.id, 'pressed', IN_MARK_COLUMN);
      unmount();
    }
  });

  it('draws a plain CKB cell as a bare stone, and a token with its window and glyph', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    const tags = (seal: HTMLElement) =>
      Array.from(seal.querySelectorAll('svg *')).map((node) => node.tagName.toLowerCase());
    render(<CellTile cell={outputs[0]} />);
    const plain = expectOneRowSeal(
      screen.getByTestId('tx-io-output-0'),
      outputs[0].seal.id,
      'pressed',
      IN_MARK_COLUMN
    );
    expect(outputs[0].seal.id.type).toBeNull();
    expect(plain.querySelector('svg')).toHaveAttribute(
      'aria-label',
      expect.stringContaining(', ckb,')
    );
    // the rim, a bead for each digit of 61 CKB, the owner's mark; no window, no glyph
    expect(tags(plain)).toEqual([expect.any(String), 'circle', 'circle', 'rect']);
    expect(plain.innerHTML).not.toContain('fill="none"');
    render(<CellTile cell={outputs[1]} />);
    const usdi = expectOneRowSeal(
      screen.getByTestId('tx-io-output-1'),
      outputs[1].seal.id,
      'pressed',
      IN_MARK_COLUMN
    );
    expect(usdi.querySelector('svg')).toHaveAttribute(
      'aria-label',
      expect.stringContaining(', tokens,')
    );
    // the tokens' window, a square, and the glyph of the type script's hash after the beads
    expect(usdi.querySelector('rect[fill="none"]')).not.toBeNull();
    expect(tags(usdi).length).toBeGreaterThan(tags(plain).length + 1);
  });

  it('gives a cellbase’s null input no tile and no seal; its outputs their own', () => {
    const reward = { capacity: '100000000000', commonKnowledgeSize: '61', display: null, lock };
    const cellbase = {
      hash: HASH,
      status: 'committed',
      isCellbase: true,
      // the null out point: no cell behind it, so no scripts and no address
      inputs: [
        {
          previousOutput: { txHash: `0x${'00'.repeat(32)}`, index: 4294967295 },
          display: null,
          lock: null,
          type: null,
          address: null,
        },
      ],
      outputs: [{ ...reward, address: ADDRESS, status: 'live', isChange: false }],
      // a cellbase runs nothing: its one group, the output's lock, did not run
      scriptGroups: [group('lock', false, LOCK, [], [0])],
    } as unknown as TransactionDetail;
    const { inputs, outputs } = cellTiles(cellbase, undefined);
    expect(inputs).toEqual([]);
    expect(outputs[0].seal).toEqual({
      id: {
        kind: 'cell',
        tx: HASH,
        index: 0,
        lock: LOCK,
        type: null,
        capacity: BigInt(100000000000),
      },
      state: 'pressed',
    });
  });

  it('requires the transaction’s script groups, which the API always sends', () => {
    const { scriptGroups: _drop, ...without } = transfer('committed');
    void _drop;
    expect(() => cellTiles(without as TransactionDetail, undefined)).toThrow(
      `Transaction ${HASH} missing required field "scriptGroups"`
    );
  });

  it('refuses a seal that is not the tile’s cell', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      const other = { ...outputs[1], seal: outputs[0].seal };
      expect(() => render(<CellTile cell={other} />)).toThrow(
        `out:1: its seal (cell ${HASH}:0, ckb, 6100000000 shannons) is not the tile's (cell ${HASH}:1, tokens, 14200000000 shannons)`
      );
    } finally {
      quiet.mockRestore();
    }
  });
});

/* ---------- one mark column, one text edge (C11) ---------- */

/** A tile's four rows, each a `contents` pair of its mark cell and its text cell. */
function tileRows(tile: HTMLElement) {
  const rows = Array.from(tile.children) as HTMLElement[];
  expect(rows.map((row) => row.getAttribute('data-tile-row'))).toEqual([
    'cell',
    'owner',
    'content',
    'occupied',
  ]);
  return rows.map((row) => {
    // the row adds no box of its own: its two cells are the grid's
    expect(row).toHaveClass('contents');
    expect(row.children).toHaveLength(2);
    const [mark, text] = Array.from(row.children) as HTMLElement[];
    expect(mark).toHaveAttribute('data-tile-mark');
    expect(text).toHaveAttribute('data-tile-text');
    return { row, mark, text };
  });
}

/** A text cell's right-end item: last, pushed right, staying right when it wraps under the text. */
function expectEnd(text: HTMLElement, content: RegExp) {
  expect(text).toHaveClass('flex', 'min-w-0', 'flex-wrap', 'justify-between');
  const end = text.lastElementChild as HTMLElement;
  expect(end).toHaveAttribute('data-tile-end');
  expect(end).toHaveClass('ml-auto', 'max-w-full', 'text-right');
  expect(end).toHaveTextContent(content);
  expect(text.querySelectorAll('[data-tile-end]')).toHaveLength(1);
  return end;
}

// The user's Gate C ("看着也不舒服"): the old tile's three marks of three widths (the 30 px cell
// stone, the 15 px owner's stone, the 16 px class mark) each pushed their text to its own x, and
// its first flex line wrapped seal + out point, owner and lock kind differently at every width.
// The tile is now a grid of a 30 px mark column and the text, 10 px apart, in four rows: every
// mark centred in the one column, every text from the one edge, the right ends one column.
// Re-pinned on purpose (readability R6): the column is as wide as its widest mark, the cell's row
// stone, 18 px since the stone is (it was 30); renamed: "the 30 px mark column".
describe('CellTile: one mark column, one text edge (C11)', () => {
  it('is a grid of the mark column, as wide as its widest mark, and the text, 10 px apart, in four rows of one mark each', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[0]} />);
    const tile = screen.getByTestId('tx-io-output-0');
    expect(tile.tagName).toBe('ARTICLE');
    // the widest of the three marks: the cell's stone (18), the owner's (13.5), the class mark (16)
    const column = Math.max(ROW_SEAL.cell.width, OWNER_SEAL.width, 16);
    expect(column).toBe(18);
    expect(tile).toHaveClass(
      'grid',
      `grid-cols-[${column}px_minmax(0,1fr)]`,
      'gap-x-2.5',
      'gap-y-2.5'
    );
    // 12 px above (it held the 30 px stone's overhang; since readability R6 the 18 px stone stands
    // inside its line, 1 px clear of it)
    expect(tile).toHaveClass('pt-3', 'pb-2', 'px-3', 'min-w-0');
    const [cell, owner, content, occupied] = tileRows(tile);
    expect(cell.mark.children).toHaveLength(1);
    expect(cell.mark.firstElementChild).toHaveAttribute('data-seal', 'cell');
    expect(owner.mark.children).toHaveLength(1);
    expect(owner.mark.firstElementChild).toHaveAttribute('data-owner-seal');
    expect(content.mark.children).toHaveLength(1);
    expect(content.mark.firstElementChild).toHaveAttribute('data-class', 'ckb');
    expect(content.mark.firstElementChild).toHaveAttribute('width', '16');
    expect(occupied.mark.childNodes).toHaveLength(0);
    // each mark centred in the column, on its row's first line (the cell's stone centred in a box
    // of the 20 px line, the owner's 18 px stone too; the 16 px class mark sits 1 px down)
    for (const { mark } of [cell, owner, content])
      expect(mark).toHaveClass('flex', 'items-start', 'justify-center');
    expect(content.mark).toHaveClass('pt-px');
    // no mark anywhere in the text column
    for (const { text } of [cell, owner, content, occupied]) {
      expect(text.querySelector('[data-seal], [data-class], [data-owner-seal]')).toBeNull();
      expect(text).toHaveClass('min-w-0');
    }
    // nothing hidden, nothing twice
    expect(tile.querySelector('.hidden, .sr-only')).toBeNull();
    expect(tile.querySelectorAll('[data-seal]')).toHaveLength(2);
    expect(tile.querySelectorAll('[data-ckb-amount]')).toHaveLength(1);
    expect(within(tile).getAllByRole('link')).toHaveLength(2);
  });

  // Round 2 (the orchestrator's Chrome check): the state at row 1's right end wrapped on every tile
  // between 1024 and about 1100 px (a 203 px text column: 'input 0 0x7bed...faac:0' 166 px and
  // 'consumed' 53 px), costing each tile a line. Row 1 is the label and the out point alone.
  it('reads the label and the out point alone on the first row', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[0]} />);
    const tile = screen.getByTestId('tx-io-output-0');
    const [cell] = tileRows(tile);
    const seal = expectOneRowSeal(cell.mark, outputs[0].seal.id, 'pressed', IN_MARK_COLUMN);
    // centred on the 20 px row line, 1 px clear above and below (re-pinned on purpose, readability
    // R6: the 30 px stone was 5 px over it, `-my-[5px]`)
    expect(seal).toHaveClass('h-row', 'items-center');
    expect(seal.className).not.toMatch(/-my-/);
    // the out point with its index is one unit, never broken, and the cell's link
    const outPoint = within(cell.text).getByRole('link', {
      name: /^0x[0-9a-f]{4}…[0-9a-f]{4}:0$/,
    });
    expect(outPoint).toHaveClass('whitespace-nowrap');
    expect(outPoint).toHaveAttribute('title', `${HASH}:0`);
    expect(cell.text).toHaveClass('flex', 'min-w-0', 'flex-wrap', 'items-baseline', 'gap-x-2');
    expect(cell.text.children).toHaveLength(1);
    expect(cell.text).toHaveTextContent(/^output 0 0x[0-9a-f]{4}…[0-9a-f]{4}:0$/);
    expect(cell.text.querySelector('[data-tile-end]')).toBeNull();
  });

  it('reads the owner, never broken, on the second row, the lock’s kind at its right end', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[0]} />);
    const [, owner] = tileRows(screen.getByTestId('tx-io-output-0'));
    expect(owner.row).toHaveAttribute('data-tile-owner');
    expect(owner.text).toHaveClass('items-baseline', 'gap-x-2');
    expectOwnerSealOf(owner.mark, LOCK, IN_MARK_COLUMN);
    const name = owner.text.firstElementChild as HTMLElement;
    expect(name).toHaveClass('whitespace-nowrap');
    expect(within(name).getByRole('link', { name: /^ckb1…[0-9a-z]{8}$/ })).toHaveAttribute(
      'href',
      `/mainnet/address/${ADDRESS}`
    );
    // B4 fix round (re-pinned on purpose): a tile names its owner's lock by the one decision every
    // row reads (U-15: a lock the registry does not name is "lock <code hash>"; here no lookup
    // names the fixture's made-up 0x9b… lock), then "change" on the change output, dim. It wrote
    // 'change' alone and left an unregistered lock unnamed, so it read as the default lock.
    expect(
      expectEnd(owner.text, /^lock 0x9b9b9b9b…9b9b9b · change$/).firstElementChild
    ).toHaveClass('text-text-dim');
    cleanup();
    render(<CellTile cell={outputs[1]} />);
    const [, other] = tileRows(screen.getByTestId('tx-io-output-1'));
    expectEnd(other.text, /^lock 0x9b9b9b9b…9b9b9b$/);
  });

  it('reads the content, bright, on the third row, the exact capacity at its right end', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[1]} />);
    const tile = screen.getByTestId('tx-io-output-1');
    const [, , content] = tileRows(tile);
    expect(content.text).toHaveClass('items-baseline', 'gap-x-2');
    expect(content.mark.firstElementChild).toHaveAttribute('data-class', 'tokens');
    const what = content.text.firstElementChild as HTMLElement;
    expect(what).toHaveClass('text-text-bright', 'min-w-0', 'max-w-full');
    expect(what).toHaveTextContent('USDI');
    const capacity = expectEnd(content.text, /^142(\.0+)? CKB$/);
    expect(capacity.querySelector('[data-ckb-amount]')).not.toBeNull();
    expect(capacity.querySelector('[data-ckb-amount]')).toHaveClass('whitespace-nowrap');
  });

  // Round 2: the cell's state is the last row's right end again, beside the bar and the bytes; the
  // tile's right edge reads, top to bottom, nothing, the lock's kind, the capacity, the state.
  it('draws the occupied bar and its bytes on the fourth row, the cell’s state at its right end', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[0]} />);
    const tile = screen.getByTestId('tx-io-output-0');
    const [, , , occupied] = tileRows(tile);
    expect(occupied.text).toHaveClass('items-center', 'gap-x-1.5');
    expect(occupied.text.children).toHaveLength(3);
    const [bar, bytes] = Array.from(occupied.text.children) as HTMLElement[];
    expect(bar).toHaveAttribute('aria-hidden', 'true');
    expect(bar).toHaveClass('h-1', 'w-14', 'shrink-0');
    expect(bytes).toHaveTextContent(/^occupied 61 B$/);
    expect(bytes).toHaveClass('text-meta', 'text-text-dim');
    const state = expectEnd(occupied.text, /^live$/);
    // re-pinned with readability ruling R3: the state is a value, the reading ink (it was dim)
    expect(state.firstElementChild).toHaveClass('text-meta', 'text-text');
    // the state once in the tile
    expect(within(tile).getAllByText('live')).toHaveLength(1);
    // The line's budget at a stated bound (B4 fix round, tests NOTE-4; it was the sample
    // 'occupied 231'): a cell's occupied bytes are no more than the transaction that made it, and
    // the largest is genesis's cellbase, past the block limit (LARGEST_TX_BYTES, of record), so the
    // widest figure is "occupied 1,236,271 B", 20 glyphs at the meta step (7.2 a glyph): 144 px.
    // With the 56 px bar, 'consumed' and the 6 px gaps the line is 269.6 px.
    //
    // The tile's text column, derived (second fix round, tests NOTE-2: 343 was typed, 215 came
    // from a recorded 203): the transaction page's main column (the container less the 280 px rail
    // and the 56 px gap, its lg grid), half of it less the 4 px seam (the two sheets), less the
    // sheet's lane on the seam side (SHEETS.lane) and its outer side (12 px and the 1 px border),
    // the tile's sides (12 px and 1 px each), its mark column and the 10 px gap: 215 px at 1024,
    // 343 at 1280.
    const mark = Math.max(ROW_SEAL.cell.width, OWNER_SEAL.width, 16);
    const textColumn = (viewport: number) =>
      (containerContentPx(viewport) - 280 - 56 - SHEETS.seam) / 2 -
      SHEETS.lane -
      (12 + 1) -
      2 * (12 + 1) -
      mark -
      10;
    expect([textColumn(1024), textColumn(1280)]).toEqual([215, 343]);
    const lineOf = (words: string) =>
      56 + 6 + textPx(words.length, fontPx('meta')) + 6 + textPx(8, fontPx('meta'));
    const widest = `occupied ${occupiedBytesText(BigInt(LARGEST_TX_BYTES), 'a cell')}`;
    expect(widest).toBe('occupied 1,236,271\u00a0B');
    expect(lineOf(widest)).toBeCloseTo(269.6, 10);
    // it holds at 1280; at 1024 the line holds up to "occupied 9 B" (212 px) and not from
    // "occupied 10 B" (219.2) on: every real cell, which occupies at least 41 bytes (its capacity
    // field, 8, and its lock's code hash and hash type, 33), stands its state under the bar there,
    // as it did before the unit was written (the type build, measured in Chrome)
    expect(lineOf(widest)).toBeLessThanOrEqual(textColumn(1280));
    expect(lineOf(`occupied ${occupiedBytesText(BigInt(9), 'a cell')}`)).toBeLessThanOrEqual(
      textColumn(1024)
    );
    expect(lineOf(`occupied ${occupiedBytesText(BigInt(10), 'a cell')}`)).toBeGreaterThan(
      textColumn(1024)
    );
    expect(lineOf(`occupied ${occupiedBytesText(BigInt(41), 'a cell')}`)).toBeGreaterThan(
      textColumn(1024)
    );
    // where it does not fit, the line wraps and its end (the state) drops under the bar: the row
    // is a wrapping flex row, the state its own last item
    expect(occupied.text).toHaveClass('flex', 'flex-wrap');
    expect(occupied.text.lastElementChild).toHaveAttribute('data-tile-end');
    // with the cell's anatomy, its byte bar takes the line's free width, the bytes after it
    cleanup();
    render(
      <CellTile
        cell={sealedTile({
          id: 'in:0',
          testId: 'tile',
          href: '/cell/0x-0',
          label: 'input 0',
          owner: 'owner',
          lockKind: null,
          kind: 'ckb',
          content: 'CKB',
          capacity: '6100000000',
          occupiedBytes: '61',
          state: 'consumed',
          anatomy: { lock: '53', type: '0', data: '0' },
        })}
      />
    );
    const [, , , anatomy] = tileRows(screen.getByTestId('tile'));
    const [cellBar, words, end] = Array.from(anatomy.text.children) as HTMLElement[];
    expect(end).toHaveTextContent(/^consumed$/);
    expect(within(cellBar).getByRole('img')).toHaveAttribute(
      'aria-label',
      'in:0: 61 occupied bytes'
    );
    expect(cellBar).toHaveClass('min-w-0', 'flex-1');
    expect(words).toHaveTextContent(/^occupied 61 B$/);
  });

  // Lane D3 (the tile's last line; the readability review's note, ruled at Gate B for D3): the
  // bar, "occupied N B" and the state stood on two lines in the narrow tiles, the state under the
  // bar. A tile's text column, as the pages draw it since lane D2 gave tier 1 the whole measure:
  // the sheet less its 12 px padding and 1 px border on the outer side (and its 36 px lane on the
  // seam side from lg, where the sheets stand side by side; below lg they stack, the padding on
  // both sides), the tile's 12 px padding and 1 px border on each side, the mark column and its
  // 10 px gap. The line is the bar's track, 6 px, the bytes, 6 px and the state, at the meta step.
  it('holds the bar, its bytes and the state on one line in every tile the pages draw, for every cell under 1,000,000 bytes', () => {
    render(
      <CellTile
        cell={sealedTile({
          id: 'in:0',
          testId: 'tile',
          href: '/cell/0x-0',
          label: 'input 0',
          owner: 'owner',
          lockKind: null,
          kind: 'ckb',
          content: 'CKB',
          capacity: '6100000000',
          occupiedBytes: '61',
          state: 'consumed',
        })}
      />
    );
    const [, , , occupied] = tileRows(screen.getByTestId('tile'));
    const track = boxPx(occupied.text.firstElementChild!, 1024)!;
    const mark = Math.max(ROW_SEAL.cell.width, OWNER_SEAL.width, 16);
    const text = (sheet: number) => sheet - 2 * (12 + 1) - mark - 10;
    /** The sheets side by side from lg, less the onward bridge's 96 px column where a page has it. */
    const sideBySide = (viewport: number, bridge = 0) =>
      text((containerContentPx(viewport) - SHEETS.seam - bridge) / 2 - SHEETS.lane - (12 + 1));
    const stacked = (viewport: number) => text(containerContentPx(viewport) - 2 * (12 + 1));
    const columns = {
      'transaction page 1280': sideBySide(1280),
      'transaction page 1024': sideBySide(1024),
      'cell page 1024, beside its onward bridge': sideBySide(1024, 96),
      'stacked 640': stacked(640),
      'stacked 390': stacked(390),
      'stacked 360': stacked(360),
    };
    expect(columns).toEqual({
      'transaction page 1280': 511,
      'transaction page 1024': 383,
      'cell page 1024, beside its onward bridge': 335,
      'stacked 640': 528,
      'stacked 390': 278,
      'stacked 360': 248,
    });
    const meta = fontPx('meta');
    const lineOf = (bytes: number) =>
      track +
      6 +
      textPx(`occupied ${occupiedBytesText(BigInt(bytes), 'a cell')}`.length, meta) +
      6 +
      textPx('consumed'.length, meta);
    // every cell under 1,000,000 bytes (the widest state, "consumed") holds its line in every tile
    for (const [where, width] of Object.entries(columns))
      expect(lineOf(999_999), where).toBeLessThanOrEqual(width);
    // the widest of record (genesis's cellbase, LARGEST_TX_BYTES) holds it from 390 up
    for (const [where, width] of Object.entries(columns).filter(([w]) => !w.endsWith('360')))
      expect(lineOf(LARGEST_TX_BYTES), where).toBeLessThanOrEqual(width);
  });
});

describe('a transaction of 300 cells draws each tile’s seals once (seal kinds, C3)', () => {
  it('draws 300 row seals and 300 owner’s stones once, and nothing again when the tiles are rebuilt', () => {
    const n = 150;
    const cell = (i: number) => ({
      capacity: `${6100000000 + i * 100000000}`,
      commonKnowledgeSize: '61',
      display: null,
      lock: { ...lock, args: `0x${i.toString(16).padStart(40, '0')}` },
      address: ckbAddress({
        codeHash: lock.codeHash,
        args: `0x${i.toString(16).padStart(40, '0')}`,
      }),
    });
    const lockOf = (side: 'in' | 'out', i: number) => sealHash(`c3 300 ${side} lock ${i}`);
    const tx = {
      hash: HASH,
      status: 'committed',
      isCellbase: false,
      fee: '0',
      inputs: Array.from({ length: n }, (_, i) => ({
        ...cell(i),
        previousOutput: { txHash: PREV, index: i },
      })),
      outputs: Array.from({ length: n }, (_, i) => ({
        ...cell(i),
        status: 'live',
        isChange: false,
      })),
      scriptGroups: Array.from({ length: n }, (_, i) => [
        group('lock', true, lockOf('in', i), [i], []),
        group('lock', false, lockOf('out', i), [], [i]),
      ]).flat(),
    } as unknown as TransactionDetail;
    const draw = vi.mocked(drawSeal);
    const view = (tiles: ReturnType<typeof cellTiles>) => (
      <TxStructure txHash={HASH} inputs={tiles.inputs} outputs={tiles.outputs} fee="0" />
    );
    draw.mockClear();
    const started = performance.now();
    const { container, rerender } = render(view(cellTiles(tx, undefined)));
    const first = performance.now() - started;
    expect(container.querySelectorAll('[data-cell-id]')).toHaveLength(2 * n);
    expect(container.querySelectorAll('[data-seal="cell"]')).toHaveLength(2 * n);
    expect(container.querySelectorAll('[data-owner-seal] [data-seal="script"]')).toHaveLength(
      2 * n
    );
    // every cell's stone and every owner's stone, once
    expect(draw).toHaveBeenCalledTimes(4 * n);
    const cellIds = new Set(
      draw.mock.calls
        .filter(([id]) => id.kind === 'cell')
        .map(([id]) => JSON.stringify(id, (_, v) => (typeof v === 'bigint' ? `${v}` : v)))
    );
    expect(cellIds.size).toBe(2 * n);
    // the transaction page rebuilds its tiles on every render: fresh objects, the same ids
    const again = performance.now();
    rerender(view(cellTiles(tx, undefined)));
    const second = performance.now() - again;
    expect(draw).toHaveBeenCalledTimes(4 * n);
    console.log(
      `seal kinds C3: ${2 * n} tiles, ${4 * n} seals drawn in a first render of ${first.toFixed(0)} ms; a re-render with rebuilt tiles drew 0 in ${second.toFixed(0)} ms`
    );
  });
});
