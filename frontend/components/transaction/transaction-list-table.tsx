'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { RowSeal } from '@/components/ui/row-seal';
import { ClassMark } from '@/components/ui/class-mark';
import { AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import {
  ExactCkb,
  activityClassMark,
  activityHeadline,
  protocolActionNoteParts,
  formatScriptRef,
  ActivityReadError,
} from '@/components/activity-event-row';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTick } from '@/lib/chart-ticks';
import { parseShannons } from '@/lib/ckb-decimal';
import { Utc8Age } from '@/components/ui/utc8-date';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import type { ShortSize } from '@/lib/shorten';
import type { GlobalActivity, Transaction } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';

/**
 * The Cycles bar's track, as long as the Size one: its head's ticks say the scale as a tick does
 * ("2M", "750M"), so any scale under a billion cycles, "0" … "750M" (5 characters at 11 px,
 * 33 px), fits it. It was 80 for "0" … "750,000,000" (79.2 px); its 24 px went to What it did
 * (readability B5), whose note was cut mid-word. The figure beside each bar stays exact.
 */
const CYCLES_TRACK = 56;

/**
 * The Size bar's track, shorter than the Cycles one: its head's widest tick row, "0" … "600,000"
 * (a transaction is no larger than a block's 597,000 bytes, whose scale is 600,000; 8 characters
 * at 11 px, 52.8 px), fits it with 3.2 px between the two ticks. It was 80; its 24 px went to the
 * transaction's hash (old issue C): the developer figures give way before the columns every
 * reader reads.
 */
const SIZE_TRACK = 56;

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
          <ExactCkb
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
        <ExactCkb
          shannons={parseShannons(tx.fee, `transaction ${tx.hash} fee`)}
          signed={false}
          tone="plain"
        />
      </Quiet>
    </span>
  );
  const value = tx.isCellbase ? null : (
    <Quiet when={parseShannons(tx.value, `transaction ${tx.hash} value`) === BigInt(0)}>
      <ExactCkb
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
  const columns: ListColumn<Transaction>[] = [
    {
      key: 'transaction',
      label: 'Transaction',
      // The transaction's row stone (ROW_SEAL.tx.footprint, 56.5 px), the hash at the row size, 17
      // characters at 13 px (132.6 px; it was 19 with three dots, 148.2 px), and the 16 px gap:
      // 205.1 of its 208 px. It had 221 (old issue C: the 29 px it gained over 192 came from the
      // Size bar's track and What it did); 13 of its 15.9 spare went back to What it did (B5).
      minWidth: 208,
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
              className="text-body text-text-bright leading-[18px]"
            />
          </Link>
        </span>
      ),
    },
    {
      key: 'block',
      label: 'Block · date',
      minWidth: 175,
      render: (tx) => (
        <span className="text-small whitespace-nowrap">
          <Link href={`/blocks/${tx.blockNumber}`} className="hover:text-jade">
            {formatBlockNumber(tx.blockNumber)}
          </Link>
          <span className="text-text-dim">
            {' · '}
            <Utc8Age timestamp={tx.timestamp} />
          </span>
        </span>
      ),
    },
    {
      key: 'what',
      label: 'What it did',
      // B5: the note is never cut (it was truncated mid-word, "2 participa"): it follows the
      // headline on its line when it fits and wraps under it, part by part, when it does not. The
      // mark (16), its 8 px gap, the 8 px between headline and note and the 16 px gap leave them
      // 206 px: "CKB Transfer" and "12 participants" (27 characters at 12 px, 194.4 px) or
      // "Cellbase" and "pays #21,042,681" (24, 172.8 px) on one line. Its 246 px are the 215 it
      // had, 13 from the hash's spare and 18 from the Cycles bar (its track 80 → 56).
      minWidth: 246,
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
      // Every value under a billion CKB, "999,999,999.99999999" (20 characters at 12 px, 144 px),
      // and the 16 px gap; the next width is 22 characters, so 170 held nothing more. Its 10 px
      // and the fee's 9 went to the row seal.
      minWidth: 160,
      numeric: true,
      render: (tx) =>
        tx.isCellbase ? (
          <span data-reward className="text-text-dim">
            reward{' '}
            <span className="[&_[data-ckb-amount]]:text-text-dim">
              <ExactCkb
                shannons={parseShannons(tx.outputCapacity!, `cellbase ${tx.hash} reward`)}
                signed={false}
                tone="plain"
                unit={false}
              />
            </span>
          </span>
        ) : (
          <Quiet when={parseShannons(tx.value, `transaction ${tx.hash} value`) === BigInt(0)}>
            <ExactCkb
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
      // A fee under 100 CKB, "99.99999999" (11 characters at 12 px, 79.2 px), and the 16 px gap.
      minWidth: 96,
      numeric: true,
      render: (tx) => (
        <Quiet when={parseShannons(tx.fee, `transaction ${tx.hash} fee`) === BigInt(0)}>
          <ExactCkb
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
      // "10 → 2" (6 characters, 43.2 px) and the 16 px gap, on one line: it wrapped at 56.
      minWidth: 62,
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
      // The track (SIZE_TRACK, 56), the bar's 10 px gap, a 5-digit scale's figure ("2,000", 36 px)
      // and the 16 px gap, with 3 px to spare, as at the 80 px track's 145 (it gave 24 to the hash).
      minWidth: 121,
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
      // The 56 px track, the bar's 10 px gap, the figure of any scale under a billion
      // ("750,000,000", 11 characters at 12 px, 79.2 px) and the 16 px gap: 161.2. It was 180
      // on an 80 px track, which a 9-digit scale overran (185.2).
      minWidth: 162,
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
