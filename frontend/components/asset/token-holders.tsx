'use client';
import { useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type TokenDistribution, type TokenHolder } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';
import { ENTITY_PAGE_SIZE } from '@/lib/pagination';
import { AssetCount, wordHeads } from '@/components/asset/inventory-table';
import { TokenAmount } from '@/components/ui/ckb-amount';
import { AssetReadState } from '@/components/asset/detail-shell';
import { DetailSection } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { ConcentrationBar } from '@/components/ui/concentration-bar';
import { PhoneRow } from '@/components/ui/phone-row';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { formatShareTick } from '@/components/ui/share-bar';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { rowTagClass } from '@/components/ui/row-tag';
import { LockKind } from '@/components/ui/lock-kind';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { formatTick } from '@/lib/chart-ticks';
import { formatCount, countNoun } from '@/lib/format-count';
import { percentOf } from '@/lib/format-unit';
import { unitNode } from '@/components/ui/units';

// Table-rows round, lane L2-4: the holder tables on the row rules — one 41 px line, the bar an
// 8 px AlignedBar on the table's own scale, what left the row in its title.
/** The holders-per-range bar of "Holders by balance", and the share bar of the holder list. */
const BUCKET_TRACK = 40;
const SHARE_TRACK = 48;
/** The figures right of those bars: a range's holder count, a holder's share ("41.23%"). */
const BUCKET_FIGURE = 44;
const SHARE_FIGURE = 48;
/** "Under 1", "1 – under 10", "100,000 and above": exact bounds, trailing zeros dropped. */
function bucketRange(lowerBound: string, upperBound: string | null, decimals: number | null) {
  const bound = (amount: string) => formatTokenBalanceWithRawMarker(amount, decimals);
  if (upperBound === null) return `${bound(lowerBound)} and above`;
  return lowerBound === '0'
    ? `Under ${bound(upperBound)}`
    : `${bound(lowerBound)} – under ${bound(upperBound)}`;
}
export interface TokenDistributionViewProps {
  distribution: TokenDistribution;
  symbol: string | null;
  holders?: ReactNode;
}
export function TokenDistributionView({
  distribution: d,
  symbol,
  holders,
}: TokenDistributionViewProps) {
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
      // "10,000 – under 100,000" on one 13 px line.
      minWidth: 188,
      render: (b) => (
        <span className="text-body text-text-bright whitespace-nowrap leading-[18px]">
          {bucketRange(b.lowerBound, b.upperBound, d.decimals)}
        </span>
      ),
    },
    {
      key: 'holders',
      label: 'Holders',
      minWidth: BUCKET_TRACK + 10 + BUCKET_FIGURE + 16,
      header: (
        <ScaleHeader
          label="Holders"
          scale={holderScale}
          trackWidth={BUCKET_TRACK}
          format={(n) => formatTick(n, n)}
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
      minWidth: 64,
      numeric: true,
      render: (b) => <span>{share(BigInt(b.balance))}</span>,
    },
  ]);
  return (
    <div className="space-y-4">
      {total > BigInt(0) && (
        // F2 objects (IV-18): the shared concentration bar, in the token colour.
        <ConcentrationBar
          label="Share of supply"
          assetClass="tokens"
          total={total}
          groups={groups.map(([label], i) => ({ label, value: groupAmounts[i] }))}
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
                          className="bg-token block h-full"
                          style={{
                            width:
                              d.holdersCount === 0
                                ? '0%'
                                : `${(b.holdersCount / d.holdersCount) * 100}%`,
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
        </div>
      </div>
    </div>
  );
}
export interface TokenHoldersProps {
  typeHash: string;
  symbol: string | null;
  decimals: number | null;
}
export function TokenHolders({ typeHash, symbol, decimals }: TokenHoldersProps) {
  const distributionSource = `holder distribution from /tokens/${typeHash}/distribution`;
  const [all, setAll] = useState(false),
    pagination = useCursorPagination();
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
  const holders = useQuery({
    queryKey: ['token-holders', typeHash, all, pagination.cursor, limit],
    queryFn: () => api.getTokenHolders(typeHash, { limit, cursor: pagination.cursor }),
  });
  /**
   * A holder's share of the same response's supply, exact to 0.01 %: its text, and the same
   * rounding as an integer of hundredths for the bar's geometry.
   */
  const holderShare = (h: TokenHolder) => {
    const total = exactUnsignedInteger(holders.data!.totalSupply, 'holder-page supply'),
      amount = exactUnsignedInteger(h.balance, 'holder balance');
    if (amount > total) throw new Error('Holder balance exceeds same-response supply');
    if (total === BigInt(0)) return { text: '—', hundredths: 0 };
    const hundredths = Number(formatScaledInteger(amount * BigInt(10000), total, 0));
    return { text: percentOf(amount, total), hundredths };
  };
  // Board L5 on the row rules (table-rows L2-4): rank and address bright, the lock a tag, the
  // amount to 2 dp and the share as an 8 px bar on the page's scale — 552 px of minimums, so the
  // table holds in the token page's 554 px column at 1280; the two-line phone row takes over below
  // it. Every digit of the amount is in the row's title.
  /** R7: the one scale of the page's share bars, the table's and the phone rows' alike. */
  const shareScaleOf = (rows: readonly TokenHolder[]) =>
    resourceScale(rows.map((h) => holderShare(h).hundredths));
  const holderColumns = (rows: readonly TokenHolder[]): ListColumn<TokenHolder>[] => {
    const shareScale = shareScaleOf(rows);
    return [
      {
        key: 'rank',
        label: '#',
        minWidth: 32,
        className: 'whitespace-nowrap',
        render: (h) => <span className="text-text-bright">{rank(h)}</span>,
      },
      {
        key: 'address',
        label: 'Address',
        minWidth: 170,
        render: (h) => (
          <AppLink
            href={`/address/${h.address ?? h.lockScriptHash}`}
            className="text-body text-text-bright leading-[18px]"
          >
            <Hash
              hash={h.address ?? h.lockScriptHash}
              kind={h.address ? 'address' : 'hash'}
              size="row"
              copyable={false}
              variant="bright"
              className="text-body leading-[18px]"
            />
          </AppLink>
        ),
      },
      {
        key: 'lock',
        label: 'Lock',
        minWidth: 116,
        // A flex line holds the chip at its own 18 px, so the row stays 41 px; a long lock kind
        // ends in an ellipsis, whole in the row's hover (the chip's hover is the code hash).
        // U-15: a lock the registry does not name reads "lock <code hash>", as on the collection
        // holders.
        render: (h) => (
          <span className="flex min-w-0">
            <LockKind
              lockKind={h.lockKind}
              lockCodeHash={h.lockCodeHash}
              size="row"
              className={rowTagClass('default', 'min-w-0 truncate')}
            />
          </span>
        ),
      },
      {
        key: 'balance',
        label: symbol ?? 'Balance',
        minWidth: 112,
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
        minWidth: SHARE_TRACK + 10 + SHARE_FIGURE + 16,
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
  const holderTitle = (h: TokenHolder) =>
    [
      `#${rank(h)}`,
      h.address ?? h.lockScriptHash,
      h.lockCodeHash === null ? 'lock not indexed' : (h.lockKind ?? h.lockCodeHash),
      `${formatTokenBalanceWithRawMarker(h.balance, decimals)}${symbol ? ` ${symbol}` : ''}`,
      `${holderShare(h).text} of supply`,
    ].join(' · ');
  const rank = (h: TokenHolder) =>
    (pagination.page - 1) * limit + holders.data!.data.indexOf(h) + 1;
  const holderTable = (
    <div className="min-w-0">
      {holders.data ? (
        <>
          <AssetReadState error={holders.error} label="holders" retained />
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
                    <AppLink href={`/address/${h.address ?? h.lockScriptHash}`}>
                      {/* Below sm the tight size, 11 characters for an address, 4…6: rank,
                          address, lock and amount share a 358 px line (L5); from sm the row
                          size, the table's (R9). */}
                      <Hash
                        hash={h.address ?? h.lockScriptHash}
                        kind={h.address ? 'address' : 'hash'}
                        size={phoneSize}
                        copyable={false}
                        className="text-body"
                      />
                    </AppLink>
                  }
                  lock={
                    // In the row's own dim small ink, as the kind was written before U-15.
                    <LockKind
                      lockKind={h.lockKind}
                      lockCodeHash={h.lockCodeHash}
                      size={phoneSize}
                      className="text-text-dim text-small"
                    />
                  }
                  amount={<TokenAmount form="summary" amount={h.balance} decimals={decimals} />}
                  share={share.text}
                  bar={{ value: share.hundredths, scale, color: CLASS_COLORS.tokens }}
                  axis={rows[0] === h ? formatShareTick(scale) : undefined}
                  wide={wide}
                />
              );
            }}
          />
          {all && (
            <CursorPagination
              page={pagination.page}
              pageSize={limit}
              total={holders.data.total ?? undefined}
              totalLabel="holders"
              currentCount={holders.data.data.length}
              hasMore={holders.data.hasMore}
              hasPrevious={pagination.hasPrevious}
              onNext={() => pagination.goToNext(holders.data.nextCursor)}
              onPrevious={pagination.goToPrevious}
            />
          )}
        </>
      ) : (
        <AssetReadState error={holders.error} read={holders} label="holders" />
      )}
    </div>
  );
  return (
    <DetailSection
      title="Who holds it"
      actions={
        <button
          className="text-jade text-small"
          onClick={() => {
            setAll(!all);
            pagination.reset();
          }}
        >
          {unitNode(
            all
              ? 'Top 10 holders'
              : `All ${distribution.data ? formatCount(distribution.data.holdersCount, 'holder') : 'holders'} →`
          )}
        </button>
      }
    >
      {distribution.data ? (
        <>
          <AssetReadState error={distribution.error} label={distributionSource} retained />
          <TokenDistributionView
            distribution={distribution.data}
            symbol={symbol}
            holders={holderTable}
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
