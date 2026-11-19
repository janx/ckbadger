'use client';

import type { ReactNode } from 'react';
import {
  ActivityRow,
  ActivityTitle,
  activityTitleText,
  type ActivityRowData,
} from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { activityHeadline } from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
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
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import {
  MAX_BLOCK_BYTES,
  MAX_BLOCK_CYCLES,
  WIDEST_BLOCK_NUMBER,
  WIDEST_COUNT,
  WIDEST_COUNT_DIGITS,
  WIDEST_TX_FEE,
  WIDEST_TX_VALUE,
  shortChars,
} from '@/lib/list-bounds';
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
function objectHeadline(
  participant: ParticipantInfo,
  txHash: string
): { node: ReactNode; text: string } {
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
  return names.size === 1 && collection
    ? {
        node: (
          <>
            {movement}
            <span className="text-text-dim font-normal"> · {collection}</span>
          </>
        ),
        text: `${movement} · ${collection}`,
      }
    : { node: movement, text: movement };
}

/**
 * What the address page draws of one activity: the row (this address's participant line alone),
 * and its headline, in words for its run (readability B4) and as drawn.
 */
function addressRow(activity: Activity, address: Address) {
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
  const words =
    (itemClass === 'token' || itemClass === 'identity') && primary
      ? `${itemClass[0].toUpperCase()}${itemClass.slice(1)} ${BigInt(primary.delta) > BigInt(0) ? 'received' : BigInt(primary.delta) < BigInt(0) ? 'sent' : 'changed'}`
      : activityHeadline(row);
  const headline =
    itemClass === 'object'
      ? objectHeadline(participant, activity.txHash)
      : { node: words, text: words };
  return { row, participant, headline };
}

/**
 * One activity of this address: the site's one activity row (consistency C3), its participant line
 * this address's — first, as text, tagged "this address" (`focusLockHash`). The endpoint supplies
 * this participant's deltas only; the other parties of the transaction are on its page.
 */
export function AddressActivity({ activity, address }: AddressActivityProps) {
  const { row, headline } = addressRow(activity, address);
  return (
    <ActivityRow
      headline={headline.node}
      activity={row}
      focusLockHash={address.lockScriptHash}
      foldObjects
      itemTone="sign"
      // the transaction's other parties are references here, their changes not on the row
      unlistedParties={activity.participants.length}
    />
  );
}
/**
 * The address's activities with their runs folded (readability B4, `lib/activity-runs.ts`): a run
 * is compared on this address's line and the transaction's other parties, which the row does not
 * draw but the activity names, so two transfers to different counterparties never fold. A run
 * folds within the page shown.
 */
export function AddressActivities({ rows, address }: { rows: Activity[]; address: Address }) {
  return (
    <ActivityRuns
      focusLockHash={address.lockScriptHash}
      rows={rows.map((activity) => {
        const { row, participant, headline } = addressRow(activity, address);
        return {
          key: activity.txHash,
          subject: runSubject(row, activityTitleText(row, headline.text), [
            participant,
            ...activity.participants,
          ]),
          activity: row,
          title: <ActivityTitle activity={row} headline={headline.node} />,
          row: <AddressActivity activity={activity} address={address} />,
        };
      })}
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
        <Hash hash={tx.txHash} size={size} className="text-small" />
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

/** A run of characters at the cells' 13 px (readability F3-e: every column budgets what it holds). */
const cell = (chars: number) => textPx(chars, fontPx('small'));

/**
 * The address's transactions (readability F3-e): every column budgeted from the widest value it
 * holds (lib/list-bounds.ts), the 13 px cells at 0.6 em a glyph and the 16 px gap after every
 * column but the last. Scripts is the flexible column.
 */
const columns: ListColumn<AddressTransaction>[] = [
  {
    key: 'hash',
    label: 'Transaction',
    // the hash at the row size (17 characters, 132.6) and the gap; it was 172
    minWidth: columnPx(cell(shortChars(SHORT_FORMS.row.hash)) + CELL_GAP_PX),
    render: (tx) => <TxHash tx={tx} size="row" />,
  },
  {
    key: 'block',
    label: 'Block · date',
    // a pool transaction's wait on one line, "in pool for 99d" (waits under 100 days; 117), wider
    // than "#99,999,999", a date or its status badge, and the gap; it was 136
    minWidth: columnPx(cell('in pool for 99d'.length) + CELL_GAP_PX),
    render: (tx) => <TxPosition tx={tx} />,
  },
  {
    key: 'cells',
    label: 'Cells in → out',
    // four digits a side, the inputs as written and the outputs grouped, "9999 → 9,999" (93.6),
    // and the gap; it was 100
    minWidth: columnPx(cell(`${WIDEST_COUNT_DIGITS} → ${WIDEST_COUNT}`.length) + CELL_GAP_PX),
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
    // a change under a billion CKB with its sign on one line (21 characters, 163.8) and the gap
    minWidth: columnPx(cell(`+${WIDEST_TX_VALUE}`.length) + CELL_GAP_PX),
    numeric: true,
    className: 'whitespace-normal',
    render: (tx) => (
      <CkbAmount form="column" shannons={BigInt(tx.capacityChange)} wrap unit={false} />
    ),
  },
  {
    key: 'fee',
    label: 'Fee, CKB',
    // a fee under 100 CKB (85.8) and the gap; it was 136
    minWidth: columnPx(cell(WIDEST_TX_FEE.length) + CELL_GAP_PX),
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
    // "Unavailable" (85.8), wider than a size of "597,000", and the gap; it was 90
    minWidth: columnPx(
      cell(Math.max('Unavailable'.length, MAX_BLOCK_BYTES.toLocaleString('en-US').length)) +
        CELL_GAP_PX
    ),
    numeric: true,
    render: (tx) => (tx.txSize === null ? 'Unavailable' : tx.txSize.toLocaleString('en-US')),
  },
  {
    key: 'cycles',
    label: 'Cycles',
    // the cycle limit, "3,500,000,000" (101.4), wider than "Not computed", and the gap; it was 120
    minWidth: columnPx(cell(MAX_BLOCK_CYCLES.toLocaleString('en-US').length) + CELL_GAP_PX),
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
    // The flexible column: a script's 12 px mark, its 6 px gap and 12 characters of its name at
    // its minimum (a longer list wraps between scripts); the last column, no gap after it. It was
    // 146.
    minWidth: columnPx(12 + 6 + cell(12)),
    width: 'auto',
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
      <Hash hash={c.txHash} size={size} className="text-small" />
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
          <Hash hash={c.typeScriptHash} size={size} className="text-small" />
        </Link>
      )}
    </span>
  );
}

/**
 * The live cells table's columns (readability F3-e: each budgeted from the widest value it holds,
 * the 13 px cells and 12 px units at 0.6 em a glyph, 16 px between columns, none after the last):
 * 953 px, the width it changes form at (it was 992, then 965 with the 30 px stone). The seals by
 * kind (C4) lead the Cell column with the cell's 18 px row seal and its 8 px gap (readability R6).
 * Contains is the flexible column.
 */
const cellColumns: ListColumn<AddressCell>[] = [
  {
    key: 'outpoint',
    label: 'Cell',
    // the stone and its gap (ROW_SEAL.cell.footprint, 26), the out point at the row size with a
    // four-digit index (22 characters, 171.6) and the gap: 213.6 (it was 238, 12.4 of it spare,
    // then 225.6 with the 30 px stone)
    minWidth: columnPx(
      ROW_SEAL.cell.footprint +
        cell(shortChars(SHORT_FORMS.row.hash) + `:${WIDEST_COUNT_DIGITS}`.length) +
        CELL_GAP_PX
    ),
    // The cell's seal leads the row, centred on the out point's line (RowSeal), so a row keeps its
    // height; the out point stays the row's link (the seal is an image, not a link).
    render: (c) => (
      <span className="flex items-start">
        <RowSeal id={addressCellSealId(c)} />
        <CellOutPoint cell={c} size="row" />
      </span>
    ),
  },
  {
    key: 'content',
    label: 'Contains',
    // The flexible column: what a cell holds in its class form whole at its minimum, the 16 px
    // mark, "Other scripts" and the type script's hash at the row size, 8 px apart, and the gap:
    // 281.8 (it was 280); a cell's content takes what the container gives beyond the sum.
    minWidth: columnPx(
      16 +
        8 +
        cell('Other scripts'.length) +
        8 +
        cell(shortChars(SHORT_FORMS.row.hash)) +
        CELL_GAP_PX
    ),
    width: 'auto',
    render: (c) => <CellContains cell={c} size="row" />,
  },
  {
    key: 'capacity',
    label: 'Capacity, CKB',
    // every amount under a billion CKB on one line (156) and its unit at 12 px (28.8), and the gap
    minWidth: columnPx(
      cell(WIDEST_TX_VALUE.length) + textPx(' CKB'.length, fontPx('meta')) + CELL_GAP_PX
    ),
    numeric: true,
    className: 'whitespace-normal',
    render: (c) => (
      <CkbAmount form="column" shannons={BigInt(c.capacity)} signed={false} tone="plain" wrap />
    ),
  },
  {
    key: 'occupied',
    label: 'Occupied, CKB',
    // a cell is at most a block's 597,000 bytes, so at most "597,000.00000000" (124.8) and its
    // unit (28.8), and the gap
    minWidth: columnPx(
      cell(`${MAX_BLOCK_BYTES.toLocaleString('en-US')}.00000000`.length) +
        textPx(' CKB'.length, fontPx('meta')) +
        CELL_GAP_PX
    ),
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
    // "#99,999,999" (85.8); the last column, no gap after it
    minWidth: columnPx(cell(WIDEST_BLOCK_NUMBER.length)),
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
          // the cell's seal is the row's mark, beside every line, centred in a row line's box at
          // the top (RowSeal)
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
