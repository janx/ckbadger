'use client';

import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import Link from '@/components/ui/link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { ApiRequestError, api, type DaoDepositorRow } from '@/lib/api';
import { parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatBlockNumber } from '@/lib/format-block';

const PAGE = 50;

function Ckb({ shannons }: { shannons: string }) {
  const parts = splitCkbAmount(parseShannons(shannons, 'DAO depositor capacity'));
  return (
    <span className="whitespace-nowrap tabular-nums">
      {parts.integer}
      <span className="text-text-dim">{parts.fraction}</span>
      <span className="text-text-ghost">{parts.ghost}</span>
    </span>
  );
}

const COLUMNS: ListColumn<DaoDepositorRow>[] = [
  {
    key: 'rank',
    label: '#',
    minWidth: 56,
    render: (d) => <span className="text-text-dim">{d.rank.toLocaleString('en-US')}</span>,
  },
  {
    key: 'depositor',
    label: 'Depositor',
    minWidth: 260,
    render: (d) => (
      <Link href={`/address/${d.address}`} className="text-text-bright hover:underline">
        {d.address.slice(0, 12)}…{d.address.slice(-7)}
      </Link>
    ),
  },
  {
    key: 'deposits',
    label: 'Deposits',
    minWidth: 80,
    numeric: true,
    render: (d) => d.depositCount.toLocaleString('en-US'),
  },
  {
    key: 'capacity',
    label: 'CKB deposited',
    minWidth: 220,
    numeric: true,
    render: (d) => <Ckb shannons={d.capacity} />,
  },
  {
    key: 'share',
    label: 'Share',
    minWidth: 70,
    numeric: true,
    render: (d) => <span className="text-text-dim">{d.sharePercent}%</span>,
  },
];

/**
 * Every current (phase-0) depositor, ranked by CKB deposited (DAO-12, R2b-a 42). The cursor is bound
 * to the tip the ranking was made at; when the tip moves the API answers 400 and the page says so
 * and starts again from rank 1 rather than mixing two rankings.
 */
export default function DaoDepositorsPage() {
  const [cursors, setCursors] = useState<(string | undefined)[]>([undefined]);
  const cursor = cursors[cursors.length - 1];
  const query = useQuery({
    queryKey: ['dao-depositors', cursor],
    queryFn: () => api.getDaoDepositors({ limit: PAGE, cursor }),
    retry: false,
  });
  const page = query.data;
  const client = useQueryClient();
  // The tip moved: every cached page belongs to the old ranking, so rank again from 1.
  const restart = () => {
    client.removeQueries({ queryKey: ['dao-depositors'] });
    setCursors([undefined]);
  };
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        <DetailLayout
          kind="index"
          title="Nervos DAO depositors"
          actions={
            <Link
              href="/dao"
              className="text-text hover:text-text-bright font-mono text-[12px] hover:underline"
            >
              ← Nervos DAO
            </Link>
          }
          identity={
            page ? (
              <span data-role="ranked-at">
                {page.total.toLocaleString('en-US')} depositors ranked by CKB deposited at block{' '}
                {formatBlockNumber(page.anchor.blockNumber)}
              </span>
            ) : undefined
          }
        >
          {query.isError ? (
            <div role="alert" className="text-rouge space-y-2 text-xs">
              <div>
                {query.error instanceof ApiRequestError && query.error.status === 400 && cursor
                  ? `The ranking moved with the chain tip: ${query.error.apiMessage}`
                  : `DAO depositors could not be loaded: ${String(query.error)}`}
              </div>
              {cursor && (
                <button
                  type="button"
                  onClick={restart}
                  className="border-base-border text-text hover:text-text-bright h-8 rounded-md border px-3"
                >
                  Start again from rank 1
                </button>
              )}
            </div>
          ) : !page ? (
            <div className="text-text-dim py-6 text-xs">Ranking every depositor…</div>
          ) : (
            <>
              <ListTable
                label="DAO depositors"
                rows={page.data}
                columns={COLUMNS}
                rowKey={(d) => d.lockScriptHash}
                empty="No live deposits at this tip."
              />
              <div className="flex justify-between gap-3 pt-4">
                {cursors.length > 1 ? (
                  <button
                    type="button"
                    onClick={() => setCursors((c) => c.slice(0, -1))}
                    className="border-base-border text-text hover:text-text-bright h-9 rounded-md border px-4 text-xs"
                  >
                    ← Previous
                  </button>
                ) : (
                  <span />
                )}
                {page.hasMore && page.nextCursor && (
                  <button
                    type="button"
                    onClick={() => setCursors((c) => [...c, page.nextCursor ?? undefined])}
                    className="border-base-border text-text hover:text-text-bright h-9 rounded-md border px-4 text-xs"
                  >
                    Next {PAGE} →
                  </button>
                )}
              </div>
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
