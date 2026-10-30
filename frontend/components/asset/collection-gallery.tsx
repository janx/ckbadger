'use client';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type CollectionItem, type ItemStatusFilter } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { AppLink } from '@/components/ui/app-link';
import { AssetReadState } from '@/components/asset/detail-shell';
import { getIdentityItemDetailHref } from '@/lib/detail-routes';
import { formatStandard } from '@/components/activity-event-row';
import { formatBlockNumber } from '@/lib/format-block';
// F2 objects (round 2, OB-C3): the 24-card gallery was replaced by the Collection board's
// Newest list (`collection-objects.tsx`).
export interface CollectionItemsProps {
  id: string;
  kind: 'object' | 'identity';
}
// ── F2 identities (round 2): R3 owners on item rows (ID-COL-2 / ID-COL-3) ──
/**
 * An item row's owner as the API names it: its address; a lock hash whose script is not
 * indexed, as a hash; a `.cell` owner prefix no indexed lock has, as that prefix. A lock hash is
 * never passed off as an address, and a named owner is never reported as "No live owner".
 */
function ItemOwner({ item }: { item: CollectionItem }) {
  if (item.ownerAddress) return <Address address={item.ownerAddress} className="text-[11px]" />;
  if (item.ownerLockHash)
    return (
      <span className="text-[11px]">
        <Hash hash={item.ownerLockHash} copyable={false} className="text-[11px]" />
        <span className="text-text-dim block">address not indexed</span>
      </span>
    );
  if (item.ownerHashPrefix)
    return (
      <span className="text-[11px] [overflow-wrap:anywhere]">
        <Hash
          hash={item.ownerHashPrefix}
          truncate={false}
          copyable={false}
          className="text-[11px]"
        />
        <span className="text-text-dim block">named owner prefix · no indexed lock</span>
      </span>
    );
  if (item.isLive)
    return (
      <span role="alert" className="text-text-dim text-[11px]">
        {item.nftId}: a live item with no owner in the API response
      </span>
    );
  return <span className="text-text-dim text-[11px]">No live owner</span>;
}

export function CollectionItems({ id, kind }: CollectionItemsProps) {
  const [search, setSearch] = useState(''),
    [status, setStatus] = useState<ItemStatusFilter>('all');
  const pages = useCursorPagination();
  const query = useQuery({
    queryKey: ['collection-items', kind, id, search, status, pages.cursor],
    queryFn: () =>
      kind === 'object'
        ? api.getObjectCollectionItems(id, {
            limit: 24,
            cursor: pages.cursor,
            search: search || undefined,
            status,
          })
        : api.getIdentityCollectionItems(id, {
            limit: 24,
            cursor: pages.cursor,
            search: search || undefined,
            status,
          }),
  });
  const href = (item: CollectionItem) =>
    kind === 'identity'
      ? getIdentityItemDetailHref(item.standard, item.nftId)
      : item.standard === 'm-nft'
        ? `/objects/mnft/${item.nftId}`
        : `/objects/${item.nftId}`;
  return (
    <section aria-label="Collection items" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-text-bright text-[15px]">
          {kind === 'identity' ? 'Identities' : 'Objects'}
        </h2>
        <div className="flex max-w-full flex-wrap gap-3">
          <input
            aria-label="Search collection"
            value={search}
            onChange={(e) => {
              pages.reset();
              setSearch(e.target.value);
            }}
            className="bg-base-bg border-base-border max-w-full border-b py-2"
            placeholder="Search names or IDs"
          />
          <select
            aria-label="Item status"
            value={status}
            onChange={(e) => {
              pages.reset();
              setStatus(e.target.value as ItemStatusFilter);
            }}
            className="bg-base-bg border-base-border border-b py-2"
          >
            <option value="all">All</option>
            <option value="live">Live</option>
            <option value="recycled">Inactive</option>
          </select>
        </div>
      </div>
      {query.data ? (
        <>
          <AssetReadState label="collection items" error={query.error} retained />
          <div className="divide-base-border divide-y">
            {query.data.data.map((item) => (
              <article
                key={item.nftId}
                className="grid min-w-0 gap-3 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"
              >
                <div className="min-w-0">
                  <AppLink href={href(item)} className="text-text-bright break-words">
                    {item.name ?? (
                      <Hash hash={item.nftId} copyable={false} className="text-[11px]" />
                    )}
                  </AppLink>
                  <span className="text-text-dim mt-1 block text-[11px]">
                    {formatStandard(item.standard)} · {item.isLive ? 'Live' : 'Inactive'}
                  </span>
                </div>
                <div className="min-w-0">
                  <ItemOwner item={item} />
                </div>
                <AppLink href={`/blocks/${item.createdAtBlock}`} className="text-[11px]">
                  {formatBlockNumber(item.createdAtBlock)}
                </AppLink>
              </article>
            ))}
          </div>
          {query.data.data.length === 0 && <p className="text-text-dim">No matching items.</p>}
          <CursorPagination
            total={query.data.total}
            totalLabel="items"
            pageSize={24}
            page={pages.page}
            hasMore={query.data.hasMore}
            hasPrevious={pages.hasPrevious}
            onNext={() => pages.goToNext(query.data!.nextCursor)}
            onPrevious={pages.goToPrevious}
          />
        </>
      ) : (
        <AssetReadState label="collection items" error={query.error} loading={query.isLoading} />
      )}
    </section>
  );
}
