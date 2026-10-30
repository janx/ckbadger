'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import type { GlobalActivity, Transaction } from '@/lib/api';
import { formatUtc8DateTime } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';

function actionLabel(actions: GlobalActivity['protocolActions']): string {
  const dao = actions.find((action) => action.protocol === 'dao');
  if (!dao) throw new Error('DAO-filtered source row has no DAO protocol action');
  return dao.action.replaceAll('_', ' ');
}

function ActivityRow({ row }: { row: GlobalActivity }) {
  return (
    <tr className="border-base-border/50 border-b">
      <td className="py-2 capitalize text-[#f2c55c]">{actionLabel(row.protocolActions)}</td>
      <td>
        <Link href={`/tx/${row.txHash}`} className="text-text-bright font-mono hover:underline">
          {row.txHash.slice(0, 12)}…{row.txHash.slice(-8)}
        </Link>
      </td>
      <td>
        <Link
          href={`/blocks/${row.blockNumber}`}
          className="text-text-dim font-mono hover:underline"
        >
          {formatBlockNumber(row.blockNumber)}
        </Link>
      </td>
      <td className="text-text-dim text-right">{formatUtc8DateTime(row.timestamp)}</td>
    </tr>
  );
}

function TransactionRow({ row }: { row: Transaction }) {
  const step = row.activity ? actionLabel(row.activity.protocolActions) : 'DAO transaction';
  return (
    <tr className="border-base-border/50 border-b">
      <td className="py-2 capitalize text-[#f2c55c]">{step}</td>
      <td>
        <Link href={`/tx/${row.hash}`} className="text-text-bright font-mono hover:underline">
          {row.hash.slice(0, 12)}…{row.hash.slice(-8)}
        </Link>
      </td>
      <td>
        <Link
          href={`/blocks/${row.blockNumber}`}
          className="text-text-dim font-mono hover:underline"
        >
          {formatBlockNumber(row.blockNumber)}
        </Link>
      </td>
      <td className="text-text-dim text-right">{formatUtc8DateTime(row.timestamp)}</td>
    </tr>
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
    <section className="border-base-border border-t pt-5" aria-label="DAO source rows">
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <div className="flex gap-4 text-[15px]">
          {(['activities', 'transactions'] as const).map((choice) => (
            <button
              key={choice}
              type="button"
              onClick={() => {
                setTab(choice);
                setCursor(undefined);
              }}
              className={`${tab === choice ? 'border-b border-[#f2c55c] text-[#f2c55c]' : 'text-text-dim'} capitalize`}
            >
              {choice}
            </button>
          ))}
        </div>
        <span className="text-text-dim font-mono text-[11px]">
          deposits · withdrawal requests · completed withdrawals
        </span>
      </div>
      {result.isError ? (
        <div role="alert" className="text-rouge text-xs">
          DAO {tab} could not be loaded: {String(result.error)}
        </div>
      ) : result.isLoading ? (
        <div className="text-text-dim py-7 text-xs">Loading DAO {tab}…</div>
      ) : !page?.data.length ? (
        <div className="text-text-dim py-7 text-xs">No DAO {tab} at this indexed tip.</div>
      ) : (
        <>
          <div className="overflow-x-auto">
            <table className="w-full min-w-[650px] text-left text-[12px]">
              <thead className="text-text-dim border-base-border border-b">
                <tr>
                  <th className="py-2">DAO step</th>
                  <th>Transaction</th>
                  <th>Block</th>
                  <th className="text-right">UTC+8</th>
                </tr>
              </thead>
              <tbody>
                {tab === 'activities'
                  ? activities.data?.data.map((row) => (
                      <ActivityRow key={`${row.txHash}:${row.txIndex}`} row={row} />
                    ))
                  : transactions.data?.data.map((row) => (
                      <TransactionRow key={row.hash} row={row} />
                    ))}
              </tbody>
            </table>
          </div>
          {page.hasMore && page.nextCursor && (
            <button
              type="button"
              className="mt-3 text-xs text-[#f2c55c] hover:underline"
              onClick={() => setCursor(page.nextCursor ?? undefined)}
            >
              Older DAO {tab} →
            </button>
          )}
        </>
      )}
    </section>
  );
}
