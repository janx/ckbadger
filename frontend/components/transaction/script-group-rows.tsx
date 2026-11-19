import Link from '@/components/ui/link';
import { ShortId } from '@/components/ui/short-id';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { formatStandard } from '@/components/activity-event-row';
import { kindOf, registeredName } from '@/components/transaction/transaction-cell-tiles';
import { cellLockFacts, lockWords } from '@/components/ui/lock-kind';
import type { MatrixColumn, MatrixGroup } from '@/components/transaction/script-group-matrix';
import type {
  CellDep,
  ScriptLookupResponse,
  TransactionDetail,
  TransactionScriptGroup,
  TransactionWitness,
} from '@/lib/api';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { lockArgsLine } from '@/lib/lock-args';
import { shortenHash, type ShortSize } from '@/lib/shorten';
import { shortCkb } from '@/lib/tx-detail-model';
import { TYPE_ID_CODE_HASH } from '@/lib/tx-continuity';

/**
 * A transaction's script groups as rows (Main.dc.html Script groups; lane D3: one wording on the
 * transaction page and on the cell page, PA-24): what `ScriptGroupMatrix` draws, built once from the
 * API's transaction, its cell deps and the registry's names. The page that owns the witness
 * inspector passes `select`; a page without one draws the same rows with nothing to select.
 */

function required<T>(value: T | null | undefined, hash: string, field: string): T {
  if (value === null || value === undefined)
    throw new Error(`Transaction ${hash} missing required field "${field}"`);
  return value;
}

/** A group's identity on the page: its kind, whether it ran, its script hash. */
export function groupKey(group: TransactionScriptGroup): string {
  return `${group.kind}:${group.ran ? 'run' : 'attached'}:${group.scriptHash}`;
}

/** The tiles a group covers, by their ids (`in:0`, `out:1`). */
export function groupCells(g: TransactionScriptGroup): string[] {
  return [...g.inputIndices.map((i) => `in:${i}`), ...g.outputIndices.map((i) => `out:${i}`)];
}

/**
 * A group's name and its script page, from one read of the registry's name of its script's pair
 * (the one rule, `registeredName`; lane rd/api-lock-names): the name, or the hash at the row size;
 * the script's page by that name, or by the code hash where the registry has none. A lock group's
 * name is the one decision's (`lockWords`, lane P1 follow-up): a lock on the default lock's code
 * hash by data is another lock, never named the default lock, and it is written by its code hash
 * and "by data", its page that lock's.
 */
function groupNaming(
  g: TransactionScriptGroup,
  lookup: ScriptLookupResponse | undefined
): { label: string; href: string } {
  const registered = registeredName(lookup, g.script.codeHash, g.script.hashType);
  let name = registered;
  let by = '';
  if (g.kind === 'lock') {
    const lock = lockWords(
      cellLockFacts(
        { lock: g.script, name: registered, address: null },
        `lock group ${g.scriptHash}`
      ),
      'column'
    );
    if (lock === null)
      throw new Error(`lock group ${g.scriptHash}: a column's lock words are never empty`);
    name = lock.form === 'not-indexed' ? null : lock.name;
    by = lock.form === 'lock' && lock.byHashType ? ` by ${lock.byHashType}` : '';
  }
  return {
    label: `${name ?? shortenHash(g.script.codeHash, 'row')}${by}`,
    href: getScriptDetailHref({
      name,
      codeHash: g.script.codeHash,
      hashType: g.script.hashType,
      scriptKind: g.kind,
    }),
  };
}

/** The registry's name for a group's code hash, or the hash at the row size. */
export function groupLabel(g: TransactionScriptGroup, lookup: ScriptLookupResponse | undefined) {
  return groupNaming(g, lookup).label;
}

/** The group's script page. */
export function groupHref(g: TransactionScriptGroup, lookup: ScriptLookupResponse | undefined) {
  return groupNaming(g, lookup).href;
}

/** A witness's kind in words. */
export function witnessKindLabel(kind: TransactionWitness['kind']): string {
  switch (kind) {
    case 'witness_args':
      return 'WitnessArgs';
    case 'ckbfs':
      return 'CKBFS content';
    case 'cobuild':
      return 'CoBuild message';
    case 'cellbase':
      return 'Cellbase witness';
    case 'empty':
      return 'empty';
    case 'opaque':
      return 'opaque bytes';
  }
}

export interface ScriptGroupRowsOptions {
  lookup: ScriptLookupResponse | undefined;
  /** The transaction's cell deps; undefined while they are read. */
  deps: readonly CellDep[] | undefined;
  /** Why the cell deps could not be read, when they could not. */
  depsError: Error | null;
  /** A transaction not yet committed and indexed: its outputs are no cells yet. */
  provisional: boolean;
  /** Where a code column's dep kind leads: the dep row on the transaction page. */
  depHref: (index: number) => string;
  /**
   * The page's witness inspector, where it has one: the group it has selected and how it selects a
   * witness and a group.
   */
  select?: {
    group: string | null;
    pick: (witness: number | null, group: string | null) => void;
  };
}

/** The matrix's columns (the transaction's cells in order) and its rows (its script groups). */
export function scriptGroupRows(
  tx: TransactionDetail,
  { lookup, deps, depsError, provisional, depHref, select }: ScriptGroupRowsOptions
): { inputs: MatrixColumn[]; outputs: MatrixColumn[]; groups: MatrixGroup[] } {
  const hash = tx.hash;
  const groups = required(tx.scriptGroups, hash, 'scriptGroups');
  const witnesses = required(tx.witnessDetails, hash, 'witnessDetails');
  const column = (
    side: 'in' | 'out',
    i: number,
    cell: NonNullable<TransactionDetail['inputs']>[number]
  ): MatrixColumn => {
    const display = cell.display;
    const kind = kindOf(cell);
    const top = display
      ? display.kind === 'file'
        ? display.standard.toUpperCase()
        : formatStandard(display.standard)
      : kind === 'ckb'
        ? 'plain'
        : kind === 'dao'
          ? 'DAO'
          : cell.type
            ? (registeredName(lookup, cell.type.codeHash, cell.type.hashType) ?? 'typed')
            : 'plain';
    // Lane L (L1): the column names a cell; it links to the cell's page. An input is the cell it
    // spends; an output is a cell only once the transaction is committed.
    const outPoint =
      side === 'in'
        ? required(cell.previousOutput, hash, `input ${i} outpoint`)
        : provisional
          ? null
          : { txHash: hash, index: i };
    return {
      id: `${side}:${i}`,
      top,
      label: `${side} #${i}`,
      ...(outPoint
        ? {
            href: `/cell/${outPoint.txHash}-${outPoint.index}`,
            title: `${outPoint.txHash}:${outPoint.index}`,
          }
        : { title: 'not a cell until the transaction is committed' }),
      cap:
        cell.capacity === undefined
          ? null
          : shortCkb(exactUnsignedInteger(cell.capacity, `${hash}: ${side} ${i} capacity`)),
    };
  };
  const inputs = tx.isCellbase ? [] : (tx.inputs ?? []).map((c, i) => column('in', i, c));
  const outputs = (tx.outputs ?? []).map((c, i) => column('out', i, c));
  const rows: MatrixGroup[] = groups.map((g) => {
    const key = groupKey(g);
    const codeDeps = (deps ?? []).flatMap((d, i) =>
      d.scriptHashes.includes(g.scriptHash) ? [{ d, i }] : []
    );
    const attachedDeps = (deps ?? []).flatMap((d, i) =>
      !g.ran && d.outputLockScriptHashes.includes(g.scriptHash) ? [{ d, i }] : []
    );
    const w = g.witnessIndex === null ? null : witnesses.find((x) => x.index === g.witnessIndex);
    const firstCell = g.inputIndices.length
      ? tx.inputs?.[g.inputIndices[0]]
      : tx.outputs?.[g.outputIndices[0]];
    const mark = g.kind === 'type' && firstCell ? kindOf(firstCell) : null;
    // A dep's out point, linked to its code cell, at the form's size.
    const depCell = (d: CellDep, size: ShortSize) => (
      <Link href={`/cell/${d.outPointTxHash}-${d.outPointIndex}`} look="inline">
        <ShortId value={d.outPointTxHash} kind="hash" size={size} />
        <OutpointIndex index={d.outPointIndex} />
      </Link>
    );
    return {
      id: key,
      kind: g.kind,
      name: groupLabel(g, lookup),
      href: groupHref(g, lookup),
      mark: mark && mark !== 'ckb' && mark !== 'other' ? mark : null,
      sub: (size: ShortSize) => {
        const line =
          g.kind === 'lock' && firstCell
            ? lockArgsLine(firstCell.lockArgsDecoded, firstCell.lockArgsDecodeError, size)
            : null;
        return line ?? `args ${shortenHash(g.script.args, size)}`;
      },
      ran: g.ran,
      cellIds: groupCells(g),
      cycles: g.cycles,
      status: provisional && g.ran ? 'will run when committed' : undefined,
      focusTestId: `tx-script-group-focus-${g.witnessIndex ?? 'none'}-${g.kind}`,
      onFocus: select
        ? () => (select.group === key ? select.pick(null, null) : select.pick(g.witnessIndex, key))
        : undefined,
      witness:
        g.witnessIndex === null
          ? null
          : {
              index: g.witnessIndex,
              detail: w ? { kind: witnessKindLabel(w.kind), size: w.size } : null,
              onSelect: select
                ? () => select.pick(g.witnessIndex, select.group === key ? null : key)
                : undefined,
            },
      code: (size: ShortSize) =>
        !g.ran ? (
          attachedDeps.length > 0 ? (
            attachedDeps.map(({ d, i }) => (
              // the dep and its cell are links, the reading ink (R3); "not run here" a note
              <span key={i} className="text-text-dim block">
                <span className="text-text">
                  <Link href={depHref(i)} look="inline">
                    code
                  </Link>{' '}
                  {depCell(d, size)}
                </span>{' '}
                · not run here
              </span>
            ))
          ) : null
        ) : provisional ? (
          <span className="text-text-dim">read once committed</span>
        ) : depsError ? (
          <span className="text-text-dim">cell deps unreadable</span>
        ) : !deps ? (
          <span className="text-text-dim">Resolving code…</span>
        ) : codeDeps.length === 0 ? (
          // the built-in Type ID runs with no cell dep; any other code with none is unmatched
          <span className="text-text-dim">
            {g.script.hashType === 'type' && g.script.codeHash.toLowerCase() === TYPE_ID_CODE_HASH
              ? 'Built-in Type ID, no cell dep'
              : 'No matching dep'}
          </span>
        ) : (
          // The kind jumps to the dep row's evidence; the out point opens the code cell.
          codeDeps.map(({ d, i }) => (
            <span key={i} className="block">
              <Link href={depHref(i)} look="inline">
                {d.depType === 'dep_group' ? 'dep group' : 'code'}
              </Link>{' '}
              {depCell(d, size)}
            </span>
          ))
        ),
    };
  });
  return { inputs, outputs, groups: rows };
}
