import Link from '@/components/ui/link';
import { Hash } from '@/components/ui/hash';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { AlignedBar } from '@/components/ui/aligned-bar';
import { ExactCkb } from '@/components/activity-event-row';
import { ScaleHeader, resourceScale } from '@/components/blocks/block-visual';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatBlockNumber } from '@/lib/format-block';
import type { Transaction } from '@/lib/api';

const TRACK = 96;

/** The column budget fits the block page's main column (≈ 968 px at 1440) without scrolling. */
export const BLOCK_TXS_MIN_WIDTH = 32 + 140 + 120 + 70 + 140 + 104 + 160 + 172;

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
  const columns: ListColumn<Transaction>[] = [
    {
      key: 'index',
      label: '#',
      minWidth: 32,
      render: (tx) => <span className="text-text-dim">{tx.index}</span>,
    },
    {
      key: 'transaction',
      label: 'Transaction',
      minWidth: 140,
      render: (tx) => (
        <Link href={`/tx/${tx.hash}`} className="hover:underline">
          <Hash hash={tx.hash} startChars={8} endChars={6} copyable={false} className="text-xs" />
        </Link>
      ),
    },
    {
      key: 'proposal',
      label: 'Proposed in',
      minWidth: 120,
      render: (tx) =>
        blockNumber === 0 ? (
          <span className="text-text-dim">genesis</span>
        ) : tx.isCellbase ? (
          <span className="text-text-dim">cellbase</span>
        ) : (
          <span className="whitespace-nowrap">
            <Link href={`/blocks/${tx.proposalOrigin!.blockNumber}`}>
              {formatBlockNumber(tx.proposalOrigin!.blockNumber)}
            </Link>
            <span className="text-text-dim">
              {' '}
              +{tx.proposalOrigin!.distance}
              {tx.proposalOrigin!.uncle ? ' · uncle' : ''}
            </span>
          </span>
        ),
    },
    {
      key: 'cells',
      label: 'Cells',
      minWidth: 70,
      render: (tx) => (
        <span className="whitespace-nowrap">
          <span className={tx.isCellbase ? 'text-text-dim' : undefined}>
            {tx.isCellbase ? 0 : tx.inputsCount}
          </span>
          <span aria-hidden="true" className="text-text-dim mx-1.5">
            →
          </span>
          <span>{tx.outputsCount}</span>
        </span>
      ),
    },
    {
      key: 'capacity',
      label: 'Output capacity',
      minWidth: 140,
      numeric: true,
      render: (tx) => (
        <ExactCkb
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
      minWidth: 104,
      numeric: true,
      render: (tx) => (
        <ExactCkb
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
      minWidth: 160,
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
      minWidth: 172,
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
    />
  );
}
