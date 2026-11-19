import Link from '@/components/ui/link';
import { Hash } from '@/components/ui/hash';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import {
  AlignedBar,
  BAR_FIGURE_GAP,
  COUNT_TRACK_PX,
  ScaleHeader,
} from '@/components/ui/aligned-bar';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { resourceScale } from '@/components/blocks/block-visual';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatBlockNumber } from '@/lib/format-block';
import type { Transaction } from '@/lib/api';
import { formatCount } from '@/lib/format-count';
import { SHORT_FORMS } from '@/lib/shorten';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import {
  LARGEST_TX_BYTES,
  MAX_TX_INPUTS,
  TX_CYCLES_BOUND,
  WIDEST_BLOCK_NUMBER,
  WIDEST_COUNT,
  WIDEST_COUNT_DIGITS,
  WIDEST_TX_FEE,
  WIDEST_TX_OUTPUT,
  WIDEST_TX_OUTPUT_SUMMARY,
  shortChars,
} from '@/lib/list-bounds';

/**
 * The bars' track (readability F3-e, the coordinator's ruling): the heads write their ticks in
 * the tick form ("0 … 1.5M", "0 … 750M"), so each track is COUNT_TRACK_PX, 48 px; it was 96, under
 * heads whose exact "0 … 600,000" its 12 px ticks overran.
 */
const TRACK = COUNT_TRACK_PX;

/** A cell's text measured at the small step, the table's. */
const cell = (text: string) => textPx(text.length, fontPx('small'));

/**
 * The widest scales the bars' figures write: a transaction is no larger than its block, and the
 * largest block is genesis's 1,236,271 bytes (LARGEST_TX_BYTES, the committed list's bound too: a
 * 1,500,000 scale); a transaction's cycles under a billion, as the committed list's (a 750,000,000
 * scale).
 */
const SIZE_SCALE = resourceScale([LARGEST_TX_BYTES]).toLocaleString('en-US');
const CYCLES_SCALE = resourceScale([TX_CYCLES_BOUND]).toLocaleString('en-US');

/** A CKB figure of the phone row in the summary form, after its name (`Labelled`). */
function named(label: string, shannons: string, field: string, tx: Transaction) {
  return (
    <Labelled label={label}>
      <CkbAmount
        form="summary"
        shannons={parseShannons(shannons, `transaction ${tx.hash} ${field}`)}
        signed={false}
        tone="plain"
      />
    </Labelled>
  );
}

function cells(tx: Transaction) {
  return (
    <span className="whitespace-nowrap">
      {tx.isCellbase ? 0 : tx.inputsCount} → {formatCount(tx.outputsCount, 'cell')}
    </span>
  );
}

function resources(tx: Transaction) {
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
 * Line 1 of a block's transaction on a phone (the phone-rows board's head()): the tight hash at the
 * key's 14 px (92.4; no stone, as the table's), the 12 px between the parts and its output named at
 * 13 px, at its widest the CKB a transaction outputs, eleven whole digits in the summary form,
 * "output 99,999,999,999" (WIDEST_TX_OUTPUT_SUMMARY; 21 characters, 163.8), and its unit, " CKB",
 * at the meta step an amount's unit takes (28.8): 297 of a 360 px phone's 328 px line.
 */
const BLOCK_TX_PHONE_HEAD_PX = phoneHead(
  'block transactions',
  textPx(shortChars(SHORT_FORMS.tight.hash), fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(`output ${WIDEST_TX_OUTPUT_SUMMARY}`.length, fontPx('small')) +
    textPx(' CKB'.length, fontPx('meta'))
);

/**
 * The BlockTxs board: one raw row per transaction of a block, cellbase first and set back
 * ("cellbase", 0 → 1, a grey size bar), size and cycles on the block's own round scales.
 */
export function BlockTransactionsTable({
  transactions,
  blockNumber,
}: {
  transactions: readonly Transaction[];
  blockNumber: number;
}) {
  for (const tx of transactions) {
    if (tx.blockNumber !== blockNumber)
      throw new Error(`Transaction ${tx.hash} is not in block ${blockNumber}`);
    if (!Number.isSafeInteger(tx.txSize) || tx.txSize! <= 0)
      throw new Error(`Transaction ${tx.hash} is missing its exact serialized size`);
    if (tx.cyclesStatus === 'complete' ? tx.cycles === null : tx.cycles !== null)
      throw new Error(`Transaction ${tx.hash} has inconsistent cycle evidence`);
    if (tx.outputCapacity === undefined)
      throw new Error(`Transaction ${tx.hash} is missing its output capacity`);
    if (blockNumber !== 0 && !tx.isCellbase && !tx.proposalOrigin)
      throw new Error(`Transaction ${tx.hash} is missing its proposal origin`);
  }
  const sizeScale = resourceScale(transactions.map((tx) => tx.txSize!));
  const cycleScale = resourceScale(
    transactions.flatMap((tx) => (tx.cycles === null ? [] : [tx.cycles]))
  );
  const proposedIn = (tx: Transaction) =>
    blockNumber === 0 ? (
      <span className="text-text-dim">genesis</span>
    ) : tx.isCellbase ? (
      <span className="text-text-dim">cellbase</span>
    ) : (
      <span className="whitespace-nowrap">
        <Link href={`/blocks/${tx.proposalOrigin!.blockNumber}`} look="inline">
          {formatBlockNumber(tx.proposalOrigin!.blockNumber)}
        </Link>
        <span className="text-text-dim">
          {' '}
          +{tx.proposalOrigin!.distance}
          {tx.proposalOrigin!.uncle ? ' · uncle' : ''}
        </span>
      </span>
    );
  const columns: ListColumn<Transaction>[] = [
    {
      key: 'index',
      label: '#',
      // A transaction's index in its block, four digits as written ("9999"), and the gap. It was
      // 32, which held two digits.
      minWidth: columnPx(cell(WIDEST_COUNT_DIGITS) + CELL_GAP_PX),
      render: (tx) => <span className="text-text-dim">{tx.index}</span>,
    },
    {
      key: 'transaction',
      label: 'Transaction',
      // The hash at the row size, 17 characters at 13 px (132.6), and the gap; alone in its cell,
      // so not the tight size. It was 140.
      minWidth: columnPx(textPx(shortChars(SHORT_FORMS.row.hash), fontPx('small')) + CELL_GAP_PX),
      render: (tx) => (
        <Link href={`/tx/${tx.hash}`} className="hover:underline">
          <Hash hash={tx.hash} size="row" copyable={false} className="text-small" />
        </Link>
      ),
    },
    {
      key: 'proposal',
      label: 'Proposed in',
      // The proposing block and how far before it was, "#99,999,999 +10 · uncle" (a proposal is
      // committed 2 to 10 blocks after it; 23 characters at 13 px, 179.4), and the gap. It was
      // 120, which held the block alone.
      minWidth: columnPx(cell(`${WIDEST_BLOCK_NUMBER} +10 · uncle`) + CELL_GAP_PX),
      render: proposedIn,
    },
    {
      key: 'cells',
      label: 'Cells',
      // The most inputs a transaction holds as written, "13568" (review fb-tests 6: it counted four
      // digits while 13,568 44-byte inputs fit a block), the arrow between its 6 px margins, four
      // digits of outputs, "9,999" (97.8 px), and the gap. It was 70.
      minWidth: columnPx(
        cell(String(MAX_TX_INPUTS)) + 6 + cell('→') + 6 + cell(WIDEST_COUNT) + CELL_GAP_PX
      ),
      render: (tx) => (
        <span className="whitespace-nowrap">
          <span className={tx.isCellbase ? 'text-text-dim' : undefined}>
            {tx.isCellbase ? 0 : tx.inputsCount}
          </span>
          <span aria-hidden="true" className="text-text-dim mx-1.5">
            →
          </span>
          <span>{tx.outputsCount.toLocaleString('en-US')}</span>
        </span>
      ),
    },
    {
      key: 'capacity',
      label: 'Output capacity',
      // The CKB a transaction outputs, every decimal: eleven whole digits, the genesis
      // transactions' first issuance ("99,999,999,999.99999999", 23 characters at 13 px, 179.4),
      // and the gap. It was 140.
      minWidth: columnPx(cell(WIDEST_TX_OUTPUT) + CELL_GAP_PX),
      numeric: true,
      render: (tx) => (
        <CkbAmount
          form="column"
          shannons={parseShannons(tx.outputCapacity!, `transaction ${tx.hash} output capacity`)}
          signed={false}
          tone="plain"
          unit={false}
        />
      ),
    },
    {
      key: 'fee',
      label: 'Fee',
      // A fee under 100 CKB, "99.99999999" (85.8), and the gap. It was 104.
      minWidth: columnPx(cell(WIDEST_TX_FEE) + CELL_GAP_PX),
      numeric: true,
      render: (tx) => (
        <CkbAmount
          form="column"
          shannons={parseShannons(tx.fee, `transaction ${tx.hash} fee`)}
          signed={false}
          tone="plain"
          unit={false}
        />
      ),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      header: <ScaleHeader label="Size, bytes" scale={sizeScale} trackWidth={TRACK} />,
      // The 48 px track, the bar's 10 px gap, the scale's figure ("1,500,000", 70.2) and the gap.
      // It was 160.
      minWidth: columnPx(TRACK + BAR_FIGURE_GAP + cell(SIZE_SCALE) + CELL_GAP_PX),
      className: 'whitespace-nowrap',
      render: (tx) => (
        <AlignedBar
          label="Size, bytes"
          scale={sizeScale}
          trackWidth={TRACK}
          segments={[
            {
              key: 'size',
              label: tx.isCellbase ? 'cellbase' : 'transaction',
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
      header: <ScaleHeader label="Cycles" scale={cycleScale} trackWidth={TRACK} />,
      // The 48 px track, the bar's 10 px gap and the scale's figure ("750,000,000", 85.8); the last
      // column, no gap after it. It was 172.
      minWidth: columnPx(TRACK + BAR_FIGURE_GAP + cell(CYCLES_SCALE)),
      className: 'whitespace-nowrap',
      render: (tx) =>
        tx.cycles === null ? (
          <span className="text-text-dim">
            {tx.cyclesStatus === 'failed' ? 'Replay failed' : 'Not yet run'}
          </span>
        ) : (
          <AlignedBar
            label="Cycles"
            scale={cycleScale}
            trackWidth={TRACK}
            segments={[{ key: 'cycles', label: 'cycles', value: tx.cycles, tone: 'data' }]}
          />
        ),
    },
  ];
  return (
    <ListTable
      label="Transactions in this block"
      rows={transactions}
      columns={columns}
      rowKey={(tx) => tx.hash}
      rowClassName={(tx) => (tx.isCellbase ? 'text-text-dim' : '')}
      empty="No transactions."
      // The phone-rows board's row: the hash and its output; where it was proposed; quiet, its
      // index, its cells and its fee (the cards from 560 px keep its bytes and cycles there too).
      mobileRow={(tx, wide) => (
        <PhoneRow
          testId="block-tx-row-mobile"
          head={BLOCK_TX_PHONE_HEAD_PX}
          lines={[
            {
              key: 'identity',
              tone: 'head',
              left: (
                <Link href={`/tx/${tx.hash}`} look="key">
                  <Hash hash={tx.hash} size="tight" copyable={false} className="text-body" />
                </Link>
              ),
              right: named('output', tx.outputCapacity!, 'output capacity', tx),
            },
            {
              key: 'proposal',
              tone: 'body',
              left:
                blockNumber === 0 || tx.isCellbase ? (
                  proposedIn(tx)
                ) : (
                  <Labelled label="proposed in">{proposedIn(tx)}</Labelled>
                ),
            },
            {
              key: 'figures',
              tone: 'quiet',
              left: (
                <Dotted
                  parts={[
                    <Labelled key="index" label="index">
                      {tx.index}
                    </Labelled>,
                    cells(tx),
                    // a cellbase pays no fee
                    !tx.isCellbase && named('fee', tx.fee, 'fee', tx),
                    ...(wide ? resources(tx) : []),
                  ]}
                />
              ),
            },
          ]}
        />
      )}
    />
  );
}
