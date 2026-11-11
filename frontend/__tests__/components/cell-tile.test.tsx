import { describe, it, expect, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { CellTile } from '@/components/ui/cell-tile';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { TxStructure } from '@/components/transaction/tx-structure';
import type { TransactionDetail, TransactionScriptGroup } from '@/lib/api';
import { drawSeal } from '@/lib/seal/stones';
import { sealedTile } from '@/__tests__/utils/cell-tile-seal';
import {
  expectBefore,
  expectOneRowSeal,
  expectOwnerSealOf,
  sealHash,
} from '@/__tests__/utils/seals';

// The drawing is counted: every seal on a tile is drawn once per id, however often it renders.
vi.mock('@/lib/seal/stones', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/seal/stones')>();
  return { ...actual, drawSeal: vi.fn(actual.drawSeal) };
});

describe('CellTile occupied wording (TX-19)', () => {
  it('states occupied capacity as whole bytes, as the board does ("occupied 195")', () => {
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
          occupied: '19500000000',
          state: 'consumed',
        })}
      />
    );
    const tile = screen.getByTestId('tile');
    expect(tile).toHaveTextContent('occupied 195');
    expect(tile).not.toHaveTextContent('occupied 195.00000000');
  });

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
            occupied: '6100000001',
            state: 'consumed',
          })}
        />
      )
    ).toThrow('in:0: occupied 6100000001 shannons is not whole bytes');
  });
});

/* ---------- the seals by kind (C3) ---------- */

const ADDRESS = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqwyt2p74pg74c';
const OWNER_ADDRESS = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq2fh0zmj';
const HASH = sealHash('c3 tile transaction');
const PREV = sealHash('c3 tile previous transaction');
/** The real script hash of { code_hash 0x9b × 32, hash_type type, args 0x } (blake2b, CKB). */
const LOCK = '0x82cbea9785c59f24117b45294cff296e8ab0ac2f29bbfa78af6b01e4d5325644';
const NEW_OWNER = sealHash('c3 tile new owner');
const TOKEN = sealHash('c3 tile token type');
const lock = { codeHash: `0x${'9b'.repeat(32)}`, hashType: 'type', args: '0x' };
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
      'pressed'
    );
    // it overhangs the 18 px line by 6 px above and below, so the first line keeps its height
    expect(seal).toHaveClass('-my-[6px]');
    const outPoint = within(tile).getByRole('link', { name: /^0x[0-9a-f]{4}\.\.\.[0-9a-f]{4}:1$/ });
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

  it('stands the owner’s stone of the cell’s lock directly before the owner’s address, outside its link', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[1]} />);
    const tile = screen.getByTestId('tx-io-output-1');
    const holder = tile.querySelector('[data-tile-owner]')!;
    const stone = expectOwnerSealOf(holder, NEW_OWNER);
    const address = within(tile).getByRole('link', { name: /^ckb1qzda0c/ });
    expect(address).toHaveAttribute('href', `/mainnet/address/${OWNER_ADDRESS}`);
    // directly before it: the stone, then the address, nothing between
    expect(stone.nextElementSibling).toBe(address);
    expect(holder.firstElementChild).toBe(stone);
    // one line: the stone at the line's height (20 px, 1 px over above and below)
    expect(stone).toHaveClass('-my-px');
    // the out point and its seal come first; the owner, then the lock's kind
    expectBefore(within(tile).getByRole('link', { name: /:1$/ }), stone);
  });

  it('inks the cells a committed transaction consumed and the outputs consumed since', () => {
    const { inputs, outputs } = cellTiles(transfer('committed', 'consumed'), undefined);
    for (const cell of [...inputs, ...outputs]) {
      const { unmount } = render(<CellTile cell={cell} />);
      expectOneRowSeal(screen.getByTestId(cell.testId!), cell.seal.id, 'ink');
      unmount();
    }
    // a live output is pressed
    const live = cellTiles(transfer('committed', 'live'), undefined).outputs[0];
    render(<CellTile cell={live} />);
    expectOneRowSeal(screen.getByTestId('tx-io-output-0'), live.seal.id, 'pressed');
  });

  it('draws the outputs of a transaction not yet committed dashed, and its inputs still pressed', () => {
    const { inputs, outputs } = cellTiles(transfer('pending'), undefined);
    for (const cell of outputs) {
      const { unmount } = render(<CellTile cell={cell} />);
      const seal = expectOneRowSeal(screen.getByTestId(cell.testId!), cell.seal.id, 'unpressed');
      expect(seal.querySelector('[stroke-dasharray]')).not.toBeNull();
      unmount();
    }
    for (const cell of inputs) {
      const { unmount } = render(<CellTile cell={cell} />);
      expectOneRowSeal(screen.getByTestId(cell.testId!), cell.seal.id, 'pressed');
      unmount();
    }
  });

  it('draws a plain CKB cell as a bare stone, and a token with its window and glyph', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    const tags = (seal: HTMLElement) =>
      Array.from(seal.querySelectorAll('svg *')).map((node) => node.tagName.toLowerCase());
    render(<CellTile cell={outputs[0]} />);
    const plain = expectOneRowSeal(screen.getByTestId('tx-io-output-0'), outputs[0].seal.id);
    expect(outputs[0].seal.id.type).toBeNull();
    expect(plain.querySelector('svg')).toHaveAttribute(
      'aria-label',
      expect.stringContaining(', ckb,')
    );
    // the rim, a bead for each digit of 61 CKB, the owner's mark; no window, no glyph
    expect(tags(plain)).toEqual([expect.any(String), 'circle', 'circle', 'rect']);
    expect(plain.innerHTML).not.toContain('fill="none"');
    render(<CellTile cell={outputs[1]} />);
    const usdi = expectOneRowSeal(screen.getByTestId('tx-io-output-1'), outputs[1].seal.id);
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

  it('keeps the tile’s lines: the out point and the owner each one unit, the lock’s kind last', () => {
    const { outputs } = cellTiles(transfer('committed'), undefined);
    render(<CellTile cell={outputs[0]} />);
    const tile = screen.getByTestId('tx-io-output-0');
    // 12 px above, so the stone overhanging its line by 6 px stays 6 px inside the border
    expect(tile).toHaveClass('pt-3', 'pb-2', 'px-3');
    const line = tile.firstElementChild!;
    expect(line).toHaveClass('flex', 'flex-wrap', 'items-start', 'justify-between', 'gap-x-2');
    // a 12 px row gap: a wrapped owner's stone stands 5 px under the cell's stone
    expect(line).toHaveClass('gap-y-3');
    const [lead, owner, kind] = Array.from(line.children);
    expect(lead.firstElementChild).toHaveAttribute('data-seal', 'cell');
    expect(within(lead as HTMLElement).getByRole('link', { name: /:0$/ })).toHaveClass(
      'whitespace-nowrap'
    );
    expect(owner).toHaveAttribute('data-tile-owner');
    expect(owner).toHaveClass('mr-auto', 'whitespace-nowrap');
    expect(kind).toHaveTextContent('change');
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
      address: ADDRESS,
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
