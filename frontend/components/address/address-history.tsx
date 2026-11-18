'use client';

import type { ReactNode } from 'react';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { activityHeadline } from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { Hash } from '@/components/ui/hash';
import Link from '@/components/ui/link';
import { PoolStatusBadge, TimeInPool, PoolInterpretationNotice } from '@/components/ui/pool-status';
import { TransactionCellContent } from '@/components/transaction/transaction-cell-content';
import { ClassMark } from '@/components/ui/class-mark';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import { ADDRESS_CLASSES } from '@/components/address/address-holdings';
import { addressCellSealId } from '@/lib/address-cell-seal';
import type {
  Activity,
  Address,
  AddressTransaction,
  AddressCell,
  ParticipantInfo,
} from '@/lib/api';

import type { AssetClass } from '@/lib/design-tokens';
import { classifyActivity } from '@/lib/activity-classify';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import type { ShortSize } from '@/lib/shorten';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { Utc8Date } from '@/components/ui/utc8-date';

interface AddressActivityProps {
  activity: Activity;
  address: Address;
}

const plural = (n: number, noun: string) => formatCount(n, noun);

/**
 * What this address's object changes were, as Address.dc.html → History heads them: "Received 9
 * objects · Nervape Special", "Sent 2 objects", "Received 2, sent 1 objects" — counted from this
 * participant's own deltas, never a transaction-wide mint or burn. The collection follows, dim, when
 * every object shares one named collection.
 */
function objectHeadline(participant: ParticipantInfo, txHash: string): ReactNode {
  const objects = participant.itemDeltas.flatMap((d) => (d.kind === 'object' ? [d] : []));
  if (objects.length === 0)
    throw new Error(`transaction ${txHash}: an object activity of this address moved no object`);
  const arrived = objects.filter((o) => o.delta > 0).length;
  const departed = objects.length - arrived;
  const movement =
    departed === 0
      ? `Received ${plural(arrived, 'object')}`
      : arrived === 0
        ? `Sent ${plural(departed, 'object')}`
        : `Received ${arrived}, sent ${plural(departed, 'object')}`;
  const names = new Set(objects.map((o) => o.collection?.name?.trim() || null));
  const [collection] = names;
  return names.size === 1 && collection ? (
    <>
      {movement}
      <span className="text-text-dim font-normal"> · {collection}</span>
    </>
  ) : (
    movement
  );
}

/**
 * One activity of this address: the site's one activity row (consistency C3), its participant line
 * this address's — first, as text, tagged "this address" (`focusLockHash`). The endpoint supplies
 * this participant's deltas only; the other parties of the transaction are on its page.
 */
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
    itemClass === 'object'
      ? objectHeadline(participant, activity.txHash)
      : (itemClass === 'token' || itemClass === 'identity') && primary
        ? `${itemClass[0].toUpperCase()}${itemClass.slice(1)} ${BigInt(primary.delta) > BigInt(0) ? 'received' : BigInt(primary.delta) < BigInt(0) ? 'sent' : 'changed'}`
        : activityHeadline(row);
  return (
    <ActivityRow
      headline={headline}
      activity={row}
      focusLockHash={address.lockScriptHash}
      foldObjects
      itemTone="sign"
    />
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
            <span className="text-text-dim">
              <Utc8Date timestamp={tx.timestamp} />
            </span>
          )}
        </>
      )}
    </span>
  );
}
/** A transaction's hash and its notes, at the form's size (U-09): row in the table, tight below. */
function TxHash({ tx, size }: { tx: AddressTransaction; size: ShortSize }) {
  return (
    <span className="space-y-1">
      <Link href={`/tx/${tx.txHash}`}>
        <Hash hash={tx.txHash} size={size} copyable={false} className="text-small" />
      </Link>
      <PoolInterpretationNotice
        interpretation={tx.interpretation}
        className="text-warning text-meta block"
      />
      {tx.txType === 'named' && (
        <span className="text-text-dim text-meta block">Named participant</span>
      )}
    </span>
  );
}

const columns: ListColumn<AddressTransaction>[] = [
  {
    key: 'hash',
    label: 'Transaction',
    minWidth: 172,
    render: (tx) => <TxHash tx={tx} size="row" />,
  },
  { key: 'block', label: 'Block · date', minWidth: 136, render: (tx) => <TxPosition tx={tx} /> },
  {
    key: 'cells',
    label: 'Cells in → out',
    minWidth: 100,
    render: (tx) => (
      <span>
        {tx.isCellbase ? 0 : tx.inputsCount} <span className="text-text-dim">→</span>{' '}
        {tx.outputsCount.toLocaleString('en-US')}
      </span>
    ),
  },
  {
    key: 'change',
    label: 'This address, CKB',
    minWidth: 180,
    numeric: true,
    className: 'whitespace-normal',
    render: (tx) => (
      <CkbAmount form="column" shannons={BigInt(tx.capacityChange)} wrap unit={false} />
    ),
  },
  {
    key: 'fee',
    label: 'Fee, CKB',
    minWidth: 136,
    numeric: true,
    className: 'whitespace-normal',
    render: (tx) => (
      <CkbAmount
        form="column"
        shannons={BigInt(tx.fee)}
        signed={false}
        tone="plain"
        wrap
        unit={false}
      />
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
            <TxHash tx={tx} size="tight" />
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
/**
 * A live cell's out point, the link to its page, at the form's size (U-09): "0xdce40476…3993e1:4"
 * in the table, "0xdce4…93e1:4" in the phone row.
 */
function CellOutPoint({ cell: c, size }: { cell: AddressCell; size: ShortSize }) {
  return (
    <Link href={`/cell/${c.txHash}-${c.outputIndex}`}>
      <Hash hash={c.txHash} size={size} copyable={false} className="text-small" />
      <OutpointIndex index={c.outputIndex} />
    </Link>
  );
}

/** What a live cell holds, its ids at the form's size (U-09). */
function CellContains({ cell: c, size }: { cell: AddressCell; size: ShortSize }) {
  return c.display ? (
    <TransactionCellContent
      display={c.display}
      cellHref={`/cell/${c.txHash}-${c.outputIndex}`}
      size={size}
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
          <Hash hash={c.typeScriptHash} size={size} className="text-small" copyable={false} />
        </Link>
      )}
    </span>
  );
}

/**
 * The live cells table's columns (12 px monospace, 7.2 px a character; 11 px, 6.6; 16 px between
 * columns, none after the last). Its minimum is 992 px, the width it changes form at. The seals
 * by kind (C4) lead the Cell column with the cell's 30 px row seal, its footprint (30 + 8 = 38 px)
 * taken from measured slack so the total and the form change stay 992:
 * - Cell 200 → 238: the out point at the row size with a four-digit index, 22 characters (158.4;
 *   it was 24 with three dots) + 16, and the seal's 38: 212.4.
 * - Occupied 196 → 178: a cell is at most a block's 597,000 bytes, so at most "597,000.00000000"
 *   (16 characters, 115.2) and " CKB" at 11 px (26.4) + 16: 157.6.
 * - Created at 120 → 100 (the last column): "#99,999,999" is 11 characters (79.2), and
 *   "#100,000,000" 12 (86.4).
 * Contains (280) and Capacity (196: every amount under a billion CKB on one line, 186.4) keep
 * theirs.
 */
const cellColumns: ListColumn<AddressCell>[] = [
  {
    key: 'outpoint',
    label: 'Cell',
    minWidth: 200 + ROW_SEAL.cell.footprint,
    // The cell's seal leads the row, overhanging the out point's 18 px line (RowSeal), so a row
    // keeps its height; the out point stays the row's link (the seal is an image, not a link).
    render: (c) => (
      <span className="flex items-start">
        <RowSeal id={addressCellSealId(c)} overhang />
        <CellOutPoint cell={c} size="row" />
      </span>
    ),
  },
  {
    key: 'content',
    label: 'Contains',
    minWidth: 280,
    render: (c) => <CellContains cell={c} size="row" />,
  },
  {
    key: 'capacity',
    label: 'Capacity, CKB',
    minWidth: 196,
    numeric: true,
    className: 'whitespace-normal',
    render: (c) => (
      <CkbAmount form="column" shannons={BigInt(c.capacity)} signed={false} tone="plain" wrap />
    ),
  },
  {
    key: 'occupied',
    label: 'Occupied, CKB',
    minWidth: 178,
    numeric: true,
    className: 'whitespace-normal',
    render: (c) => (
      <span>
        <CkbAmount
          form="column"
          shannons={BigInt(c.occupiedCapacity)}
          signed={false}
          tone="plain"
          wrap
        />
        <span aria-hidden="true" className="bg-bar-track mt-1 block h-1">
          <span
            className="bg-jade scan-fill block h-1"
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
    minWidth: 100,
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
            occupied{' '}
            <CkbAmount
              form="summary"
              shannons={BigInt(c.occupiedCapacity)}
              signed={false}
              tone="plain"
            />
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
          // the cell's seal is the row's mark, beside every line (it overhangs nothing here)
          mark={<RowSeal id={addressCellSealId(c)} />}
          lines={[
            {
              key: 'cell',
              tone: 'head',
              left: <CellOutPoint cell={c} size="tight" />,
              right: (
                <CkbAmount
                  form="summary"
                  shannons={BigInt(c.capacity)}
                  signed={false}
                  tone="plain"
                />
              ),
            },
            {
              key: 'contains',
              tone: 'body',
              left: <CellContains cell={c} size="tight" />,
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
