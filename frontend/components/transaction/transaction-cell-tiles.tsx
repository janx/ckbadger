'use client';
import Link from '@/components/ui/link';
import { TransactionCellContent } from '@/components/transaction/transaction-cell-content';
import type { CellTileData } from '@/components/ui/cell-tile';
import type { TransactionDetail, TransactionCellDisplay, ScriptLookupResponse } from '@/lib/api';
import {
  inputSealState,
  outputSealState,
  transactionCellSealId,
  transactionScriptHashes,
} from '@/lib/cell-seal';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { cellLockFacts, lockWords, lockWordsText } from '@/components/ui/lock-kind';
import { ShortId } from '@/components/ui/short-id';
function required<T>(value: T | null | undefined, hash: string, field: string): T {
  if (value === null || value === undefined)
    throw new Error(`Transaction ${hash} missing required field "${field}"`);
  return value;
}
export function kindOf(cell: {
  /** `null` is the API's: a cellbase's null-out-point input has no cell, so no type. */
  type?: { codeHash: string } | null;
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

/**
 * A code hash's name in the script registry, the one rule for every writer of one: the lookup's
 * name, unless the registry has none ('unknown' is its word for none).
 */
export function registeredName(
  lookup: ScriptLookupResponse | undefined,
  codeHash: string
): string | null {
  const name = lookup?.[codeHash]?.name;
  return name && name.toLowerCase() !== 'unknown' ? name : null;
}

/** An output's state once a later transaction spent it (lane P1): the widest state a tile writes. */
export const OUTPUT_SPENT = 'spent later';

export function cellTiles(
  tx: TransactionDetail,
  lookup: ScriptLookupResponse | undefined
): { inputs: CellTileData[]; outputs: CellTileData[] } {
  const inputs = required(tx.inputs, tx.hash, 'inputs');
  const outputs = required(tx.outputs, tx.hash, 'outputs');
  // A cell's seal (the seals by kind): its lock and type hash from the transaction's script
  // groups, read once here (the API always sends them, the groups that did not run too).
  const scripts = transactionScriptHashes(
    tx.hash,
    required(tx.scriptGroups, tx.hash, 'scriptGroups')
  );
  // A tile is drawn for a cell (an input that is one: never the cellbase's null out point, whose
  // address and lock the API serves as null and which gets no tile), and its owner and its lock
  // are that one cell's (lane B4 second fix round). One path: the lock is the cell's script, and
  // its words are the one decision's (`lockWords`: the default lock is not named; the registry's
  // name, else "lock <code hash>"); the address, where the lock has one, is that lock spelled, so
  // a cell with no lock, or an address that spells another lock, is refused with the transaction
  // and the cell named. A lock with no address form (args over 599 bytes: the API sends no
  // address) is owned by its lock hash, as a row writes a party with no address.
  const ownerOf = (
    cell: {
      lock?: { codeHash: string; hashType: string; args: string } | null;
      address?: string | null;
    },
    side: 'input' | 'output',
    i: number
  ) => {
    const context = `${side} ${i}`;
    const lock = required(cell.lock ?? undefined, tx.hash, `${context}.lock`);
    const address = cell.address ?? null;
    const words = lockWords(
      cellLockFacts(
        { lock, name: registeredName(lookup, lock.codeHash), address },
        `Transaction ${tx.hash} ${context}`
      ),
      'inline'
    );
    const lockHash =
      address === null
        ? required(scripts[side].get(i)?.lock, tx.hash, `${context}'s lock script group`)
        : null;
    return {
      owner: (
        // the row size, as the summary, the balances and What moved write the same party (R9): a
        // tile's text column is 215 px at its narrowest (1024; 343 at 1280, 246 at 360), where
        // the address, 13 characters at 12 px (93.6 px), stands whole, its lock wrapping under it
        <Link
          href={`/address/${address ?? lockHash}`}
          className="text-text hover:text-jade"
          title={lockHash ?? undefined}
        >
          {address !== null ? (
            <ShortId value={address} kind="address" size="row" />
          ) : (
            <ShortId value={lockHash!} kind="hash" size="row" />
          )}
        </Link>
      ),
      lockKind: words && lockWordsText(words, 'row'),
    };
  };
  const content = (cell: { type?: { codeHash: string } | null; cellType?: string | null }) => {
    const kind = kindOf(cell);
    const name = cell.type ? registeredName(lookup, cell.type.codeHash) : null;
    return name !== null
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
          const kind = kindOf(cell);
          const capacity = required(cell.capacity, tx.hash, `input ${i} capacity`);
          const { owner, lockKind } = ownerOf(cell, 'input', i);
          return {
            id: `in:${i}`,
            testId: `tx-io-input-${i}`,
            href: `/cell/${out.txHash}-${out.index}`,
            outPoint: { txHash: out.txHash, index: out.index },
            label: `input ${i}`,
            owner,
            lockKind,
            kind,
            seal: {
              id: transactionCellSealId(scripts, 'input', i, {
                outPoint: out,
                cls: kind,
                capacity,
              }),
              state: inputSealState(tx.status),
            },
            content: cell.display ? (
              <TransactionCellContent
                display={cell.display}
                cellHref={`/cell/${out.txHash}-${out.index}`}
                size="row"
              />
            ) : (
              content(cell)
            ),
            capacity,
            occupiedBytes: exactUnsignedInteger(
              required(cell.commonKnowledgeSize, tx.hash, `input ${i} occupied`),
              `${tx.hash}: input ${i} occupied bytes`
            ).toString(),
            state: 'consumed',
          };
        }),
    outputs: outputs.map((cell, i) => {
      const outPoint = { txHash: tx.hash, index: i };
      const kind = kindOf(cell);
      const status = required(cell.status, tx.hash, `output ${i} status`);
      const { owner, lockKind } = ownerOf(cell, 'output', i);
      return {
        id: `out:${i}`,
        testId: `tx-io-output-${i}`,
        href: `/cell/${tx.hash}-${i}`,
        outPoint,
        label: `output ${i}`,
        owner,
        // F2-tx (R2b-a 40, TX-22): the API marks change outputs; the tile says so beside the lock.
        lockKind: changeMarked(lockKind, cell.isChange),
        kind,
        seal: {
          id: transactionCellSealId(scripts, 'output', i, {
            outPoint,
            cls: kind,
            capacity: cell.capacity,
          }),
          state: outputSealState(status),
        },
        content: cell.display ? (
          <TransactionCellContent
            display={cell.display}
            cellHref={`/cell/${tx.hash}-${i}`}
            size="row"
          />
        ) : cell.codeCell ? (
          <CodeCellContent codeCell={cell.codeCell} />
        ) : (
          content(cell)
        ),
        capacity: cell.capacity,
        occupiedBytes: exactUnsignedInteger(
          cell.commonKnowledgeSize,
          `${tx.hash}: output ${i} occupied bytes`
        ).toString(),
        // lane P1 (the plan's P1 paragraph): an output spent since was spent by a later
        // transaction, "spent later"; "consumed" is its input neighbours' word, consumed here
        state: status === 'consumed' ? OUTPUT_SPENT : status,
      };
    }),
  };
}

/**
 * The two sheets' totals (seals by kind, C11): the API's exact sums of the capacity a transaction
 * consumed and created (`inputsCapacity`, `outputsCapacity`), never summed from the tiles. The
 * detail endpoint serves both for every transaction it describes — indexed, in the pool, or
 * committed but not indexed yet; a cellbase's consumed sum is 0 — so a missing one is an error,
 * raised with the transaction named.
 */
export function sheetTotals(tx: TransactionDetail): {
  consumedCapacity: string;
  createdCapacity: string;
} {
  return {
    consumedCapacity: required(tx.inputsCapacity, tx.hash, 'inputsCapacity'),
    createdCapacity: required(tx.outputsCapacity, tx.hash, 'outputsCapacity'),
  };
}

/**
 * Round 3 (contracts R-B · R-9, SC-K5): an output whose data is a registry script's binary, as
 * the CellCode board's tiles say it ("code JoyID v5"). `version` is the API's first-deployment
 * place; null means no indexed code cell carries this binary yet (a pending deployment).
 */
function CodeCellContent({
  codeCell,
}: {
  codeCell: NonNullable<NonNullable<TransactionDetail['outputs']>[number]['codeCell']>;
}) {
  return (
    <span title={codeCell.versionHash}>
      code{' '}
      <span className="text-text-dim">
        {codeCell.version === null
          ? `${codeCell.scriptName}, new version, not indexed yet`
          : `${codeCell.scriptName} v${codeCell.version}`}
      </span>
    </span>
  );
}

// F2-tx (TX-22): a change output says so beside its lock, "JoyID · change", and alone where its
// lock is not named (the default lock): "change".
function changeMarked(lockKind: string | null, isChange: boolean): string | null {
  if (!isChange) return lockKind;
  return lockKind ? `${lockKind} · change` : 'change';
}
