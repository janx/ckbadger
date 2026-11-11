import { kindOf } from '@/components/transaction/transaction-cell-tiles';
import type { AddressCell } from '@/lib/api';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import type { SealId } from '@/lib/seal/stones';

/** The seal id of a cell (`lib/seal`). */
export type CellSealId = Extract<SealId, { kind: 'cell' }>;

/**
 * The seal of one of an address's live cells (the seals by kind, C3): its out point, its lock,
 * its type's class and hash, its capacity in shannons. The class is the one the transaction page
 * and the cell page give the same cell — `kindOf()`, called with the API's `cellType` vocabulary
 * (`typeCellType` here) — so a cell has one seal everywhere; `AddressCell.class` is the address
 * page's own grouping (its filters) and is not read here.
 *
 * A plain mapping, no second reading of the chain: a cell kindOf calls plain CKB has no type
 * script and its seal no type; any other class has one and its seal carries that type script's
 * hash. A cell that breaks either, a response without `typeCellType` (an API older than this
 * page), or a capacity that is not a whole number of shannons is refused, naming the out point.
 */
export function addressCellSealId(c: AddressCell): CellSealId {
  const out = `address cell ${c.txHash}:${c.outputIndex}`;
  if (typeof c.typeCellType !== 'string')
    throw new Error(
      `${out}: the API served no typeCellType, so the cell cannot be classed as the transaction page classes it`
    );
  const cls = kindOf({
    display: c.display,
    cellType: c.typeCellType,
    type: c.typeCodeHash ? { codeHash: c.typeCodeHash } : null,
  });
  const capacity = exactUnsignedInteger(c.capacity, `${out}: capacity`);
  const base = {
    kind: 'cell',
    tx: c.txHash,
    index: c.outputIndex,
    lock: c.lockScriptHash,
    capacity,
  } as const;
  if (cls === 'ckb') {
    if (c.typeScriptHash !== null)
      throw new Error(
        `${out}: class ckb (cell type ${JSON.stringify(c.typeCellType)}) with type script hash ${c.typeScriptHash}`
      );
    return { ...base, type: null };
  }
  if (c.typeScriptHash === null)
    throw new Error(
      `${out}: class ${cls} (cell type ${JSON.stringify(c.typeCellType)}) with no type script hash`
    );
  return { ...base, type: { class: cls, hash: c.typeScriptHash } };
}
