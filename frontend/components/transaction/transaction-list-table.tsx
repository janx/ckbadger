'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { CELL_GAP_PX, ListTable, minimumWidth, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import { ClassMark } from '@/components/ui/class-mark';
import { AlignedBar, BAR_FIGURE_GAP, ScaleHeader, trackPx } from '@/components/ui/aligned-bar';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import {
  activityClassMark,
  activityHeadline,
  protocolActionNoteParts,
  formatScriptRef,
  ActivityReadError,
} from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTick } from '@/lib/chart-ticks';
import { parseShannons } from '@/lib/ckb-decimal';
import { Utc8Age } from '@/components/ui/utc8-date';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import type { GlobalActivity, Transaction } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import {
  MAX_BLOCK_BYTES,
  TX_CYCLES_BOUND,
  WIDEST_AGE,
  WIDEST_BLOCK_NUMBER,
  WIDEST_BLOCK_REWARD,
  shortChars,
  WIDEST_COMPACT_TICK,
  WIDEST_COUNT,
  WIDEST_TX_FEE,
  WIDEST_TX_VALUE,
} from '@/lib/list-bounds';

/**
 * The widest scale each bar column writes (readability F3-e). Size: a transaction is no larger
 * than a block's 597,000 bytes, whose scale is 600,000; its head writes the scale exact, "0" …
 * "600,000", which its track holds whole (64.8 px). Cycles: any scale under a billion cycles (lane
 * B5's bound of record: a page's largest transaction under 750,000,000), its head's ticks in the
 * tick form ("0" … "750M", at most four characters), so its track is the shortest a bar takes, 48
 * px; the exact figure beside each bar keeps its 11 characters.
 */
const SIZE_SCALE = resourceScale([MAX_BLOCK_BYTES]).toLocaleString('en-US');
const CYCLES_SCALE = resourceScale([TX_CYCLES_BOUND]).toLocaleString('en-US');
const SIZE_TRACK = trackPx(['0', SIZE_SCALE]);
const CYCLES_TRACK = trackPx(['0', WIDEST_COMPACT_TICK]);

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
      <Link href={`/blocks/${tx.pays.hash}`} className="hover:text-jade">
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

/** A count of resources on the quiet line: "3 → 3 cells · 903 B · 1,744,520 cycles". */
function quietFigures(tx: Transaction): ReactNode[] {
  return [
    <span key="cells" className="whitespace-nowrap">
      {consumedCells(tx)} → {formatCount(tx.outputsCount, 'cell')}
    </span>,
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
 * The phone row of the Transactions board (board L2): who and when, then what it did (wrapping,
 * never truncated), then value and fee with the developer figures quiet at the end. In a wide
 * container (640) the value and fee join what it did and the figures take the right edge.
 */
function TransactionPhoneRow({ tx, wide }: { tx: Transaction; wide: boolean }) {
  const mark = tx.activity ? activityClassMark(tx.activity) : null;
  // What it did, as parts of the line's one separated list (PA-25: no "·" ends or opens a line).
  const what: ReactNode[] = tx.isCellbase
    ? [
        <>
          <span className="text-text-bright">Cellbase</span> {paysNote(tx)}
        </>,
        <>
          reward{' '}
          <CkbAmount
            form="summary"
            shannons={parseShannons(tx.outputCapacity!, `cellbase ${tx.hash} reward`)}
            signed={false}
            tone="plain"
          />
        </>,
      ]
    : [
        <>
          {mark && <ClassMark kind={mark} size={12} className="mr-1.5 self-center" />}
          <span className="text-text-bright">{activityHeadline(tx.activity!)}</span>
        </>,
        ...whatNote(tx.activity!, 'tight').map((part, i) => (
          <span key={i} className="text-text-dim">
            {part}
          </span>
        )),
      ];
  const fee = (
    <span className="whitespace-nowrap">
      fee{' '}
      <Quiet when={parseShannons(tx.fee, `transaction ${tx.hash} fee`) === BigInt(0)}>
        <CkbAmount
          form="summary"
          shannons={parseShannons(tx.fee, `transaction ${tx.hash} fee`)}
          signed={false}
          tone="plain"
        />
      </Quiet>
    </span>
  );
  const value = tx.isCellbase ? null : (
    <Quiet when={parseShannons(tx.value, `transaction ${tx.hash} value`) === BigInt(0)}>
      <CkbAmount
        form="summary"
        shannons={parseShannons(tx.value, `transaction ${tx.hash} value`)}
        signed={false}
        tone="plain"
      />
    </Quiet>
  );
  // A cellbase pays its reward on the what line; its fee joins the quiet figures.
  const quiet = tx.isCellbase
    ? [...quietFigures(tx).slice(0, 2), fee, quietFigures(tx)[2]]
    : quietFigures(tx);
  return (
    <PhoneRow
      testId="transaction-row-mobile"
      mark={<RowSeal id={txSealId(tx)} />}
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: (
            <Link href={`/tx/${tx.hash}`} className="hover:underline">
              <Hash hash={tx.hash} size="tight" copyable={false} className="text-body" />
            </Link>
          ),
          right: (
            <span className="whitespace-nowrap">
              <Link href={`/blocks/${tx.blockNumber}`} className="text-text-bright hover:text-jade">
                {formatBlockNumber(tx.blockNumber)}
              </Link>
              <span className="text-text">
                {' · '}
                <Utc8Age timestamp={tx.timestamp} />
              </span>
            </span>
          ),
        },
        wide
          ? {
              key: 'what',
              tone: 'body',
              left: <Dotted parts={tx.isCellbase ? what : [...what, value, fee]} />,
              right: (
                <span className="text-text-dim text-meta">
                  <Dotted parts={quiet} />
                </span>
              ),
            }
          : { key: 'what', tone: 'body', left: <Dotted parts={what} /> },
        !wide && {
          key: 'figures',
          tone: 'quiet',
          left: tx.isCellbase ? (
            <Dotted parts={quiet.slice(0, 3)} />
          ) : (
            <span className="text-text">
              <Dotted parts={[value, fee]} />
            </span>
          ),
          right: <Dotted parts={tx.isCellbase ? quiet.slice(3) : quiet} />,
        },
      ]}
    />
  );
}

/**
 * The committed list's columns (readability F3-e: each budgeted from the widest value it holds),
 * on the page's two bar scales.
 */
function committedColumns(sizeScale: number, cycleScale: number): ListColumn<Transaction>[] {
  return [
    {
      key: 'transaction',
      label: 'Transaction',
      // The transaction's row stone (ROW_SEAL.tx.footprint, 56.5 px), the hash at the row size, 17
      // characters at the key's 14 px (142.8), and the gap: 215.3. Its 2 px spare went
      // (readability F3-e): it stood for no content, and a glyph advances exactly 0.6 em.
      minWidth: columnPx(
        ROW_SEAL.tx.footprint +
          textPx(shortChars(SHORT_FORMS.row.hash), fontPx('body')) +
          CELL_GAP_PX
      ),
      // Table-rows T5: the hash is the row's key, 13 px bright on the row's 18 px line (a cellbase
      // row keeps its dim row class); its stone leads it, overhanging that line (RowSeal).
      render: (tx) => (
        <span className="flex items-start">
          <RowSeal id={txSealId(tx)} overhang />
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
          <span className="text-text-dim block whitespace-nowrap">
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
      // ("CKB Transfer" and "12 participants", 258.6 px on one line) goes under its headline there
      // and joins it from a container of 1,275 px.
      minWidth: columnPx(
        16 + 8 + textPx('DAO Withdraw Complete'.length, fontPx('small')) + CELL_GAP_PX
      ),
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
              <span
                className={
                  tx.isCellbase
                    ? 'text-text whitespace-nowrap'
                    : 'text-text-bright whitespace-nowrap'
                }
              >
                {tx.isCellbase ? 'Cellbase' : activityHeadline(tx.activity!)}
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
      // Four digits a side, "9,999 → 9,999" (13 characters at 13 px, 101.4), and the gap, on one
      // line. It held "10 → 2" (65 px), and the genesis cellbase's "0 → 671" ran past it.
      minWidth: columnPx(
        textPx(`${WIDEST_COUNT} → ${WIDEST_COUNT}`.length, fontPx('small')) + CELL_GAP_PX
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
      // The track (SIZE_TRACK, its head's "0" … "600,000" whole, 64.8), the bar's 10 px gap, the
      // scale's figure ("600,000", 7 characters at 13 px, 54.6) and the gap. It held a 5-digit
      // scale ("2,000") on a 56 px track its head's ticks ran past.
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
        <ScaleHeader
          label="Cycles"
          scale={cycleScale}
          trackWidth={CYCLES_TRACK}
          tickGap={2}
          format={(n) => formatTick(n, n)}
        />
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
 * The width the committed list changes form at, the sum of its columns' minimums (1,220 px): the
 * pool list on the same page changes form there too (both lists are one page).
 */
export const TRANSACTION_LIST_MIN_PX = minimumWidth(
  'Committed transactions',
  committedColumns(1, 1)
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
}: {
  transactions: readonly Transaction[];
  label?: string;
}) {
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
  const columns = committedColumns(sizeScale, cycleScale);
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
