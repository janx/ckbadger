'use client';
import { useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type TokenDistribution, type TokenHolder } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { AssetAmount, AssetCount } from '@/components/asset/inventory-table';
import { AssetReadState } from '@/components/asset/detail-shell';
import { DetailSection } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
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
  const groups = [
    ['Largest holder', d.largestHolderBalance],
    ['Next 9', d.nextNineBalance],
    ['Remaining holders', d.remainingBalance],
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
  const columns: ListColumn<TokenDistribution['buckets'][number]>[] = [
    {
      key: 'range',
      label: `Balance${d.unit === 'raw' ? ' · raw' : symbol ? `, ${symbol}` : ''}`,
      minWidth: 130,
      render: (b) => (
        <>
          {b.lowerBound === '0' ? (
            'Under '
          ) : (
            <>
              <AssetAmount amount={b.lowerBound} decimals={d.decimals} />
              {b.upperBound === null ? ' and above' : ' – under '}
            </>
          )}
          {b.upperBound !== null && <AssetAmount amount={b.upperBound} decimals={d.decimals} />}
        </>
      ),
    },
    {
      key: 'holders',
      label: 'Holders',
      minWidth: 105,
      numeric: true,
      render: (b) => (
        <span className="flex flex-wrap items-center justify-end gap-2">
          <span aria-hidden="true" className="bg-base-elevated h-1.5 w-10">
            <span
              className="bg-jade block h-full"
              style={{
                width: d.holdersCount === 0 ? '0%' : `${(b.holdersCount / d.holdersCount) * 100}%`,
              }}
            />
          </span>
          <AssetCount value={b.holdersCount} />
        </span>
      ),
    },
    {
      key: 'share',
      label: 'Of supply',
      minWidth: 90,
      numeric: true,
      render: (b) => share(BigInt(b.balance)),
    },
  ];
  return (
    <div className="space-y-4">
      {total > BigInt(0) && (
        <div
          className="flex h-2"
          role="img"
          aria-label={groups.map(([n], i) => `${n} ${share(groupAmounts[i])}`).join(', ')}
        >
          {groupAmounts.map((v, i) => (
            <span
              key={i}
              className={['bg-jade', 'bg-jade/50', 'bg-jade/20'][i]}
              style={{ width: `${(Number(v) / Number(total)) * 100}%` }}
            />
          ))}
        </div>
      )}
      <dl className="flex flex-wrap gap-x-6 gap-y-2 text-xs">
        {groups.map(([n], i) => (
          <div key={n}>
            <dt className="text-text-dim">{n}</dt>
            <dd>{share(groupAmounts[i])}</dd>
          </div>
        ))}
      </dl>
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
  const [all, setAll] = useState(false),
    pagination = useCursorPagination();
  const distribution = useQuery({
    queryKey: ['token-distribution', typeHash],
    queryFn: () => api.getTokenDistribution(typeHash),
  });
  const holders = useQuery({
    queryKey: ['token-holders', typeHash, all, pagination.cursor],
    queryFn: () =>
      api.getTokenHolders(typeHash, { limit: all ? 50 : 10, cursor: pagination.cursor }),
  });
  const columns: ListColumn<TokenHolder>[] = [
    {
      key: 'rank',
      label: '#',
      minWidth: 28,
      render: (h) => (pagination.page - 1) * (all ? 50 : 10) + holders.data!.data.indexOf(h) + 1,
    },
    {
      key: 'address',
      label: 'Address',
      minWidth: 200,
      render: (h) => (
        <>
          <AppLink href={`/address/${h.address ?? h.lockScriptHash}`}>
            <Hash
              hash={h.address ?? h.lockScriptHash}
              copyable={false}
              startChars={12}
              endChars={6}
              className="text-xs"
            />
          </AppLink>
          <span className="text-text-dim ml-2 text-[11px]">
            {h.lockKind ?? 'Lock kind unavailable'}
          </span>
        </>
      ),
    },
    {
      key: 'balance',
      label: symbol ?? 'Balance',
      minWidth: 170,
      numeric: true,
      className: '!whitespace-normal',
      render: (h) => <AssetAmount amount={h.balance} decimals={decimals} />,
    },
    {
      key: 'share',
      label: 'Share',
      minWidth: 90,
      numeric: true,
      render: (h) => {
        const total = exactUnsignedInteger(holders.data!.totalSupply, 'holder-page supply'),
          amount = exactUnsignedInteger(h.balance, 'holder balance');
        if (amount > total) throw new Error('Holder balance exceeds same-response supply');
        return total === BigInt(0)
          ? '—'
          : `${formatScaledInteger(amount * BigInt(100), total, 2)}%`;
      },
    },
  ];
  const holderTable = (
    <div className="min-w-0">
      {holders.data ? (
        <>
          <AssetReadState error={holders.error} label="holders" retained />
          <ListTable
            label={all ? 'All holders' : 'Top 10 holders'}
            columns={columns}
            rows={holders.data.data}
            rowKey={(h) => h.lockScriptHash}
            empty="No holders."
          />
          {all && (
            <CursorPagination
              page={pagination.page}
              pageSize={50}
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
          className="text-jade text-xs"
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
          <AssetReadState error={distribution.error} label="holder distribution" retained />
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
            label="holder distribution"
          />
          {holderTable}
        </>
      )}
    </DetailSection>
  );
}
