/**
 * A cell's seal (the seals by kind, §0): which stone a cell presses and in which ink, built from
 * what the API serves for it — its out point, its lock and type script hashes, the class
 * `kindOf()` gives it, its capacity in shannons, its state. Nothing is hashed, classed or counted
 * a second way here: each field is read from one API field, and a field that is not there, or two
 * that disagree, is refused with the cell named.
 *
 * Two data shapes: a cell of a transaction detail (its script hashes from the transaction's
 * `scriptGroups`, looked up by its position), and a `Cell` of `/cells/{tx}/{index}` (the cell
 * page, its own `lockScriptHash` / `typeScriptHash`).
 */
import type {
  Cell,
  TransactionCellDisplay,
  TransactionScriptGroup,
  TransactionStatus,
} from '@/lib/api';
import type { AssetClass } from '@/lib/design-tokens';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import type { SealId, SealState } from '@/lib/seal/stones';

/** The seal id of a cell: its out point, its lock, its type's class and hash, its capacity. */
export type CellSealId = Extract<SealId, { kind: 'cell' }>;

/** A cell's seal: the stone of its id, in the ink of its state. */
export interface CellSeal {
  readonly id: CellSealId;
  readonly state: SealState;
}

/** What a cell's seal id is read from, every field as the API serves it. */
export interface CellSealFields {
  /** The out point: its transaction's hash and the output index. */
  readonly tx: string;
  readonly index: number;
  /** The lock script's hash. */
  readonly lock: string;
  /** The type script's hash; null when the cell has no type script. */
  readonly typeHash: string | null;
  /** `kindOf()`'s class of the cell: `ckb` exactly when it has no type script. */
  readonly cls: AssetClass;
  /** Shannons, the API's decimal string. */
  readonly capacity: string;
}

/**
 * The seal id of a cell (`of` names it in a refusal). Class `ckb` is the class of a cell with no
 * type script, so it is `type: null`; a class other than `ckb` with no type hash, or `ckb` with
 * one, is two API fields disagreeing about the cell and is refused, never drawn as either.
 */
export function cellSealId(of: string, fields: CellSealFields): CellSealId {
  const { tx, index, lock, typeHash, cls, capacity } = fields;
  let type: CellSealId['type'];
  if (cls === 'ckb') {
    if (typeHash !== null)
      throw new Error(`${of}: class ckb (no type script) but a type script hash ${typeHash}`);
    type = null;
  } else {
    if (typeHash === null) throw new Error(`${of}: class ${cls} but no type script hash`);
    type = { class: cls, hash: typeHash };
  }
  return {
    kind: 'cell',
    tx,
    index,
    lock,
    type,
    capacity: exactUnsignedInteger(capacity, `${of}: capacity`),
  };
}

/* ---------- a cell of a transaction detail ---------- */

/** Which side of a transaction a cell is on: what it consumed, or what it created. */
export type CellSide = 'input' | 'output';

/** The script hashes of one cell of a transaction, as its script groups list them. */
interface ScriptSlot {
  lock?: string;
  type?: string;
}

/**
 * A transaction's script hashes by cell: for each input and output position, the hash of the lock
 * group and of the type group that list it. Built once per transaction from its `scriptGroups`
 * (the API lists every group, the ones that did not run too, so every output's lock is there).
 */
export interface TransactionScriptHashes {
  readonly txHash: string;
  readonly input: ReadonlyMap<number, Readonly<ScriptSlot>>;
  readonly output: ReadonlyMap<number, Readonly<ScriptSlot>>;
}

/**
 * Reads a transaction's script groups into its cells' script hashes. A cell has one lock script
 * and at most one type script, so a cell listed by two groups of one kind is refused, naming the
 * transaction, the cell and both hashes.
 */
export function transactionScriptHashes(
  txHash: string,
  groups: readonly TransactionScriptGroup[]
): TransactionScriptHashes {
  const input = new Map<number, ScriptSlot>();
  const output = new Map<number, ScriptSlot>();
  const list = (side: CellSide, slots: Map<number, ScriptSlot>, group: TransactionScriptGroup) => {
    for (const position of side === 'input' ? group.inputIndices : group.outputIndices) {
      if (!Number.isSafeInteger(position) || position < 0)
        throw new Error(
          `Transaction ${txHash}: ${group.kind} group ${group.scriptHash} lists ${side} ${String(position)}, not a position`
        );
      const slot = slots.get(position) ?? {};
      const listed = slot[group.kind];
      if (listed !== undefined)
        throw new Error(
          `Transaction ${txHash}: ${side} ${position} is listed by two ${group.kind} groups, ${listed} and ${group.scriptHash}`
        );
      slot[group.kind] = group.scriptHash;
      slots.set(position, slot);
    }
  };
  for (const group of groups) {
    if (group.kind !== 'lock' && group.kind !== 'type')
      throw new Error(
        `Transaction ${txHash}: script group ${group.scriptHash} of no kind ${JSON.stringify(group.kind)}`
      );
    list('input', input, group);
    list('output', output, group);
  }
  return { txHash, input, output };
}

/**
 * The seal id of a transaction's input or output at `position`: its out point (an input's
 * previous output; an output's this transaction and its position), `kindOf()`'s class of it and
 * its capacity, its lock and type hash from the transaction's script groups. A cell no lock group
 * lists, or a typed cell no type group lists, is refused, naming the transaction and the cell.
 */
export function transactionCellSealId(
  scripts: TransactionScriptHashes,
  side: CellSide,
  position: number,
  cell: {
    readonly outPoint: { readonly txHash: string; readonly index: number };
    readonly cls: AssetClass;
    readonly capacity: string;
  }
): CellSealId {
  const { outPoint } = cell;
  const of = `Transaction ${scripts.txHash} ${side} ${position} (cell ${outPoint.txHash}:${outPoint.index})`;
  const slot = scripts[side].get(position);
  if (slot?.lock === undefined) throw new Error(`${of}: no lock script group lists it`);
  return cellSealId(of, {
    tx: outPoint.txHash,
    index: outPoint.index,
    lock: slot.lock,
    typeHash: slot.type === undefined ? null : slot.type,
    cls: cell.cls,
    capacity: cell.capacity,
  });
}

/**
 * The ink of a transaction's input: a committed transaction consumed it (the dead ink); a
 * transaction not yet committed has consumed nothing yet, so its input is still a live cell
 * (pressed).
 */
export function inputSealState(txStatus: TransactionStatus): SealState {
  switch (txStatus) {
    case 'committed':
      return 'ink';
    case 'pending':
    case 'proposed':
      return 'pressed';
  }
  throw new Error(`no such transaction status ${JSON.stringify(txStatus)}`);
}

/**
 * The ink of a transaction's output, by the API's state of it: live (pressed), consumed since (the
 * dead ink), or the output of a transaction this explorer has not indexed as committed yet, a cell
 * that does not stand yet (`pending`: unpressed, the dashed stone).
 */
export function outputSealState(status: 'pending' | 'live' | 'consumed'): SealState {
  switch (status) {
    case 'live':
      return 'pressed';
    case 'consumed':
      return 'ink';
    case 'pending':
      return 'unpressed';
  }
  throw new Error(`no such output status ${JSON.stringify(status)}`);
}

/* ---------- the cell page ---------- */

/** What `kindOf()` classes a cell by. */
export interface CellClassFields {
  readonly type: NonNullable<Cell['type']> | null;
  readonly cellType: string;
  readonly display: TransactionCellDisplay | null;
}

/**
 * The seal of the cell `/cells/{tx}/{index}` serves: its id — the out point, `lockScriptHash`,
 * `typeScriptHash`, the class `classOf` (the transaction page's `kindOf()`) gives its type script,
 * `typeCellType` and `display` (the transaction detail's own class fields for this cell), its
 * capacity — and its ink: a spent cell in the dead ink, a live one pressed. The class fields and
 * the status are always sent there; their absence is refused, never classed as a plain cell.
 */
export function cellPageSeal(
  cell: Pick<
    Cell,
    | 'txHash'
    | 'outputIndex'
    | 'lockScriptHash'
    | 'typeScriptHash'
    | 'type'
    | 'typeCellType'
    | 'display'
    | 'capacity'
    | 'status'
  >,
  classOf: (fields: CellClassFields) => AssetClass
): CellSeal {
  const of = `Cell ${cell.txHash}:${cell.outputIndex}`;
  if (cell.typeCellType === undefined) throw new Error(`${of}: no typeCellType`);
  if (cell.display === undefined) throw new Error(`${of}: no display`);
  const cls = classOf({
    type: cell.type === undefined ? null : cell.type,
    cellType: cell.typeCellType,
    display: cell.display,
  });
  return {
    id: cellSealId(of, {
      tx: cell.txHash,
      index: cell.outputIndex,
      lock: cell.lockScriptHash,
      typeHash:
        cell.typeScriptHash === undefined || cell.typeScriptHash === null
          ? null
          : cell.typeScriptHash,
      cls,
      capacity: cell.capacity,
    }),
    state: cellPageSealState(of, cell.status),
  };
}

/** The ink of the cell page's seal: a spent cell in the dead ink, a live one pressed. */
export function cellPageSealState(of: string, status: Cell['status']): SealState {
  switch (status) {
    case 'live':
      return 'pressed';
    case 'dead':
      return 'ink';
  }
  throw new Error(`${of}: no such cell status ${JSON.stringify(status)}`);
}
