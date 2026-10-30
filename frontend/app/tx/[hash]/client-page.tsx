'use client';
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useParams, usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import Link from '@/components/ui/link';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import { HexMap } from '@/components/ui/hex-map';
import { Hash } from '@/components/ui/hash';
import { ActivityRow } from '@/components/activity/activity-row';
import { ExactCkb, ItemDeltaFact } from '@/components/activity-event-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { TxStructure, type TxStructureGroup } from '@/components/transaction/tx-structure';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { DotCellRecordsTable } from '@/components/identity/dotcell-records-table';
import { poolStatusLabel } from '@/components/ui/pool-status';
import { useCyclesCalculation } from '@/hooks/useCyclesCalculation';
import {
  api,
  isNotFoundApiError,
  isServiceUnavailableError,
  type TransactionDetail,
  type TransactionScriptGroup,
  type TransactionWitness,
  type ScriptLookupResponse,
  type GlobalActivity,
  type CellDep,
} from '@/lib/api';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { getScriptDetailHref, getIdentityItemDetailHref } from '@/lib/detail-routes';
import { cn, formatTimeAgo, truncateHash, formatUtc8DateTime } from '@/lib/utils';
import dynamic from '@/lib/dynamic-client';
import { formatBlockNumber } from '@/lib/format-block';

const DeferredCellGraph = dynamic(() => import('@/components/cell-graph'), {
  loading: () => <p className="text-text-dim text-xs">Loading graph section...</p>,
});
const POLL_MS = 3000;
function provisional(tx: TransactionDetail | undefined): boolean {
  return (
    !!tx &&
    (tx.status === 'pending' ||
      tx.status === 'proposed' ||
      tx.poolStatus === 'committed_awaiting_index')
  );
}
function required<T>(value: T | null | undefined, hash: string, field: string): T {
  if (value === null || value === undefined)
    throw new Error(`Transaction ${hash} missing required field "${field}"`);
  return value;
}
function groupKey(group: TransactionScriptGroup): string {
  return `${group.kind}:${group.ran ? 'run' : 'attached'}:${group.scriptHash}`;
}
function indexParam(value: string | null): number | null {
  if (value === null) return null;
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)))
    throw new Error(`Invalid witness index ${value}`);
  return Number(value);
}
function PendingValue() {
  return <span className="text-text-dim">pending...</span>;
}
function ScriptName({
  group,
  lookup,
}: {
  group: TransactionScriptGroup;
  lookup: ScriptLookupResponse | undefined;
}) {
  const info = lookup?.[group.script.codeHash];
  const name = info?.name;
  return (
    <Link
      href={getScriptDetailHref({
        name,
        codeHash: group.script.codeHash,
        hashType: group.script.hashType,
        scriptKind: group.kind,
      })}
      className="hover:text-jade"
    >
      {name && name.toLowerCase() !== 'unknown' ? name : truncateHash(group.script.codeHash, 10, 6)}
    </Link>
  );
}
function WitnessInspector({
  tx,
  witness,
  onSelect,
  onHover,
}: {
  tx: TransactionDetail;
  witness: TransactionWitness | null;
  onSelect: (index: number) => void;
  onHover: (index: number | null) => void;
}) {
  const details = required(tx.witnessDetails, tx.hash, 'witnessDetails');
  if (!tx.witnessesAvailable)
    return (
      <p data-testid="tx-witness-tab" className="text-text-dim text-xs">
        Witness bytes are unavailable
      </p>
    );
  return (
    <div data-testid="tx-witness-tab" className="space-y-4">
      <div className="divide-base-elevated divide-y">
        {details.map((w) => (
          <button
            key={w.index}
            type="button"
            data-testid={`tx-witness-item-${w.index}`}
            aria-pressed={witness?.index === w.index}
            onClick={() => onSelect(w.index)}
            onMouseEnter={() => onHover(w.index)}
            onMouseLeave={() => onHover(null)}
            onFocus={() => onHover(w.index)}
            onBlur={() => onHover(null)}
            className={cn(
              'flex w-full flex-wrap justify-between gap-2 py-3 text-left text-xs',
              witness?.index === w.index ? 'text-text-bright' : 'text-text'
            )}
          >
            <span>
              Witness #{w.index}{' '}
              <span className="text-text-dim">{w.kind.replaceAll('_', ' ')}</span>
              {tx.dotcellNames?.some((name) => name.outputIndex === w.index) && (
                <span className="text-text-dim ml-2">.cell records</span>
              )}
            </span>
            <span className="text-text-dim">{w.size.toLocaleString('en-US')} B</span>
          </button>
        ))}
      </div>
      {witness ? (
        <div data-testid="tx-witness-deterministic-section" className="space-y-4">
          <h3 className="text-text-bright text-xs">
            {witness.kind === 'witness_args'
              ? 'WitnessArgs'
              : witness.kind === 'opaque'
                ? 'Opaque witness bytes'
                : witness.kind.replaceAll('_', ' ')}
          </h3>
          <HexMap
            subject={`transaction ${tx.hash} witness ${witness.index}`}
            hex={required(tx.witnesses[witness.index], tx.hash, `witness ${witness.index} bytes`)}
            fields={witness.fields
              .filter((field) => field.end > field.start)
              .map((field, i) => ({
                id: `${witness.index}:${i}`,
                label: field.name,
                offset: field.start,
                length: field.end - field.start,
                description: `${field.name}: ${field.hex}`,
              }))}
          />
          {tx.dotcellNames
            ?.filter((name) => name.outputIndex === witness.index)
            .map((name) => (
              <div key={name.identityId} data-testid="tx-witness-dotcell-records">
                <p className="text-text-dim mb-3 text-xs">
                  records payload of{' '}
                  <Link
                    href={getIdentityItemDetailHref('dotcell', name.label)}
                    className="text-text hover:text-jade"
                  >
                    {name.name}
                  </Link>{' '}
                  (output #{name.outputIndex})
                </p>
                <DotCellRecordsTable records={name.records} />
              </div>
            ))}
        </div>
      ) : (
        <p data-testid="tx-witness-selection-empty" className="text-text-dim text-xs">
          Select a witness to inspect its bytes.
        </p>
      )}
    </div>
  );
}
function BalanceChanges({ activity }: { activity: GlobalActivity }) {
  const rows = activity.participants.map((participant, i) => ({
    ...participant,
    key: `${participant.lockHash ?? participant.lockHashPrefix ?? participant.address}:${i}`,
  }));
  return (
    <DetailSection title="Balance changes">
      <ListTable
        label="Balance changes"
        rows={rows}
        rowKey={(row) => row.key}
        columns={[
          {
            key: 'participant',
            label: 'Participant',
            minWidth: 220,
            render: (row) => <ParticipantRefView participant={row} compact quiet />,
          },
          {
            key: 'ckb',
            label: 'CKB',
            minWidth: 210,
            numeric: true,
            render: (row) => <ExactCkb shannons={BigInt(row.ckbDelta)} />,
          },
          {
            key: 'occupied',
            label: 'Occupied',
            minWidth: 140,
            numeric: true,
            render: (row) => <ExactCkb shannons={BigInt(row.usedDelta)} />,
          },
          {
            key: 'items',
            label: 'Items',
            minWidth: 230,
            render: (row) => (
              <div className="flex flex-wrap gap-2">
                {row.itemDeltas.map((item, i) => (
                  <ItemDeltaFact key={i} item={item} />
                ))}
              </div>
            ),
          },
        ]}
      />
    </DetailSection>
  );
}
function Dependencies({
  deps,
  activeHashes,
  onHover,
}: {
  deps: CellDep[];
  activeHashes: string[];
  onHover: (hashes: string[] | null) => void;
}) {
  return (
    <div className="divide-base-elevated divide-y">
      {deps.map((dep, i) => (
        <div
          key={`${dep.outPointTxHash}:${dep.outPointIndex}`}
          id={`tx-dep-${i}`}
          onMouseEnter={() => onHover(dep.scriptHashes)}
          onMouseLeave={() => onHover(null)}
          onFocus={() => onHover(dep.scriptHashes)}
          onBlur={() => onHover(null)}
          className={cn(
            'flex flex-wrap justify-between gap-3 py-3 text-xs',
            activeHashes.some((h) => dep.scriptHashes.includes(h)) && 'bg-base-surface'
          )}
        >
          <Link
            href={`/cell/${dep.outPointTxHash}-${dep.outPointIndex}`}
            className="text-text hover:text-jade"
          >
            dep {i} · {truncateHash(dep.outPointTxHash, 10, 6)}:{dep.outPointIndex}
          </Link>
          <span className="text-text-dim">{dep.depType} · read, not consumed</span>
          {dep.depType === 'dep_group' && (
            <div className="w-full space-y-2 pl-3">
              {dep.resolvedCells.map((cell) => (
                <Link
                  key={`${cell.txHash}:${cell.index}`}
                  href={`/cell/${cell.txHash}-${cell.index}`}
                  className="text-text-dim hover:text-jade block"
                >
                  ↳ {truncateHash(cell.txHash, 10, 6)}:{cell.index}
                </Link>
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
export default function TransactionDetailPage() {
  const { hash } = useParams() as { hash: string };
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const router = useRouter();
  const {
    data: tx,
    error,
    isLoading,
  } = useQuery<TransactionDetail>({
    queryKey: ['transaction', hash],
    queryFn: () => api.getTransactionDetail(hash),
    refetchInterval: (q) =>
      provisional(q.state.data) || isServiceUnavailableError(q.state.error) ? POLL_MS : false,
    refetchIntervalInBackground: true,
  });
  const isProvisional = provisional(tx);
  const awaitingInputs = isServiceUnavailableError(error);
  const { data: activity, error: activityError } = useQuery({
    queryKey: ['transaction-activity', hash],
    queryFn: () => api.getTransactionActivity(hash),
    enabled: !!tx && !isProvisional && !tx.isCellbase,
  });
  const { data: deps, error: depsError } = useQuery({
    queryKey: ['txCellDeps', hash],
    queryFn: () => api.getTransactionCellDeps(hash),
    enabled: !!tx && !isProvisional,
  });
  const { data: lifecycle } = useQuery({
    queryKey: ['txLifecycle', hash],
    queryFn: () => api.getTransactionLifecycle(hash),
    enabled: !!tx && !isProvisional && !tx.isCellbase,
  });
  const [graphOpen, setGraphOpen] = useState(searchParams.get('tab') === 'graph');
  const { data: graph, error: graphError } = useQuery({
    queryKey: ['txGraph', hash],
    queryFn: () => api.getTransactionGraph(hash),
    enabled: !!tx && !isProvisional && graphOpen,
  });
  const codeHashes = useMemo(
    () =>
      Array.from(
        new Set(
          [...(tx?.inputs ?? []), ...(tx?.outputs ?? [])]
            .flatMap((c) => [c.lock?.codeHash, c.type?.codeHash])
            .filter((v): v is string => v !== undefined)
        )
      ),
    [tx]
  );
  const { data: lookup } = useQuery({
    queryKey: ['scriptLookup', codeHashes, hash],
    queryFn: () => api.lookupScripts(codeHashes, hash),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });
  const { cycles, hasCycles, isCalculating, hasFailed, isPaused } = useCyclesCalculation(
    hash,
    tx?.cycles,
    (tx?.isCellbase ?? false) || isProvisional || tx?.blockNumber === 0,
    tx?.cyclesStatus,
    !isProvisional && tx?.scriptGroups?.some((group) => group.ran && group.cycles === null) === true
  );
  const [selectedWitness, setSelectedWitness] = useState<number | null>(() =>
    indexParam(searchParams.get('witness'))
  );
  const [hoveredCells, setHoveredCells] = useState<string[] | null>(null);
  const [hoveredCell, setHoveredCell] = useState<string | null>(null);
  const [selectedGroup, setSelectedGroup] = useState<string | null>(searchParams.get('wg'));
  useEffect(() => {
    setSelectedWitness(indexParam(searchParams.get('witness')));
    setSelectedGroup(searchParams.get('wg'));
  }, [hash, searchParams]);
  const select = (index: number | null, group: string | null) => {
    setSelectedWitness(index);
    setSelectedGroup(group);
    const next = new URLSearchParams(searchParams.toString());
    if (index === null) next.delete('witness');
    else next.set('witness', String(index));
    if (group === null) next.delete('wg');
    else next.set('wg', group);
    router.replace(next.size ? `${pathname}?${next}` : pathname, { scroll: false });
  };
  const shell = (children: ReactNode) => (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">{children}</main>
    </div>
  );
  if (isLoading) return shell(<div className="bg-base-surface h-72 animate-pulse" />);
  if (awaitingInputs && !tx)
    return shell(
      <DetailLayout
        title="Transaction"
        identity={hash}
        status={<span className="text-text-dim text-xs">Pending</span>}
      >
        <div data-testid="tx-inputs-not-yet-resolvable" className="text-text-dim space-y-3 text-xs">
          <p>
            waiting for the node... the inputs of this unconfirmed transaction cannot be resolved
            yet. Retrying automatically.
          </p>
          <p className="break-all">{error.apiMessage}</p>
        </div>
      </DetailLayout>
    );
  if (error && !tx && isNotFoundApiError(error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'transaction',
          identifier: hash,
          checkedEndpoint: `/transactions/${encodeURIComponent(hash)}/detail`,
        }}
      />
    );
  }
  if (!tx) {
    const message = error instanceof Error ? error.message : 'Transaction response is empty';
    return shell(
      <DetailLayout title="Failed to load transaction">
        <p role="alert" className="text-text-dim break-words text-xs">
          {message}
        </p>
      </DetailLayout>
    );
  }
  const status =
    tx.poolStatus === 'committed_awaiting_index'
      ? poolStatusLabel(tx.poolStatus)
      : tx.status === 'pending'
        ? 'Pending'
        : tx.status === 'proposed'
          ? 'Proposed'
          : 'Committed';
  const groups = isProvisional ? [] : required(tx.scriptGroups, hash, 'scriptGroups');
  const witnesses = isProvisional ? [] : required(tx.witnessDetails, hash, 'witnessDetails');
  const chosenWitness =
    selectedWitness === null || isProvisional
      ? null
      : required(
          witnesses.find((w) => w.index === selectedWitness),
          hash,
          `witness ${selectedWitness}`
        );
  if (
    !isProvisional &&
    selectedGroup !== null &&
    !groups.some((g) => groupKey(g) === selectedGroup)
  )
    throw new Error(`Transaction ${hash}: unknown script group ${selectedGroup}`);
  const tiles = isProvisional ? null : cellTiles(tx, lookup);
  const activeIds = groups
    .filter((g) =>
      selectedGroup !== null
        ? groupKey(g) === selectedGroup
        : selectedWitness !== null && g.witnessIndex === selectedWitness
    )
    .flatMap((g) => [
      ...g.inputIndices.map((i) => `in:${i}`),
      ...g.outputIndices.map((i) => `out:${i}`),
    ]);
  const evidenceCells = hoveredCells ?? (hoveredCell ? [hoveredCell] : activeIds);
  const activeHashes = groups
    .filter((g) =>
      [...g.inputIndices.map((i) => `in:${i}`), ...g.outputIndices.map((i) => `out:${i}`)].some(
        (id) => evidenceCells.includes(id)
      )
    )
    .map((g) => g.scriptHash);
  const groupCells = (g: TransactionScriptGroup) => [
    ...g.inputIndices.map((i) => `in:${i}`),
    ...g.outputIndices.map((i) => `out:${i}`),
  ];
  const structureGroups: TxStructureGroup[] = groups.map((g) => {
    const key = groupKey(g);
    const info = lookup?.[g.script.codeHash];
    return {
      id: key,
      kind: g.kind,
      label:
        info?.name && info.name.toLowerCase() !== 'unknown'
          ? info.name
          : truncateHash(g.script.codeHash, 10, 6),
      href: getScriptDetailHref({
        name: info?.name,
        codeHash: g.script.codeHash,
        hashType: g.script.hashType,
        scriptKind: g.kind,
      }),
      cellIds: [...g.inputIndices.map((i) => `in:${i}`), ...g.outputIndices.map((i) => `out:${i}`)],
      executed: g.ran,
      witness:
        g.witnessIndex === null ? (
          <span className="text-text-dim">No witness</span>
        ) : (
          <button
            type="button"
            className="hover:text-jade"
            onClick={() => select(g.witnessIndex, selectedGroup === key ? null : key)}
          >
            Witness #{g.witnessIndex}
          </button>
        ),
      dependency: (
        <span className="text-text-dim">
          {!g.ran
            ? 'Runs when spent'
            : deps
              ? deps.some((d) => d.scriptHashes.includes(g.scriptHash))
                ? deps.flatMap((d, i) =>
                    d.scriptHashes.includes(g.scriptHash)
                      ? [
                          <a key={i} href={`#tx-dep-${i}`} className="hover:text-jade mr-2">
                            dep {i}
                          </a>,
                        ]
                      : []
                  )
                : 'No matching dep'
              : 'Resolving code...'}
        </span>
      ),
      cycles: g.cycles,
    };
  });
  const raw = (
    <div className="space-y-5">
      <details>
        <summary className="text-text-dim cursor-pointer">Cells</summary>
        <pre className="mt-3 whitespace-pre-wrap break-all">
          {JSON.stringify({ inputs: tx.inputs, outputs: tx.outputs }, null, 2)}
        </pre>
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">Scripts</summary>
        <div className="mt-3 space-y-3">
          {groups.map((g) => (
            <div key={groupKey(g)}>
              <ScriptName group={g} lookup={lookup} />
              <pre className="mt-2 whitespace-pre-wrap break-all">
                {JSON.stringify(g.script, null, 2)}
              </pre>
            </div>
          ))}
        </div>
      </details>
      <details open={graphOpen} onToggle={(e) => setGraphOpen(e.currentTarget.open)}>
        <summary className="text-text-dim cursor-pointer">Cell graph</summary>
        {isProvisional ? (
          <PendingValue />
        ) : graphError ? (
          <p role="alert">{graphError.message}</p>
        ) : graph ? (
          <DeferredCellGraph
            nodes={graph.nodes}
            links={graph.links}
            height={340}
            onNodeClick={(node) => {
              if (node.nodeType === 'transaction' && node.data?.hash)
                router.push(`/tx/${node.data.hash}`);
              if (
                node.nodeType === 'cell' &&
                node.data?.txHash !== undefined &&
                node.data.outputIndex !== undefined
              )
                router.push(`/cell/${node.data.txHash}-${node.data.outputIndex}`);
            }}
          />
        ) : (
          <p className="text-text-dim py-4">Loading graph...</p>
        )}
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">JSON</summary>
        <pre className="mt-3 whitespace-pre-wrap break-all">{JSON.stringify(tx, null, 2)}</pre>
      </details>
    </div>
  );
  return shell(
    <DetailLayout
      title="Transaction"
      identity={<Hash hash={tx.hash} truncate={false} />}
      status={
        <span className={cn('text-xs', tx.status === 'proposed' ? 'text-gold' : 'text-text-dim')}>
          {status}
          {!isProvisional && (
            <>
              {' '}
              · {required(tx.confirmations, hash, 'confirmations').toLocaleString('en-US')}{' '}
              Confirmations
            </>
          )}
        </span>
      }
      tier2={
        <>
          <DetailSection
            title="Script groups"
            actions={
              <span className="text-text-dim text-[11px]">
                {groups.filter((g) => g.ran).length} ran
              </span>
            }
          >
            {isProvisional ? (
              <PendingValue />
            ) : (
              <div className="space-y-3">
                <p className="text-text-dim text-xs">
                  Cells with the same script form a group, and each group runs once.
                </p>
                <div className="text-text-dim border-base-elevated hidden grid-cols-[minmax(0,1fr)_minmax(0,1fr)_160px_180px] gap-4 border-b py-2 text-[11px] lg:grid">
                  <span>Script group</span>
                  <span>Cells</span>
                  <span>Witness it reads</span>
                  <span>Code, from</span>
                </div>
                {groups.map((g, i) => (
                  <div
                    key={groupKey(g)}
                    onMouseEnter={() => setHoveredCells(groupCells(g))}
                    onMouseLeave={() => setHoveredCells(null)}
                    onFocus={() => setHoveredCells(groupCells(g))}
                    onBlur={() => setHoveredCells(null)}
                    data-script-group={groupKey(g)}
                    data-highlighted={groupCells(g).some((id) => evidenceCells.includes(id))}
                    className={cn(
                      'border-base-elevated grid gap-3 border-b py-3 text-xs sm:grid-cols-2 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_160px_180px]',
                      groupCells(g).some((id) => evidenceCells.includes(id)) && 'bg-base-surface'
                    )}
                  >
                    <div>
                      <span className="text-text-dim mr-2">{g.kind}</span>
                      <ScriptName group={g} lookup={lookup} />
                      <span className="text-text-dim mt-1 block">
                        {g.ran
                          ? g.cycles === null
                            ? 'runs here · cycles not available'
                            : `${g.cycles} cycles`
                          : 'attached, runs when spent'}
                      </span>
                    </div>
                    <button
                      type="button"
                      data-testid={`tx-script-group-focus-${g.witnessIndex ?? 'none'}-${g.kind}`}
                      className="text-text hover:text-jade text-left"
                      onClick={() =>
                        selectedGroup === groupKey(g)
                          ? select(null, null)
                          : select(g.witnessIndex, groupKey(g))
                      }
                    >
                      {g.inputIndices
                        .map((i) => `input ${i}`)
                        .concat(g.outputIndices.map((i) => `output ${i}`))
                        .join(', ')}
                    </button>
                    <div>{structureGroups[i].witness}</div>
                    <div>{structureGroups[i].dependency}</div>
                  </div>
                ))}
              </div>
            )}
          </DetailSection>
          <DetailSection
            title={`Witnesses${tx.witnessesAvailable ? ` · ${witnesses.length}` : ' · unavailable'}`}
          >
            {isProvisional ? (
              <PendingValue />
            ) : (
              <>
                <WitnessInspector
                  tx={tx}
                  witness={chosenWitness}
                  onHover={(index) =>
                    setHoveredCells(
                      index === null
                        ? null
                        : groups.filter((g) => g.witnessIndex === index).flatMap(groupCells)
                    )
                  }
                  onSelect={(index) =>
                    selectedWitness === index ? select(null, null) : select(index, null)
                  }
                />
                {selectedGroup && (
                  <p
                    data-testid="tx-witness-focused-group"
                    className="text-text-dim mt-3 break-all text-xs"
                  >
                    Focused script group: {selectedGroup}{' '}
                    <button
                      type="button"
                      className="text-text hover:text-jade"
                      onClick={() => select(selectedWitness, null)}
                    >
                      Clear focus
                    </button>
                  </p>
                )}
                {selectedWitness !== null && (
                  <button
                    type="button"
                    data-testid="tx-witness-clear-selection"
                    className="text-text-dim hover:text-jade mt-3 text-xs"
                    onClick={() => select(null, null)}
                  >
                    Clear witness selection
                  </button>
                )}
              </>
            )}
          </DetailSection>
          <DetailSection
            title="Cell deps"
            actions={
              deps && (
                <span className="text-text-dim text-[11px]">
                  {deps.length} · read, not consumed
                </span>
              )
            }
          >
            {isProvisional ? (
              <PendingValue />
            ) : depsError ? (
              <p role="alert" className="text-xs">
                {depsError.message}
              </p>
            ) : deps ? (
              <Dependencies
                deps={deps}
                activeHashes={activeHashes}
                onHover={(hashes) =>
                  setHoveredCells(
                    hashes === null
                      ? null
                      : groups.filter((g) => hashes.includes(g.scriptHash)).flatMap(groupCells)
                  )
                }
              />
            ) : (
              <p className="text-text-dim text-xs">Resolving cell deps...</p>
            )}
          </DetailSection>
          <dl className="grid gap-5 text-xs sm:grid-cols-2">
            <div>
              <dt className="text-text-dim">Proposed, then committed</dt>
              <dd className="mt-2">
                {lifecycle?.proposedIn && (
                  <>
                    <Link href={`/blocks/${lifecycle.proposedIn.blockNumber}`}>
                      {formatBlockNumber(lifecycle.proposedIn.blockNumber)}
                    </Link>{' '}
                    →{' '}
                  </>
                )}
                {tx.blockNumber !== undefined ? (
                  <Link href={`/blocks/${tx.blockNumber}`}>
                    {formatBlockNumber(tx.blockNumber)}
                  </Link>
                ) : (
                  <PendingValue />
                )}
                {tx.index !== undefined && lifecycle?.commitmentWindow && (
                  <span className="text-text-dim">
                    {' '}
                    · position {tx.index}
                    {lifecycle.commitmentDistance !== null &&
                      lifecycle.commitmentDistance !== undefined && (
                        <> · committed {lifecycle.commitmentDistance} blocks later</>
                      )}{' '}
                    · window {lifecycle.commitmentWindow.close}–{lifecycle.commitmentWindow.far}
                  </span>
                )}
              </dd>
            </div>
            <div>
              <dt className="text-text-dim">Size · cycles</dt>
              <dd className="mt-2">
                {tx.txSize === null ? <PendingValue /> : <>{tx.txSize.toLocaleString('en-US')} B</>}{' '}
                ·{' '}
                {hasCycles && cycles !== null ? (
                  cycles.toLocaleString('en-US')
                ) : isPaused ? null : isCalculating ? (
                  'Calculating...'
                ) : hasFailed ? (
                  'Calculation failed'
                ) : isProvisional ? (
                  <PendingValue />
                ) : tx.isCellbase ? (
                  'Cellbase does not execute scripts'
                ) : (
                  'Not available'
                )}
                {isPaused && (
                  <span className={hasCycles ? 'text-text-dim mt-1 block' : 'text-text-dim'}>
                    Paused: indexer is on a deep fork
                  </span>
                )}
                {tx.feeRate !== null && (
                  <span className="text-text-dim mt-1 block">
                    fee rate{' '}
                    {exactUnsignedInteger(tx.feeRate, `${hash}: fee rate`).toLocaleString('en-US')}{' '}
                    shannon/KB
                  </span>
                )}
              </dd>
            </div>
            <div className="sm:col-span-2">
              <dt className="text-text-dim">Capacity</dt>
              <dd className="mt-2 flex flex-wrap items-baseline gap-2">
                {isProvisional ? (
                  <PendingValue />
                ) : (
                  <>
                    <ExactCkb
                      shannons={exactUnsignedInteger(
                        required(tx.inputsCapacity, hash, 'inputsCapacity'),
                        `${hash}: inputs capacity`
                      )}
                      signed={false}
                      tone="plain"
                    />{' '}
                    in →{' '}
                    <ExactCkb
                      shannons={exactUnsignedInteger(
                        required(tx.outputsCapacity, hash, 'outputsCapacity'),
                        `${hash}: outputs capacity`
                      )}
                      signed={false}
                      tone="plain"
                    />{' '}
                    out
                  </>
                )}
              </dd>
              {!isProvisional && (
                <dd className="text-text-dim mt-1">
                  occupied{' '}
                  {exactUnsignedInteger(
                    required(tx.inputsCommonKnowledgeSize, hash, 'inputsCommonKnowledgeSize'),
                    `${hash}: input occupied bytes`
                  ).toLocaleString('en-US')}{' '}
                  →{' '}
                  {exactUnsignedInteger(
                    required(tx.outputsCommonKnowledgeSize, hash, 'outputsCommonKnowledgeSize'),
                    `${hash}: output occupied bytes`
                  ).toLocaleString('en-US')}{' '}
                  bytes
                </dd>
              )}
            </div>
          </dl>
        </>
      }
      rawData={raw}
      rawOpen={graphOpen}
    >
      {error && (
        <p role="alert">
          Unable to refresh transaction: {error.message}. Showing previously loaded data.
        </p>
      )}
      {isProvisional ? (
        <p className="text-text-dim text-xs">
          Activity and canonical cell structure are available once this transaction is committed and
          indexed.
        </p>
      ) : tx.isCellbase ? (
        <DetailSection title="Mining reward">
          <p className="text-text text-xs">Cellbase carries this block’s mining reward outputs.</p>
        </DetailSection>
      ) : activityError ? (
        <p role="alert" className="text-xs">
          Cannot read activity for {hash}: {activityError.message}
        </p>
      ) : activity === undefined ? (
        <div className="bg-base-surface h-24 animate-pulse" />
      ) : activity === null ? (
        <p role="alert" className="text-xs">
          Transaction {hash} has no indexed activity.
        </p>
      ) : (
        <>
          <ActivityRow
            activity={activity}
            showParticipants={false}
            showEvidence={false}
            showScriptDetails={false}
          />
          <BalanceChanges activity={activity} />
        </>
      )}
      {tiles && (
        <DetailSection
          title={`${tiles.inputs.length} cells consumed → ${tiles.outputs.length} cells created`}
        >
          <p className="text-text-dim mb-4 text-xs">
            A transaction spends whole cells and creates new ones.
          </p>
          <TxStructure
            txHash={hash}
            inputs={tiles.inputs}
            outputs={tiles.outputs}
            fee={tx.fee}
            groups={structureGroups}
            hideGroups
            focusedCellIds={hoveredCells ?? activeIds}
            onCellHover={setHoveredCell}
            poolCapacity={
              tx.isCellbase ? undefined : required(tx.inputsCapacity, hash, 'inputsCapacity')
            }
            continuity={groups.flatMap((g) => {
              if (g.kind !== 'type' || g.inputIndices.length !== 1 || g.outputIndices.length !== 1)
                return [];
              const inputIndex = g.inputIndices[0];
              const outputIndex = g.outputIndices[0];
              const kind = tiles.inputs[inputIndex].kind;
              const inputDisplay = tx.inputs![inputIndex].display;
              const outputDisplay = tx.outputs![outputIndex].display;
              if (
                inputDisplay &&
                outputDisplay &&
                (inputDisplay.kind !== outputDisplay.kind ||
                  inputDisplay.standard !== outputDisplay.standard ||
                  inputDisplay.id !== outputDisplay.id)
              )
                return [];
              const file = inputDisplay?.kind === 'file' && outputDisplay?.kind === 'file';
              if (!file && !['objects', 'identities'].includes(kind)) return [];
              return [
                {
                  from: `in:${inputIndex}`,
                  to: `out:${outputIndex}`,
                  label: `same ${file ? 'file cell' : kind === 'objects' ? 'object' : 'identity'} continues`,
                  kind: file ? ('file' as const) : kind,
                },
              ];
            })}
            onClearFocus={() => select(null, null)}
          />
        </DetailSection>
      )}
      <dl className="grid gap-x-8 gap-y-4 text-xs sm:grid-cols-2 lg:grid-cols-4">
        <div>
          <dt className="text-text-dim">State</dt>
          <dd className="text-text-bright mt-1">{status}</dd>
        </div>
        <div>
          <dt className="text-text-dim">Time</dt>
          <dd className="text-text-bright mt-1">
            {isProvisional ? (
              <PendingValue />
            ) : (
              <time dateTime={new Date(required(tx.timestamp, hash, 'timestamp')).toISOString()}>
                {formatUtc8DateTime(required(tx.timestamp, hash, 'timestamp'))}
                <span className="text-text-dim">
                  {' '}
                  · {formatTimeAgo(required(tx.timestamp, hash, 'timestamp'))}
                </span>
              </time>
            )}
          </dd>
        </div>
        <div>
          <dt className="text-text-dim">Block</dt>
          <dd className="text-text-bright mt-1">
            {tx.blockNumber !== undefined ? (
              <Link href={`/blocks/${tx.blockNumber}`}>{formatBlockNumber(tx.blockNumber)}</Link>
            ) : (
              <PendingValue />
            )}
          </dd>
        </div>
        <div>
          <dt className="text-text-dim">Fee</dt>
          <dd className="mt-1">
            <ExactCkb
              shannons={exactUnsignedInteger(tx.fee, `${hash}: fee`)}
              signed={false}
              tone="plain"
            />
          </dd>
        </div>
        {isProvisional && tx.pendingSince && (
          <div>
            <dt className="text-text-dim">Pending Since</dt>
            <dd className="mt-1">
              {formatUtc8DateTime(tx.pendingSince)} · {formatTimeAgo(tx.pendingSince)}
            </dd>
          </div>
        )}
      </dl>
    </DetailLayout>
  );
}
