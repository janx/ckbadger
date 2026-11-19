'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { CELL_GAP_PX, ListTable, minimumWidth, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { NoBreak } from '@/components/ui/no-break';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import { ClassMark } from '@/components/ui/class-mark';
import {
  AlignedBar,
  BAR_FIGURE_GAP,
  COUNT_TRACK_PX,
  ScaleHeader,
} from '@/components/ui/aligned-bar';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import {
  activityClassMark,
  activityHeadline,
  protocolActionNoteParts,
  formatScriptRef,
  ActivityReadError,
} from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { resourceScale } from '@/components/blocks/block-visual';
import { parseShannons } from '@/lib/ckb-decimal';
import { Utc8Age } from '@/components/ui/utc8-date';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import type { GlobalActivity, Transaction } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import {
  LARGEST_TX_BYTES,
  MAX_TX_INPUTS,
  TX_CYCLES_BOUND,
  WIDEST_AGE,
  WIDEST_AGE_UNDER_A_YEAR,
  WIDEST_BLOCK_NUMBER,
  WIDEST_BLOCK_REWARD,
  shortChars,
  WIDEST_COUNT,
  WIDEST_TX_FEE,
  WIDEST_TX_VALUE,
} from '@/lib/list-bounds';

/**
 * The widest scale each bar column's figure writes (readability F3-e). Size: a transaction is no
 * larger than its block, and the largest block is genesis, whose 1,235,438-byte cellbase the oldest
 * page lists (LARGEST_TX_BYTES, as the block page's table; review fb-logic 6: it was the 597,000
 * limit, a 600,000 scale): a 1,500,000 scale. Cycles: any scale under a billion cycles (lane B5's
 * bound of record: a page's largest transaction under 750,000,000). Both heads write their ticks in
 * the tick form ("0 … 1.5M", "0 … 750M"; the rule at `trackPx`), so each track is COUNT_TRACK_PX,
 * 48 px; the exact figure beside each bar keeps its digits.
 */
const SIZE_SCALE = resourceScale([LARGEST_TX_BYTES]).toLocaleString('en-US');
const CYCLES_SCALE = resourceScale([TX_CYCLES_BOUND]).toLocaleString('en-US');
const SIZE_TRACK = COUNT_TRACK_PX;
const CYCLES_TRACK = COUNT_TRACK_PX;

/**
 * Where a headline's separated parts meet: `protocolActionLabel` joins a protocol's name and its
 * action with it (".bit · recycle expired account"), the separator the `Dotted` list draws.
 */
const HEADLINE_SEPARATOR = ' · ';

/** The cells a transaction consumed: a cellbase none (the API counts its one null input). */
function consumedCells(tx: Transaction): number {
  return tx.isCellbase ? 0 : tx.inputsCount;
}

/** The transaction's stone: its hash, the cells it consumed and the cells it created. */
function txSealId(tx: Transaction): SealId {
  return { kind: 'tx', hash: tx.hash, consumed: consumedCells(tx), created: tx.outputsCount };
}

/** A zero amount drawn quiet (board: `#4b5266`), never a different number. */
function Quiet({ when, children }: { when: boolean; children: ReactNode }) {
  return when ? (
    <span data-zero className="[&_[data-ckb-amount]]:text-text-dim">
      {children}
    </span>
  ) : (
    <>{children}</>
  );
}

/** A cellbase's note (R2b-a 35): the block whose reward it pays (linked by hash, R-6a), or that it pays none. */
function paysNote(tx: Transaction): ReactNode {
  if (tx.pays === undefined) throw new Error(`Cellbase ${tx.hash} has no "pays" field`);
  if (tx.pays === null) return 'pays no reward yet';
  return (
    <>
      pays{' '}
      <Link href={`/blocks/${tx.pays.hash}`} look="inline">
        {formatBlockNumber(tx.pays.number)}
      </Link>
    </>
  );
}

/**
 * The quiet note beside a row's kind, as parts of the row's one separated list (PA-25): its
 * protocol facts, its type scripts, or its parties; their ids at the form's size (U-09).
 */
function whatNote(activity: GlobalActivity, size: ShortSize): ReactNode[] {
  if (activity.protocolActions.length > 0) {
    try {
      return activity.protocolActions.flatMap((pa) => protocolActionNoteParts(pa, size));
    } catch (error) {
      return [<ActivityReadError key="error" txHash={activity.txHash} error={error} />];
    }
  }
  const names = [
    ...new Set(
      activity.typeCalls.map((sc) =>
        formatScriptRef({ codeHash: sc.typeCodeHash, scriptName: sc.scriptName }, size)
      )
    ),
  ];
  if (names.length > 0) return names;
  return [formatCount(activity.participants.length, 'participant')];
}

/** The developer figures a card keeps from 560 px, where no table stands yet: "903 B · 1,744,520 cycles". */
function resourceFigures(tx: Transaction): ReactNode[] {
  return [
    <span key="size" className="whitespace-nowrap">
      {tx.txSize!.toLocaleString('en-US')} B
    </span>,
    <span key="cycles" className="whitespace-nowrap">
      {tx.cycles === null
        ? tx.cyclesStatus === 'failed'
          ? 'replay failed'
          : 'cycles not yet run'
        : `${tx.cycles.toLocaleString('en-US')} cycles`}
    </span>,
  ];
}

/**
 * Line 1 of a transaction's phone row (the phone-rows board's head(): 309 of 328 px there for
 * "0xccf9…091f" beside "#21,009,135 · 20s ago"): the transaction's stone and its gap
 * (ROW_SEAL.tx.footprint, 40.3), the tight hash at the key's 14 px (11 characters, 92.4), the 12 px
 * between the parts and, at 13 px, the widest block with the widest age under a year, "#99,999,999
 * · 364d ago" (22 characters, 171.6): 316.3 of a 360 px phone's 328 px line. An age of a year or
 * more, "10y 364d ago" at its widest (WIDEST_AGE; 26 characters, 202.8), makes it 347.5: inside 390's
 * 358, and at 360 the block and its age drop under the hash, whole and right-aligned (PhoneRow's
 * rule), on a page a year back or more.
 */
const TX_PHONE_HEAD_PX = phoneHead(
  'transactions',
  ROW_SEAL.tx.footprint +
    textPx(shortChars(SHORT_FORMS.tight.hash), fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(`${WIDEST_BLOCK_NUMBER} · ${WIDEST_AGE_UNDER_A_YEAR}`.length, fontPx('small'))
);

/**
 * The phone row of the Transactions list (the phone-rows board, gate D0′): the hash and the block
 * with its age; what it did, in the reading ink, wrapping between its parts and never cut; then,
 * quiet, what every reader uses — the cells, the value and the fee, each figure after its name (a
 * cellbase: the reward it pays, named; it pays no fee). Bytes and cycles are on the transaction's
 * page and in the table; the cards from 560 px (`wide`, where no table stands until the list's
 * bound) keep them at the end of the quiet line.
 */
function TransactionPhoneRow({ tx, wide }: { tx: Transaction; wide: boolean }) {
  const mark = tx.activity ? activityClassMark(tx.activity) : null;
  // What it did, as parts of the line's one separated list (PA-25: no "·" ends or opens a line).
  const what: ReactNode[] = tx.isCellbase
    ? ['Cellbase', paysNote(tx)]
    : [
        <>
          {mark && <ClassMark kind={mark} size={12} className="mr-1.5 self-center" />}
          {activityHeadline(tx.activity!)}
        </>,
        ...whatNote(tx.activity!, 'tight'),
      ];
  const amount = (shannons: string, field: string) => {
    const value = parseShannons(shannons, `transaction ${tx.hash} ${field}`);
    return (
      <Quiet when={value === BigInt(0)}>
        <CkbAmount form="summary" shannons={value} signed={false} tone="plain" />
      </Quiet>
    );
  };
  const figures: ReactNode[] = [
    <span key="cells" className="whitespace-nowrap">
      {consumedCells(tx)} → {formatCount(tx.outputsCount, 'cell')}
    </span>,
    ...(tx.isCellbase
      ? [
          <Labelled key="reward" label="reward">
            {amount(tx.outputCapacity!, 'reward')}
          </Labelled>,
        ]
      : [
          <Labelled key="value" label="value">
            {amount(tx.value, 'value')}
          </Labelled>,
          <Labelled key="fee" label="fee">
            {amount(tx.fee, 'fee')}
          </Labelled>,
        ]),
    ...(wide ? resourceFigures(tx) : []),
  ];
  return (
    <PhoneRow
      testId="transaction-row-mobile"
      head={TX_PHONE_HEAD_PX}
      mark={<RowSeal id={txSealId(tx)} />}
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: (
            <Link href={`/tx/${tx.hash}`} look="key">
              <Hash hash={tx.hash} size="tight" copyable={false} className="text-body" />
            </Link>
          ),
          // the block and its age are each a unit; "this device's clock is behind" in the age's
          // place is words and breaks between them (Utc8Age), never one run with the block (L2)
          right: (
            <>
              <NoBreak>
                <Link href={`/blocks/${tx.blockNumber}`} look="inline">
                  {formatBlockNumber(tx.blockNumber)}
                </Link>
              </NoBreak>
              {' · '}
              <Utc8Age timestamp={tx.timestamp} />
            </>
          ),
        },
        { key: 'what', tone: 'body', left: <Dotted parts={what} /> },
        { key: 'figures', tone: 'quiet', left: <Dotted parts={figures} /> },
      ]}
    />
  );
}

/**
 * The committed list's columns (readability F3-e: each budgeted from the widest value it holds),
 * on the page's two bar scales.
 */
function committedColumns(
  sizeScale: number,
  cycleScale: number,
  sameWidth: number
): ListColumn<Transaction>[] {
  return [
    {
      key: 'transaction',
      label: 'Transaction',
      // The transaction's row stone (ROW_SEAL.tx.footprint, 40.3 px), the hash at the row size, 17
      // characters at the key's 14 px (142.8), and the gap: 199.1. Its 2 px spare went
      // (readability F3-e): it stood for no content, and a glyph advances exactly 0.6 em.
      minWidth: columnPx(
        ROW_SEAL.tx.footprint +
          textPx(shortChars(SHORT_FORMS.row.hash), fontPx('body')) +
          CELL_GAP_PX
      ),
      // Table-rows T5: the hash is the row's key, bright on the 20 px row line (a cellbase row
      // keeps its dim row class); its stone leads it, centred on that line (RowSeal).
      render: (tx) => (
        <span className="flex items-start">
          <RowSeal id={txSealId(tx)} />
          <Link href={`/tx/${tx.hash}`} className="hover:underline">
            <Hash
              hash={tx.hash}
              size="row"
              copyable={false}
              className="text-body text-text-bright leading-row"
            />
          </Link>
        </span>
      ),
    },
    {
      key: 'block',
      label: 'Block · date',
      // Two data on two lines (readability F3-e): the block, "#99,999,999", over its age, "10y
      // 364d ago" (12 characters at 13 px, 93.6, the wider), and the gap. On one line, with " · "
      // between them, they took 25 characters (211 px; it was 175, which held "59m ago" and not an
      // older page's age), and the table could not hold its columns at 1,232 px; at its minimum
      // every row is two lines already, the note under its headline in What it did.
      minWidth: columnPx(
        textPx(Math.max(WIDEST_BLOCK_NUMBER.length, WIDEST_AGE.length), fontPx('small')) +
          CELL_GAP_PX
      ),
      render: (tx) => (
        <span className="text-small">
          <Link
            href={`/blocks/${tx.blockNumber}`}
            className="hover:text-jade block whitespace-nowrap"
          >
            {formatBlockNumber(tx.blockNumber)}
          </Link>
          <span className="text-text-dim block">
            <Utc8Age timestamp={tx.timestamp} />
          </span>
        </span>
      ),
    },
    {
      key: 'what',
      label: 'What it did',
      // The table's flexible column (readability F3-e): it takes what the container gives beyond
      // the sum. B5: the note is never cut (it was truncated mid-word, "2 participa"): it follows
      // the headline on its line when it fits and wraps under it, part by part, when it does not.
      // At its minimum the mark (16), its 8 px gap, the longest DAO headline, "DAO Withdraw
      // Complete" (21 characters at 13 px, 163.8), whole, and the gap: 203.8; a common row's note
      // ("CKB Transfer" and "12 participants", 258.6 px on one line) goes under its headline there.
      // On the Transactions page it also keeps `sameWidth`, what the pool list's sum is over this
      // one's, so both of the page's lists change form at one width.
      minWidth:
        columnPx(16 + 8 + textPx('DAO Withdraw Complete'.length, fontPx('small')) + CELL_GAP_PX) +
        sameWidth,
      width: 'auto',
      render: (tx) => {
        const mark = tx.activity ? activityClassMark(tx.activity) : null;
        return (
          <span className="flex min-w-0 items-start gap-2">
            {/* One line tall (1lh), so the mark stands centred on the headline's line. */}
            <span className="flex h-[1lh] w-4 flex-none items-center">
              {mark && <ClassMark kind={mark} />}
            </span>
            <span className="flex min-w-0 flex-wrap items-baseline gap-x-2">
              {/* The headline is words (the lead's ruling): it breaks between its separated parts
                  first, the " · " with the part after it, then between the words of a part still
                  too long, and the row grows taller; it never draws past the cell. */}
              <span className={tx.isCellbase ? 'text-text min-w-0' : 'text-text-bright min-w-0'}>
                {tx.isCellbase ? (
                  'Cellbase'
                ) : (
                  <Dotted parts={activityHeadline(tx.activity!).split(HEADLINE_SEPARATOR)} />
                )}
              </span>
              <span className="text-text-dim text-small min-w-0">
                {tx.isCellbase ? paysNote(tx) : <Dotted parts={whatNote(tx.activity!, 'row')} />}
              </span>
            </span>
          </span>
        );
      },
    },
    {
      key: 'value',
      label: 'Value, CKB',
      // Every value under a billion CKB, "999,999,999.99999999" (20 characters), or a cellbase's
      // "reward" and a reward of four whole digits, "reward 9,999.99999999" (21, the wider: 163.8 at
      // 13 px; the first epochs paid 1,917 CKB), and the gap.
      minWidth: columnPx(
        textPx(
          Math.max(WIDEST_TX_VALUE.length, `reward ${WIDEST_BLOCK_REWARD}`.length),
          fontPx('small')
        ) + CELL_GAP_PX
      ),
      numeric: true,
      render: (tx) =>
        tx.isCellbase ? (
          <span data-reward className="text-text-dim">
            reward{' '}
            <span className="[&_[data-ckb-amount]]:text-text-dim">
              <CkbAmount
                form="column"
                shannons={parseShannons(tx.outputCapacity!, `cellbase ${tx.hash} reward`)}
                signed={false}
                tone="plain"
                unit={false}
              />
            </span>
          </span>
        ) : (
          <Quiet when={parseShannons(tx.value, `transaction ${tx.hash} value`) === BigInt(0)}>
            <CkbAmount
              form="column"
              shannons={parseShannons(tx.value, `transaction ${tx.hash} value`)}
              signed={false}
              tone="plain"
              unit={false}
            />
          </Quiet>
        ),
    },
    {
      key: 'fee',
      label: 'Fee, CKB',
      // A fee under 100 CKB, "99.99999999" (11 characters at 13 px, 85.8), and the gap.
      minWidth: columnPx(textPx(WIDEST_TX_FEE.length, fontPx('small')) + CELL_GAP_PX),
      numeric: true,
      render: (tx) => (
        <Quiet when={parseShannons(tx.fee, `transaction ${tx.hash} fee`) === BigInt(0)}>
          <CkbAmount
            form="column"
            shannons={parseShannons(tx.fee, `transaction ${tx.hash} fee`)}
            signed={false}
            tone="plain"
            unit={false}
          />
        </Quiet>
      ),
    },
    {
      key: 'cells',
      label: 'Cells',
      // The most inputs a transaction holds and the widest output count, "13,568 → 9,999" (14
      // characters at 13 px, 109.2), and the gap, on one line (review fb-tests 6: it counted four
      // digits of inputs, "9,999", while 13,568 44-byte inputs fit a block). It held "10 → 2"
      // (65 px), and the genesis cellbase's "0 → 671" ran past it.
      minWidth: columnPx(
        textPx(
          `${MAX_TX_INPUTS.toLocaleString('en-US')} → ${WIDEST_COUNT}`.length,
          fontPx('small')
        ) + CELL_GAP_PX
      ),
      className: 'whitespace-nowrap',
      render: (tx) => (
        <span className="text-text-dim text-small">
          {consumedCells(tx).toLocaleString('en-US')} → {tx.outputsCount.toLocaleString('en-US')}
        </span>
      ),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      // The 48 px track (its head's "0 … 1.5M" in the tick form), the bar's 10 px gap, the scale's
      // figure ("1,500,000", genesis's cellbase; 9 characters at 13 px, 70.2) and the gap: 144.2
      // (review fb-logic 6: it was "600,000", 128.6, and genesis's figure ran 15.6 px past it). It
      // held a 5-digit scale ("2,000") on a 56 px track its head's exact ticks ran past.
      minWidth: columnPx(
        SIZE_TRACK + BAR_FIGURE_GAP + textPx(SIZE_SCALE.length, fontPx('small')) + CELL_GAP_PX
      ),
      // The shared head at this table's own geometry: a tick row as long as the track, 2 px under
      // the name.
      header: (
        <ScaleHeader label="Size, bytes" scale={sizeScale} trackWidth={SIZE_TRACK} tickGap={2} />
      ),
      render: (tx) => (
        <AlignedBar
          label="Size, bytes"
          scale={sizeScale}
          trackWidth={SIZE_TRACK}
          segments={[
            {
              key: 'value',
              label: 'Size, bytes',
              value: tx.txSize!,
              tone: tx.isCellbase ? 'overhead' : 'data',
            },
          ]}
        />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      // The track (CYCLES_TRACK, 48: its head's ticks in the tick form), the bar's 10 px gap and
      // the figure of any scale under a billion ("750,000,000", 11 characters at 13 px, 85.8); the
      // last column, no gap after it (it counted 16).
      minWidth: columnPx(
        CYCLES_TRACK + BAR_FIGURE_GAP + textPx(CYCLES_SCALE.length, fontPx('small'))
      ),
      header: (
        <ScaleHeader label="Cycles" scale={cycleScale} trackWidth={CYCLES_TRACK} tickGap={2} />
      ),
      render: (tx) =>
        tx.cycles === null ? (
          <span className="text-text-dim text-small">
            {tx.cyclesStatus === 'failed' ? 'replay failed' : 'not yet run'}
          </span>
        ) : (
          <AlignedBar
            label="Cycles"
            scale={cycleScale}
            trackWidth={CYCLES_TRACK}
            segments={[{ key: 'value', label: 'Cycles', value: tx.cycles, tone: 'data' }]}
          />
        ),
    },
  ];
}

/**
 * The committed list's own sum of its columns' minimums (1,211 px). The Transactions page changes
 * both of its lists' form at one width, the larger of the two sums (`formWidth`).
 */
export const TRANSACTION_LIST_MIN_PX = minimumWidth(
  'Committed transactions',
  committedColumns(1, 1, 0)
);

/**
 * The committed list of the Transactions board (TX-25 / TX-26): general columns first (tx,
 * block · age, what it did, value, fee), then the quieter cells / size / cycles on one shared
 * scale per column. A cellbase row is set back (the quiet variant: every word dim, its stone
 * pressed; R4) and shows the reward it pays, grey (D5): Value
 * itself excludes cellbase issuance, so the reward is never added to a total.
 */
export function TransactionListTable({
  transactions,
  label = 'Committed transactions',
  formWidth = TRANSACTION_LIST_MIN_PX,
}: {
  transactions: readonly Transaction[];
  label?: string;
  /**
   * The width this list changes form at, at least its own sum: the page that draws it beside
   * another list gives the larger of their sums, and What it did keeps the difference.
   */
  formWidth?: number;
}) {
  if (!(formWidth >= TRANSACTION_LIST_MIN_PX))
    throw new Error(
      `${label}: cannot change form at ${formWidth} px, under its ${TRANSACTION_LIST_MIN_PX}`
    );
  for (const tx of transactions) {
    if (!Number.isSafeInteger(tx.txSize) || tx.txSize! <= 0)
      throw new Error(`Transaction ${tx.hash} is missing its exact serialized size`);
    if (tx.cyclesStatus === 'complete' ? tx.cycles === null : tx.cycles !== null)
      throw new Error(`Transaction ${tx.hash} has inconsistent cycle evidence`);
    if (tx.isCellbase ? tx.outputCapacity === undefined : !tx.activity)
      throw new Error(
        `Transaction ${tx.hash} is missing its ${tx.isCellbase ? 'output capacity' : 'activity'}`
      );
  }
  const sizeScale = resourceScale(transactions.map((tx) => tx.txSize!));
  const cycleScale = resourceScale(
    transactions.flatMap((tx) => (tx.cycles === null ? [] : [tx.cycles]))
  );
  const columns = committedColumns(sizeScale, cycleScale, formWidth - TRANSACTION_LIST_MIN_PX);
  return (
    <ListTable
      label={label}
      rows={transactions}
      columns={columns}
      rowKey={(tx) => tx.hash}
      rowClassName={(tx) => (tx.isCellbase ? 'quiet' : '')}
      empty="No transactions."
      mobileRow={(tx, wide) => <TransactionPhoneRow tx={tx} wide={wide} />}
    />
  );
}
