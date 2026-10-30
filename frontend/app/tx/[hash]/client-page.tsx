'use client';
import { Fragment, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useParams, usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import Link from '@/components/ui/link';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { HexMap } from '@/components/ui/hex-map';
import { ClassMark } from '@/components/ui/class-mark';
import {
  ActivityReadError,
  DotJoined,
  ExactCkb,
  ItemDeltaFact,
  activityClassMark,
  activityHeadline,
  formatStandard,
  protocolActionNoteParts,
} from '@/components/activity-event-row';
import { formatOccupiedDelta } from '@/components/activity/activity-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { TxStructure, type TxContinuity } from '@/components/transaction/tx-structure';
import { cellTiles, kindOf } from '@/components/transaction/transaction-cell-tiles';
import { ScriptGroupMatrix, type MatrixGroup } from '@/components/transaction/script-group-matrix';
import { ProposalRuler } from '@/components/transaction/proposal-ruler';
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
  type ItemDelta,
  type ParticipantInfo,
} from '@/lib/api';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import {
  getScriptDetailHref,
  getIdentityItemDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { cn, truncateHash, formatUtc8DateTime } from '@/lib/utils';
import { formatAgeTwoUnits } from '@/lib/format-age';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
import { CLASS_COLORS } from '@/lib/design-tokens';
import {
  balanceNet,
  capacityEquation,
  shortCkb,
  txLinks,
  txMovements,
  type TxHover,
  type TxLinkSets,
} from '@/lib/tx-detail-model';
import dynamic from '@/lib/dynamic-client';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';

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
function groupCells(g: TransactionScriptGroup): string[] {
  return [...g.inputIndices.map((i) => `in:${i}`), ...g.outputIndices.map((i) => `out:${i}`)];
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
/** The registry's name for a code hash, or null when it has none. */
function registeredName(lookup: ScriptLookupResponse | undefined, codeHash: string) {
  const name = lookup?.[codeHash]?.name;
  return name && name.toLowerCase() !== 'unknown' ? name : null;
}
function groupLabel(g: TransactionScriptGroup, lookup: ScriptLookupResponse | undefined) {
  return registeredName(lookup, g.script.codeHash) ?? truncateHash(g.script.codeHash, 10, 6);
}
function groupHref(g: TransactionScriptGroup, lookup: ScriptLookupResponse | undefined) {
  return getScriptDetailHref({
    name: lookup?.[g.script.codeHash]?.name,
    codeHash: g.script.codeHash,
    hashType: g.script.hashType,
    scriptKind: g.kind,
  });
}
/** Total DAO compensation the transaction's completed withdrawals pay, from their metadata. */
function daoCompensation(activity: GlobalActivity): bigint | null {
  const completes = activity.protocolActions.filter(
    (pa) => pa.protocol === 'dao' && pa.action === 'withdraw_complete'
  );
  if (completes.length === 0) return null;
  return completes.reduce((sum, pa) => sum + protocolActionMeta(pa).compensation!, BigInt(0));
}
/** A part the page cannot read is said in place, with the transaction it belongs to. */
function readable(txHash: string, render: () => ReactNode): ReactNode {
  try {
    return render();
  } catch (error) {
    return <ActivityReadError txHash={txHash} error={error} />;
  }
}

function CopyHash({ hash }: { hash: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      aria-label="Copy transaction hash"
      title={copied ? 'Copied' : 'Copy transaction hash'}
      onClick={async () => {
        await navigator.clipboard.writeText(hash);
        setCopied(true);
      }}
      className="text-text-dim hover:text-jade -my-3 flex h-11 w-11 shrink-0 items-center justify-center"
    >
      <svg
        aria-hidden="true"
        width="14"
        height="14"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.3"
      >
        <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
        <path d="M3.5 10.5h-1v-8h8v1" />
      </svg>
    </button>
  );
}

/** `A`, `A and B`, `A, B and 3 more`: the parties on one side of a movement. */
function Parties({ parties }: { parties: readonly ParticipantInfo[] }) {
  const shown = parties.slice(0, 2);
  return (
    <>
      {shown.map((p, i) => (
        <Fragment key={i}>
          {i > 0 && (parties.length > 2 ? ', ' : ' and ')}
          <ParticipantRefView participant={p} compact quiet />
        </Fragment>
      ))}
      {parties.length > 2 && ` and ${parties.length - 2} more`}
    </>
  );
}

/** An item as the summary names it: its class mark and a link (`Nervape 3x 0x51c7...96f2`). */
function ItemName({ item }: { item: ItemDelta }) {
  switch (item.kind) {
    case 'object':
      return (
        <>
          <ClassMark kind="objects" size={12} className="mr-1.5" />
          <Link
            href={getObjectDetailHref(item.objectId)}
            className="text-text-bright hover:text-jade"
          >
            {item.collection?.name ? `${item.collection.name} ` : 'Object '}
            {truncateHash(item.objectId, 6, 4)}
          </Link>
        </>
      );
    case 'token':
      return (
        <>
          <ClassMark kind="tokens" size={12} className="mr-1.5" />
          <Link
            href={getTokenDetailHref(item.typeScriptHash)}
            className="text-text-bright hover:text-jade"
          >
            {item.symbol?.trim() || truncateHash(item.typeScriptHash, 8, 6)}
          </Link>
        </>
      );
    case 'identity':
      return (
        <>
          <ClassMark kind="identities" size={12} className="mr-1.5" />
          <Link
            href={getIdentityItemDetailHref(item.standard, item.identityId)}
            className="text-text-bright hover:text-jade"
          >
            {formatStandard(item.standard)} {truncateHash(item.identityId, 6, 4)}
          </Link>
        </>
      );
  }
}

/**
 * The plain summary under the headline (TX-11): what moved from whom to whom, read off the
 * participants' signed changes, or the protocol action's own facts; then the fee.
 */
function TxSummary({ activity, fee }: { activity: GlobalActivity; fee: string }) {
  const moves = txMovements(activity);
  const feeShannons = exactUnsignedInteger(fee, `${activity.txHash}: fee`);
  return (
    <p data-testid="tx-summary" className="text-text max-w-[80ch] text-[15px]">
      {moves.map((m, i) => (
        <Fragment key={i}>
          {m.item ? <ItemName item={m.item} /> : 'CKB'}
          {m.from.length > 0 && m.to.length > 0 ? (
            <>
              {' moved from '}
              <Parties parties={m.from} />
              {' to '}
              <Parties parties={m.to} />
            </>
          ) : m.to.length > 0 ? (
            <>
              {' arrived at '}
              <Parties parties={m.to} />
            </>
          ) : (
            <>
              {' left '}
              <Parties parties={m.from} />
            </>
          )}
          {'. '}
        </Fragment>
      ))}
      {moves.length === 0 &&
        activity.protocolActions.map((pa, i) => (
          <Fragment key={i}>
            {readable(activity.txHash, () => {
              const parts = protocolActionNoteParts(pa);
              return parts.length ? (
                <>
                  <DotJoined parts={parts} />
                  {'. '}
                </>
              ) : null;
            })}
          </Fragment>
        ))}
      {feeShannons > BigInt(0) && (
        <>
          The transaction paid a <ExactCkb shannons={feeShannons} signed={false} tone="plain" />{' '}
          fee.
        </>
      )}
    </p>
  );
}

const BALANCE_GRID = 'grid grid-cols-[190px_180px_100px_minmax(0,1fr)] gap-4';

/** Balance changes (TX-12, TX-19): every participant, then the Net row that explains the sum. */
function BalanceChanges({
  activity,
  fee,
  compensation,
  genesis,
}: {
  activity: GlobalActivity;
  fee: string;
  compensation: bigint | null;
  genesis: boolean;
}) {
  return (
    <DetailSection title="Balance changes">
      <div className="overflow-x-auto">
        <div role="table" aria-label="Balance changes by participant" className="min-w-[700px]">
          <div
            role="row"
            className={cn(
              BALANCE_GRID,
              'border-base-border-subtle text-text-dim border-b pb-1.5 text-[11px]'
            )}
          >
            <span role="columnheader">Participant</span>
            <span role="columnheader" className="text-right">
              CKB
            </span>
            <span role="columnheader" className="text-right">
              Occupied
            </span>
            <span role="columnheader">Items</span>
          </div>
          {activity.participants.map((p, i) => (
            <div
              key={`${p.lockHash ?? p.lockHashPrefix ?? p.address}:${i}`}
              role="row"
              className={cn(BALANCE_GRID, 'border-base-elevated items-baseline border-b py-2')}
            >
              <span role="cell" className="min-w-0">
                <ParticipantRefView participant={p} compact quiet />
                {p.lockKind && <span className="text-text-dim ml-2 text-[11px]">{p.lockKind}</span>}
              </span>
              <span role="cell" className="text-right">
                <ExactCkb shannons={BigInt(p.ckbDelta)} />
              </span>
              <span role="cell" className="text-right tabular-nums">
                {formatOccupiedDelta(p.usedDelta)}
              </span>
              <span role="cell" className="flex min-w-0 flex-wrap gap-x-3.5 gap-y-0.5">
                {p.itemDeltas.map((item, j) => (
                  <ItemDeltaFact key={j} item={item} />
                ))}
              </span>
            </div>
          ))}
          <div
            role="row"
            data-testid="tx-balance-net"
            className={cn(BALANCE_GRID, 'text-text-dim items-baseline py-2')}
          >
            {readable(activity.txHash, () => {
              const net = balanceNet({
                hash: activity.txHash,
                participants: activity.participants,
                fee,
                compensation,
                genesis,
              });
              return (
                <>
                  <span role="cell">Net</span>
                  <span role="cell" className="text-right">
                    <ExactCkb shannons={net.ckb} tone="plain" />
                  </span>
                  <span role="cell" className="text-right tabular-nums">
                    {formatOccupiedDelta(net.occupied.toString())}
                  </span>
                  <span role="cell">
                    {net.note === 'fee'
                      ? 'the fee, paid to the miner'
                      : net.note === 'compensation'
                        ? 'Nervos DAO compensation, less the fee'
                        : 'issued in the genesis block'}
                  </span>
                </>
              );
            })}
          </div>
        </div>
      </div>
    </DetailSection>
  );
}

function witnessKindLabel(kind: TransactionWitness['kind']): string {
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

/** The printable start of a cellbase message (the client version) and how many bytes follow. */
function cellbaseMessage(hex: string): { text: string; rest: number } {
  const body = hex.startsWith('0x') ? hex.slice(2) : hex;
  if (body.length % 2 !== 0 || /[^0-9a-f]/i.test(body))
    throw new Error(`cellbase message ${hex} is not hex`);
  const bytes = body.match(/../g)?.map((b) => parseInt(b, 16)) ?? [];
  const end = bytes.findIndex((b) => b < 0x20 || b > 0x7e);
  const printable = end === -1 ? bytes : bytes.slice(0, end);
  return { text: String.fromCharCode(...printable), rest: bytes.length - printable.length };
}

const OVERHEAD_FIELD = /header|length|union|offset/i;

/**
 * One witness row (Main.dc.html Witnesses): kind and size, which groups read it, the bytes as a
 * bar per field, and what the API decoded (CoBuild actions, the cellbase miner lock and message).
 */
function WitnessRow({
  tx,
  w,
  readers,
  scale,
  selected,
  lit,
  lookup,
  groupNameByHash,
  onSelect,
  onHover,
}: {
  tx: TransactionDetail;
  w: TransactionWitness;
  readers: string[];
  scale: number;
  selected: boolean;
  lit: TxLinkSets | null;
  lookup: ScriptLookupResponse | undefined;
  groupNameByHash: Map<string, string>;
  onSelect: () => void;
  onHover: (hover: TxHover | null) => void;
}) {
  const on = lit?.witnesses.has(w.index) ?? false;
  const fields = w.fields.filter((f) => f.end > f.start);
  const note = (() => {
    if (w.kind === 'cobuild' && w.actions && w.actions.length > 0) {
      const names = w.actions.map(
        (a) => groupNameByHash.get(a.scriptHash) ?? truncateHash(a.scriptHash, 8, 4)
      );
      return `message: ${formatCount(w.actions.length, 'action')} for ${[...new Set(names)].join(', ')}`;
    }
    if (w.kind === 'cellbase') {
      const witness = required(tx.cellbaseWitness, tx.hash, 'cellbaseWitness');
      const message = cellbaseMessage(witness.message);
      const lock =
        registeredName(lookup, witness.lock.codeHash) ?? truncateHash(witness.lock.codeHash, 10, 6);
      return `miner lock ${lock}(${truncateHash(witness.lock.args, 6, 4)}) · message “${message.text}”${message.rest ? ` + ${formatCount(message.rest, 'byte')}` : ''}`;
    }
    return fields.map((f) => `${f.name} ${formatCount(f.end - f.start, 'B', 'B')}`).join(' · ');
  })();
  return (
    <button
      type="button"
      data-testid={`tx-witness-item-${w.index}`}
      data-lit={on}
      aria-pressed={selected}
      onClick={onSelect}
      onMouseEnter={() => onHover({ t: 'witness', index: w.index })}
      onMouseLeave={() => onHover(null)}
      onFocus={() => onHover({ t: 'witness', index: w.index })}
      onBlur={() => onHover(null)}
      className={cn(
        'border-base-elevated -mx-1.5 flex w-[calc(100%+12px)] flex-col gap-1 rounded border-b px-1.5 py-2.5 text-left text-xs transition-opacity',
        on && 'bg-base-surface',
        lit !== null && !on && 'opacity-25'
      )}
    >
      <span className="flex w-full flex-wrap justify-between gap-3">
        <span className={selected ? 'text-jade' : 'text-text-bright'}>
          #{w.index} · {witnessKindLabel(w.kind)}
          {tx.dotcellNames?.some((name) => name.outputIndex === w.index) && (
            <span className="text-text-dim ml-2">.cell records</span>
          )}
        </span>
        <span className="text-text-dim">
          {formatCount(w.size, 'B', 'B')}
          {readers.length > 0 && ` · read by ${readers.join(', ')}`}
        </span>
      </span>
      {fields.length > 0 && (
        <span aria-hidden="true" className="flex h-2 gap-0.5">
          {fields.map((f, i) => (
            <span
              key={i}
              className={cn('rounded-sm', OVERHEAD_FIELD.test(f.name) ? '' : 'bg-jade')}
              style={{
                width: Math.max(2, Math.round(((f.end - f.start) / scale) * 320)),
                backgroundColor: OVERHEAD_FIELD.test(f.name) ? CLASS_COLORS.ckb : undefined,
              }}
            />
          ))}
        </span>
      )}
      {note && <span className="text-text-dim [overflow-wrap:anywhere]">{note}</span>}
    </button>
  );
}

function WitnessInspector({ tx, witness }: { tx: TransactionDetail; witness: TransactionWitness }) {
  return (
    <div data-testid="tx-witness-deterministic-section" className="mt-4 space-y-4">
      <h3 className="text-text-bright text-xs">
        {witness.kind === 'witness_args'
          ? 'WitnessArgs'
          : witness.kind === 'opaque'
            ? 'Opaque witness bytes'
            : witnessKindLabel(witness.kind)}
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
  );
}

/** One cell dep (TX-15): its out point, what it supplies code for, and its members. */
function DepRow({
  dep,
  index,
  lit,
  groupNameByHash,
  onHover,
}: {
  dep: CellDep;
  index: number;
  lit: TxLinkSets | null;
  groupNameByHash: Map<string, string>;
  onHover: (hover: TxHover | null) => void;
}) {
  const on = lit?.deps.has(index) ?? false;
  const names = (hashes: readonly string[]) => [
    ...new Set(hashes.flatMap((h) => (groupNameByHash.has(h) ? [groupNameByHash.get(h)!] : []))),
  ];
  const serves = names(dep.scriptHashes);
  return (
    <div
      id={`tx-dep-${index}`}
      data-lit={on}
      tabIndex={0}
      onMouseEnter={() => onHover({ t: 'dep', index })}
      onMouseLeave={() => onHover(null)}
      onFocus={() => onHover({ t: 'dep', index })}
      onBlur={() => onHover(null)}
      className={cn(
        'border-base-elevated -mx-1.5 grid grid-cols-[minmax(0,1fr)_auto] gap-x-3 gap-y-0.5 rounded border-b px-1.5 py-2.5 text-xs transition-opacity',
        on && 'bg-base-surface',
        lit !== null && !on && 'opacity-25'
      )}
    >
      <Link
        href={`/cell/${dep.outPointTxHash}-${dep.outPointIndex}`}
        className="text-info/80 hover:text-jade min-w-0 [overflow-wrap:anywhere]"
      >
        {truncateHash(dep.outPointTxHash, 10, 6)}:{dep.outPointIndex}
      </Link>
      <span className="text-text-dim">{dep.depType === 'dep_group' ? 'dep group' : 'code'}</span>
      <span className="text-text-dim">
        {serves.length > 0 ? `supplies code for ${serves.join(', ')}` : 'no script here runs it'}
      </span>
      <span className={serves.length > 0 ? 'text-text' : 'text-text-dim'}>
        {serves.length > 0 ? `for ${serves.join(', ')}` : 'not run here'}
      </span>
      {dep.depType === 'dep_group' && (
        <div className="col-span-2 space-y-1 pl-3 pt-1">
          {dep.resolvedCells.map((cell) => {
            const memberServes = names(cell.scriptHashes);
            return (
              <span key={`${cell.txHash}:${cell.index}`} className="text-text-dim block">
                <Link href={`/cell/${cell.txHash}-${cell.index}`} className="hover:text-jade">
                  ↳ {truncateHash(cell.txHash, 10, 6)}:{cell.index}
                </Link>
                {memberServes.length > 0
                  ? ` · code for ${memberServes.join(', ')}`
                  : ' · no script here names it'}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}

function EvidenceHeading({ title, note }: { title: string; note: ReactNode }) {
  return (
    <div className="border-base-border-subtle flex flex-wrap items-baseline justify-between gap-2 border-b pb-1.5">
      <h2 className="text-[13px] font-medium text-[#a0a8be]">{title}</h2>
      <span className="text-text-dim text-xs">{note}</span>
    </div>
  );
}

const CONTINUITY_STROKE = { file: '#a0a8be', objects: CLASS_COLORS.objects } as const;

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
            .concat(tx?.cellbaseWitness ? [tx.cellbaseWitness.lock.codeHash] : [])
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
  const [hover, setHover] = useState<TxHover | null>(null);
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
  // The API serves script groups and witnesses for an uncommitted transaction too (TX-37).
  const groups = required(tx.scriptGroups, hash, 'scriptGroups');
  const witnesses = required(tx.witnessDetails, hash, 'witnessDetails');
  const headerDeps = required(tx.headerDeps, hash, 'headerDeps');
  const chosenWitness =
    selectedWitness === null
      ? null
      : required(
          witnesses.find((w) => w.index === selectedWitness),
          hash,
          `witness ${selectedWitness}`
        );
  if (selectedGroup !== null && !groups.some((g) => groupKey(g) === selectedGroup))
    throw new Error(`Transaction ${hash}: unknown script group ${selectedGroup}`);
  const tiles = isProvisional ? null : cellTiles(tx, lookup);
  const genesis = tx.blockNumber === 0;
  const compensation = activity ? daoCompensation(activity) : undefined;

  // ---- hover linking by group identity (TX-14) ----
  const linkGroups = groups.map((g) => ({
    id: groupKey(g),
    cellIds: groupCells(g),
    witnessIndex: g.witnessIndex,
    scriptHash: g.scriptHash,
  }));
  const linkDeps = (deps ?? []).map((d, index) => ({ index, scriptHashes: d.scriptHashes }));
  const focus: TxHover | null =
    hover ??
    (selectedGroup !== null
      ? { t: 'group', id: selectedGroup }
      : selectedWitness !== null
        ? { t: 'witness', index: selectedWitness }
        : null);
  const lit = focus ? txLinks(focus, linkGroups, linkDeps, hash) : null;
  const groupNameByHash = new Map(groups.map((g) => [g.scriptHash, groupLabel(g, lookup)]));
  const readersOf = (index: number) => [
    ...new Set(groups.filter((g) => g.witnessIndex === index).map((g) => groupLabel(g, lookup))),
  ];
  const witnessScale = Math.max(1, ...witnesses.map((w) => w.size));
  const ran = groups.filter((g) => g.ran).length;

  // ---- continuity (same object / same file cell continues) ----
  const continuity: TxContinuity[] = tiles
    ? groups.flatMap((g) => {
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
      })
    : [];
  const legend = [...new Set(continuity.map((l) => l.label))].map((label) => ({
    label,
    links: continuity.filter((l) => l.label === label),
  }));

  // ---- script-group matrix ----
  const column = (
    side: 'in' | 'out',
    i: number,
    cell: NonNullable<TransactionDetail['inputs']>[number]
  ) => {
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
            ? (registeredName(lookup, cell.type.codeHash) ?? 'typed')
            : 'plain';
    return {
      id: `${side}:${i}`,
      top,
      label: `${side} #${i}`,
      cap:
        cell.capacity === undefined
          ? null
          : shortCkb(exactUnsignedInteger(cell.capacity, `${hash}: ${side} ${i} capacity`)),
    };
  };
  const matrixInputs = tx.isCellbase ? [] : (tx.inputs ?? []).map((c, i) => column('in', i, c));
  const matrixOutputs = (tx.outputs ?? []).map((c, i) => column('out', i, c));
  const matrixGroups: MatrixGroup[] = groups.map((g) => {
    const key = groupKey(g);
    const codeDeps = (deps ?? []).flatMap((d, i) =>
      d.scriptHashes.includes(g.scriptHash) ? [{ d, i }] : []
    );
    const w = g.witnessIndex === null ? null : witnesses.find((x) => x.index === g.witnessIndex);
    const firstCell = g.inputIndices.length
      ? tx.inputs?.[g.inputIndices[0]]
      : tx.outputs?.[g.outputIndices[0]];
    const mark = g.kind === 'type' && firstCell ? kindOf(firstCell) : null;
    return {
      id: key,
      kind: g.kind,
      name: groupLabel(g, lookup),
      href: groupHref(g, lookup),
      mark: mark && mark !== 'ckb' && mark !== 'other' ? mark : null,
      sub: `args ${truncateHash(g.script.args, 6, 4)}`,
      ran: g.ran,
      cellIds: groupCells(g),
      cycles: g.cycles,
      status: isProvisional && g.ran ? 'will run when committed' : undefined,
      focusTestId: `tx-script-group-focus-${g.witnessIndex ?? 'none'}-${g.kind}`,
      onFocus: () => (selectedGroup === key ? select(null, null) : select(g.witnessIndex, key)),
      witness:
        g.witnessIndex === null ? (
          <span className="text-text-dim">—</span>
        ) : (
          <button
            type="button"
            className="hover:text-jade text-left"
            onClick={() => select(g.witnessIndex, selectedGroup === key ? null : key)}
          >
            #{g.witnessIndex}
            {w && (
              <span className="text-text-dim">
                {' '}
                · {witnessKindLabel(w.kind)}, {formatCount(w.size, 'B', 'B')}
              </span>
            )}
          </button>
        ),
      code: !g.ran ? (
        <span className="text-text-dim">—</span>
      ) : isProvisional ? (
        <span className="text-text-dim">read once committed</span>
      ) : depsError ? (
        <span className="text-text-dim">cell deps unreadable</span>
      ) : !deps ? (
        <span className="text-text-dim">Resolving code...</span>
      ) : codeDeps.length === 0 ? (
        <span className="text-text-dim">No matching dep</span>
      ) : (
        codeDeps.map(({ d, i }) => (
          <a key={i} href={`#tx-dep-${i}`} className="hover:text-jade block">
            {d.depType === 'dep_group' ? 'dep group' : 'code'}{' '}
            <span className="text-text-dim">
              {truncateHash(d.outPointTxHash, 10, 6)}:{d.outPointIndex}
            </span>
          </a>
        ))
      ),
    };
  });

  // ---- headline and summary (TX-11) ----
  const headline = isProvisional
    ? tx.poolStatus === 'committed_awaiting_index'
      ? 'Committed, being indexed'
      : 'Waiting in the pool'
    : tx.isCellbase
      ? 'Cellbase'
      : activity
        ? activityHeadline(activity)
        : 'Transaction';
  const mark = activity ? activityClassMark(activity) : null;
  const summary = isProvisional ? (
    <p className="text-text-dim text-xs">
      Activity and canonical cell structure are available once this transaction is committed and
      indexed.
    </p>
  ) : tx.isCellbase ? (
    <p data-testid="tx-summary" className="text-text max-w-[80ch] text-[15px]">
      The first transaction of block{' '}
      {formatBlockNumber(required(tx.blockNumber, hash, 'blockNumber'))}. It consumes no cell and
      creates the miner&rsquo;s reward,{' '}
      <ExactCkb
        shannons={exactUnsignedInteger(
          required(tx.outputsCapacity, hash, 'outputsCapacity'),
          `${hash}: outputs capacity`
        )}
        signed={false}
        tone="plain"
      />
      .
    </p>
  ) : activityError ? (
    <p role="alert" className="text-xs">
      Cannot read activity for {hash}: {activityError.message}
    </p>
  ) : activity === undefined ? (
    <div className="bg-base-surface h-5 w-96 max-w-full animate-pulse" />
  ) : activity === null ? (
    <p role="alert" className="text-xs">
      Transaction {hash} has no indexed activity.
    </p>
  ) : (
    readable(hash, () => <TxSummary activity={activity} fee={tx.fee} />)
  );

  // ---- capacity (TX-19, TX-33) ----
  const capacityFact = isProvisional ? (
    <PendingValue />
  ) : tx.isCellbase ? (
    <>
      block reward{' '}
      <ExactCkb
        shannons={exactUnsignedInteger(
          required(tx.outputsCapacity, hash, 'outputsCapacity'),
          `${hash}: outputs capacity`
        )}
        signed={false}
        tone="plain"
      />{' '}
      = out
    </>
  ) : compensation === undefined ? (
    activityError ? (
      <span className="text-text-dim">unreadable without the transaction&rsquo;s activity</span>
    ) : (
      <span className="text-text-dim">Reading...</span>
    )
  ) : (
    readable(hash, () => {
      const eq = capacityEquation({
        hash,
        inputs: required(tx.inputsCapacity, hash, 'inputsCapacity'),
        outputs: required(tx.outputsCapacity, hash, 'outputsCapacity'),
        fee: tx.fee,
        compensation,
      });
      return (
        <span data-testid="tx-capacity-equation">
          <ExactCkb shannons={eq.inputs} signed={false} tone="plain" /> in
          {eq.compensation !== null && (
            <>
              {' + '}
              <ExactCkb shannons={eq.compensation} signed={false} tone="plain" /> compensation
            </>
          )}
          {' = '}
          <ExactCkb shannons={eq.outputs} signed={false} tone="plain" /> out +{' '}
          <ExactCkb shannons={eq.fee} signed={false} tone="plain" /> fee
        </span>
      );
    })
  );
  const occupiedIn = isProvisional
    ? null
    : exactUnsignedInteger(
        required(tx.inputsCommonKnowledgeSize, hash, 'inputsCommonKnowledgeSize'),
        `${hash}: input occupied bytes`
      );
  const occupiedOut = isProvisional
    ? null
    : exactUnsignedInteger(
        required(tx.outputsCommonKnowledgeSize, hash, 'outputsCommonKnowledgeSize'),
        `${hash}: output occupied bytes`
      );
  const knowledgeDelta =
    occupiedIn !== null && occupiedOut !== null ? occupiedOut - occupiedIn : null;

  const inflow =
    tiles && tx.isCellbase
      ? {
          tone: 'reward' as const,
          label: 'block reward',
          amount: required(tx.outputsCapacity, hash, 'outputsCapacity'),
        }
      : compensation
        ? {
            tone: 'dao' as const,
            label: 'Nervos DAO compensation',
            amount: compensation.toString(),
          }
        : undefined;

  const raw = (
    <div className="space-y-5">
      <p>
        <Link
          href={`/tx/${hash}.md`}
          target="_blank"
          rel="noreferrer"
          className="text-text-dim hover:text-jade"
        >
          Markdown
        </Link>
      </p>
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
              <Link href={groupHref(g, lookup)} className="hover:text-jade">
                {groupLabel(g, lookup)}
              </Link>
              <pre className="mt-2 whitespace-pre-wrap break-all">
                {JSON.stringify(g.script, null, 2)}
              </pre>
            </div>
          ))}
        </div>
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">
          Witness bytes · {tx.witnessesAvailable ? tx.witnesses.length : 'unavailable'}
        </summary>
        <div className="mt-3 space-y-2">
          {tx.witnesses.map((w, i) => (
            <p key={i} className="break-all">
              <span className="text-text-dim">#{i} </span>
              {w}
            </p>
          ))}
        </div>
      </details>
      <details>
        <summary className="text-text-dim cursor-pointer">
          Header deps · {headerDeps.length}
        </summary>
        <div className="mt-3 space-y-2">
          {headerDeps.map((h) => (
            <p key={h.blockHash} className="break-all">
              {h.blockHash}
            </p>
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
    <div className="min-w-0 font-mono text-[13px]">
      {error && (
        <p role="alert" className="mb-4 text-xs">
          Unable to refresh transaction: {error.message}. Showing previously loaded data.
        </p>
      )}
      {/* Tier 1: what happened, to whom, for how much, and which cells changed hands. */}
      <div data-tier="1" className="flex flex-wrap items-start gap-x-14 gap-y-8">
        <div className="flex min-w-0 flex-[999_1_640px] flex-col gap-8">
          <header className="flex flex-col gap-2">
            <span className="text-text-dim text-xs">Transaction</span>
            <h1 className="text-text-bright break-words text-[26px] font-medium">
              {mark && <ClassMark kind={mark} className="mr-2.5 align-[-1px]" />}
              {headline}
            </h1>
            {summary}
            <div className="flex min-w-0 items-center gap-0.5 text-xs">
              <span className="text-text-dim min-w-0 [overflow-wrap:anywhere]">{tx.hash}</span>
              <CopyHash hash={tx.hash} />
            </div>
          </header>
          {activity && (
            <BalanceChanges
              activity={activity}
              fee={tx.fee}
              compensation={compensation ?? null}
              genesis={genesis}
            />
          )}
          {tiles && (
            <DetailSection
              title={`${formatCount(tiles.inputs.length, 'cell')} consumed → ${formatCount(tiles.outputs.length, 'cell')} created`}
              actions={
                legend.length > 0 && (
                  <span className="text-text-dim flex flex-wrap items-center gap-x-3.5 gap-y-1 text-[11.5px]">
                    {legend.map((l) => (
                      <button
                        key={l.label}
                        type="button"
                        onMouseEnter={() =>
                          setHover({ t: 'cells', ids: l.links.flatMap((x) => [x.from, x.to]) })
                        }
                        onMouseLeave={() => setHover(null)}
                        onFocus={() =>
                          setHover({ t: 'cells', ids: l.links.flatMap((x) => [x.from, x.to]) })
                        }
                        onBlur={() => setHover(null)}
                        className="hover:text-text flex items-center gap-1.5"
                      >
                        <svg aria-hidden="true" width="22" height="6" className="shrink-0">
                          <path
                            d="M0 3h22"
                            stroke={
                              l.links[0].kind === 'file'
                                ? CONTINUITY_STROKE.file
                                : l.links[0].kind
                                  ? CLASS_COLORS[l.links[0].kind]
                                  : CONTINUITY_STROKE.objects
                            }
                            strokeWidth="1.3"
                            strokeDasharray="4 3"
                          />
                        </svg>
                        {l.label}
                      </button>
                    ))}
                  </span>
                )
              }
            >
              <p className="text-text-dim -mt-2 mb-4 text-xs">
                {tx.isCellbase
                  ? 'A cellbase consumes no cell: it creates the reward of a block that is now final.'
                  : compensation
                    ? 'A transaction spends whole cells and creates new ones. The Nervos DAO adds the compensation; the rest of the difference is the fee.'
                    : 'A transaction spends whole cells and creates new ones. Capacity is conserved; the difference is the fee.'}
              </p>
              <TxStructure
                txHash={hash}
                inputs={tiles.inputs}
                outputs={tiles.outputs}
                fee={tx.fee}
                hideGroups
                linking="page"
                inflow={inflow}
                continuityList={false}
                focusedCellIds={lit ? [...lit.cells] : []}
                onCellHover={(id) => setHover(id ? { t: 'cell', id } : null)}
                continuity={continuity}
                onClearFocus={() => select(null, null)}
              />
            </DetailSection>
          )}
        </div>
        <aside
          aria-label="Transaction facts"
          className="flex flex-[1_1_280px] flex-col gap-4 pt-6 sm:max-w-[340px]"
        >
          <div className="flex flex-col gap-0.5">
            <span className="text-text-bright flex items-center gap-2">
              <span
                aria-hidden="true"
                className={cn(
                  'h-2 w-2 rounded-full',
                  tx.status === 'proposed' ? 'bg-gold' : isProvisional ? 'bg-info' : 'bg-jade'
                )}
              />
              {status}
            </span>
            {!isProvisional && (
              <span className="text-text-dim text-xs">
                {formatCount(required(tx.confirmations, hash, 'confirmations'), 'confirmation')}
              </span>
            )}
          </div>
          <dl className="flex flex-col gap-4">
            <div className="flex flex-col gap-0.5">
              <dt className="text-text-dim text-xs">Time</dt>
              <dd className="text-text-bright">
                {isProvisional ? (
                  <PendingValue />
                ) : (
                  <time
                    dateTime={new Date(required(tx.timestamp, hash, 'timestamp')).toISOString()}
                  >
                    {formatUtc8DateTime(required(tx.timestamp, hash, 'timestamp'))}
                    <span className="text-text-dim block text-xs">
                      {formatAgeTwoUnits(required(tx.timestamp, hash, 'timestamp'))}
                    </span>
                  </time>
                )}
              </dd>
            </div>
            <div className="flex flex-col gap-0.5">
              <dt className="text-text-dim text-xs">Block</dt>
              <dd className="text-text-bright">
                {tx.blockNumber !== undefined ? (
                  <Link href={`/blocks/${tx.blockNumber}`}>
                    {formatBlockNumber(tx.blockNumber)}
                  </Link>
                ) : (
                  <PendingValue />
                )}
              </dd>
            </div>
            <div className="flex flex-col gap-0.5">
              <dt className="text-text-dim text-xs">Fee</dt>
              <dd className="text-[15px]">
                <ExactCkb
                  shannons={exactUnsignedInteger(tx.fee, `${hash}: fee`)}
                  signed={false}
                  tone="plain"
                />
              </dd>
            </div>
            {isProvisional && tx.pendingSince && (
              <div className="flex flex-col gap-0.5">
                <dt className="text-text-dim text-xs">Pending since</dt>
                <dd>
                  {formatUtc8DateTime(tx.pendingSince)}
                  <span className="text-text-dim block text-xs">
                    {formatAgeTwoUnits(tx.pendingSince)}
                  </span>
                </dd>
              </div>
            )}
          </dl>
        </aside>
      </div>

      {/* Tier 2, set quieter: the evidence behind tier 1, linked to it by hover. */}
      <DetailTier2 className="text-xs">
        <DetailSection
          title="Script groups"
          actions={
            <span className="text-text-dim flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px]">
              <span data-testid="tx-groups-ran">
                {isProvisional
                  ? 'will run when committed'
                  : tx.isCellbase
                    ? 'a cellbase runs no script'
                    : hasCycles && cycles !== null
                      ? `${ran} ran, ${cycles.toLocaleString('en-US')} cycles`
                      : `${ran} ran`}
              </span>
              <span className="flex items-center gap-1.5">
                <span aria-hidden="true" className="bg-jade h-[9px] w-[9px] rounded-full" />
                runs here
              </span>
              <span className="flex items-center gap-1.5">
                <span
                  aria-hidden="true"
                  className="border-text-dim h-[9px] w-[9px] rounded-full border-[1.2px]"
                />
                attached, runs when spent
              </span>
            </span>
          }
        >
          <p className="text-text-dim -mt-2 mb-3">
            Cells with the same script form a group, and each group runs once.
          </p>
          <ScriptGroupMatrix
            txHash={hash}
            inputs={matrixInputs}
            outputs={matrixOutputs}
            groups={matrixGroups}
            lit={lit}
            onHover={setHover}
          />
        </DetailSection>

        <section className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,560px),1fr))] gap-x-12 gap-y-7">
          <div className="min-w-0">
            <EvidenceHeading
              title={`Witnesses${tx.witnessesAvailable ? ` · ${witnesses.length}` : ' · unavailable'}`}
              note={
                tx.witnessesAvailable
                  ? formatCount(
                      witnesses.reduce((sum, w) => sum + w.size, 0),
                      'byte'
                    )
                  : null
              }
            />
            <div data-testid="tx-witness-tab">
              {!tx.witnessesAvailable ? (
                <p className="text-text-dim py-3">Witness bytes are unavailable</p>
              ) : (
                <>
                  {witnesses.map((w) => (
                    <WitnessRow
                      key={w.index}
                      tx={tx}
                      w={w}
                      readers={readersOf(w.index)}
                      scale={witnessScale}
                      selected={selectedWitness === w.index}
                      lit={lit}
                      lookup={lookup}
                      groupNameByHash={groupNameByHash}
                      onSelect={() =>
                        selectedWitness === w.index ? select(null, null) : select(w.index, null)
                      }
                      onHover={setHover}
                    />
                  ))}
                  {chosenWitness ? (
                    <WitnessInspector tx={tx} witness={chosenWitness} />
                  ) : (
                    <p data-testid="tx-witness-selection-empty" className="text-text-dim mt-3">
                      Select a witness to inspect its bytes.
                    </p>
                  )}
                  {selectedGroup && (
                    <p
                      data-testid="tx-witness-focused-group"
                      className="text-text-dim mt-3 break-all"
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
                      className="text-text-dim hover:text-jade mt-3"
                      onClick={() => select(null, null)}
                    >
                      Clear witness selection
                    </button>
                  )}
                </>
              )}
            </div>
          </div>
          <div className="flex min-w-0 flex-col gap-7">
            <div>
              <EvidenceHeading
                title="Cell deps"
                note={deps ? `${deps.length} · read, not consumed` : 'read, not consumed'}
              />
              {isProvisional ? (
                <p className="py-3">
                  <PendingValue />
                </p>
              ) : depsError ? (
                <p role="alert" className="py-3">
                  {depsError.message}
                </p>
              ) : deps ? (
                deps.map((dep, i) => (
                  <DepRow
                    key={`${dep.outPointTxHash}:${dep.outPointIndex}`}
                    dep={dep}
                    index={i}
                    lit={lit}
                    groupNameByHash={groupNameByHash}
                    onHover={setHover}
                  />
                ))
              ) : (
                <p className="text-text-dim py-3">Resolving cell deps...</p>
              )}
            </div>
            {headerDeps.length > 0 && (
              <div data-testid="tx-header-deps">
                <EvidenceHeading
                  title="Header deps"
                  note={`${headerDeps.length} · read, not consumed`}
                />
                {headerDeps.map((h) => (
                  <div
                    key={h.blockHash}
                    className="border-base-elevated flex flex-wrap justify-between gap-3 border-b py-2.5"
                  >
                    {h.blockNumber === null ? (
                      <span className="text-text-dim">block not indexed yet</span>
                    ) : (
                      <Link href={`/blocks/${h.blockNumber}`} className="text-text hover:text-jade">
                        {formatBlockNumber(h.blockNumber)}
                      </Link>
                    )}
                    <span className="text-text-dim">{truncateHash(h.blockHash, 10, 6)}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>

        <dl className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,260px),1fr))] gap-x-10 gap-y-5">
          <div className="flex flex-col gap-1">
            <dt className="text-text-dim">Proposed, then committed</dt>
            <dd className="flex flex-col gap-1">
              {tx.isCellbase ? (
                <span className="text-text-dim">
                  never proposed: a cellbase is its block&rsquo;s first transaction
                </span>
              ) : isProvisional ? (
                <PendingValue />
              ) : (
                <>
                  {lifecycle?.proposedIn && tx.blockNumber !== undefined && (
                    <ProposalRuler
                      proposedIn={lifecycle.proposedIn.blockNumber}
                      committedIn={tx.blockNumber}
                      window={lifecycle.commitmentWindow}
                    />
                  )}
                  <span>
                    {lifecycle?.proposedIn && (
                      <>
                        <Link href={`/blocks/${lifecycle.proposedIn.blockNumber}`}>
                          {formatBlockNumber(lifecycle.proposedIn.blockNumber)}
                        </Link>{' '}
                        →{' '}
                      </>
                    )}
                    {tx.blockNumber !== undefined && (
                      <Link href={`/blocks/${tx.blockNumber}`}>
                        {formatBlockNumber(tx.blockNumber)}
                      </Link>
                    )}
                    {tx.index !== undefined && lifecycle?.commitmentWindow && (
                      <span className="text-text-dim">
                        {' '}
                        · position {tx.index}
                        {lifecycle.commitmentDistance !== null &&
                          lifecycle.commitmentDistance !== undefined && (
                            <>
                              {' '}
                              · committed {formatCount(lifecycle.commitmentDistance, 'block')} later
                            </>
                          )}{' '}
                        · window {lifecycle.commitmentWindow.close}–{lifecycle.commitmentWindow.far}
                      </span>
                    )}
                  </span>
                </>
              )}
            </dd>
          </div>
          <div className="flex flex-col gap-0.5">
            <dt className="text-text-dim">Size · cycles</dt>
            <dd>
              {tx.txSize === null ? <PendingValue /> : <>{tx.txSize.toLocaleString('en-US')} B</>} ·{' '}
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
          <div className="flex flex-col gap-0.5 sm:col-span-2">
            <dt className="text-text-dim">Capacity</dt>
            <dd>{capacityFact}</dd>
            {knowledgeDelta !== null && (
              <dd>
                occupied {occupiedIn!.toLocaleString('en-US')} →{' '}
                {occupiedOut!.toLocaleString('en-US')}{' '}
                <span className="text-text-dim">
                  · common knowledge{' '}
                  {formatOccupiedDelta((knowledgeDelta * BigInt(100000000)).toString())}
                </span>
              </dd>
            )}
          </div>
        </dl>

        <details open={graphOpen} className="border-base-border-subtle border-t pt-4">
          <summary className="text-text-dim hover:text-text cursor-pointer text-xs">
            Raw data
          </summary>
          <div className="mt-4 min-w-0 overflow-x-auto text-xs">{raw}</div>
        </details>
      </DetailTier2>
    </div>
  );
}
