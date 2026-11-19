'use client';
import { useId, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type TokenDistribution, type TokenHolder } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';
import { ENTITY_PAGE_SIZE, rankedPageAt } from '@/lib/pagination';
import { AssetCount, wordHeads } from '@/components/asset/inventory-table';
import { TokenAmount } from '@/components/ui/ckb-amount';
import { AssetReadState } from '@/components/asset/detail-shell';
import { DetailSection } from '@/components/ui/detail-layout';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { Pager } from '@/components/ui/pager';
import { FoldToggle } from '@/components/ui/fold-toggle';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { ConcentrationBar } from '@/components/ui/concentration-bar';
import { PhoneRow } from '@/components/ui/phone-row';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { SHARE_TRACK_PX, formatShareTick } from '@/components/ui/share-bar';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { LockKind, LockWordsView, lockWordsText, rowLock } from '@/components/ui/lock-kind';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { countNoun } from '@/lib/format-count';
import { percentOf } from '@/lib/format-unit';
import {
  AlignedBar,
  BAR_FIGURE_GAP,
  COUNT_TRACK_PX,
  ScaleHeader,
} from '@/components/ui/aligned-bar';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import { SHORT_FORMS } from '@/lib/shorten';
import { WIDEST_TOKEN_HOLDER_RANK, WIDEST_TOKEN_HOLDERS, shortChars } from '@/lib/list-bounds';
import { PartReadBoundary, PartReadError, readPart } from '@/components/read-failure';

// Table-rows round, lane L2-4: the holder tables on the row rules — one 41 px line, the bar an
// 8 px AlignedBar on the table's own scale, what left the row in its title.
// Readability F3-e: every column budgeted from what it holds (lib/list-bounds.ts), the 13 px cells
// at 0.6 em a glyph and the 16 px gap after every column but the last.
/**
 * A lock's name a column ends in the one ellipsis: 12 characters of it, so the default lock's,
 * "Default Lock", stands whole.
 */
const NAME_CHARS = 12;
/**
 * A balance the column ends in the one ellipsis, exact in its title: 11 characters of it. Lane P2's
 * follow-up (item 7): the rank's bound grew a digit (WIDEST_TOKEN_HOLDER_RANK, a token's holders
 * under a million), and in the 554 px column the table has at 1280 the balance gives that 8 px.
 */
const BALANCE_CHARS = 11;
const cell = (chars: number) => textPx(chars, fontPx('small'));
/**
 * The holders-per-range bar of "Holders by balance", and the share bar of the holder list: each
 * track holds its head's ticks whole. The count's ("0" … "750M", the tick form) are under the 48 px
 * a bar keeps at least, so 48 (the bands' was 40); the share's widest is "0" … "0.75%" (a top ten
 * under 1 % each), 50.4 px (review fb-logic 7: it was measured from "100%", 48).
 */
const BUCKET_TRACK = COUNT_TRACK_PX;
const SHARE_TRACK = SHARE_TRACK_PX;
/**
 * The figures right of those bars: a range's holder count, "99,999" (it was 44, five
 * characters), and a holder's share, "100.00%" (it was 48, "41.23%").
 */
const BUCKET_FIGURE = columnPx(cell(WIDEST_TOKEN_HOLDERS.length));
// the share's box is its text exactly (54.6): its column rounds once, the 50.4 px track, the gap
// and the figure together, 115 px (two roundings, 55 and then 116, made the table 555 px, a pixel
// wider than the token page's 554 px column at 1280)
const SHARE_FIGURE = cell('100.00%'.length);
/** "Under 1", "1 – under 10", "100,000 and above": exact bounds, trailing zeros dropped. */
function bucketRange(lowerBound: string, upperBound: string | null, decimals: number | null) {
  const bound = (amount: string) => formatTokenBalanceWithRawMarker(amount, decimals);
  if (upperBound === null) return `${bound(lowerBound)} and above`;
  return lowerBound === '0'
    ? `Under ${bound(upperBound)}`
    : `${bound(lowerBound)} – under ${bound(upperBound)}`;
}
/**
 * Line 1 of a token holder on a phone (the phone-rows board's head()): a rank of two digits in its
 * 22 px box, the holder's address at the tight size and the key's 14 px (92.4), the 12 px between
 * the parts, and the room that leaves the balance with its symbol at 14 px: 24 characters, 201.6, the
 * line's 328 px at 360 (28 characters at 390). A token's balance has no bound of its own (a supply
 * is the issuer's): a longer one drops under the holder, whole and right-aligned (PhoneRow's rule),
 * and a rank of three digits or more takes 8.4 px a digit from it.
 */
const TOKEN_HOLDER_PHONE_HEAD_PX = phoneHead(
  'token holders',
  22 +
    textPx(shortChars(SHORT_FORMS.tight.address), fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(24, fontPx('body'))
);
export interface TokenDistributionViewProps {
  distribution: TokenDistribution;
  symbol: string | null;
  holders?: ReactNode;
  /** The token it draws (B4 third round): said where a part of it cannot be drawn (D7). */
  where: string;
}
/**
 * A token's distribution: the share of supply, and the holders by balance beside the holders it is
 * given. A distribution that breaks its invariants, or a part of it that cannot be drawn, fails in
 * its place (D7, B4 third round), said with the token; the holders beside it stand, with their pager.
 */
export function TokenDistributionView({
  distribution: d,
  symbol,
  holders,
  where,
}: TokenDistributionViewProps) {
  const read = readPart(() => distributionParts(d, symbol));
  return (
    <div className="space-y-4">
      {read.value !== undefined && read.value.total > BigInt(0) && (
        // F2 objects (IV-18): the shared concentration bar, in the token colour.
        <PartReadBoundary
          what="the share of supply"
          where={where}
          render={() => (
            <ConcentrationBar
              label="Share of supply"
              assetClass="tokens"
              total={read.value.total}
              groups={read.value.groups}
            />
          )}
        />
      )}
      {/* Two tables, never one row grid (readability B5): from xl, where they stand side by side
          on rows of one height, a 32 px gutter, a rule in the row-rule ink and 32 px more lie
          between them (a column gap inside a table is 16 px), so a holder's row does not read on
          into a band's. Below xl they stack. The bands list sits in a column of its own, so it
          measures its own width without the gutter. */}
      <div
        className={
          holders
            ? 'grid min-w-0 gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] xl:gap-x-8'
            : 'min-w-0'
        }
      >
        {holders}
        <div className={holders ? 'xl:border-row-rule min-w-0 xl:border-l xl:pl-8' : 'min-w-0'}>
          {read.error !== undefined ? (
            <PartReadError what="the holder distribution" where={where} error={read.error} />
          ) : (
            <PartReadBoundary
              what="the holders by balance"
              where={where}
              render={read.value.bands}
            />
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * The distribution's parts, read from it once its totals agree with its own supply: the share
 * bar's groups and the bands table (drawn inside its part). Throws where they do not.
 */
function distributionParts(d: TokenDistribution, symbol: string | null) {
  const total = exactUnsignedInteger(d.totalSupply, 'distribution supply');
  // F2 objects (IV-18): the board's names for the three groups.
  const next = Math.max(Math.min(9, d.holdersCount - 1), 0);
  const groups = [
    ['Largest holder', d.largestHolderBalance],
    [`Next ${next}`, d.nextNineBalance],
    [
      d.holdersCount > 10 ? `Other ${(d.holdersCount - 10).toLocaleString('en-US')}` : 'Others',
      d.remainingBalance,
    ],
  ] as const;
  const groupAmounts = groups.map(([name, value]) => exactUnsignedInteger(value, name));
  if (groupAmounts.reduce((a, b) => a + b, BigInt(0)) !== total)
    throw new Error('Token distribution concentration does not equal supply');
  const bucketBalance = d.buckets.reduce(
    (sum, b) => sum + exactUnsignedInteger(b.balance, 'bucket balance'),
    BigInt(0)
  );
  const bucketHolders = d.buckets.reduce((sum, b) => {
    if (!Number.isSafeInteger(b.holdersCount) || b.holdersCount < 0)
      throw new Error('Invalid bucket holder count');
    return sum + b.holdersCount;
  }, 0);
  if (bucketBalance !== total || bucketHolders !== d.holdersCount)
    throw new Error('Token distribution buckets do not equal totals');
  const share = (amount: bigint) => (total === BigInt(0) ? '—' : percentOf(amount, total));
  const holderScale = resourceScale(d.buckets.map((b) => b.holdersCount));
  const columns: ListColumn<TokenDistribution['buckets'][number]>[] = wordHeads([
    {
      key: 'range',
      label: `Balance${d.unit === 'raw' ? ' · raw' : symbol ? `, ${symbol}` : ''}`,
      // The flexible column (readability F3-e): "10,000 – under 100,000" whole on its 14 px line
      // (22 characters, 184.8) and the gap at its minimum (it was 188, with no gap); a wider range
      // ends in the one ellipsis, whole in its title.
      minWidth: columnPx(textPx('10,000 – under 100,000'.length, fontPx('body')) + CELL_GAP_PX),
      width: 'auto',
      className: 'truncate',
      render: (b) => {
        const range = bucketRange(b.lowerBound, b.upperBound, d.decimals);
        return (
          <span title={range} className="text-body text-text-bright leading-row whitespace-nowrap">
            {range}
          </span>
        );
      },
    },
    {
      key: 'holders',
      label: 'Holders',
      // The track, the bar's 10 px gap, the count and the gap.
      minWidth: columnPx(BUCKET_TRACK + BAR_FIGURE_GAP + BUCKET_FIGURE + CELL_GAP_PX),
      header: (
        <ScaleHeader
          label="Holders"
          scale={holderScale}
          trackWidth={BUCKET_TRACK}
          figureWidth={BUCKET_FIGURE}
        />
      ),
      render: (b) => (
        <AlignedBar
          label="Holders"
          scale={holderScale}
          trackWidth={BUCKET_TRACK}
          minFill={2}
          segments={[{ key: 'holders', label: 'holders', value: b.holdersCount, tone: 'tokens' }]}
          display={
            <span>
              <AssetCount value={b.holdersCount} />
            </span>
          }
          displayWidth={BUCKET_FIGURE}
        />
      ),
    },
    {
      key: 'share',
      label: 'Of supply',
      // "100.00%" (7 characters at 13 px, 54.6), wider than the head's word "supply"; the last
      // column, no gap after it (it counted 16).
      minWidth: columnPx(Math.max(cell('100.00%'.length), textPx('supply'.length, fontPx('meta')))),
      numeric: true,
      render: (b) => <span>{share(BigInt(b.balance))}</span>,
    },
  ]);
  const bands = () => (
    <ListTable
      label="Holders by balance"
      columns={columns}
      rows={d.buckets}
      rowKey={(b) => b.lowerBound}
      empty="No holders."
      density="row"
      mobileRow={(b) => (
        <PhoneRow
          testId="holder-bucket-row-mobile"
          lines={[
            {
              key: 'range',
              tone: 'body',
              left: columns[0].render(b),
              right: (
                <span className="whitespace-nowrap">
                  <AssetCount value={b.holdersCount} />{' '}
                  <span className="text-text-dim">
                    {countNoun(b.holdersCount, 'holder', 'holders')}
                  </span>
                </span>
              ),
            },
            {
              key: 'share',
              tone: 'quiet',
              left: (
                <span aria-hidden="true" className="bg-bar-track inline-block h-1.5 w-10">
                  <span
                    className="bg-token scan-fill block h-full"
                    style={{
                      width:
                        d.holdersCount === 0 ? '0%' : `${(b.holdersCount / d.holdersCount) * 100}%`,
                    }}
                  />
                </span>
              ),
              right: <>{share(BigInt(b.balance))} of supply</>,
            },
          ]}
        />
      )}
    />
  );
  return {
    total,
    groups: groups.map(([label], i) => ({ label, value: groupAmounts[i] })),
    bands,
  };
}
/**
 * The rows a holders' response counts, the figure its "all N holders ↓" states (a pager states its
 * own response's figures); a holders' route always counts them, so a response without its count is
 * a contract break, named.
 */
export function listTotal(read: { total?: number }, what: string): number {
  if (read.total === undefined) throw new Error(`${what}: the response counts no rows`);
  return read.total;
}
export interface TokenHoldersProps {
  typeHash: string;
  symbol: string | null;
  decimals: number | null;
}
/**
 * The token's holders. A refusal while the holders' table is drawn fails that table in its place
 * (D7), said with the token and the view; the heading, its button and the pager stay, and the
 * table starts again on another view or page.
 */
export function TokenHolders({ typeHash, symbol, decimals }: TokenHoldersProps) {
  const distributionSource = `holder distribution from /tokens/${typeHash}/distribution`;
  const [all, setAll] = useState(false);
  // The phone row is drawn wherever the holders' column is narrower than the table's 552 px: on a
  // phone, and in a desktop column (the 3fr beside the balance bands from 1280 is within a few px
  // of it): tight only below sm (R9).
  const phoneSize = useTightBelowSm();
  const distribution = useQuery({
    queryKey: ['token-distribution', typeHash],
    queryFn: () => api.getTokenDistribution(typeHash),
  });
  // "All holders" pages by the list size (R2: twenty below md); the top ten stay ten.
  const allPageSize = usePageSize(ENTITY_PAGE_SIZE);
  const limit = all ? allPageSize : 10;
  // a change of page size (at md, or the top ten opened to all) starts the walk again (lane
  // c2-finish, 1d)
  // and so does a change of the list it pages (lane cd-fixes-controls): the top ten or all of them
  const pagination = useCursorPagination(limit, { list: [all] });
  const showAll = (open: boolean) => setAll(open);
  // The top ten, read whether the list is folded or open: the fold's count is theirs, and folding
  // back finds them read, so the fold's one button never waits on a read (lane P2's follow-up,
  // item 10: the loading list unmounted it and the focus fell to the page)
  const top = useQuery({
    queryKey: ['token-holders', typeHash, 'top'],
    queryFn: () => api.getTokenHolders(typeHash, { limit: 10, cursor: undefined }),
  });
  // all holders, in pages of the walk
  const allHolders = useQuery({
    queryKey: ['token-holders', typeHash, 'all', pagination.cursor, limit],
    queryFn: pagination.ask((cursor) => api.getTokenHolders(typeHash, { limit, cursor })),
    enabled: all,
  });
  const holders = all ? allHolders : top;
  const listId = useId();
  /**
   * A holder's share of the same response's supply: the stated share is its text (`percentOf`);
   * the bar's width is drawing, the share in hundredths cut by `formatScaledInteger`, never a
   * figure on the page (review of phases C and D, charts NOTE 9: this said "the same rounding").
   */
  const holderShare = (h: TokenHolder) => {
    const total = exactUnsignedInteger(holders.data!.totalSupply, 'holder-page supply'),
      amount = exactUnsignedInteger(h.balance, 'holder balance');
    if (amount > total) throw new Error('Holder balance exceeds same-response supply');
    if (total === BigInt(0)) return { text: '—', hundredths: 0 };
    const hundredths = Number(formatScaledInteger(amount * BigInt(10000), total, 0));
    return { text: percentOf(amount, total), hundredths };
  };
  // Board L5 on the row rules (table-rows L2-4): rank and address bright, the lock its words (a
  // tag before lane p-assets), the amount to 2 dp and the share as an 8 px bar on the page's scale
  // — 549 px of minimums (rank 55, address 159, lock 110, balance 110, share 115; the lock was a
  // 115 px tag, 554 in all), so the table holds in the token page's 554 px
  // column at 1280; the two-line phone row takes over below it. Every digit of the amount is in the row's title.
  /** R7: the one scale of the page's share bars, the table's and the phone rows' alike. */
  const shareScaleOf = (rows: readonly TokenHolder[]) =>
    resourceScale(rows.map((h) => holderShare(h).hundredths));
  const holderColumns = (rows: readonly TokenHolder[]): ListColumn<TokenHolder>[] => {
    const shareScale = shareScaleOf(rows);
    return [
      {
        key: 'rank',
        label: '#',
        // A rank of six digits, "999999" (46.8 at 13 px; WIDEST_TOKEN_HOLDER_RANK), and the gap; it
        // was five (lane P2's follow-up, item 7), 32 before, two digits.
        minWidth: columnPx(cell(WIDEST_TOKEN_HOLDER_RANK.length) + CELL_GAP_PX),
        className: 'whitespace-nowrap',
        render: (h) => <span className="text-text-bright">{rank(h)}</span>,
      },
      {
        key: 'address',
        label: 'Address',
        // The key at 14 px: an address at the row size (13 characters) or a lock no address
        // encodes by its hash at the row size (17, 142.8, the wider), and the gap. It was 170.
        minWidth: columnPx(
          textPx(
            Math.max(shortChars(SHORT_FORMS.row.address), shortChars(SHORT_FORMS.row.hash)),
            fontPx('body')
          ) + CELL_GAP_PX
        ),
        render: (h) => (
          <AppLink
            look="key"
            href={`/address/${h.address ?? h.lockScriptHash}`}
            className="text-body leading-row"
          >
            <Hash
              hash={h.address ?? h.lockScriptHash}
              kind={h.address ? 'address' : 'hash'}
              size="row"
              variant="bright"
              className="text-body leading-row"
            />
          </AppLink>
        ),
      },
      {
        key: 'lock',
        label: 'Lock',
        // The flexible column (readability F3-e): 12 characters of the lock's name at the cells'
        // 13 px (93.6) and the gap at its minimum, and what the container gives beyond the sum. It
        // was 115, a tag of 12 characters at 12 px with its sides and border (98.4).
        minWidth: columnPx(cell(NAME_CHARS) + CELL_GAP_PX),
        width: 'auto',
        // A lock is plain words, no chip (lane p-assets; D5 as the readability plan changed it):
        // the column's value in the reading ink; a long one ends in an ellipsis, its code hash in
        // its hover. U-15: a lock the registry does not name reads "lock <code hash>", as on the
        // collection holders.
        render: (h) => (
          <span className="flex min-w-0">
            {/* a column headed "Lock" is never empty: the default lock by its name (B4) */}
            <LockKind
              lockKind={h.lockKind}
              lockCodeHash={h.lockCodeHash}
              address={h.address}
              size="row"
              place="column"
              className="text-text text-small min-w-0 truncate"
            />
          </span>
        ),
      },
      {
        key: 'balance',
        label: symbol ?? 'Balance',
        // 11 characters of the amount (85.8 at 13 px) and the gap (lane P2's follow-up, item 7: it
        // was 12, 110, before the rank's bound grew a digit; 112 before that).
        minWidth: columnPx(cell(BALANCE_CHARS) + CELL_GAP_PX),
        numeric: true,
        // One line; an amount too long for the column ends in an ellipsis, exact in the hover.
        render: (h) => (
          <span className="block truncate">
            <TokenAmount form="summary" amount={h.balance} decimals={decimals} />
          </span>
        ),
      },
      {
        key: 'share',
        label: 'Share',
        // The track, the bar's 10 px gap and the share; the last column, no gap after it (it
        // counted 16).
        minWidth: columnPx(SHARE_TRACK + BAR_FIGURE_GAP + SHARE_FIGURE),
        header: (
          <ScaleHeader
            label="Share"
            scale={shareScale}
            trackWidth={SHARE_TRACK}
            format={formatShareTick}
            figureWidth={SHARE_FIGURE}
          />
        ),
        render: (h) => {
          const share = holderShare(h);
          return (
            <AlignedBar
              label="Share of supply"
              scale={shareScale}
              trackWidth={SHARE_TRACK}
              minFill={2}
              segments={[{ key: 'share', label: 'share', value: share.hundredths, tone: 'tokens' }]}
              display={<span>{share.text}</span>}
              displayWidth={SHARE_FIGURE}
            />
          );
        },
      },
    ];
  };
  /** A holder's lock beside it on a phone row, or null where none is written (B4). */
  const phoneLock = (h: TokenHolder) => rowLock(h, 'inline');
  const holderTitle = (h: TokenHolder) =>
    [
      `#${rank(h)}`,
      h.address ?? h.lockScriptHash,
      // what its Lock cell says, from the one decision (B4)
      lockWordsText(rowLock(h, 'column')!, 'row'),
      `${formatTokenBalanceWithRawMarker(h.balance, decimals)}${symbol ? ` ${symbol}` : ''}`,
      `${holderShare(h).text} of supply`,
    ].join(' · ');
  const rank = (h: TokenHolder) =>
    (pagination.page - 1) * limit + holders.data!.data.indexOf(h) + 1;
  // "All holders" pages through the list; the top ten are one page
  const holdersAt =
    all && allHolders.data ? rankedPageAt('holders', allHolders.data, pagination) : null;
  const holderTable = (
    <div className="min-w-0">
      <div id={listId} className="min-w-0">
        {holders.data ? (
          <>
            <AssetReadState error={holders.error} label="holders" retained />
            <PartReadBoundary
              what="the holders"
              where={`token ${typeHash}, ${all ? `all holders, page ${pagination.page}` : 'top 10'}`}
              render={() => (
                <ListTable
                  label={all ? 'All holders' : 'Top 10 holders'}
                  columns={wordHeads(holderColumns(holders.data.data))}
                  rows={holders.data.data}
                  rowKey={(h) => h.lockScriptHash}
                  empty="No holders."
                  density="row"
                  rowTitle={holderTitle}
                  mobileRow={(h, wide) => {
                    const share = holderShare(h);
                    const rows = holders.data!.data;
                    const scale = shareScaleOf(rows);
                    return (
                      <HolderPhoneRow
                        rank={rank(h)}
                        party={
                          <AppLink look="key" href={`/address/${h.address ?? h.lockScriptHash}`}>
                            {/* Below sm the tight size, 11 characters for an address, 4…6: rank,
                          address, lock and amount share a 358 px line (L5); from sm the row
                          size, the table's (R9). */}
                            <Hash
                              hash={h.address ?? h.lockScriptHash}
                              kind={h.address ? 'address' : 'hash'}
                              size={phoneSize}
                              className="text-body"
                            />
                          </AppLink>
                        }
                        lock={
                          // In the row's own dim small ink, as the kind was written before U-15; no
                          // part where the row names no lock (B4)
                          phoneLock(h) && (
                            <LockWordsView
                              words={phoneLock(h)!}
                              size={phoneSize}
                              className="text-text-dim text-small"
                            />
                          )
                        }
                        // the balance named by its token's symbol (the table heads its column with it)
                        amount={
                          <span className="whitespace-nowrap">
                            <TokenAmount form="summary" amount={h.balance} decimals={decimals} />
                            {symbol && ` ${symbol}`}
                          </span>
                        }
                        share={share.text}
                        bar={{ value: share.hundredths, scale, color: CLASS_COLORS.tokens }}
                        axis={rows[0] === h ? formatShareTick(scale) : undefined}
                        head={TOKEN_HOLDER_PHONE_HEAD_PX}
                        wide={wide}
                      />
                    );
                  }}
                />
              )}
            />
          </>
        ) : (
          <AssetReadState error={holders.error} read={holders} label="holders" />
        )}
      </div>
      {top.data && (all || top.data.hasMore) && (
        // the list's one disclosure control (FoldToggle, lane P1): "all 522 holders ↓", the count
        // the top ten's own response states, opens the list in place; "fewer ↑" folds it back. It
        // stays mounted while the list loads and keeps the focus; the pager stands over it, a
        // ranked list in pages, every figure the response's
        <div className="mt-3 space-y-1">
          {holdersAt && <Pager mode="pages" label="holders" {...holdersAt} />}
          <FoldToggle
            open={all}
            count={listTotal(top.data, `the holders of token ${typeHash}`)}
            singular="holder"
            controls={listId}
            onToggle={showAll}
          />
        </div>
      )}
    </div>
  );
  return (
    <DetailSection title="Who holds it">
      {distribution.data ? (
        <>
          <AssetReadState error={distribution.error} label={distributionSource} retained />
          <TokenDistributionView
            distribution={distribution.data}
            symbol={symbol}
            holders={holderTable}
            where={`token ${typeHash}`}
          />
        </>
      ) : (
        <>
          <AssetReadState
            error={distribution.error}
            read={distribution}
            label={distributionSource}
          />
          {holderTable}
        </>
      )}
    </DetailSection>
  );
}
