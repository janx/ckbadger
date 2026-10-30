'use client';

import Link from '@/components/ui/link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { ExactCkb, activityHeadline } from '@/components/activity-event-row';
import { ResourceBar, resourceScale } from '@/components/blocks/block-visual';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo } from '@/lib/utils';
import type { Transaction } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';

interface Props {
  transactions: readonly Transaction[];
  label?: string;
  showBlock?: boolean;
}

export function TransactionTable({
  transactions,
  label = 'Committed transactions',
  showBlock = true,
}: Props) {
  for (const tx of transactions) {
    if (!Number.isSafeInteger(tx.txSize) || tx.txSize! <= 0)
      throw new Error(`Transaction ${tx.hash} is missing its exact serialized size`);
    if (tx.cyclesStatus === 'complete' ? tx.cycles === null : tx.cycles !== null)
      throw new Error(`Transaction ${tx.hash} has inconsistent cycle evidence`);
    if (!showBlock && tx.outputCapacity === undefined)
      throw new Error(`Transaction ${tx.hash} is missing its output capacity`);
    if (!showBlock && tx.blockNumber !== 0 && !tx.isCellbase && !tx.proposalOrigin)
      throw new Error(`Transaction ${tx.hash} is missing its proposal origin`);
    if (showBlock && !tx.isCellbase && !tx.activity)
      throw new Error(`Transaction ${tx.hash} is missing its activity`);
  }
  const sizeScale = resourceScale(transactions.map((tx) => tx.txSize!));
  const cycleScale = resourceScale(
    transactions.flatMap((tx) => (tx.cycles === null ? [] : [tx.cycles]))
  );
  const columns: ListColumn<Transaction>[] = [
    {
      key: 'transaction',
      label: 'Transaction',
      minWidth: showBlock ? 190 : 176,
      render: (tx) => (
        <div className="space-y-1">
          <Link href={`/tx/${tx.hash}`} className="hover:underline">
            <Hash
              hash={tx.hash}
              startChars={12}
              endChars={6}
              copyable={false}
              className="text-xs"
            />
          </Link>
          {showBlock && (
            <Link
              href={`/blocks/${tx.blockNumber}`}
              className="text-text-dim hover:text-jade block text-[11px]"
            >
              {formatBlockNumber(tx.blockNumber)}
            </Link>
          )}
          {tx.isCellbase ? (
            <span className="text-text-dim block text-[11px]">◆ Cellbase</span>
          ) : showBlock ? (
            <span className="text-text-dim block text-[11px]">
              {activityHeadline(tx.activity!)}
            </span>
          ) : null}
        </div>
      ),
    },
    {
      key: showBlock ? 'time' : 'proposal',
      label: showBlock ? 'Age' : 'Proposed in',
      minWidth: showBlock ? 105 : 108,
      render: (tx) =>
        showBlock ? (
          <span className="text-text-dim">{formatTimeAgo(tx.timestamp)}</span>
        ) : tx.blockNumber === 0 ? (
          <span className="text-text-dim">Genesis</span>
        ) : tx.isCellbase ? (
          <span className="text-text-dim">—</span>
        ) : (
          <div className="space-y-1">
            <Link
              href={`/blocks/${tx.proposalOrigin!.blockNumber}`}
              className="text-gold hover:underline"
            >
              {formatBlockNumber(tx.proposalOrigin!.blockNumber)}
            </Link>
            <span className="text-text-dim block text-[11px]">
              {tx.proposalOrigin!.distance} blocks back
              {tx.proposalOrigin!.uncle ? ' · uncle' : ''}
            </span>
          </div>
        ),
    },
    {
      key: 'cells',
      label: 'Cells',
      minWidth: showBlock ? 75 : 64,
      numeric: true,
      render: (tx) => (
        <span>
          <span>{tx.isCellbase ? 0 : tx.inputsCount}</span>
          <span className="text-text-dim mx-2">→</span>
          <span>{tx.outputsCount}</span>
        </span>
      ),
    },
    {
      key: 'value',
      label: showBlock ? 'Value' : 'Output capacity',
      minWidth: showBlock ? 180 : 164,
      numeric: true,
      render: (tx) => (
        <ExactCkb
          shannons={parseShannons(
            showBlock ? tx.value : tx.outputCapacity!,
            `transaction ${tx.hash} ${showBlock ? 'value' : 'output capacity'}`
          )}
          signed={false}
          tone="plain"
          wrap={!showBlock}
        />
      ),
    },
    {
      key: 'fee',
      label: 'Fee',
      minWidth: showBlock ? 150 : 128,
      numeric: true,
      render: (tx) => (
        <ExactCkb
          shannons={parseShannons(tx.fee, `transaction ${tx.hash} fee`)}
          signed={false}
          tone="plain"
          wrap={!showBlock}
        />
      ),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      minWidth: showBlock ? 155 : 106,
      numeric: true,
      render: (tx) => (
        <ResourceBar
          label="Size, bytes"
          value={tx.txSize!}
          scale={sizeScale}
          stacked={!showBlock}
        />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      minWidth: showBlock ? 180 : 116,
      numeric: true,
      render: (tx) =>
        tx.cycles === null ? (
          <span className="text-text-dim">
            {tx.cyclesStatus === 'failed' ? 'Replay failed' : 'Not yet run'}
          </span>
        ) : (
          <ResourceBar label="Cycles" value={tx.cycles} scale={cycleScale} stacked={!showBlock} />
        ),
    },
  ];
  if (!showBlock)
    columns.unshift({
      key: 'index',
      label: '#',
      minWidth: 36,
      numeric: true,
      render: (tx) => tx.index,
    });
  return (
    <ListTable
      label={label}
      rows={transactions}
      columns={columns}
      rowKey={(tx) => tx.hash}
      empty="No transactions."
    />
  );
}
