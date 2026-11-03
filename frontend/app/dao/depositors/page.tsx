'use client';

import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import Link from '@/components/ui/link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { usePageSize } from '@/hooks/usePageSize';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { shareBarWidthFromPercent } from '@/components/ui/share-bar';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { api, type DaoDepositorRow, type DaoDepositorsPage as RankedPage } from '@/lib/api';
import { parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatBlockNumber } from '@/lib/format-block';
import { BlockLink } from '@/components/ui/block-link';
import { retryTransientOnly } from '@/lib/query-client';
import { cn } from '@/lib/utils';

/** Depositors a page from md up; twenty below it (R2). */
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

/**
 * A depositor by address, or by its lock hash when the lock encodes to no address (L1). In the
 * table it is the row's key (table-rows T5): `className` sets it at body on the row's 18 px line.
 */
function Depositor({ row, className }: { row: DaoDepositorRow; className?: string }) {
  const id = row.address ?? row.lockScriptHash;
  return (
    <Link
      href={`/address/${id}`}
      className={cn('text-text-bright hover:underline', className)}
      title={id}
    >
      {id.slice(0, 12)}…{id.slice(-7)}
    </Link>
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
    render: (d) => <Depositor row={d} className="text-body leading-[18px]" />,
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
 * Every current (phase-0) depositor, ranked by CKB deposited (DAO-12, R2b-a 42). Round 3 (L1, J2):
 * the cursor is the rank served so far, not bound to a tip; each page says the tip it was ranked
 * at, and a page ranked after the tip moved says so (a depositor whose rank changed between pages
 * can repeat or be missing — accepted by J2).
 */
export default function DaoDepositorsPage() {
  const [cursors, setCursors] = useState<(string | undefined)[]>([undefined]);
  const cursor = cursors[cursors.length - 1];
  const pageSize = usePageSize(PAGE);
  const query = useQuery({
    queryKey: ['dao-depositors', cursor, pageSize],
    queryFn: () => api.getDaoDepositors({ limit: pageSize, cursor }),
    retry: retryTransientOnly,
  });
  const page = query.data;
  const client = useQueryClient();
  const first = client.getQueryData<RankedPage>(['dao-depositors', undefined, pageSize]);
  const moved =
    page && first && cursor !== undefined && first.anchor.blockNumber !== page.anchor.blockNumber
      ? { from: first.anchor.blockNumber, to: page.anchor.blockNumber }
      : null;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 pb-16 pt-6">
        <DetailLayout
          kind="index"
          title="Nervos DAO depositors"
          actions={
            <Link
              href="/dao"
              className="text-text hover:text-text-bright text-small font-mono hover:underline"
            >
              ← Nervos DAO
            </Link>
          }
          identity={
            page ? (
              <span data-role="ranked-at">
                {page.total.toLocaleString('en-US')} depositors ranked by CKB deposited at block{' '}
                <BlockLink number={page.anchor.blockNumber} />
              </span>
            ) : undefined
          }
        >
          {query.isError ? (
            <div role="alert" className="text-rouge text-small">
              DAO depositors could not be loaded: {String(query.error)}
            </div>
          ) : !page ? (
            <div className="text-text-dim text-small py-6">Ranking every depositor…</div>
          ) : (
            <>
              {moved && (
                <p
                  data-testid="ranking-moved"
                  className="text-text-dim text-small mb-3 max-w-[80ch]"
                >
                  The chain tip moved since the first page (ranked at block{' '}
                  {formatBlockNumber(moved.from)}): this page continues the ranking at block{' '}
                  {formatBlockNumber(moved.to)}, so a depositor whose rank changed may repeat or be
                  missing.
                </p>
              )}
              <ListTable
                label="DAO depositors"
                rows={page.data}
                columns={COLUMNS}
                rowKey={(d) => d.lockScriptHash}
                empty="No live deposits at this tip."
                // Board L5: the holder row — rank, depositor and CKB deposited on one line, the
                // share as a bar (10 % = full) with its number and the deposit count on the second.
                mobileRow={(d, wide) => (
                  <HolderPhoneRow
                    rank={d.rank}
                    party={<Depositor row={d} />}
                    amount={
                      <span className="whitespace-nowrap">
                        <Ckb shannons={d.capacity} /> <span className="text-text-dim">CKB</span>
                      </span>
                    }
                    share={`${d.sharePercent}% · ${d.depositCount.toLocaleString('en-US')} deposit${
                      d.depositCount === 1 ? '' : 's'
                    }`}
                    barWidth={shareBarWidthFromPercent(d.sharePercent)}
                    barColor={CLASS_COLORS.dao}
                    wide={wide}
                  />
                )}
              />
              <div className="flex justify-between gap-3 pt-4">
                {cursors.length > 1 ? (
                  <button
                    type="button"
                    onClick={() => setCursors((c) => c.slice(0, -1))}
                    className="border-base-border text-text hover:text-text-bright text-small h-9 rounded-md border px-4"
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
                    className="border-base-border text-text hover:text-text-bright text-small h-9 rounded-md border px-4"
                  >
                    Next {pageSize} →
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
