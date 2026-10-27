'use client';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { DetailSection } from '@/components/ui/detail-layout';
import { TxStructure, type TxStructureGroup } from '@/components/transaction/tx-structure';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { api, type Cell, type TransactionDetail } from '@/lib/api';

const TYPE_ID_CODE_HASH = '0x00000000000000000000000000000000000000000000000000545950455f4944';

export function TransactionEvidence({
  hash,
  label,
  focus,
}: {
  hash: string;
  label: string;
  focus?: string;
}) {
  const transaction = useQuery({
    queryKey: ['cell-transaction-detail', hash],
    queryFn: () => api.getTransactionDetail(hash),
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
  const tiles = cellTiles(tx, undefined);
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
      id: `${group.kind}:${group.scriptHash}`,
      kind: group.kind,
      label: `${group.kind} · ${group.script.codeHash.slice(0, 16)}…`,
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
      dependency: tx.isCellbase
        ? 'Cellbase group: no script execution'
        : group.ran
          ? deps.length
            ? deps
                .map((dep) => `${dep.outPointTxHash}:${dep.outPointIndex} (${dep.depType})`)
                .join(', ')
            : group.kind === 'type' &&
                group.script.hashType === 'type' &&
                group.script.codeHash.toLowerCase() === TYPE_ID_CODE_HASH
              ? 'Built-in Type ID · no cell dep'
              : 'No matching cell dep in transaction evidence'
          : group.inputIndices.length === 0
            ? 'Output-only group: no execution in this transaction'
            : 'Group did not execute in this transaction',
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
            {tx.timestamp}
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
        focusedCellIds={focus ? [focus] : []}
      />
    </div>
  );
}

export function CellTransactions({ cell }: { cell: Cell }) {
  if (cell.status === 'dead' && (!cell.consumedByTx || cell.consumingInputIndex == null))
    throw new Error(`Consumed cell ${cell.txHash}:${cell.outputIndex} lacks exact spending input`);
  return (
    <DetailSection title="Its transactions">
      <div className="space-y-8">
        <TransactionEvidence hash={cell.txHash} label="Created" focus={`out:${cell.outputIndex}`} />
        {cell.status === 'dead' && cell.consumedByTx && cell.consumingInputIndex != null ? (
          <TransactionEvidence
            hash={cell.consumedByTx}
            label="Spent"
            focus={`in:${cell.consumingInputIndex}`}
          />
        ) : (
          <div className="space-y-2 text-xs">
            <p className="text-text-dim">
              Current reference candidates at the indexed tip. A future spender chooses dependencies
              and witnesses; none are historical execution for this live cell.
            </p>
            {cell.currentSpendRequirements?.map((requirement) => (
              <div
                key={requirement.kind}
                className="border-base-border grid gap-1 border-t py-2 sm:grid-cols-[3rem_1fr]"
              >
                <span className="text-text-bright capitalize">{requirement.kind}</span>
                <div className="min-w-0 break-all">
                  <Link
                    href={`/script/${requirement.referenceHash}?hashType=${requirement.hashType}&kind=${requirement.kind}`}
                    className="text-[#7aa7d6]"
                  >
                    {requirement.referenceHash}
                  </Link>
                  <span className="text-text-dim">
                    {' '}
                    · {requirement.hashType} ·{' '}
                    {requirement.resolution === 'builtin'
                      ? 'Built-in Type ID; no code-cell dep'
                      : requirement.resolution === 'not_found'
                        ? 'No current code cell resolved'
                        : requirement.resolution === 'unavailable'
                          ? 'No live code cell for this reference'
                          : requirement.resolution === 'ambiguous'
                            ? 'Ambiguous current code cells'
                            : 'Current code cell resolved'}
                  </span>
                  {requirement.versionHashes.map((hash) => (
                    <p key={hash} className="text-text-dim break-all">
                      Version {hash}
                    </p>
                  ))}
                  {requirement.candidateCells.map((candidate) => (
                    <p
                      key={`${candidate.txHash}:${candidate.outputIndex}`}
                      className="text-text-dim break-all"
                    >
                      <Link
                        href={`/cell/${candidate.txHash}-${candidate.outputIndex}`}
                        className="text-[#7aa7d6]"
                      >
                        Candidate cell {candidate.txHash}:{candidate.outputIndex}
                      </Link>{' '}
                      · live
                    </p>
                  ))}
                  {requirement.historicalCells?.map((historical) => (
                    <p
                      key={`${historical.txHash}:${historical.outputIndex}`}
                      className="text-text-dim break-all"
                    >
                      <Link
                        href={`/cell/${historical.txHash}-${historical.outputIndex}`}
                        className="text-text-dim"
                      >
                        Historical cell {historical.txHash}:{historical.outputIndex}
                      </Link>{' '}
                      · spent, not a current dependency
                    </p>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </DetailSection>
  );
}
