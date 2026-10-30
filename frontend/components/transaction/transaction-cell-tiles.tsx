'use client';
import Link from '@/components/ui/link';
import { TransactionCellContent } from '@/components/transaction/transaction-cell-content';
import type { CellTileData } from '@/components/ui/cell-tile';
import type { TransactionDetail, TransactionCellDisplay, ScriptLookupResponse } from '@/lib/api';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { truncateHash } from '@/lib/utils';
function required<T>(value: T | null | undefined, hash: string, field: string): T {
  if (value === null || value === undefined)
    throw new Error(`Transaction ${hash} missing required field "${field}"`);
  return value;
}
export function kindOf(cell: {
  type?: { codeHash: string };
  cellType?: string | null;
  display?: TransactionCellDisplay | null;
}): CellTileData['kind'] {
  if (cell.display) {
    const kinds = {
      token: 'tokens',
      object: 'objects',
      identity: 'identities',
      file: 'other',
    } as const;
    return kinds[cell.display.kind];
  }
  switch (cell.cellType) {
    case 'nervos-dao':
      return 'dao';
    case 'udt':
    case 'sudt':
    case 'xudt':
      return 'tokens';
    case 'spore':
    case 'm-nft':
      return 'objects';
    case 'bit-account':
    case 'bit-cell':
    case 'did-ckb':
    case 'dotcell-account':
      return 'identities';
    default:
      return cell.type ? 'other' : 'ckb';
  }
}
export function cellTiles(
  tx: TransactionDetail,
  lookup: ScriptLookupResponse | undefined
): { inputs: CellTileData[]; outputs: CellTileData[] } {
  const inputs = required(tx.inputs, tx.hash, 'inputs');
  const outputs = required(tx.outputs, tx.hash, 'outputs');
  const owner = (address: string | undefined, context: string) => (
    <Link
      href={`/address/${required(address, tx.hash, `${context}.address`)}`}
      className="text-text hover:text-jade"
    >
      {truncateHash(address!, 10, 6)}
    </Link>
  );
  const lockKind = (codeHash: string | undefined) => {
    const name = codeHash ? lookup?.[codeHash]?.name : undefined;
    return name && name.toLowerCase() !== 'unknown' ? name : null;
  };
  const content = (cell: { type?: { codeHash: string }; cellType?: string | null }) => {
    const kind = kindOf(cell);
    const name = cell.type ? lookup?.[cell.type.codeHash]?.name : undefined;
    return name && name.toLowerCase() !== 'unknown'
      ? name
      : kind === 'ckb'
        ? 'CKB'
        : kind === 'tokens'
          ? 'Token'
          : kind === 'objects'
            ? 'Object'
            : kind === 'identities'
              ? 'Identity'
              : kind === 'dao'
                ? 'Nervos DAO'
                : 'Type script';
  };
  return {
    inputs: tx.isCellbase
      ? []
      : inputs.map((cell, i) => {
          const out = required(cell.previousOutput, tx.hash, `input ${i} outpoint`);
          return {
            id: `in:${i}`,
            testId: `tx-io-input-${i}`,
            href: `/cell/${out.txHash}-${out.index}`,
            label: `input ${i}`,
            owner: owner(cell.address, `input ${i}`),
            lockKind: lockKind(cell.lock?.codeHash),
            kind: kindOf(cell),
            content: cell.display ? (
              <TransactionCellContent
                display={cell.display}
                cellHref={`/cell/${out.txHash}-${out.index}`}
              />
            ) : (
              content(cell)
            ),
            capacity: required(cell.capacity, tx.hash, `input ${i} capacity`),
            occupied: (
              exactUnsignedInteger(
                required(cell.commonKnowledgeSize, tx.hash, `input ${i} occupied`),
                `${tx.hash}: input ${i} occupied bytes`
              ) * BigInt(100000000)
            ).toString(),
            state: 'consumed',
          };
        }),
    outputs: outputs.map((cell, i) => ({
      id: `out:${i}`,
      testId: `tx-io-output-${i}`,
      href: `/cell/${tx.hash}-${i}`,
      label: `output ${i}`,
      owner: owner(cell.address, `output ${i}`),
      // F2-tx (R2b-a 40, TX-22): the API marks change outputs; the tile says so beside the lock.
      lockKind: changeMarked(lockKind(cell.lock?.codeHash), cell.isChange),
      kind: kindOf(cell),
      content: cell.display ? (
        <TransactionCellContent display={cell.display} cellHref={`/cell/${tx.hash}-${i}`} />
      ) : (
        content(cell)
      ),
      capacity: cell.capacity,
      occupied: (
        exactUnsignedInteger(cell.commonKnowledgeSize, `${tx.hash}: output ${i} occupied bytes`) *
        BigInt(100000000)
      ).toString(),
      state: required(cell.status, tx.hash, `output ${i} status`),
    })),
  };
}

// F2-tx (TX-22): "Default Lock · change" (Main.dc.html out #2).
function changeMarked(lockKind: string | null, isChange: boolean): string | null {
  if (!isChange) return lockKind;
  return lockKind ? `${lockKind} · change` : 'change';
}
