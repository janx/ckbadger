'use client';

import { useState } from 'react';
import { LockKind } from '@/components/ui/lock-kind';
import { useQuery } from '@tanstack/react-query';
import type { CollectionHolder, CursorPaginatedResponse } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { AssetCount } from '@/components/asset/inventory-table';
import { AssetReadState } from '@/components/asset/detail-shell';
import { DotCellParty } from '@/components/identity/dotcell-party';
import { DetailSection } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { PhoneRow } from '@/components/ui/phone-row';

export interface CollectionHoldersProps {
  load: (params: {
    limit?: number;
    cursor?: string;
  }) => Promise<CursorPaginatedResponse<CollectionHolder>>;
  queryKey: string;
  distribution?: readonly { range: string; holders: number; items: number }[];
  /** F2 objects (OB-C7): draw the counts as bars in this class colour (Collection board). */
  assetClass?: AssetClass;
}

/** F2 objects (OB-C7): a count with its bar, as long as its share of the largest in the list. */
function CountBar({ value, max, color }: { value: number; max: number; color: string }) {
  return (
    <span className="inline-flex items-center justify-end gap-2">
      <span
        aria-hidden
        data-testid="count-bar"
        className="bg-base-elevated inline-block h-1.5 w-16 overflow-hidden rounded-sm"
      >
        <span
          className="block h-full"
          style={{ width: `${max ? (value / max) * 100 : 0}%`, backgroundColor: color }}
        />
      </span>
      <AssetCount value={value} />
    </span>
  );
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

function ScopedCollectionHolders({
  load,
  queryKey,
  distribution,
  assetClass,
}: CollectionHoldersProps) {
  const [all, setAll] = useState(false);
  const pagination = useCursorPagination();
  // "All holders" pages by the list size (R2: twenty below md); the top ten stay ten.
  const allPageSize = usePageSize(50);
  const limit = all ? allPageSize : 10;
  const holders = useQuery({
    queryKey: ['collection-holders', queryKey, all, pagination.cursor, limit],
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
  const rank = (holder: CollectionHolder) =>
    (pagination.page - 1) * limit + holders.data!.data.indexOf(holder) + 1;
  const party = (holder: CollectionHolder) => (
    <DotCellParty
      party={{
        hashPrefix: holder.ownerHashPrefix ?? null,
        lockHash: holder.lockScriptHash,
        address: holder.address,
      }}
      truncate
      className="max-w-full break-all [&>span]:text-xs"
    />
  );
  const columns: ListColumn<CollectionHolder>[] = [
    {
      key: 'rank',
      label: '#',
      minWidth: 28,
      render: (holder) => <span className="text-text-dim">{rank(holder)}</span>,
    },
    {
      key: 'owner',
      label: 'Holder',
      minWidth: 176,
      render: party,
    },
    {
      key: 'lock',
      label: 'Lock',
      minWidth: 104,
      className: 'text-meta',
      render: (holder) => (
        <LockKind lockKind={holder.lockKind} lockCodeHash={holder.lockCodeHash} />
      ),
    },
    {
      key: 'items',
      label: 'Items',
      minWidth: 140,
      numeric: true,
      className: '!whitespace-normal break-all',
      render: (holder) =>
        assetClass ? (
          <CountBar
            value={holder.itemCount}
            max={Math.max(...holders.data!.data.map((h) => h.itemCount))}
            color={CLASS_COLORS[assetClass]}
          />
        ) : (
          <AssetCount value={holder.itemCount} />
        ),
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
      render: (bucket) =>
        assetClass ? (
          <CountBar
            value={bucket.holders}
            max={Math.max(...(distribution ?? []).map((b) => b.holders))}
            color={CLASS_COLORS[assetClass]}
          />
        ) : (
          <AssetCount value={bucket.holders} />
        ),
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
          className="text-jade text-meta"
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
                mobileRow={(holder, wide) => {
                  const max = Math.max(...holders.data!.data.map((h) => h.itemCount));
                  return (
                    <HolderPhoneRow
                      rank={rank(holder)}
                      party={party(holder)}
                      lock={
                        <LockKind lockKind={holder.lockKind} lockCodeHash={holder.lockCodeHash} />
                      }
                      amount={
                        <span className="whitespace-nowrap">
                          <AssetCount value={holder.itemCount} />{' '}
                          <span className="text-text-dim text-xs">
                            {holder.itemCount === 1 ? 'item' : 'items'}
                          </span>
                        </span>
                      }
                      // The bar is the holder's count against the largest on the page, as the
                      // table's Items column draws it.
                      barWidth={`${max ? (holder.itemCount / max) * 100 : 0}%`}
                      barColor={assetClass ? CLASS_COLORS[assetClass] : undefined}
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
                  className="[&>div>span]:text-meta [&>span]:text-meta [&_button]:text-meta flex-wrap gap-3 [&>div]:flex-wrap [&>div]:gap-2 [&_button]:px-3"
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
            <p className="text-text-dim text-meta mb-2 break-words">
              Full collection: <AssetCount value={totalHolders.toString()} /> holders ·{' '}
              <AssetCount value={totalItems.toString()} /> items
            </p>
            {readsDiffer && (
              <p role="note" className="text-text-dim text-meta mb-2">
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
              mobileRow={(bucket) => (
                <PhoneRow
                  testId="holder-bucket-row-mobile"
                  lines={[
                    {
                      key: 'range',
                      tone: 'body',
                      left: (
                        <>
                          holds {bucket.range}
                          <span className="text-text-dim">
                            {' · '}
                            <AssetCount value={bucket.items} />{' '}
                            {bucket.items === 1 ? 'item' : 'items'}
                          </span>
                        </>
                      ),
                      right: (
                        <span className="inline-flex items-center gap-1.5">
                          {bucketColumns[1].render(bucket)}
                          <span className="text-text-dim">
                            {bucket.holders === 1 ? 'holder' : 'holders'}
                          </span>
                        </span>
                      ),
                    },
                  ]}
                />
              )}
            />
          </div>
        )}
      </div>
    </DetailSection>
  );
}
