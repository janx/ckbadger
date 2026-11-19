'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { CELL_GAP_PX, ListTable, minimumWidth, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { NoBreak } from '@/components/ui/no-break';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import { ClassMark } from '@/components/ui/class-mark';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import {
  activityClassMark,
  activityHeadline,
  protocolActionNoteParts,
  formatScriptRef,
  ActivityReadError,
} from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { parseShannons } from '@/lib/ckb-decimal';
import { Utc8Age } from '@/components/ui/utc8-date';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import type { GlobalActivity, Transaction } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import {
  MAX_TX_INPUTS,
  WIDEST_AGE,
  WIDEST_AGE_UNDER_A_YEAR,
  WIDEST_BLOCK_NUMBER,
  shortChars,
  WIDEST_COUNT,
  WIDEST_TX_FEE,
  WIDEST_TX_OUTPUT,
  WIDEST_TX_VALUE,
} from '@/lib/list-bounds';

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

/**
 * What a cellbase pays, in its own words (R2b-a 35; the lists lane P3: cellbases are a list of their
 * own): the reward for the block it names (linked by hash, R-6a), and the name its amount takes;
 * none yet in a chain's first blocks, whose cellbases pay no reward; and block 0's, which is no
 * reward at all but the genesis issuance, the CKB first issued (its output is 33.6 billion CKB on
 * mainnet).
 */
function cellbasePays(tx: Transaction): {
  words: ReactNode;
  label: 'reward' | 'issued' | 'output';
} {
  if (!tx.isCellbase) throw new Error(`Transaction ${tx.hash} is no cellbase`);
  if (tx.pays === undefined) throw new Error(`Cellbase ${tx.hash} has no "pays" field`);
  if (tx.blockNumber === 0) {
    if (tx.pays !== null) throw new Error(`Cellbase ${tx.hash} of block 0 pays a reward`);
    return { words: 'the genesis issuance', label: 'issued' };
  }
  if (tx.pays === null) return { words: 'no reward yet', label: 'output' };
  return {
    words: (
      <>
        reward for{' '}
        <Link href={`/blocks/${tx.pays.hash}`} look="inline">
          {formatBlockNumber(tx.pays.number)}
        </Link>
      </>
    ),
    label: 'reward',
  };
}

/**
 * The quiet note beside a row's kind, as parts of the row's one separated list (PA-25): its
 * protocol facts, its type scripts, or its parties; their ids at the form's size (U-09).
 */
function whatNote(activity: GlobalActivity, size: ShortSize): ReactNode[] {
  if (activity.protocolActions.length > 0) {
    try {
      return activity.protocolActions.flatMap((pa) => protocolActionNoteParts(pa, size, 'summary'));
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
 * The phone row of the Transactions lists (the phone-rows board, gate D0′): the hash and the block
 * with its age; what it did, in the reading ink, wrapping between its parts and never cut; then,
 * quiet, what every reader uses — the cells, the value and the fee, each figure after its name (a
 * cellbase: what it pays, its amount named "reward", "issued" for block 0's genesis issuance, or
 * "output"; it pays no fee). Bytes and cycles are on the transaction's page; the cards from 560 px
 * (`wide`, where no table stands until the lists' bound) keep them at the end of the quiet line.
 */
function TransactionPhoneRow({ tx, wide }: { tx: Transaction; wide: boolean }) {
  const mark = tx.activity ? activityClassMark(tx.activity) : null;
  const pays = tx.isCellbase ? cellbasePays(tx) : null;
  // What it did, as parts of the line's one separated list (PA-25: no "·" ends or opens a line).
  const what: ReactNode[] = pays
    ? ['Cellbase', pays.words]
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
    ...(pays
      ? [
          <Labelled key="paid" label={pays.label}>
            {amount(tx.outputCapacity!, pays.label)}
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
              <Hash hash={tx.hash} size="tight" variant="inherit" className="text-body" />
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
 * The columns both lists begin with (readability F3-e: each budgeted from the widest value it
 * holds): the transaction, its row's key, and its block over its age.
 */
function keyColumns(): ListColumn<Transaction>[] {
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
      // Table-rows T5: the hash is the row's key (the key look's bright ink) on the 20 px row line;
      // its stone leads it, centred on that line (RowSeal).
      render: (tx) => (
        <span className="flex items-start">
          <RowSeal id={txSealId(tx)} />
          <Link href={`/tx/${tx.hash}`} look="key">
            <Hash hash={tx.hash} size="row" variant="inherit" className="text-body leading-row" />
          </Link>
        </span>
      ),
    },
    {
      key: 'block',
      // its two lines are the block and its age (the notes: it said "date" over an age)
      label: 'Block · age',
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
            look="inline"
            className="block whitespace-nowrap"
          >
            {formatBlockNumber(tx.blockNumber)}
          </Link>
          <span className="text-text-dim block">
            <Utc8Age timestamp={tx.timestamp} />
          </span>
        </span>
      ),
    },
  ];
}

/**
 * The cells a transaction consumed and created (its stone's rods), counted, on one line: the most
 * inputs a transaction holds and the widest output count, "13,568 → 9,999" (14 characters at 13
 * px, 109.2), and the gap (review fb-tests 6: it counted four digits of inputs, "9,999", while
 * 13,568 44-byte inputs fit a block). It held "10 → 2" (65 px), and the genesis cellbase's "0 → 671"
 * ran past it. The last column of both lists: no gap after it.
 */
const CELLS_COLUMN: ListColumn<Transaction> = {
  key: 'cells',
  label: 'Cells',
  minWidth: columnPx(
    textPx(`${MAX_TX_INPUTS.toLocaleString('en-US')} → ${WIDEST_COUNT}`.length, fontPx('small'))
  ),
  className: 'whitespace-nowrap',
  render: (tx) => (
    <span className="text-small">
      {consumedCells(tx).toLocaleString('en-US')} → {tx.outputsCount.toLocaleString('en-US')}
    </span>
  ),
};

/**
 * The user transactions' columns (the lists lane P3): what a reader of the list asks first — which
 * transaction, when, what it did, what moved and its fee, its cells. The size and the cycles bars
 * left the list for What it did's width (a common row on one line at 1,232 px: ".bit Time" rows
 * needed 430.2 px in 225); they are on each transaction's page and in the cards from 560 px.
 */
function committedColumns(sameWidth: number): ListColumn<Transaction>[] {
  return [
    ...keyColumns(),
    {
      key: 'what',
      label: 'What it did',
      // The table's flexible column (readability F3-e): it takes what the container gives beyond
      // the sum. B5: the note is never cut (it was truncated mid-word, "2 participa"): it follows
      // the headline on its line when it fits and wraps under it, part by part, when it does not.
      // At its minimum the mark (16), its 8 px gap, the longest DAO headline, "DAO Withdraw
      // Complete" (21 characters at 13 px, 163.8), whole, and the gap: 203.8. On the Transactions
      // page it also keeps `sameWidth`, what the page's widest list's sum is over this one's, so
      // the page's lists change form at one width.
      minWidth:
        columnPx(16 + 8 + textPx('DAO Withdraw Complete'.length, fontPx('small')) + CELL_GAP_PX) +
        sameWidth,
      width: 'auto',
      render: (tx) => {
        const mark = activityClassMark(tx.activity!);
        return (
          <span className="flex min-w-0 items-start gap-2">
            {/* One line tall (1lh), so the mark stands centred on the headline's line. */}
            <span className="flex h-[1lh] w-4 flex-none items-center">
              {mark && <ClassMark kind={mark} />}
            </span>
            <span className="flex min-w-0 flex-wrap items-baseline gap-x-2">
              {/* The headline is words (the lead's ruling): it breaks between its separated parts
                  first, the " · " with the part after it, then between the words of a part still
                  too long, and the row grows taller; it never draws past the cell. A name inside
                  it is a user's word with no bound (a .cell label in "ring pointer changed for
                  …"): it breaks anywhere rather than run out of the cell (the notes, logic N6). */}
              <span className="text-text-bright min-w-0 [overflow-wrap:anywhere]">
                <Dotted parts={activityHeadline(tx.activity!).split(HEADLINE_SEPARATOR)} />
              </span>
              <span className="text-text-dim text-small min-w-0">
                <Dotted parts={whatNote(tx.activity!, 'row')} />
              </span>
            </span>
          </span>
        );
      },
    },
    {
      key: 'value',
      label: 'Value, CKB',
      // Every value under a billion CKB, "999,999,999.99999999" (20 characters at 13 px, 156), and
      // the gap (a cellbase's "reward 9,999.99999999", 21, left with the cellbases).
      minWidth: columnPx(textPx(WIDEST_TX_VALUE.length, fontPx('small')) + CELL_GAP_PX),
      numeric: true,
      // a column of the subject amounts (R5's column form), aligned on the point
      render: (tx) => (
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
    CELLS_COLUMN,
  ];
}

/**
 * The cellbases' columns (the lists lane P3: a list of their own, with their own words and
 * bounds): which transaction and when; what it pays (a reward for an earlier block, none yet, or
 * block 0's genesis issuance), the flexible column; the CKB it outputs; its cells.
 */
function cellbaseColumns(sameWidth: number): ListColumn<Transaction>[] {
  return [
    ...keyColumns(),
    {
      key: 'pays',
      label: 'Pays',
      // "reward for #99,999,999" (22 characters at 13 px, 171.6) on one line, and the gap; it takes
      // what the container gives beyond the sum (`sameWidth`, as What it did does).
      minWidth:
        columnPx(
          textPx(`reward for ${WIDEST_BLOCK_NUMBER}`.length, fontPx('small')) + CELL_GAP_PX
        ) + sameWidth,
      width: 'auto',
      render: (tx) => <span className="text-small">{cellbasePays(tx).words}</span>,
    },
    {
      key: 'output',
      label: 'Output, CKB',
      // Every output of eleven whole digits, "99,999,999,999.99999999" (WIDEST_TX_OUTPUT, 23
      // characters at 13 px, 179.4: block 0's 33,600,000,000 CKB is 20), and the gap.
      minWidth: columnPx(textPx(WIDEST_TX_OUTPUT.length, fontPx('small')) + CELL_GAP_PX),
      numeric: true,
      render: (tx) => (
        <Quiet when={parseShannons(tx.outputCapacity!, `cellbase ${tx.hash} output`) === BigInt(0)}>
          <CkbAmount
            form="column"
            shannons={parseShannons(tx.outputCapacity!, `cellbase ${tx.hash} output`)}
            signed={false}
            tone="plain"
            unit={false}
          />
        </Quiet>
      ),
    },
    CELLS_COLUMN,
  ];
}

/**
 * The user transactions list's own sum of its columns' minimums (898 px). The Transactions page
 * changes its lists' form at one width, the largest of their sums (`formWidth`).
 */
export const TRANSACTION_LIST_MIN_PX = minimumWidth('Committed transactions', committedColumns(0));

/** The cellbases list's own sum of its columns' minimums (804 px). */
export const CELLBASE_LIST_MIN_PX = minimumWidth('Cellbases', cellbaseColumns(0));

/** A list's width to change form at: at least its own sum (its flexible column keeps the rest). */
function checkedFormWidth(label: string, formWidth: number, own: number): number {
  if (!(formWidth >= own))
    throw new Error(`${label}: cannot change form at ${formWidth} px, under its ${own}`);
  return formWidth - own;
}

/** The size and the cycles a card from 560 px writes: the exact evidence, or the list fails. */
function checkResources(tx: Transaction) {
  if (!Number.isSafeInteger(tx.txSize) || tx.txSize! <= 0)
    throw new Error(`Transaction ${tx.hash} is missing its exact serialized size`);
  if (tx.cyclesStatus === 'complete' ? tx.cycles === null : tx.cycles !== null)
    throw new Error(`Transaction ${tx.hash} has inconsistent cycle evidence`);
}

/**
 * The committed user transactions of the Transactions board (TX-25 / TX-26; lists lane P3): which
 * transaction and when, what it did, its value and fee, its cells. A cellbase is not one of them
 * (the cellbases are a list of their own): one here is an error, as is a row without its activity.
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
   * other lists gives the largest of their sums, and What it did keeps the difference.
   */
  formWidth?: number;
}) {
  const sameWidth = checkedFormWidth(label, formWidth, TRANSACTION_LIST_MIN_PX);
  for (const tx of transactions) {
    if (tx.isCellbase)
      throw new Error(
        `Transaction ${tx.hash} is a cellbase: the cellbases are a list of their own`
      );
    if (!tx.activity) throw new Error(`Transaction ${tx.hash} is missing its activity`);
    checkResources(tx);
  }
  return (
    <ListTable
      label={label}
      rows={transactions}
      columns={committedColumns(sameWidth)}
      rowKey={(tx) => tx.hash}
      empty="No transactions."
      mobileRow={(tx, wide) => <TransactionPhoneRow tx={tx} wide={wide} />}
    />
  );
}

/**
 * The cellbases of the Transactions board, one in every block, newest first (lists lane P3; plan
 * §7.6: the page opens on user transactions, the cellbases one tab away): what each pays and the
 * CKB it outputs. A transaction that is no cellbase is an error here.
 */
export function CellbaseListTable({
  transactions,
  formWidth = CELLBASE_LIST_MIN_PX,
}: {
  transactions: readonly Transaction[];
  /** As `TransactionListTable`'s: Pays keeps what this is over the list's own sum. */
  formWidth?: number;
}) {
  const label = 'Cellbases';
  const sameWidth = checkedFormWidth(label, formWidth, CELLBASE_LIST_MIN_PX);
  for (const tx of transactions) {
    if (!tx.isCellbase) throw new Error(`Transaction ${tx.hash} is no cellbase`);
    if (tx.outputCapacity === undefined)
      throw new Error(`Cellbase ${tx.hash} is missing its output capacity`);
    checkResources(tx);
  }
  return (
    <ListTable
      label={label}
      rows={transactions}
      columns={cellbaseColumns(sameWidth)}
      rowKey={(tx) => tx.hash}
      empty="No cellbases."
      mobileRow={(tx, wide) => <TransactionPhoneRow tx={tx} wide={wide} />}
    />
  );
}
