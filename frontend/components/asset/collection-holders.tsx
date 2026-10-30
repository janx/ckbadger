'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { CollectionHolder, CursorPaginatedResponse } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { AssetCount } from '@/components/asset/inventory-table';
import { AssetReadState } from '@/components/asset/detail-shell';
import { DotCellParty } from '@/components/identity/dotcell-party';
import { DetailSection } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { CursorPagination } from '@/components/ui/cursor-pagination';

export interface CollectionHoldersProps {
  load: (params: {
    limit?: number;
    cursor?: string;
  }) => Promise<CursorPaginatedResponse<CollectionHolder>>;
  queryKey: string;
  distribution?: readonly { range: string; holders: number; items: number }[];
}

function count(value: number, label: string): bigint {
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`Invalid collection ${label}: ${value}`);
  return BigInt(value);
}

function holderKey(holder: CollectionHolder): string {
  const key = holder.lockScriptHash ?? holder.ownerHashPrefix ?? holder.address;
  if (!key) throw new Error('Collection holder has no lock or named owner prefix');
  return key;
}

export function CollectionHolders(props: CollectionHoldersProps) {
  // A different collection starts a new cursor scope and Top 10 view.
  return <ScopedCollectionHolders key={props.queryKey} {...props} />;
}

function ScopedCollectionHolders({ load, queryKey, distribution }: CollectionHoldersProps) {
  const [all, setAll] = useState(false);
  const pagination = useCursorPagination();
  const limit = all ? 50 : 10;
  const holders = useQuery({
    queryKey: ['collection-holders', queryKey, all, pagination.cursor],
    queryFn: () => load({ limit, cursor: pagination.cursor }),
  });
  let totalHolders = BigInt(0);
  let totalItems = BigInt(0);
  const ranges = new Set<string>();
  for (const bucket of distribution ?? []) {
    if (!bucket.range || ranges.has(bucket.range))
      throw new Error('Collection holder distribution has an empty or repeated range');
    ranges.add(bucket.range);
    const n = count(bucket.holders, 'bucket holders');
    const items = count(bucket.items, 'bucket items');
    if (items < n || (n === BigInt(0) && items !== BigInt(0)))
      throw new Error(`Collection holder distribution has inconsistent counts: ${bucket.range}`);
    totalHolders += n;
    totalItems += items;
  }
  for (const holder of holders.data?.data ?? []) {
    holderKey(holder);
    count(holder.itemCount, 'holder item count');
  }
  // The list (server-cached up to 30 s) and the distribution are separate reads: when a page
  // lists more holders than the distribution counts, say so and show both (#6).
  const readsDiffer =
    !!distribution && !!holders.data && BigInt(holders.data.data.length) > totalHolders;
  const columns: ListColumn<CollectionHolder>[] = [
    {
      key: 'rank',
      label: '#',
      minWidth: 32,
      render: (holder) => (pagination.page - 1) * limit + holders.data!.data.indexOf(holder) + 1,
    },
    {
      key: 'owner',
      label: 'Holder',
      minWidth: 210,
      render: (holder) => (
        <DotCellParty
          party={{
            hashPrefix: holder.ownerHashPrefix ?? null,
            lockHash: holder.lockScriptHash,
            address: holder.address,
          }}
          truncate
          className="max-w-full break-all [&>span]:text-xs"
        />
      ),
    },
    {
      key: 'items',
      label: 'Items',
      minWidth: 100,
      numeric: true,
      className: '!whitespace-normal break-all',
      render: (holder) => <AssetCount value={holder.itemCount} />,
    },
  ];
  const bucketColumns: ListColumn<NonNullable<CollectionHoldersProps['distribution']>[number]>[] = [
    { key: 'range', label: 'Holds', minWidth: 65, render: (bucket) => bucket.range },
    {
      key: 'holders',
      label: 'Holders',
      minWidth: 100,
      numeric: true,
      className: '!whitespace-normal break-all',
      render: (bucket) => <AssetCount value={bucket.holders} />,
    },
    {
      key: 'items',
      label: 'Items',
      minWidth: 100,
      numeric: true,
      className: '!whitespace-normal break-all',
      render: (bucket) => <AssetCount value={bucket.items} />,
    },
  ];
  return (
    <DetailSection
      title="Who holds it"
      actions={
        <button
          type="button"
          className="text-jade text-[11px]"
          onClick={() => {
            setAll(!all);
            pagination.reset();
          }}
        >
          {all ? 'Top 10 holders' : 'All holders →'}
        </button>
      }
    >
      <div
        className={
          distribution ? 'grid min-w-0 gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]' : 'min-w-0'
        }
      >
        <div className="min-w-0">
          {holders.data ? (
            <>
              <AssetReadState error={holders.error} label="collection holders" retained />
              <ListTable
                label={all ? 'All collection holders' : 'Top 10 collection holders'}
                columns={columns}
                rows={holders.data.data}
                rowKey={holderKey}
                empty="No current holders."
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
                  className="flex-wrap gap-3 [&>div>span]:text-[11px] [&>div]:flex-wrap [&>div]:gap-2 [&>span]:text-[11px] [&_button]:px-3 [&_button]:text-[11px]"
                />
              )}
            </>
          ) : (
            <AssetReadState
              error={holders.error}
              loading={holders.isLoading}
              label="collection holders"
            />
          )}
        </div>
        {distribution && (
          <div className="min-w-0">
            <p className="text-text-dim mb-2 break-words text-[11px]">
              Full collection: <AssetCount value={totalHolders.toString()} /> holders ·{' '}
              <AssetCount value={totalItems.toString()} /> items
            </p>
            {readsDiffer && (
              <p role="note" className="text-text-dim mb-2 text-[11px]">
                The holder list ({holders.data!.data.length.toLocaleString('en-US')} shown) and the
                distribution ({totalHolders.toLocaleString('en-US')} holder
                {totalHolders === BigInt(1) ? '' : 's'}) were read at different moments.
              </p>
            )}
            <ListTable
              label="Holders by how many items they hold"
              columns={bucketColumns}
              rows={distribution}
              rowKey={(bucket) => bucket.range}
              empty="No current holders."
            />
          </div>
        )}
      </div>
    </DetailSection>
  );
}
