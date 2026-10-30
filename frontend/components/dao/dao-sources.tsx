'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { ActivityRow } from '@/components/activity/activity-row';
import { TransactionTable } from '@/components/transaction/transaction-table';

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
          {tab === 'activities' ? (
            activities.data?.data.map((row) => (
              <ActivityRow key={`${row.txHash}:${row.txIndex}`} activity={row} />
            ))
          ) : (
            <TransactionTable transactions={transactions.data?.data ?? []} />
          )}
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
