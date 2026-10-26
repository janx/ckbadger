'use client';

import Link from '@/components/ui/link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { HexDisplay } from '@/components/ui/hex-display';
import { ExactCkb, activityHeadline } from '@/components/activity-event-row';
import { ResourceBar, resourceScale } from '@/components/blocks/block-visual';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo } from '@/lib/utils';
import type { Transaction } from '@/lib/api';

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
    if (!showBlock && !tx.isCellbase && !tx.proposalOrigin)
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
      minWidth: 190,
      render: (tx) => (
        <div className="space-y-1">
          <Link href={`/tx/${tx.hash}`} className="hover:underline">
            <HexDisplay value={tx.hash} startChars={10} endChars={6} />
          </Link>
          {showBlock && (
            <Link
              href={`/blocks/${tx.blockNumber}`}
              className="text-text-dim hover:text-jade block text-[11px]"
            >
              #{tx.blockNumber.toLocaleString('en-US')}
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
      minWidth: 105,
      render: (tx) =>
        showBlock ? (
          <span className="text-text-dim">{formatTimeAgo(tx.timestamp)}</span>
        ) : tx.isCellbase ? (
          <span className="text-text-dim">—</span>
        ) : (
          <div className="space-y-1">
            <Link
              href={`/blocks/${tx.proposalOrigin!.blockNumber}`}
              className="text-gold hover:underline"
            >
              #{tx.proposalOrigin!.blockNumber.toLocaleString('en-US')}
            </Link>
            <span className="text-text-dim block text-[11px]">
              {tx.proposalOrigin!.distance} blocks earlier
              {tx.proposalOrigin!.uncle ? ' · uncle' : ''}
            </span>
          </div>
        ),
    },
    {
      key: 'cells',
      label: 'Cells',
      minWidth: 75,
      numeric: true,
      render: (tx) => (
        <span>
          <span>{tx.inputsCount}</span>
          <span className="text-text-dim mx-2">→</span>
          <span>{tx.outputsCount}</span>
        </span>
      ),
    },
    {
      key: 'value',
      label: showBlock ? 'Value' : 'Output capacity',
      minWidth: 180,
      numeric: true,
      render: (tx) => (
        <ExactCkb
          shannons={parseShannons(
            showBlock ? tx.value : tx.outputCapacity!,
            `transaction ${tx.hash} ${showBlock ? 'value' : 'output capacity'}`
          )}
          signed={false}
          tone="plain"
        />
      ),
    },
    {
      key: 'fee',
      label: 'Fee',
      minWidth: 150,
      numeric: true,
      render: (tx) => (
        <ExactCkb
          shannons={parseShannons(tx.fee, `transaction ${tx.hash} fee`)}
          signed={false}
          tone="plain"
        />
      ),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      minWidth: 155,
      numeric: true,
      render: (tx) => <ResourceBar label="Size, bytes" value={tx.txSize!} scale={sizeScale} />,
    },
    {
      key: 'cycles',
      label: 'Cycles',
      minWidth: 180,
      numeric: true,
      render: (tx) =>
        tx.cycles === null ? (
          <span className="text-text-dim">
            {tx.cyclesStatus === 'failed' ? 'Replay failed' : 'Not yet run'}
          </span>
        ) : (
          <ResourceBar label="Cycles" value={tx.cycles} scale={cycleScale} />
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
