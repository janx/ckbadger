'use client';
import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { DetailSection } from '@/components/ui/detail-layout';
import { TxStructure, type TxStructureGroup } from '@/components/transaction/tx-structure';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { api, type Cell, type TransactionDetail } from '@/lib/api';
import { formatDurationMs, formatUtc8DateTime } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { CELL_COLORS } from '@/lib/design-tokens';
import { TYPE_ID_CODE_HASH, txContinuity } from '@/lib/tx-continuity';

function short(hash: string, head = 8, tail = 6): string {
  return hash.length <= head + tail + 3 ? hash : `${hash.slice(0, 2 + head)}…${hash.slice(-tail)}`;
}

export function TransactionEvidence({
  hash,
  label,
  focus,
  onward,
  afterOnward,
}: {
  hash: string;
  label: string;
  focus?: string;
  /** Drawn under the onward bridge (the dashed "Not spent yet" box). */
  afterOnward?: ReactNode;
  /** The focused output's txviz bridge label (the cell carries on past this transaction). */
  onward?: string;
}) {
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
  if (transaction.isPending || dependencies.isPending)
    return (
      <p className="text-text-dim text-xs">Loading {label.toLowerCase()} transaction evidence…</p>
    );
  if (transaction.error || dependencies.error)
    return (
      <p role="alert" className="text-rouge break-all text-xs">
        {label} transaction evidence unavailable: {String(transaction.error ?? dependencies.error)}
      </p>
    );
  const tx: TransactionDetail = transaction.data;
  if (
    tx.status !== 'committed' ||
    !tx.inputs ||
    !tx.outputs ||
    !tx.scriptGroups ||
    !tx.witnessDetails
  )
    throw new Error(`${label} transaction ${hash} lacks committed structure`);
  const tiles = cellTiles(tx, lookup.data);
  const scriptName = (codeHash: string) => {
    if (codeHash.toLowerCase() === TYPE_ID_CODE_HASH) return 'Type ID';
    const name = lookup.data?.[codeHash]?.name;
    return name && name.toLowerCase() !== 'unknown' ? name : short(codeHash);
  };
  if (focus && ![...tiles.inputs, ...tiles.outputs].some((tile) => tile.id === focus))
    throw new Error(`${label} transaction ${hash} does not contain focused cell ${focus}`);
  const groups: TxStructureGroup[] = tx.scriptGroups.map((group) => {
    const witness =
      group.witnessIndex === null
        ? 'No witness'
        : tx.witnessDetails!.find((entry) => entry.index === group.witnessIndex);
    if (group.witnessIndex !== null && !witness)
      throw new Error(
        `${hash} script group ${group.scriptHash} references missing witness ${group.witnessIndex}`
      );
    const deps = dependencies.data.filter((dep) => dep.scriptHashes.includes(group.scriptHash));
    return {
      id: `${group.kind}:${group.ran ? 'run' : 'attached'}:${group.scriptHash}`,
      kind: group.kind,
      label: scriptName(group.script.codeHash),
      href: `/script/${group.script.codeHash}?hashType=${group.script.hashType}&kind=${group.kind}`,
      cellIds: [
        ...group.inputIndices.map((index) => `in:${index}`),
        ...group.outputIndices.map((index) => `out:${index}`),
      ],
      executed: group.ran,
      witness:
        typeof witness === 'string'
          ? witness
          : `Witness #${witness!.index} · ${witness!.size} bytes`,
      // Prose wraps between words; only the outpoints may break anywhere (SC-C8).
      dependency: (
        <span className="break-normal">
          {tx.isCellbase
            ? 'Cellbase group: no script execution'
            : group.ran
              ? deps.length
                ? deps.map((dep, i) => (
                    <span key={`${dep.outPointTxHash}:${dep.outPointIndex}`}>
                      {i > 0 && ', '}
                      <span className="[overflow-wrap:anywhere]">
                        {dep.outPointTxHash}:{dep.outPointIndex}
                      </span>{' '}
                      ({dep.depType})
                    </span>
                  ))
                : group.kind === 'type' &&
                    group.script.hashType === 'type' &&
                    group.script.codeHash.toLowerCase() === TYPE_ID_CODE_HASH
                  ? 'Built-in Type ID · no cell dep'
                  : 'No matching cell dep in transaction evidence'
              : group.inputIndices.length === 0
                ? 'Output-only group: runs when these cells are spent'
                : 'Group did not execute in this transaction'}
        </span>
      ),
      cycles: group.cycles,
    };
  });
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-baseline gap-3 text-xs">
        <span className="text-text-bright">{label}</span>
        <Link href={`/tx/${hash}`} className="break-all text-[#7aa7d6]">
          {hash}
        </Link>
        {tx.timestamp && (
          <time dateTime={tx.timestamp} className="text-text-dim">
            {formatUtc8DateTime(tx.timestamp)}
          </time>
        )}
      </div>
      <TxStructure
        txHash={hash}
        inputs={tiles.inputs}
        outputs={tiles.outputs}
        fee={tx.fee}
        poolCapacity={tx.isCellbase ? undefined : (tx.inputsCapacity ?? undefined)}
        groups={groups}
        continuity={txContinuity(tx, tiles.inputs)}
        focusedCellIds={focus ? [focus] : []}
        meOut={onward !== undefined ? focus : undefined}
        onward={onward}
        afterOnward={afterOnward}
      />
    </div>
  );
}

/** The dashed "Not spent yet" box: the groups the transaction that spends this cell will run. */
function NotSpentYet({
  cell,
  lockName,
  typeName,
}: {
  cell: Cell;
  lockName: string | null;
  typeName: string | null;
}) {
  const replaced =
    cell.type?.hashType === 'type' && cell.type.codeHash.toLowerCase() === TYPE_ID_CODE_HASH;
  return (
    <div
      data-testid="not-spent-yet"
      className="flex max-w-[640px] flex-col gap-1.5 rounded-md border border-dashed border-[#4b5266] px-3 py-2.5 text-xs"
    >
      <div className="flex flex-wrap justify-between gap-2">
        <span className="text-text-bright">{replaced ? 'Not replaced yet' : 'Not spent yet'}</span>
        {cell.liveAgeMs != null && (
          <span className="text-text-dim">
            {formatDurationMs(cell.liveAgeMs)}
            {cell.liveAgeAtBlock != null && <> · as of {formatBlockNumber(cell.liveAgeAtBlock)}</>}
          </span>
        )}
      </div>
      <div className="text-text-dim">
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
                href={`/script/${requirement.referenceHash}?hashType=${requirement.hashType}&kind=${requirement.kind}`}
                className="text-text hover:text-jade"
                title={`${requirement.hashType} ${requirement.referenceHash}`}
              >
                {(requirement.kind === 'lock' ? lockName : typeName) ??
                  short(requirement.referenceHash)}
              </Link>
              <span className="text-text-dim">
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
                    className="text-[#6aa9c4]"
                    title={`Candidate cell ${candidate.txHash}:${candidate.outputIndex} · version ${candidate.versionHash}`}
                  >
                    {short(candidate.txHash)}:{candidate.outputIndex}
                  </Link>
                </span>
              ))}
            </span>
          </div>
        ))}
      </div>
      <p className="text-text-dim text-[11px]">
        Current code cells at the indexed tip; the spender picks its own cell deps.
      </p>
    </div>
  );
}

export function CellTransactions({
  cell,
  lockName = null,
  typeName = null,
}: {
  cell: Cell;
  lockName?: string | null;
  typeName?: string | null;
}) {
  if (cell.status === 'dead' && (!cell.consumedByTx || cell.consumingInputIndex == null))
    throw new Error(`Consumed cell ${cell.txHash}:${cell.outputIndex} lacks exact spending input`);
  const spent = cell.status === 'dead';
  return (
    <DetailSection title="Its transactions">
      <div className="space-y-8">
        <TransactionEvidence
          hash={cell.txHash}
          label="Created"
          focus={`out:${cell.outputIndex}`}
          onward={spent ? 'this cell, onward: spent below' : 'this cell, onward: not spent yet'}
          afterOnward={
            spent ? undefined : <NotSpentYet cell={cell} lockName={lockName} typeName={typeName} />
          }
        />
        {spent && (
          <TransactionEvidence
            hash={cell.consumedByTx!}
            label="Spent"
            focus={`in:${cell.consumingInputIndex}`}
          />
        )}
      </div>
    </DetailSection>
  );
}
