'use client';

import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { ExactCkb, activityHeadline } from '@/components/activity-event-row';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
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
import { formatUtc8Date, truncateHash } from '@/lib/utils';
import type { AssetClass } from '@/lib/design-tokens';
import { classifyActivity } from '@/lib/activity-classify';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { formatBlockNumber } from '@/lib/format-block';

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
        itemTone="sign"
        evidence={
          activity.blockNumber === null || activity.timestamp === null ? undefined : (
            // F2 blocks (BA-35): the Address board dates a row; "ago" drifts as the page is read.
            <>
              <Link href={`/blocks/${activity.blockNumber}`} className="hover:text-text">
                {formatBlockNumber(activity.blockNumber)}
              </Link>
              {' · '}
              <Link href={`/tx/${activity.txHash}`} className="hover:text-jade text-text-dim">
                {truncateHash(activity.txHash, 10, 6)}
              </Link>
              {' · '}
              {formatUtc8Date(activity.timestamp)}
            </>
          )
        }
      />
      {activity.roles.length > 0 && (
        <p className="text-text-dim text-small pb-2">Roles: {activity.roles.join(', ')}</p>
      )}
    </div>
  );
}
function TxPosition({ tx }: { tx: AddressTransaction }) {
  return (
    <span className="text-small flex flex-col gap-1">
      {tx.blockNumber === null ? (
        <>
          {tx.poolStatus ? <PoolStatusBadge status={tx.poolStatus} /> : <span>Unconfirmed</span>}
          {tx.timeAddedToPool && <TimeInPool since={tx.timeAddedToPool} />}
        </>
      ) : (
        <>
          <Link href={`/blocks/${tx.blockNumber}`}>{formatBlockNumber(tx.blockNumber)}</Link>
          {tx.timestamp !== null && (
            <span className="text-text-dim">{formatUtc8Date(tx.timestamp)}</span>
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
            className="text-small"
          />
        </Link>
        <PoolInterpretationNotice
          interpretation={tx.interpretation}
          className="text-warning text-meta block"
        />
        {tx.txType === 'named' && (
          <span className="text-text-dim text-meta block">Named participant</span>
        )}
      </span>
    ),
  },
  { key: 'block', label: 'Block · date', minWidth: 136, render: (tx) => <TxPosition tx={tx} /> },
  {
    key: 'cells',
    label: 'Cells in → out',
    minWidth: 100,
    render: (tx) => (
      <span>
        {tx.isCellbase ? 0 : tx.inputsCount} <span className="text-text-dim">→</span>{' '}
        {tx.outputsCount}
      </span>
    ),
  },
  {
    key: 'change',
    label: 'This address, CKB',
    minWidth: 180,
    numeric: true,
    className: 'whitespace-normal',
    render: (tx) => <ExactCkb shannons={BigInt(tx.capacityChange)} wrap unit={false} />,
  },
  {
    key: 'fee',
    label: 'Fee, CKB',
    minWidth: 136,
    numeric: true,
    className: 'whitespace-normal',
    render: (tx) => (
      <ExactCkb shannons={BigInt(tx.fee)} signed={false} tone="plain" wrap unit={false} />
    ),
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
      <span className="flex flex-wrap gap-x-2.5 gap-y-1">
        {tx.scriptLabels.map((label) => {
          const script = scriptLabel(label, tx.txHash);
          return (
            <Link
              key={label}
              href={`/scripts/${encodeURIComponent(label)}`}
              className="text-text-dim hover:text-jade inline-flex items-center gap-1.5"
            >
              <ClassMark kind={script.mark} size={12} />
              {script.name}
            </Link>
          );
        })}
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
        <div data-testid="address-tx-row-mobile" className="text-small space-y-3">
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
        <Hash
          hash={c.txHash}
          startChars={10}
          endChars={6}
          copyable={false}
          className="text-small"
        />
        :{c.outputIndex}
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
              <Hash hash={c.typeScriptHash} className="text-small" copyable={false} />
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
      <Link href={`/blocks/${c.createdAtBlock}`}>{formatBlockNumber(c.createdAtBlock)}</Link>
    ),
  },
];
/** A live cell's quiet figures on a phone: "occupied 61 CKB · created #20,000,000". */
function OccupiedAndCreated({ cell: c }: { cell: AddressCell }) {
  return (
    <span className="text-text-dim text-meta">
      <Dotted
        parts={[
          <span key="o" className="whitespace-nowrap">
            occupied <ExactCkb shannons={BigInt(c.occupiedCapacity)} signed={false} tone="plain" />
          </span>,
          <span key="c" className="whitespace-nowrap">
            created{' '}
            <Link href={`/blocks/${c.createdAtBlock}`}>{formatBlockNumber(c.createdAtBlock)}</Link>
          </span>,
        ]}
      />
    </span>
  );
}

export function AddressLiveCells({ rows }: { rows: AddressCell[] }) {
  return (
    <ListTable
      label="Live cells"
      columns={cellColumns}
      rows={rows}
      rowKey={(c) => `${c.txHash}:${c.outputIndex}`}
      empty="No live cells match this class."
      mobileRow={(c, wide) => (
        <PhoneRow
          testId="address-cell-row-mobile"
          lines={[
            {
              key: 'cell',
              tone: 'head',
              left: cellColumns[0].render(c),
              right: <ExactCkb shannons={BigInt(c.capacity)} signed={false} tone="plain" />,
            },
            {
              key: 'contains',
              tone: 'body',
              left: cellColumns[1].render(c),
              right: wide ? <OccupiedAndCreated cell={c} /> : undefined,
            },
            !wide && { key: 'figures', tone: 'quiet', left: <OccupiedAndCreated cell={c} /> },
          ]}
        />
      )}
    />
  );
}

// ---- F2 blocks (BA-36): the API's fixed script-label vocabulary, as the board writes it ----

/** `scriptLabels` come from the address index's semantic-tag bitmap: a closed set of nine. */
const SCRIPT_LABELS: Record<string, { name: string; mark: AssetClass }> = {
  NervosDAO: { name: 'Nervos DAO', mark: 'dao' },
  sUDT: { name: 'sUDT', mark: 'tokens' },
  xUDT: { name: 'xUDT', mark: 'tokens' },
  mNFT: { name: 'mNFT', mark: 'objects' },
  Spore: { name: 'Spore', mark: 'objects' },
  'Spore Cluster': { name: 'Spore Cluster', mark: 'objects' },
  '.bit': { name: '.bit', mark: 'identities' },
  'did:ckb': { name: 'did:ckb', mark: 'identities' },
  '.cell': { name: '.cell', mark: 'identities' },
};

export function scriptLabel(label: string, txHash: string) {
  const known = SCRIPT_LABELS[label];
  if (!known)
    throw new Error(`Transaction ${txHash}: unknown script label ${JSON.stringify(label)}`);
  return known;
}
