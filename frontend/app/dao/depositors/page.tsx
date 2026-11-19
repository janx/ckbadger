'use client';

import { useCursorPagination } from '@/hooks/useCursorPagination';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { DetailLayout } from '@/components/ui/detail-layout';
import Link from '@/components/ui/link';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { Pager } from '@/components/ui/pager';
import { usePageSize } from '@/hooks/usePageSize';
import { rankedPageAt } from '@/lib/pagination';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { SHARE_TRACK_PX, formatShareTick, percentHundredths } from '@/components/ui/share-bar';
import { AlignedBar, BAR_FIGURE_GAP, ScaleHeader } from '@/components/ui/aligned-bar';
import { resourceScale } from '@/components/blocks/block-visual';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { api, type DaoDepositorRow, type DaoDepositorsPage as RankedPage } from '@/lib/api';
import { parseShannons } from '@/lib/ckb-decimal';
import { CkbAmount } from '@/components/ui/ckb-amount';
import type { AmountForm } from '@/lib/amount-form';
import { BlockLink } from '@/components/ui/block-link';
import { retryTransientOnly } from '@/lib/query-client';
import { formatCount } from '@/lib/format-count';
import { ShortId } from '@/components/ui/short-id';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import {
  WIDEST_COUNT,
  WIDEST_DAO_DEPOSIT,
  WIDEST_DAO_DEPOSIT_SUMMARY,
  WIDEST_RANK,
  shortChars,
} from '@/lib/list-bounds';

/**
 * Line 1 of a depositor on a phone (the phone-rows board's head()): a rank of two digits in its 22 px
 * box, the depositor's address at the tight size and the key's 14 px (92.4), the 12 px between the
 * parts and the widest deposit in the summary form with its unit, at the key's step, "9,999,999,999
 * CKB" (WIDEST_DAO_DEPOSIT_SUMMARY, 17 characters, 142.8): 269.2 of a 360 px phone's 328 px line.
 */
const DEPOSITOR_PHONE_HEAD_PX = phoneHead(
  'DAO depositors',
  22 +
    textPx(shortChars(SHORT_FORMS.tight.address), fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(`${WIDEST_DAO_DEPOSIT_SUMMARY} CKB`.length, fontPx('body'))
);

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
    // the row's key: the key look's bright ink
    <Link href={`/address/${id}`} look="key" className={className} title={id}>
      <ShortId value={id} kind={row.address ? 'address' : 'hash'} size={size} />
    </Link>
  );
}

/**
 * The share column's bar track and the width of its number (readability F3-e): the track holds its
 * head's ticks, "0" and the widest share tick the head writes, "0.75%" (a page of depositors under
 * 1 % each), whole: 50.4 px (review fb-logic 7: it was measured from "100%", 48); the number,
 * "100.00%" (7 characters at 13 px, 54.6; it was 48, which held "12.34%").
 */
const SHARE_TRACK = SHARE_TRACK_PX;
// the figure's box is its text exactly: the column rounds once, its track, gap and figure together
const SHARE_TEXT = textPx('100.00%'.length, fontPx('small'));

/** R7: the one scale of the page's share bars — its rounded maximum — in both forms. */
const shareScale = (rows: readonly DaoDepositorRow[]) =>
  resourceScale(rows.map((d) => percentHundredths(d.sharePercent)));

const columns = (rows: readonly DaoDepositorRow[]): ListColumn<DaoDepositorRow>[] => [
  {
    key: 'rank',
    label: '#',
    // A rank under a million, "999,999" (7 characters at 13 px, 54.6), and the gap. It was 56.
    minWidth: columnPx(textPx(WIDEST_RANK.length, fontPx('small')) + CELL_GAP_PX),
    render: (d) => <span className="text-text-dim">{d.rank.toLocaleString('en-US')}</span>,
  },
  {
    key: 'depositor',
    label: 'Depositor',
    // The key at 14 px: an address at the row size (13 characters) or, for a lock no address
    // encodes, its lock hash at the row size, "0x12345678…abcdef" (17, 142.8, the wider), and the
    // gap. It was 260.
    minWidth: columnPx(
      textPx(
        Math.max(shortChars(SHORT_FORMS.row.address), shortChars(SHORT_FORMS.row.hash)),
        fontPx('body')
      ) + CELL_GAP_PX
    ),
    render: (d) => <Depositor row={d} size="row" className="text-body leading-row" />,
  },
  {
    key: 'deposits',
    label: 'Deposits',
    // Its head, "Deposits" (8 characters at 12 px, 57.6), wider than a count of four digits,
    // "9,999" (39), and the gap. It was 80.
    minWidth: columnPx(
      Math.max(
        textPx('Deposits'.length, fontPx('meta')),
        textPx(WIDEST_COUNT.length, fontPx('small'))
      ) + CELL_GAP_PX
    ),
    numeric: true,
    render: (d) => d.depositCount.toLocaleString('en-US'),
  },
  {
    key: 'capacity',
    label: 'CKB deposited',
    // One depositor's CKB under ten billion, every decimal, "9,999,999,999.99999999" (22
    // characters at 13 px, 171.6), and the gap. It was 220.
    minWidth: columnPx(textPx(WIDEST_DAO_DEPOSIT.length, fontPx('small')) + CELL_GAP_PX),
    numeric: true,
    render: (d) => <Ckb form="column" shannons={d.capacity} />,
  },
  {
    key: 'share',
    label: 'Share',
    // The track, the bar's 10 px gap and the share; the last column, no gap after it (it counted
    // 16).
    minWidth: columnPx(SHARE_TRACK + BAR_FIGURE_GAP + SHARE_TEXT),
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
  const pageSize = usePageSize(PAGE);
  // the one cursor walk (hooks/useCursorPagination): a change of page size at md starts it again,
  // and a next cursor it already stands on is taken once (lane c2-finish, 1d and 1e)
  const walk = useCursorPagination(pageSize);
  const cursor = walk.cursor;
  const query = useQuery({
    queryKey: ['dao-depositors', cursor, pageSize],
    queryFn: walk.ask((cursor) => api.getDaoDepositors({ limit: pageSize, cursor })),
    retry: retryTransientOnly,
  });
  const page = query.data;
  // a page past the end of the ranking holds no row to count from: its place and a way back
  // (lane c2-finish, 1b)
  const depositorsAt = page ? rankedPageAt('depositors', page, walk) : null;
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
            <Link href="/dao" look="onward" className="text-small font-mono">
              Nervos DAO
            </Link>
          }
          identity={
            page ? (
              <span data-role="ranked-at">
                {page.total.toLocaleString('en-US')} depositors ranked by CKB deposited at block{' '}
                <BlockLink look="inline" number={page.anchor.blockNumber} />
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
                  className="text-text-dim text-small max-w-measure mb-3"
                >
                  The chain tip moved since the first page (ranked at block{' '}
                  <BlockLink look="inline" number={moved.from} />
                  ): this page continues the ranking at block{' '}
                  <BlockLink look="inline" number={moved.to} />, so a depositor whose rank changed
                  may repeat or be missing.
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
                    head={DEPOSITOR_PHONE_HEAD_PX}
                    wide={wide}
                  />
                )}
              />
              {/* The controls board's pager for a ranked list (B5: nothing for one page): its
                  figures from the response (lib/pagination.ts), the page's size its limit (R2:
                  twenty on a phone), the page the reader's step through the cursors. */}
              {depositorsAt && (
                <div className="pt-4">
                  <Pager mode="pages" label="depositors" {...depositorsAt} />
                </div>
              )}
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
