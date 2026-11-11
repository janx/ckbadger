// The seals by kind, C3: a cell's seal id and ink, from the API's own fields only — a transaction
// detail's cell (its script hashes from the transaction's script groups) and the cell page's Cell.
import { describe, expect, it } from 'vitest';
import { cellTiles, kindOf } from '@/components/transaction/transaction-cell-tiles';
import type {
  Cell,
  TransactionCellDisplay,
  TransactionDetail,
  TransactionScriptGroup,
} from '@/lib/api';
import {
  cellPageSeal,
  cellPageSealState,
  cellSealId,
  inputSealState,
  outputSealState,
  transactionCellSealId,
  transactionScriptHashes,
  type CellSealId,
} from '@/lib/cell-seal';
import type { CellClass } from '@/lib/seal/stones';
import { sealHash } from '@/__tests__/utils/seals';

const TX = sealHash('c3 transaction');
const PREV = sealHash('c3 previous transaction');
/** The inputs' lock, and the change output's (the same script, a second group: it did not run). */
const LOCK_A = sealHash('c3 lock a');
/** Output 1's lock, the token's new owner. */
const LOCK_B = sealHash('c3 lock b');
/** Output 2's lock, a lock that did not run here. */
const LOCK_C = sealHash('c3 lock c');
const TOKEN = sealHash('c3 token type');
const script = (seed: string) => ({ codeHash: sealHash(seed), hashType: 'type', args: '0x' });
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
  script: script(`code of ${scriptHash}`),
  cycles: null,
});
/**
 * A token transfer, as the API lists its groups (transaction_structure::script_groups): input 0
 * plain and input 1 a token, both locked by A; output 0 the plain change back to A, output 1 the
 * token to B, output 2 plain to C. The type group lists both token cells; every output lock is a
 * group that did not run.
 */
const GROUPS: TransactionScriptGroup[] = [
  group('lock', true, LOCK_A, [0, 1], []),
  group('type', true, TOKEN, [1], [1]),
  group('lock', false, LOCK_A, [], [0]),
  group('lock', false, LOCK_B, [], [1]),
  group('lock', false, LOCK_C, [], [2]),
];
const CLASSES: readonly CellClass[] = ['dao', 'tokens', 'objects', 'identities', 'other'];

describe('cellSealId: a cell’s seal id from the API’s fields', () => {
  it('reads a plain CKB cell as a bare stone: class ckb, no type, its shannons exactly', () => {
    expect(
      cellSealId('cell x', {
        tx: TX,
        index: 3,
        lock: LOCK_A,
        typeHash: null,
        cls: 'ckb',
        capacity: '9007199254740993',
      })
    ).toEqual({
      kind: 'cell',
      tx: TX,
      index: 3,
      lock: LOCK_A,
      type: null,
      // past 2^53: a BigInt of the decimal string, never through a Number
      capacity: BigInt('9007199254740993'),
    });
  });

  it('reads a typed cell with its class and its type hash, for every class but ckb', () => {
    for (const cls of CLASSES)
      expect(
        cellSealId('cell x', {
          tx: TX,
          index: 0,
          lock: LOCK_A,
          typeHash: TOKEN,
          cls,
          capacity: '14200000000',
        }).type
      ).toEqual({ class: cls, hash: TOKEN });
  });

  it('refuses a class and a type hash that disagree, naming the cell', () => {
    const fields = { tx: TX, index: 0, lock: LOCK_A, capacity: '6100000000' };
    expect(() => cellSealId('cell x', { ...fields, cls: 'ckb', typeHash: TOKEN })).toThrow(
      `cell x: class ckb (no type script) but a type script hash ${TOKEN}`
    );
    for (const cls of CLASSES)
      expect(() => cellSealId('cell x', { ...fields, cls, typeHash: null })).toThrow(
        `cell x: class ${cls} but no type script hash`
      );
  });

  it('refuses a capacity that is not a whole number of shannons', () => {
    for (const capacity of ['1.5', '-1', '0x10', '', '01'])
      expect(() =>
        cellSealId('cell x', {
          tx: TX,
          index: 0,
          lock: LOCK_A,
          typeHash: null,
          cls: 'ckb',
          capacity,
        })
      ).toThrow('cell x: capacity: expected a non-negative decimal integer');
  });
});

describe('transactionScriptHashes: a transaction’s script groups by cell', () => {
  it('reads every input’s and output’s lock and type, an output lock that did not run too', () => {
    const scripts = transactionScriptHashes(TX, GROUPS);
    expect(scripts.txHash).toBe(TX);
    expect(Object.fromEntries(scripts.input)).toEqual({
      0: { lock: LOCK_A },
      1: { lock: LOCK_A, type: TOKEN },
    });
    expect(Object.fromEntries(scripts.output)).toEqual({
      0: { lock: LOCK_A },
      1: { lock: LOCK_B, type: TOKEN },
      2: { lock: LOCK_C },
    });
  });

  it('refuses a cell two groups of one kind list: a cell has one lock and at most one type', () => {
    const other = sealHash('c3 another lock');
    expect(() =>
      transactionScriptHashes(TX, [...GROUPS, group('lock', true, other, [1], [])])
    ).toThrow(`Transaction ${TX}: input 1 is listed by two lock groups, ${LOCK_A} and ${other}`);
    expect(() =>
      transactionScriptHashes(TX, [...GROUPS, group('type', true, other, [], [1])])
    ).toThrow(`Transaction ${TX}: output 1 is listed by two type groups, ${TOKEN} and ${other}`);
  });

  it('refuses a group that lists no position, or is of no kind', () => {
    expect(() => transactionScriptHashes(TX, [group('lock', true, LOCK_A, [-1], [])])).toThrow(
      `Transaction ${TX}: lock group ${LOCK_A} lists input -1, not a position`
    );
    expect(() => transactionScriptHashes(TX, [group('lock', false, LOCK_A, [], [1.5])])).toThrow(
      `Transaction ${TX}: lock group ${LOCK_A} lists output 1.5, not a position`
    );
    expect(() =>
      transactionScriptHashes(TX, [
        { ...group('lock', true, LOCK_A, [0], []), kind: 'data' as unknown as 'lock' },
      ])
    ).toThrow(`Transaction ${TX}: script group ${LOCK_A} of no kind "data"`);
  });
});

describe('transactionCellSealId: a cell of a transaction detail', () => {
  const scripts = transactionScriptHashes(TX, GROUPS);

  it('an input: the out point it spends, its lock and type from the groups', () => {
    expect(
      transactionCellSealId(scripts, 'input', 1, {
        outPoint: { txHash: PREV, index: 7 },
        cls: 'tokens',
        capacity: '14200000000',
      })
    ).toEqual({
      kind: 'cell',
      tx: PREV,
      index: 7,
      lock: LOCK_A,
      type: { class: 'tokens', hash: TOKEN },
      capacity: BigInt(14200000000),
    });
    expect(
      transactionCellSealId(scripts, 'input', 0, {
        outPoint: { txHash: PREV, index: 2 },
        cls: 'ckb',
        capacity: '6100000000',
      })
    ).toMatchObject({ tx: PREV, index: 2, lock: LOCK_A, type: null });
  });

  it('an output: this transaction’s out point, its lock from a group that did not run', () => {
    const out = (i: number, cls: 'ckb' | 'tokens') =>
      transactionCellSealId(scripts, 'output', i, {
        outPoint: { txHash: TX, index: i },
        cls,
        capacity: '6100000000',
      });
    expect(out(0, 'ckb')).toMatchObject({ tx: TX, index: 0, lock: LOCK_A, type: null });
    expect(out(1, 'tokens')).toMatchObject({
      tx: TX,
      index: 1,
      lock: LOCK_B,
      type: { class: 'tokens', hash: TOKEN },
    });
    expect(out(2, 'ckb')).toMatchObject({ tx: TX, index: 2, lock: LOCK_C, type: null });
  });

  it('refuses a cell no lock group lists, naming the transaction and the cell', () => {
    expect(() =>
      transactionCellSealId(scripts, 'output', 3, {
        outPoint: { txHash: TX, index: 3 },
        cls: 'ckb',
        capacity: '6100000000',
      })
    ).toThrow(`Transaction ${TX} output 3 (cell ${TX}:3): no lock script group lists it`);
    expect(() =>
      transactionCellSealId(scripts, 'input', 2, {
        outPoint: { txHash: PREV, index: 9 },
        cls: 'ckb',
        capacity: '6100000000',
      })
    ).toThrow(`Transaction ${TX} input 2 (cell ${PREV}:9): no lock script group lists it`);
  });

  it('refuses a typed cell no type group lists, and a plain cell a type group lists', () => {
    expect(() =>
      transactionCellSealId(scripts, 'output', 2, {
        outPoint: { txHash: TX, index: 2 },
        cls: 'tokens',
        capacity: '6100000000',
      })
    ).toThrow(`Transaction ${TX} output 2 (cell ${TX}:2): class tokens but no type script hash`);
    expect(() =>
      transactionCellSealId(scripts, 'output', 1, {
        outPoint: { txHash: TX, index: 1 },
        cls: 'ckb',
        capacity: '6100000000',
      })
    ).toThrow(
      `Transaction ${TX} output 1 (cell ${TX}:1): class ckb (no type script) but a type script hash ${TOKEN}`
    );
  });
});

describe('the ink of a cell’s seal', () => {
  it('an input: in the dead ink once its transaction is committed, pressed (still live) before', () => {
    expect(inputSealState('committed')).toBe('ink');
    expect(inputSealState('pending')).toBe('pressed');
    expect(inputSealState('proposed')).toBe('pressed');
    expect(() => inputSealState('rejected' as never)).toThrow(
      'no such transaction status "rejected"'
    );
  });

  it('an output: live pressed, consumed in the dead ink, not yet committed unpressed', () => {
    expect(outputSealState('live')).toBe('pressed');
    expect(outputSealState('consumed')).toBe('ink');
    expect(outputSealState('pending')).toBe('unpressed');
    // the cell page's word is not the transaction detail's
    expect(() => outputSealState('dead' as never)).toThrow('no such output status "dead"');
  });

  it('the cell page: live pressed, spent in the dead ink, no status refused', () => {
    expect(cellPageSealState('cell x', 'live')).toBe('pressed');
    expect(cellPageSealState('cell x', 'dead')).toBe('ink');
    expect(() => cellPageSealState('cell x', undefined)).toThrow(
      'cell x: no such cell status undefined'
    );
  });
});

const tokenDisplay: TransactionCellDisplay = {
  kind: 'token',
  standard: 'xudt',
  id: TOKEN,
  name: 'Exact xudt',
  collectionId: null,
  collectionName: null,
  symbol: 'XUDT',
  decimals: 8,
  amount: '5',
  noAmountReason: null,
  compositionTier: null,
};
const display = (kind: TransactionCellDisplay['kind'], standard: string) => ({
  ...tokenDisplay,
  kind,
  standard,
  amount: null,
  symbol: null,
  decimals: null,
});
/** Output 1 of TX, the token, as `/cells/{tx}/1` serves it. */
const tokenCell = {
  txHash: TX,
  outputIndex: 1,
  capacity: '14200000000',
  lockScriptHash: LOCK_B,
  typeScriptHash: TOKEN,
  type: script('xudt code'),
  typeCellType: 'xudt',
  display: tokenDisplay,
  status: 'live',
} as const satisfies Partial<Cell>;

describe('cellPageSeal: the cell /cells/{tx}/{index} serves', () => {
  it('reads its out point, its lock and type hash, kindOf()’s class and its capacity', () => {
    expect(cellPageSeal(tokenCell, kindOf)).toEqual({
      id: {
        kind: 'cell',
        tx: TX,
        index: 1,
        lock: LOCK_B,
        type: { class: 'tokens', hash: TOKEN },
        capacity: BigInt(14200000000),
      },
      state: 'pressed',
    });
  });

  it('classes it as the transaction page does, by the transaction detail’s own class fields', () => {
    const typed = (
      typeCellType: string,
      shown: TransactionCellDisplay | null
    ): CellSealId['type'] =>
      cellPageSeal({ ...tokenCell, typeCellType, display: shown }, kindOf).id.type;
    expect(typed('nervos-dao', null)).toEqual({ class: 'dao', hash: TOKEN });
    expect(typed('sudt', { ...tokenDisplay, standard: 'sudt' })).toEqual({
      class: 'tokens',
      hash: TOKEN,
    });
    expect(typed('spore', display('object', 'spore'))).toEqual({ class: 'objects', hash: TOKEN });
    expect(typed('did-ckb', display('identity', 'did_ckb'))).toEqual({
      class: 'identities',
      hash: TOKEN,
    });
    expect(typed('ckbfs', display('file', 'ckbfs'))).toEqual({ class: 'other', hash: TOKEN });
    // an unregistered type script
    expect(typed('typed', null)).toEqual({ class: 'other', hash: TOKEN });
    // no type script: a plain CKB cell, a bare stone
    expect(
      cellPageSeal(
        {
          ...tokenCell,
          type: undefined,
          typeScriptHash: undefined,
          typeCellType: 'plain',
          display: null,
        },
        kindOf
      ).id.type
    ).toBeNull();
  });

  it('presses a live cell and inks a spent one', () => {
    expect(cellPageSeal(tokenCell, kindOf).state).toBe('pressed');
    expect(cellPageSeal({ ...tokenCell, status: 'dead' }, kindOf).state).toBe('ink');
  });

  it('refuses a cell without the class fields the cell detail always serves, never classing it plain', () => {
    expect(() => cellPageSeal({ ...tokenCell, typeCellType: undefined }, kindOf)).toThrow(
      `Cell ${TX}:1: no typeCellType`
    );
    expect(() => cellPageSeal({ ...tokenCell, display: undefined }, kindOf)).toThrow(
      `Cell ${TX}:1: no display`
    );
    expect(() => cellPageSeal({ ...tokenCell, status: undefined }, kindOf)).toThrow(
      `Cell ${TX}:1: no such cell status undefined`
    );
  });

  it('refuses a class and a type script hash that disagree, naming the out point', () => {
    expect(() => cellPageSeal({ ...tokenCell, typeScriptHash: undefined }, kindOf)).toThrow(
      `Cell ${TX}:1: class tokens but no type script hash`
    );
    expect(() =>
      cellPageSeal({ ...tokenCell, type: undefined, typeCellType: 'plain', display: null }, kindOf)
    ).toThrow(`Cell ${TX}:1: class ckb (no type script) but a type script hash ${TOKEN}`);
  });

  it('gives a cell the same seal id on its transaction’s page and on its own page', () => {
    const output = (lock: string, over: Record<string, unknown> = {}) => ({
      capacity: '6100000000',
      commonKnowledgeSize: '61',
      address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgjk5zhk',
      lock: script(`lock ${lock}`),
      cellType: 'plain',
      display: null,
      status: 'live',
      isChange: false,
      ...over,
    });
    const tx = {
      hash: TX,
      status: 'committed',
      isCellbase: false,
      inputs: [],
      outputs: [
        output(LOCK_A),
        output(LOCK_B, {
          capacity: '14200000000',
          commonKnowledgeSize: '142',
          type: tokenCell.type,
          cellType: 'xudt',
          display: tokenDisplay,
        }),
        output(LOCK_C, { status: 'consumed' }),
      ],
      scriptGroups: [
        group('type', true, TOKEN, [], [1]),
        group('lock', false, LOCK_A, [], [0]),
        group('lock', false, LOCK_B, [], [1]),
        group('lock', false, LOCK_C, [], [2]),
      ],
    } as unknown as TransactionDetail;
    const { outputs } = cellTiles(tx, undefined);
    expect(outputs[1].seal).toEqual(cellPageSeal(tokenCell, kindOf));
    expect(outputs[2].seal).toEqual(
      cellPageSeal(
        {
          ...tokenCell,
          outputIndex: 2,
          capacity: '6100000000',
          lockScriptHash: LOCK_C,
          type: undefined,
          typeScriptHash: undefined,
          typeCellType: 'plain',
          display: null,
          status: 'dead',
        },
        kindOf
      )
    );
  });
});
