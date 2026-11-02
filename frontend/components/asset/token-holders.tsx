'use client';
import { useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type TokenDistribution, type TokenHolder } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { AssetAmount, AssetCount, TokenTwoPlaces } from '@/components/asset/inventory-table';
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
import { shareBarWidth } from '@/components/ui/share-bar';
import { AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { RowTag } from '@/components/ui/row-tag';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { formatTick } from '@/lib/chart-ticks';

// Table-rows round, lane L2-4: the holder tables on the row rules — one 41 px line, the bar an
// 8 px AlignedBar on the table's own scale, what left the row in its title.
/** The holders-per-range bar of "Holders by balance", and the share bar of the holder list. */
const BUCKET_TRACK = 40;
const SHARE_TRACK = 48;
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
  const share = (amount: bigint) =>
    total === BigInt(0) ? '—' : `${formatScaledInteger(amount * BigInt(100), total, 2)}%`;
  const holderScale = resourceScale(d.buckets.map((b) => b.holdersCount));
  const columns: ListColumn<TokenDistribution['buckets'][number]>[] = [
    {
      key: 'range',
      label: `Balance${d.unit === 'raw' ? ' · raw' : symbol ? `, ${symbol}` : ''}`,
      // "10,000 – under 100,000" on one line.
      minWidth: 176,
      render: (b) => (
        <span className="whitespace-nowrap">
          {bucketRange(b.lowerBound, b.upperBound, d.decimals)}
        </span>
      ),
    },
    {
      key: 'holders',
      label: 'Holders',
      minWidth: BUCKET_TRACK + 10 + 44 + 16,
      header: (
        <ScaleHeader
          label="Holders"
          scale={holderScale}
          trackWidth={BUCKET_TRACK}
          format={(n) => formatTick(n, n)}
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
          displayWidth={44}
        />
      ),
    },
    {
      key: 'share',
      label: 'Of supply',
      minWidth: 72,
      numeric: true,
      render: (b) => <span>{share(BigInt(b.balance))}</span>,
    },
  ];
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
      <div
        className={
          holders ? 'grid min-w-0 gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]' : 'min-w-0'
        }
      >
        {holders}
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
                        {b.holdersCount === 1 ? 'holder' : 'holders'}
                      </span>
                    </span>
                  ),
                },
                {
                  key: 'share',
                  tone: 'quiet',
                  left: (
                    <span aria-hidden="true" className="bg-base-elevated inline-block h-1.5 w-10">
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
  const distribution = useQuery({
    queryKey: ['token-distribution', typeHash],
    queryFn: () => api.getTokenDistribution(typeHash),
  });
  // "All holders" pages by the list size (R2: twenty below md); the top ten stay ten.
  const allPageSize = usePageSize(50);
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
    if (total === BigInt(0)) return { text: '—', hundredths: 0, amount, total };
    const hundredths = Number(formatScaledInteger(amount * BigInt(10000), total, 0));
    return {
      text: `${formatScaledInteger(amount * BigInt(100), total, 2)}%`,
      hundredths,
      amount,
      total,
    };
  };
  // Board L5 on the row rules (table-rows L2-4): rank and address bright, the lock a tag, the
  // amount to 2 dp and the share as an 8 px bar on the page's scale — 552 px of minimums, so the
  // table holds in the token page's 554 px column at 1280; the two-line phone row takes over below
  // it. Every digit of the amount is in the row's title.
  const holderColumns = (rows: readonly TokenHolder[]): ListColumn<TokenHolder>[] => {
    const shareScale = resourceScale(rows.map((h) => holderShare(h).hundredths));
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
              copyable={false}
              startChars={10}
              endChars={6}
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
        // ends in an ellipsis, whole in the chip's and the row's hover.
        render: (h) => {
          const kind = h.lockKind ?? 'Lock kind unavailable';
          return (
            <span className="flex min-w-0">
              <RowTag title={kind} className="min-w-0 truncate">
                {kind}
              </RowTag>
            </span>
          );
        },
      },
      {
        key: 'balance',
        label: symbol ?? 'Balance',
        minWidth: 112,
        numeric: true,
        // One line; an amount too long for the column ends in an ellipsis, exact in the hover.
        render: (h) => (
          <span className="block truncate">
            <TokenTwoPlaces amount={h.balance} decimals={decimals} />
          </span>
        ),
      },
      {
        key: 'share',
        label: 'Share',
        minWidth: SHARE_TRACK + 10 + 48 + 16,
        header: (
          <ScaleHeader
            label="Share"
            scale={shareScale}
            trackWidth={SHARE_TRACK}
            format={(n) => `${n / 100}%`}
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
              displayWidth={48}
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
      h.lockKind ?? 'lock kind unavailable',
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
            columns={holderColumns(holders.data.data)}
            rows={holders.data.data}
            rowKey={(h) => h.lockScriptHash}
            empty="No holders."
            density="row"
            rowTitle={holderTitle}
            mobileRow={(h, wide) => {
              const share = holderShare(h);
              return (
                <HolderPhoneRow
                  rank={rank(h)}
                  party={
                    <AppLink href={`/address/${h.address ?? h.lockScriptHash}`}>
                      {/* 10…6 lets rank, address, lock and amount share a 358 px line (L5). */}
                      <Hash
                        hash={h.address ?? h.lockScriptHash}
                        copyable={false}
                        startChars={10}
                        endChars={6}
                        className="text-body"
                      />
                    </AppLink>
                  }
                  lock={h.lockKind ?? 'Lock kind unavailable'}
                  amount={<AssetAmount amount={h.balance} decimals={decimals} />}
                  share={share.text}
                  barWidth={shareBarWidth(share.amount, share.total)}
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
              hasMore={holders.data.hasMore}
              hasPrevious={pagination.hasPrevious}
              onNext={() => pagination.goToNext(holders.data.nextCursor)}
              onPrevious={pagination.goToPrevious}
            />
          )}
        </>
      ) : (
        <AssetReadState error={holders.error} loading={holders.isLoading} label="holders" />
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
          {all
            ? 'Top 10 holders'
            : `All ${distribution.data?.holdersCount.toLocaleString('en-US') ?? ''} holders →`}
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
            loading={distribution.isLoading}
            label={distributionSource}
          />
          {holderTable}
        </>
      )}
    </DetailSection>
  );
}
