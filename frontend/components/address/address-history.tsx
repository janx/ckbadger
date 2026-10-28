'use client';

import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { ExactCkb, activityHeadline } from '@/components/activity-event-row';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import Link from '@/components/ui/link';
import { PoolStatusBadge, TimeInPool, PoolInterpretationNotice } from '@/components/ui/pool-status';
import { TransactionCellContent } from '@/components/transaction/transaction-cell-content';
import { ClassMark } from '@/components/ui/class-mark';
import { ADDRESS_CLASSES } from '@/components/address/address-holdings';
import type {
  Activity,
  Address,
  AddressTransaction,
  AddressCell,
  ParticipantInfo,
} from '@/lib/api';
import { formatTimeAgo } from '@/lib/utils';
import { classifyActivity } from '@/lib/activity-classify';
import { getScriptDetailHref } from '@/lib/detail-routes';

interface AddressActivityProps {
  activity: Activity;
  address: Address;
}
export function AddressActivity({ activity, address }: AddressActivityProps) {
  const participant: ParticipantInfo = {
    address: address.address ?? null,
    lockHash: address.lockScriptHash,
    lockHashPrefix: null,
    lockKind: address.lockScriptInfo?.name ?? null,
    ckbDelta: activity.ckbDelta,
    usedDelta: activity.usedDelta,
    itemDeltas: activity.itemDeltas,
    roles: activity.roles,
    tags: activity.tags,
  };
  // The endpoint supplies this participant's deltas only. Other parties remain references;
  // do not invent zero balances for them to construct a cross-user transaction.
  const row: ActivityRowData = { ...activity, participants: [participant] };
  const classification = classifyActivity(row);
  const primary = classification.primaryItemDelta;
  const itemClass = classification.displayType;
  const headline =
    (itemClass === 'token' || itemClass === 'object' || itemClass === 'identity') && primary
      ? `${itemClass[0].toUpperCase()}${itemClass.slice(1)} ${BigInt(primary.delta) > BigInt(0) ? 'received' : BigInt(primary.delta) < BigInt(0) ? 'sent' : 'changed'}`
      : activityHeadline(row);
  return (
    <div>
      <ActivityRow
        headline={headline}
        activity={row}
        mode="participant"
        participant={participant}
      />
      {activity.roles.length > 0 && (
        <p className="text-text-dim pb-2 text-xs">Roles: {activity.roles.join(', ')}</p>
      )}
    </div>
  );
}
function TxPosition({ tx }: { tx: AddressTransaction }) {
  return (
    <span className="flex flex-col gap-1 text-xs">
      {tx.blockNumber === null ? (
        <>
          {tx.poolStatus ? <PoolStatusBadge status={tx.poolStatus} /> : <span>Unconfirmed</span>}
          {tx.timeAddedToPool && <TimeInPool since={tx.timeAddedToPool} />}
        </>
      ) : (
        <>
          <Link href={`/blocks/${tx.blockNumber}`}>#{tx.blockNumber.toLocaleString('en-US')}</Link>
          {tx.timestamp !== null && (
            <span className="text-text-dim">{formatTimeAgo(tx.timestamp)}</span>
          )}
        </>
      )}
    </span>
  );
}
const columns: ListColumn<AddressTransaction>[] = [
  {
    key: 'hash',
    label: 'Transaction',
    minWidth: 172,
    render: (tx) => (
      <span className="space-y-1">
        <Link href={`/tx/${tx.txHash}`}>
          <Hash
            hash={tx.txHash}
            startChars={10}
            endChars={6}
            copyable={false}
            className="text-xs"
          />
        </Link>
        <PoolInterpretationNotice
          interpretation={tx.interpretation}
          className="text-warning block text-[11px]"
        />
        {tx.txType === 'named' && (
          <span className="text-text-dim block text-[11px]">Named participant</span>
        )}
      </span>
    ),
  },
  { key: 'block', label: 'Block · time', minWidth: 136, render: (tx) => <TxPosition tx={tx} /> },
  {
    key: 'cells',
    label: 'Cells in → out',
    minWidth: 100,
    render: (tx) => (
      <span>
        {tx.inputsCount} <span className="text-text-dim">→</span> {tx.outputsCount}
      </span>
    ),
  },
  {
    key: 'change',
    label: 'This address, CKB',
    minWidth: 180,
    numeric: true,
    className: 'whitespace-normal',
    render: (tx) => <ExactCkb shannons={BigInt(tx.capacityChange)} wrap />,
  },
  {
    key: 'fee',
    label: 'Fee, CKB',
    minWidth: 136,
    numeric: true,
    className: 'whitespace-normal',
    render: (tx) => <ExactCkb shannons={BigInt(tx.fee)} signed={false} tone="plain" wrap />,
  },
  {
    key: 'size',
    label: 'Size, B',
    minWidth: 90,
    numeric: true,
    render: (tx) => (tx.txSize === null ? 'Unavailable' : tx.txSize.toLocaleString('en-US')),
  },
  {
    key: 'cycles',
    label: 'Cycles',
    minWidth: 120,
    numeric: true,
    render: (tx) =>
      tx.cycles === null ? (
        <span className="text-text-dim">
          {tx.cyclesStatus === 'failed' ? 'Failed' : 'Not computed'}
        </span>
      ) : (
        tx.cycles.toLocaleString('en-US')
      ),
  },
  {
    key: 'scripts',
    label: 'Scripts',
    minWidth: 146,
    render: (tx) => (
      <span className="flex flex-wrap gap-x-2 gap-y-1">
        {tx.scriptLabels.map((name) => (
          <Link
            key={name}
            href={`/scripts/${encodeURIComponent(name)}`}
            className="text-text-dim hover:text-jade"
          >
            {name}
          </Link>
        ))}
      </span>
    ),
  },
];
export function AddressTransactions({ rows }: { rows: AddressTransaction[] }) {
  return (
    <ListTable
      label="Address transactions"
      columns={columns}
      rows={rows}
      rowKey={(tx) => tx.txHash}
      empty="No transactions."
      mobileRow={(tx) => (
        <div data-testid="address-tx-row-mobile" className="space-y-3 text-xs">
          <div className="flex flex-wrap justify-between gap-2">
            {columns[0].render(tx)}
            <TxPosition tx={tx} />
          </div>
          <dl className="space-y-2">
            {columns.slice(2).map((c) => (
              <div key={c.key} className="flex flex-wrap justify-between gap-2">
                <dt className="text-text-dim">{c.label}</dt>
                <dd className="max-w-full text-right">{c.render(tx)}</dd>
              </div>
            ))}
          </dl>
        </div>
      )}
    />
  );
}
const cellColumns: ListColumn<AddressCell>[] = [
  {
    key: 'outpoint',
    label: 'Cell',
    minWidth: 200,
    render: (c) => (
      <Link href={`/cell/${c.txHash}-${c.outputIndex}`}>
        <Hash hash={c.txHash} startChars={10} endChars={6} copyable={false} className="text-xs" />:
        {c.outputIndex}
      </Link>
    ),
  },
  {
    key: 'content',
    label: 'Contains',
    minWidth: 280,
    render: (c) =>
      c.display ? (
        <TransactionCellContent
          display={c.display}
          cellHref={`/cell/${c.txHash}-${c.outputIndex}`}
        />
      ) : (
        <span className="flex items-center gap-2">
          <ClassMark kind={ADDRESS_CLASSES.find((k) => k.key === c.class)!.mark} />
          {ADDRESS_CLASSES.find((k) => k.key === c.class)!.label}
          {c.typeScriptHash && (
            <Link
              href={getScriptDetailHref({
                codeHash: c.typeCodeHash!,
                hashType: c.typeHashType,
                scriptKind: 'type',
              })}
            >
              <Hash hash={c.typeScriptHash} className="text-xs" copyable={false} />
            </Link>
          )}
        </span>
      ),
  },
  {
    key: 'capacity',
    label: 'Capacity, CKB',
    minWidth: 196,
    numeric: true,
    className: 'whitespace-normal',
    render: (c) => <ExactCkb shannons={BigInt(c.capacity)} signed={false} tone="plain" wrap />,
  },
  {
    key: 'occupied',
    label: 'Occupied, CKB',
    minWidth: 196,
    numeric: true,
    className: 'whitespace-normal',
    render: (c) => (
      <span>
        <ExactCkb shannons={BigInt(c.occupiedCapacity)} signed={false} tone="plain" wrap />
        <span aria-hidden="true" className="bg-base-elevated mt-1 block h-1">
          <span
            className="bg-jade block h-1"
            style={{
              width:
                BigInt(c.capacity) === BigInt(0)
                  ? '0%'
                  : `${(Number(c.occupiedCapacity) / Number(c.capacity)) * 100}%`,
            }}
          />
        </span>
      </span>
    ),
  },
  {
    key: 'created',
    label: 'Created at',
    minWidth: 120,
    render: (c) => (
      <Link href={`/blocks/${c.createdAtBlock}`}>#{c.createdAtBlock.toLocaleString('en-US')}</Link>
    ),
  },
];
export function AddressLiveCells({ rows }: { rows: AddressCell[] }) {
  return (
    <ListTable
      label="Live cells"
      columns={cellColumns}
      rows={rows}
      rowKey={(c) => `${c.txHash}:${c.outputIndex}`}
      empty="No live cells match this class."
    />
  );
}
