'use client';
import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { BlockLink } from '@/components/ui/block-link';
import { DetailSection } from '@/components/ui/detail-layout';
import { TxStructure } from '@/components/transaction/tx-structure';
import {
  CellTilesError,
  readCellTiles,
  sheetTotals,
} from '@/components/transaction/transaction-cell-tiles';
import { ScriptGroupLegend, ScriptGroupMatrix } from '@/components/transaction/script-group-matrix';
import { scriptGroupRows } from '@/components/transaction/script-group-rows';
import { api, type Cell, type TransactionDetail } from '@/lib/api';
import { formatDurationMs } from '@/lib/utils';
import { BOARD_INKS, CELL_COLORS } from '@/lib/design-tokens';
import { TYPE_ID_CODE_HASH, txContinuity } from '@/lib/tx-continuity';
import { ShortId } from '@/components/ui/short-id';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { formatCount } from '@/lib/format-count';
import { Utc8DateTime } from '@/components/ui/utc8-date';
import { unitNode } from '@/components/ui/units';
import { transactionAtCellHref } from '@/lib/tx-cell-link';

/** Which of the cell's transactions: the one that created it, or the one that spent it. */
type Role = 'Created' | 'Spent';

/**
 * One of the cell's transactions, read once for the page (lane D3): its detail, its cell deps and
 * the registry's names for its scripts, under the same keys wherever the page reads them (the
 * embed in tier 1 and the script groups in tier 2 share the reads).
 */
function useCellTransaction(hash: string) {
  const transaction = useQuery({
    queryKey: ['cell-transaction-detail', hash],
    queryFn: () => api.getTransactionDetail(hash),
  });
  const codeHashes = Array.from(
    new Set(
      [...(transaction.data?.inputs ?? []), ...(transaction.data?.outputs ?? [])]
        .flatMap((cell) => [cell.lock?.codeHash, cell.type?.codeHash])
        .concat((transaction.data?.scriptGroups ?? []).map((group) => group.script.codeHash))
        .filter((value): value is string => value !== undefined && value !== TYPE_ID_CODE_HASH)
    )
  );
  // The transaction page's naming path: one lookup of the tx's script references.
  const lookup = useQuery({
    queryKey: ['scriptLookup', codeHashes, hash],
    queryFn: () => api.lookupScripts(codeHashes, hash),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });
  const dependencies = useQuery({
    queryKey: ['cell-transaction-deps', hash],
    queryFn: () => api.getTransactionCellDeps(hash),
  });
  return { transaction, lookup, dependencies };
}

/**
 * The read transaction, or what stands in its place: a line while it is read, its error where it
 * cannot be. A read answered without its evidence, or a transaction without its committed
 * structure, broke an invariant and is refused. `deps` says whether the part needs the cell deps
 * too (the script groups do; the embed draws none, so it neither waits for that read nor fails
 * on it: review of phases C and D, SHOULD 7).
 */
function readTransaction(
  read: ReturnType<typeof useCellTransaction>,
  hash: string,
  label: Role,
  what: string,
  deps: boolean
): { tx: TransactionDetail } | { node: ReactNode } {
  const { transaction } = read;
  const dependencies = deps ? read.dependencies : null;
  if (isAwaited(transaction) || (dependencies !== null && isAwaited(dependencies)))
    return {
      node: (
        <p className="text-text text-small max-w-measure">
          Loading {label.toLowerCase()} transaction {what}…
        </p>
      ),
    };
  if (transaction.error || dependencies?.error)
    return {
      node: (
        <p role="alert" className="text-rouge text-small max-w-measure [overflow-wrap:anywhere]">
          {label} transaction {what} unavailable: {String(transaction.error ?? dependencies?.error)}
        </p>
      ),
    };
  // awaited and failed are answered above: an answered read without its evidence broke an invariant
  if (!transaction.data || (dependencies !== null && !dependencies.data))
    throw new Error(`${label} transaction ${hash}: its read answered without its evidence`);
  const tx: TransactionDetail = transaction.data;
  if (
    tx.status !== 'committed' ||
    !tx.inputs ||
    !tx.outputs ||
    !tx.scriptGroups ||
    !tx.witnessDetails
  )
    throw new Error(`${label} transaction ${hash} lacks committed structure`);
  return { tx };
}

/**
 * One of the cell's transactions in tier 1 (lane D3; the board's embed): a line that says which of
 * the transaction's cells this one is, by whom and when, with the one link to the transaction (at
 * this cell's out point, which the transaction page keeps drawn); then the two sheets with this
 * cell's tile and its continuity partner, each sheet's rest a count (the fold, with nothing to
 * open: the way to the rest is the link). The script groups are the developer tier's.
 */
export function TransactionEvidence({
  hash,
  label,
  focus,
  cell,
  onward,
  afterOnward,
}: {
  hash: string;
  label: Role;
  /** This cell's tile in the transaction (`out:1`, `in:57`). */
  focus: string;
  /** This cell's out point: the link opens the transaction at it (`transactionAtCellHref`). */
  cell: { txHash: string; index: number };
  /** Drawn under the onward bridge (the dashed "Not spent yet" box). */
  afterOnward?: ReactNode;
  /** The focused output's txviz bridge label (the cell carries on past this transaction). */
  onward?: string;
}) {
  const read = useCellTransaction(hash);
  const got = readTransaction(read, hash, label, 'evidence', false);
  if ('node' in got) return got.node;
  const { tx } = got;
  // the tiles as the transaction page reads them: a cell the lock decision refuses fails this
  // part in its place, never the page (review of phases C and D, SHOULD 7)
  const tilesRead = readCellTiles(tx, read.lookup.data);
  const tiles = tilesRead.value;
  if (tiles && ![...tiles.inputs, ...tiles.outputs].some((tile) => tile.id === focus))
    throw new Error(`${label} transaction ${hash} does not contain focused cell ${focus}`);
  // the side's cells, the transaction's list (a tile is drawn for each)
  const side = label === 'Created' ? tx.outputs! : tx.inputs!;
  const verb = label === 'Created' ? 'created' : 'consumed';
  return (
    <div className="space-y-3">
      {/* "one of 2 cells created by 0x1422d49b…6e4c02 · 2026-11-14 12:54:57 UTC+8 · open the
          transaction →": a sentence, the reading ink (R3) */}
      <p data-testid="cell-transaction-line" className="text-text text-small max-w-measure">
        {side.length === 1 ? (
          `the one cell ${verb}`
        ) : (
          <>
            one of {unitNode(formatCount(side.length, 'cell'))} {verb}
          </>
        )}{' '}
        by{' '}
        <span title={hash}>
          <ShortId value={hash} kind="hash" size="row" />
        </span>
        {tx.timestamp && (
          <>
            {' · '}
            <Utc8DateTime timestamp={tx.timestamp} dateTime={tx.timestamp} />
          </>
        )}
        {' · '}
        <Link href={transactionAtCellHref(hash, cell)} look="onward">
          open the transaction
        </Link>
      </p>
      {tilesRead.error !== undefined ? (
        <CellTilesError tx={tx} error={tilesRead.error} />
      ) : (
        <TxStructure
          txHash={hash}
          inputs={tiles!.inputs}
          outputs={tiles!.outputs}
          fee={tx.fee}
          // C11: the sheets' heads end with the API's exact totals (the detail read carries both)
          {...sheetTotals(tx)}
          continuity={txContinuity(tx, tiles!.inputs)}
          focusedCellIds={[focus]}
          meOut={onward !== undefined ? focus : undefined}
          onward={onward}
          afterOnward={afterOnward}
          // the board's embed: this cell and its continuity partner; the rest counted, no control
          fold={{
            head: 0,
            keep: [focus],
            open: { consumed: false, created: false },
          }}
        />
      )}
    </div>
  );
}

/**
 * One transaction's script groups in the developer tier (lane D3, U-17, PA-24): the transaction
 * page's rows in its words, named by the transaction (Lane U follow-up: never two bare "Script
 * groups"). A dep's kind leads to its row on the transaction page.
 */
function TransactionScriptGroups({ hash, label }: { hash: string; label: Role }) {
  const read = useCellTransaction(hash);
  const got = readTransaction(read, hash, label, 'script groups', true);
  const title = `Script groups of the transaction that ${label === 'Created' ? 'created' : 'spent'} it`;
  if ('node' in got)
    return (
      <DetailSection title={title} actions={<ScriptGroupLegend />}>
        {got.node}
      </DetailSection>
    );
  const rows = scriptGroupRows(got.tx, {
    lookup: read.lookup.data,
    deps: read.dependencies.data,
    depsError: null,
    provisional: false,
    depHref: (i) => `/tx/${hash}#tx-dep-${i}`,
  });
  return (
    <DetailSection title={title} actions={<ScriptGroupLegend />}>
      <ScriptGroupMatrix txHash={hash} {...rows} />
    </DetailSection>
  );
}

/**
 * The cell's transactions' script groups, the developer tier's (lane D3): the transaction that
 * created it, and the one that spent it once it is spent.
 */
export function CellScriptGroups({ cell }: { cell: Cell }) {
  if (cell.status === 'dead' && !cell.consumedByTx)
    throw new Error(
      `Consumed cell ${cell.txHash}:${cell.outputIndex} lacks its spending transaction`
    );
  return (
    <>
      <TransactionScriptGroups hash={cell.txHash} label="Created" />
      {cell.status === 'dead' && (
        <TransactionScriptGroups hash={cell.consumedByTx!} label="Spent" />
      )}
    </>
  );
}

/** The dashed "Not spent yet" box: the groups the transaction that spends this cell will run. */
function NotSpentYet({
  cell,
  lockLabel,
  typeName,
}: {
  cell: Cell;
  /** The lock's name as the one decision gives it (`lockWords`): null where it names none. */
  lockLabel: string | null;
  typeName: string | null;
}) {
  const replaced =
    cell.type?.hashType === 'type' && cell.type.codeHash.toLowerCase() === TYPE_ID_CODE_HASH;
  return (
    <div
      data-testid="not-spent-yet"
      className="text-small flex max-w-[640px] flex-col gap-1.5 border border-dashed px-3 py-2.5"
      style={{ borderColor: BOARD_INKS.quiet }}
    >
      <div className="flex flex-wrap justify-between gap-2">
        <span className="text-text-bright">{replaced ? 'Not replaced yet' : 'Not spent yet'}</span>
        {cell.liveAgeMs != null && (
          // its age and the block it is as of: values, the reading ink (R3; P1 follow-up)
          <span className="text-text">
            {formatDurationMs(cell.liveAgeMs)}
            {cell.liveAgeAtBlock != null && (
              <>
                {' '}
                · as of <BlockLink look="inline" number={cell.liveAgeAtBlock} />
              </>
            )}
          </span>
        )}
      </div>
      <div className="text-text">
        {replaced
          ? 'The transaction that replaces or destroys it will run:'
          : 'The transaction that spends it will run:'}
      </div>
      <div className="grid gap-x-2.5 gap-y-1 sm:grid-cols-[76px_minmax(0,1fr)]">
        {(cell.currentSpendRequirements ?? []).map((requirement) => (
          <div key={requirement.kind} className="contents">
            <span style={{ color: CELL_COLORS[requirement.kind] }}>{requirement.kind} group</span>
            <span className="min-w-0">
              <Link
                look="inline"
                href={`/script/${requirement.referenceHash}?hashType=${requirement.hashType}&kind=${requirement.kind}`}
                title={`${requirement.hashType} ${requirement.referenceHash}`}
              >
                {(requirement.kind === 'lock' ? lockLabel : typeName) ?? (
                  <ShortId value={requirement.referenceHash} kind="hash" size="row" />
                )}
              </Link>
              <span className="text-text">
                {' '}
                ·{' '}
                {requirement.resolution === 'builtin'
                  ? 'Built-in Type ID; no code-cell dep'
                  : requirement.resolution === 'not_found'
                    ? 'No current code cell resolved'
                    : requirement.resolution === 'unavailable'
                      ? 'No live code cell for this reference'
                      : requirement.resolution === 'ambiguous'
                        ? 'Ambiguous current code cells'
                        : 'code'}
              </span>
              {requirement.candidateCells.map((candidate) => (
                <span key={`${candidate.txHash}:${candidate.outputIndex}`}>
                  {' '}
                  <Link
                    href={`/cell/${candidate.txHash}-${candidate.outputIndex}`}
                    look="inline"
                    title={`Candidate cell ${candidate.txHash}:${candidate.outputIndex} · version ${candidate.versionHash}`}
                  >
                    <ShortId value={candidate.txHash} kind="hash" size="row" />
                    <OutpointIndex index={candidate.outputIndex} />
                  </Link>
                </span>
              ))}
            </span>
          </div>
        ))}
      </div>
      {/* a sentence, the reading ink at 13 px (lane P1, the ink and size roles: quiet, 12 px) */}
      <p className="text-text text-small">
        Current code cells at the indexed tip; the spender picks its own cell deps.
      </p>
    </div>
  );
}

export function CellTransactions({
  cell,
  lockLabel = null,
  typeName = null,
}: {
  cell: Cell;
  lockLabel?: string | null;
  typeName?: string | null;
}) {
  if (cell.status === 'dead' && (!cell.consumedByTx || cell.consumingInputIndex == null))
    throw new Error(`Consumed cell ${cell.txHash}:${cell.outputIndex} lacks exact spending input`);
  const spent = cell.status === 'dead';
  // this cell's out point: either transaction's page is opened at it
  const outPoint = { txHash: cell.txHash, index: cell.outputIndex };
  return (
    // Rubbing × Song print, phase 5 (D11): the transactions it shows — the one that created it,
    // and the one that spent it once it is spent (its consumedByTx, required above).
    <DetailSection title="Transactions" args={spent ? '2' : '1'}>
      <div className="space-y-8">
        <TransactionEvidence
          hash={cell.txHash}
          label="Created"
          focus={`out:${cell.outputIndex}`}
          cell={outPoint}
          onward={spent ? 'this cell, onward: spent below' : 'this cell, onward: not spent yet'}
          afterOnward={
            spent ? undefined : (
              <NotSpentYet cell={cell} lockLabel={lockLabel} typeName={typeName} />
            )
          }
        />
        {spent && (
          <TransactionEvidence
            hash={cell.consumedByTx!}
            label="Spent"
            focus={`in:${cell.consumingInputIndex}`}
            cell={outPoint}
          />
        )}
      </div>
    </DetailSection>
  );
}
