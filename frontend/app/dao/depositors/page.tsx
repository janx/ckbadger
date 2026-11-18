'use client';

import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { DetailLayout } from '@/components/ui/detail-layout';
import Link from '@/components/ui/link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { formatShareTick, percentHundredths } from '@/components/ui/share-bar';
import { AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { resourceScale } from '@/components/blocks/block-visual';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { api, type DaoDepositorRow, type DaoDepositorsPage as RankedPage } from '@/lib/api';
import { parseShannons } from '@/lib/ckb-decimal';
import { CkbAmount } from '@/components/ui/ckb-amount';
import type { AmountForm } from '@/lib/amount-form';
import { BlockLink } from '@/components/ui/block-link';
import { retryTransientOnly } from '@/lib/query-client';
import { cn } from '@/lib/utils';
import { formatCount } from '@/lib/format-count';
import { ShortId } from '@/components/ui/short-id';
import type { ShortSize } from '@/lib/shorten';

/** Depositors a page from md up; twenty below it (R2). */
const PAGE = 50;

/** A depositor's CKB in its line's ink: a column in the table, a summary on a phone row (R5). */
function Ckb({ shannons, form }: { shannons: string; form: AmountForm }) {
  return (
    <CkbAmount
      form={form}
      shannons={parseShannons(shannons, 'DAO depositor capacity')}
      signed={false}
      tone="inherit"
      unit={false}
    />
  );
}

/**
 * A depositor by address, or by its lock hash when the lock encodes to no address (L1), at the
 * form's size (U-09). In the table it is the row's key (table-rows T5): `className` sets it at body
 * on the row's 18 px line.
 */
function Depositor({
  row,
  size,
  className,
}: {
  row: DaoDepositorRow;
  size: ShortSize;
  className?: string;
}) {
  const id = row.address ?? row.lockScriptHash;
  return (
    <Link
      href={`/address/${id}`}
      className={cn('text-text-bright hover:underline', className)}
      title={id}
    >
      <ShortId value={id} kind={row.address ? 'address' : 'hash'} size={size} />
    </Link>
  );
}

/** The share column's bar track and the width of its number (as the token holders' table). */
const SHARE_TRACK = 48;
const SHARE_TEXT = 48;

/** R7: the one scale of the page's share bars — its rounded maximum — in both forms. */
const shareScale = (rows: readonly DaoDepositorRow[]) =>
  resourceScale(rows.map((d) => percentHundredths(d.sharePercent)));

const columns = (rows: readonly DaoDepositorRow[]): ListColumn<DaoDepositorRow>[] => [
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
    render: (d) => <Depositor row={d} size="row" className="text-body leading-[18px]" />,
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
    render: (d) => <Ckb form="column" shannons={d.capacity} />,
  },
  {
    key: 'share',
    label: 'Share',
    minWidth: SHARE_TRACK + 10 + SHARE_TEXT + 16,
    header: (
      <ScaleHeader
        label="Share"
        scale={shareScale(rows)}
        trackWidth={SHARE_TRACK}
        format={formatShareTick}
        figureWidth={SHARE_TEXT}
      />
    ),
    render: (d) => (
      <AlignedBar
        label="Share of deposits"
        scale={shareScale(rows)}
        trackWidth={SHARE_TRACK}
        minFill={2}
        segments={[
          { key: 'share', label: 'share', value: percentHundredths(d.sharePercent), tone: 'dao' },
        ]}
        display={<span className="text-text-dim">{d.sharePercent}%</span>}
        displayWidth={SHARE_TEXT}
      />
    ),
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
    <div className="min-h-screen">
      <main className="container mx-auto pb-16 pt-6">
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
                  <BlockLink number={moved.from} />
                  ): this page continues the ranking at block <BlockLink number={moved.to} />, so a
                  depositor whose rank changed may repeat or be missing.
                </p>
              )}
              <ListTable
                label="DAO depositors"
                rows={page.data}
                columns={columns(page.data)}
                rowKey={(d) => d.lockScriptHash}
                empty="No live deposits at this tip."
                // Board L5: the holder row — rank, depositor and CKB deposited on one line, the
                // share as a bar on the table's scale (R7) with its number and the deposit count
                // on the second.
                mobileRow={(d, wide) => (
                  <HolderPhoneRow
                    rank={d.rank}
                    party={<Depositor row={d} size="tight" />}
                    amount={
                      <span className="whitespace-nowrap">
                        <Ckb form="summary" shannons={d.capacity} />{' '}
                        <span className="text-text-dim">CKB</span>
                      </span>
                    }
                    share={`${d.sharePercent}%`}
                    note={formatCount(d.depositCount, 'deposit')}
                    bar={{
                      value: percentHundredths(d.sharePercent),
                      scale: shareScale(page.data),
                      color: CLASS_COLORS.dao,
                    }}
                    axis={page.data[0] === d ? formatShareTick(shareScale(page.data)) : undefined}
                    wide={wide}
                  />
                )}
              />
              {/* The site's one pager (B5): it draws nothing for one page. The buttons keep this
                  page's words, the size of a page on Next (R2). */}
              <CursorPagination
                className="pt-4"
                total={page.total}
                totalLabel="depositors"
                page={cursors.length}
                pageSize={pageSize}
                currentCount={page.data.length}
                hasMore={page.hasMore}
                hasPrevious={cursors.length > 1}
                onNext={() => {
                  if (page.nextCursor === null)
                    throw new Error('DAO depositors: a page with more ranks has no next cursor');
                  setCursors((c) => [...c, page.nextCursor!]);
                }}
                onPrevious={() => setCursors((c) => c.slice(0, -1))}
                previousLabel="← Previous"
                nextLabel={`Next ${pageSize} →`}
              />
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
