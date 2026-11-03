'use client';
import { useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type CollectionItem, type ItemStatusFilter } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
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
  if (item.ownerAddress) return <Address address={item.ownerAddress} className="text-meta" />;
  if (item.ownerLockHash)
    return (
      <span className="text-meta">
        <Hash hash={item.ownerLockHash} copyable={false} className="text-meta" />
        <span className="text-text-dim block">address not indexed</span>
      </span>
    );
  if (item.ownerHashPrefix)
    return (
      <span className="text-meta [overflow-wrap:anywhere]">
        <Hash hash={item.ownerHashPrefix} truncate={false} copyable={false} className="text-meta" />
        <span className="text-text-dim block">named owner prefix · no indexed lock</span>
      </span>
    );
  if (item.isLive)
    return (
      <span role="alert" className="text-text-dim text-meta">
        {item.nftId}: a live item with no owner in the API response
      </span>
    );
  return <span className="text-text-dim text-meta">No live owner</span>;
}

/**
 * Round 3 (contracts R-A · R-1, OB-K3): an m-NFT token has no name of its own; its row reads
 * "<class name> #<token index>" (only "#<index>" for a class without an on-chain name).
 */
function mnftRowTitle(item: CollectionItem): ReactNode {
  if (item.tokenIndex === undefined)
    return (
      <span role="alert" className="text-rouge text-small font-mono">
        m-NFT item {item.nftId} has no tokenIndex
      </span>
    );
  return item.name === null ? `#${item.tokenIndex}` : `${item.name} #${item.tokenIndex}`;
}

export function CollectionItems({ id, kind }: CollectionItemsProps) {
  const [search, setSearch] = useState(''),
    // Live first: live cells are the state (DATA_DESIGN); a .bit collection is 97 % recycled names,
    // so 'all' opened on a screen of names with no live owner (review 2026-11-03, item 3).
    [status, setStatus] = useState<ItemStatusFilter>('live');
  const pages = useCursorPagination();
  const pageSize = usePageSize(24);
  const query = useQuery({
    queryKey: ['collection-items', kind, id, search, status, pages.cursor, pageSize],
    queryFn: () =>
      kind === 'object'
        ? api.getObjectCollectionItems(id, {
            limit: pageSize,
            cursor: pages.cursor,
            search: search || undefined,
            status,
          })
        : api.getIdentityCollectionItems(id, {
            limit: pageSize,
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
        <h2 className="text-text-bright text-section">
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
                    {/* F2 identities (round 2): a did:ckb row has no name but its DID (R2b-b-22). */}
                    {item.standard === 'm-nft'
                      ? mnftRowTitle(item)
                      : (item.name ??
                        item.did ?? (
                          <Hash hash={item.nftId} copyable={false} className="text-meta" />
                        ))}
                  </AppLink>
                  <span className="text-text-dim text-meta mt-1 block">
                    {formatStandard(item.standard)} · {item.isLive ? 'Live' : 'Inactive'}
                  </span>
                </div>
                <div className="min-w-0">
                  <ItemOwner item={item} />
                </div>
                <AppLink href={`/blocks/${item.createdAtBlock}`} className="text-meta">
                  {formatBlockNumber(item.createdAtBlock)}
                </AppLink>
              </article>
            ))}
          </div>
          {query.data.data.length === 0 && <p className="text-text-dim">No matching items.</p>}
          <CursorPagination
            total={query.data.total}
            totalLabel="items"
            pageSize={pageSize}
            page={pages.page}
            currentCount={query.data.data.length}
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
