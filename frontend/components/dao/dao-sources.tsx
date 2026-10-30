'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type Transaction } from '@/lib/api';
import Link from '@/components/ui/link';
import { Hash } from '@/components/ui/hash';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { ActivityRow } from '@/components/activity/activity-row';
import { ExactCkb, activityHeadline, protocolActionLabel } from '@/components/activity-event-row';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
import { splitCkbAmount } from '@/lib/ckb-decimal';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8DateTime } from '@/lib/utils';

/** Every DAO step of a transaction, read once through the shared metadata reader. */
function daoSteps(tx: Transaction) {
  if (!tx.activity) throw new Error(`DAO transaction ${tx.hash} is missing its activity`);
  return tx.activity.protocolActions
    .filter((pa) => pa.protocol === 'dao')
    .map((pa) => ({ label: protocolActionLabel(pa), meta: protocolActionMeta(pa) }));
}

/** One line per DAO step; a transaction in the DAO bucket by participant alone says what it did. */
function StepLines({
  tx,
  render,
}: {
  tx: Transaction;
  render: (step: ReturnType<typeof daoSteps>[number]) => React.ReactNode;
}) {
  const steps = daoSteps(tx);
  if (steps.length === 0) return <span className="text-text-dim">—</span>;
  return (
    <span className="flex flex-col gap-0.5">
      {steps.map((step, i) => (
        <span key={i}>{render(step)}</span>
      ))}
    </span>
  );
}

const DAO_TX_COLUMNS: ListColumn<Transaction>[] = [
  {
    key: 'transaction',
    label: 'Transaction',
    minWidth: 170,
    render: (tx) => (
      <Link href={`/tx/${tx.hash}`} className="hover:underline">
        <Hash hash={tx.hash} startChars={10} endChars={6} copyable={false} className="text-xs" />
      </Link>
    ),
  },
  {
    key: 'block',
    label: 'Block · UTC+8',
    minWidth: 200,
    render: (tx) => (
      <span>
        <Link href={`/blocks/${tx.blockNumber}`} className="hover:underline">
          {formatBlockNumber(tx.blockNumber)}
        </Link>
        <span className="text-text-dim"> · {formatUtc8DateTime(tx.timestamp).slice(0, 16)}</span>
      </span>
    ),
  },
  {
    key: 'step',
    label: 'DAO step',
    minWidth: 170,
    render: (tx) =>
      daoSteps(tx).length === 0 ? (
        <span className="text-text-dim">{activityHeadline(tx.activity!)}</span>
      ) : (
        <StepLines tx={tx} render={(step) => step.label} />
      ),
  },
  {
    key: 'deposit',
    label: 'Deposit',
    minWidth: 190,
    numeric: true,
    render: (tx) => (
      <StepLines
        tx={tx}
        render={(step) => <ExactCkb shannons={step.meta.capacity!} signed={false} tone="plain" />}
      />
    ),
  },
  {
    key: 'compensation',
    label: 'Compensation',
    minWidth: 170,
    numeric: true,
    render: (tx) => (
      <StepLines
        tx={tx}
        render={(step) =>
          step.meta.compensation === null ? (
            <span className="text-text-dim">—</span>
          ) : (
            <GoldCkb shannons={step.meta.compensation} />
          )
        }
      />
    ),
  },
  {
    key: 'parties',
    label: 'Parties',
    minWidth: 64,
    numeric: true,
    render: (tx) => <span className="text-text-dim">{tx.activity!.participants.length}</span>,
  },
];

/** Compensation in the DAO gold, digits included (the shared amount sets its own ink). */
function GoldCkb({ shannons }: { shannons: bigint }) {
  const parts = splitCkbAmount(shannons);
  return (
    <span data-role="compensation" className="text-gold whitespace-nowrap tabular-nums">
      {parts.integer}
      <span className="opacity-75">{parts.fraction}</span>
      <span className="opacity-40">{parts.ghost}</span>
      <span className="text-[11px] opacity-70"> CKB</span>
    </span>
  );
}

/** The DaoTxs board's table: what each DAO transaction did, its amounts and how many took part. */
function DaoTransactionTable({ transactions }: { transactions: readonly Transaction[] }) {
  return (
    <ListTable
      label="DAO transactions"
      rows={transactions}
      columns={DAO_TX_COLUMNS}
      rowKey={(tx) => tx.hash}
      empty="No DAO transactions."
    />
  );
}

export function DaoSources() {
  const [tab, setTab] = useState<'activities' | 'transactions'>('activities');
  const [cursor, setCursor] = useState<string | undefined>();
  const activities = useQuery({
    queryKey: ['dao-source-activities', cursor],
    queryFn: () => api.getGlobalActivities({ filter: 'dao', limit: 12, cursor }),
    enabled: tab === 'activities',
  });
  const transactions = useQuery({
    queryKey: ['dao-source-transactions', cursor],
    queryFn: () => api.getTransactions({ activityType: 'dao', limit: 12, cursor }),
    enabled: tab === 'transactions',
  });
  const result = tab === 'activities' ? activities : transactions;
  const page = result.data;
  return (
    <section className="min-w-0" aria-label="DAO source rows">
      <div className="border-base-border-subtle flex flex-wrap items-center gap-x-7 border-b">
        <div role="tablist" aria-label="What happened" className="flex gap-7">
          {(['activities', 'transactions'] as const).map((choice) => (
            <button
              key={choice}
              type="button"
              role="tab"
              aria-selected={tab === choice}
              onClick={() => {
                setTab(choice);
                setCursor(undefined);
              }}
              className={`-mb-px h-11 border-b-2 text-[15px] font-medium capitalize ${tab === choice ? 'border-jade text-text-bright' : 'text-text-dim hover:text-text border-transparent'}`}
            >
              {choice}
            </button>
          ))}
        </div>
        <span className="text-text-dim basis-full pb-2 text-[12px] sm:ml-auto sm:basis-auto sm:pb-0">
          deposits, withdrawal requests and withdrawals · newest first
        </span>
      </div>
      {result.isError ? (
        <div role="alert" className="text-rouge py-4 text-xs">
          DAO {tab} could not be loaded: {String(result.error)}
        </div>
      ) : result.isLoading ? (
        <div className="text-text-dim py-7 text-xs">Loading DAO {tab}…</div>
      ) : !page?.data.length ? (
        <div className="text-text-dim py-7 text-xs">No DAO {tab} at this indexed tip.</div>
      ) : (
        <>
          {tab === 'activities' ? (
            activities.data?.data.map((row) => (
              <ActivityRow key={`${row.txHash}:${row.txIndex}`} activity={row} />
            ))
          ) : (
            <DaoTransactionTable transactions={transactions.data?.data ?? []} />
          )}
          {page.hasMore && page.nextCursor && (
            <div className="flex justify-end pt-3.5">
              <button
                type="button"
                className="border-base-border text-text hover:text-text-bright h-9 rounded-md border px-4 text-xs"
                onClick={() => setCursor(page.nextCursor ?? undefined)}
              >
                Older DAO {tab} →
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
