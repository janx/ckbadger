'use client';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type CollectionItem, type ItemStatusFilter } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DnaStrip } from '@/components/ui/dna-strip';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { AppLink } from '@/components/ui/app-link';
import { ObjectCompositionLabel } from '@/components/address/address-holdings';
import { AssetReadState } from '@/components/asset/detail-shell';
import { getIdentityItemDetailHref } from '@/lib/detail-routes';
import { formatStandard } from '@/components/activity-event-row';
export interface CollectionGalleryProps {
  id: string;
  traits: Record<string, string>;
}
export function CollectionGallery({ id, traits }: CollectionGalleryProps) {
  const pages = useCursorPagination();
  const query = useQuery({
    queryKey: ['collection-gallery', id, traits, pages.cursor],
    queryFn: () => api.getCollectionGallery(id, { limit: 24, cursor: pages.cursor, traits }),
  });
  return (
    <section aria-label="Collection objects" className="space-y-4">
      <h2 className="text-text-bright text-[15px]">Objects</h2>
      <p className="text-text-dim text-[11px]">
        Whole-collection trait filters · newest created first
      </p>
      {query.data ? (
        <>
          <AssetReadState label="gallery" error={query.error} retained />
          <div className="grid min-w-0 grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-4">
            {query.data.data.map((item) => (
              <article
                key={item.sporeId}
                className="border-base-border min-w-0 space-y-3 border-b py-4"
              >
                <AppLink href={`/objects/${item.sporeId}`} className="block min-w-0">
                  <div className="text-text-bright break-words">
                    {item.number !== null ? (
                      `#${item.number}`
                    ) : (
                      <Hash
                        hash={item.sporeId}
                        startChars={6}
                        endChars={4}
                        copyable={false}
                        className="text-[11px]"
                      />
                    )}
                  </div>
                  {item.dnaHex !== null ? (
                    <DnaStrip dna={item.dnaHex} label={`${item.sporeId} DNA`} />
                  ) : (
                    <div className="text-text-dim py-3 text-[11px]">No DNA</div>
                  )}
                </AppLink>
                {item.mediaProfile && <ObjectCompositionLabel tier={item.mediaProfile.tier} />}
                <div className="text-text-dim text-[11px]">
                  {item.isLive ? 'Live' : 'Melted'} · {item.decodeStatus}
                  <span className="block break-all">
                    {item.contentType} · {item.contentSize.toLocaleString('en-US')} bytes
                  </span>
                  <AppLink href={`/blocks/${item.createdAtBlock}`}>
                    #{item.createdAtBlock.toLocaleString('en-US')}
                  </AppLink>
                </div>
                {item.ownerLockHash ? (
                  <Address
                    address={item.ownerAddress ?? item.ownerLockHash}
                    startChars={8}
                    endChars={4}
                    className="text-[11px]"
                  />
                ) : (
                  <span className="text-text-dim text-[11px]">No live owner</span>
                )}
                <dl className="space-y-1 text-[11px]">
                  {item.traits.map((t, index) => (
                    <div key={`${t.name}:${t.value}:${index}`} className="break-words">
                      <dt className="text-text-dim inline">{t.name}: </dt>
                      <dd className="inline">{t.value}</dd>
                    </div>
                  ))}
                </dl>
              </article>
            ))}
          </div>
          {query.data.data.length === 0 && <p className="text-text-dim">No matching objects.</p>}
          <CursorPagination
            total={query.data.total}
            totalLabel="objects"
            pageSize={24}
            page={pages.page}
            hasMore={query.data.hasMore}
            hasPrevious={pages.hasPrevious}
            onNext={() => pages.goToNext(query.data!.nextCursor)}
            onPrevious={pages.goToPrevious}
          />
        </>
      ) : (
        <AssetReadState label="gallery" error={query.error} loading={query.isLoading} />
      )}
    </section>
  );
}
export interface CollectionItemsProps {
  id: string;
  kind: 'object' | 'identity';
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
                  {item.ownerLockHash ? (
                    <Address address={item.ownerLockHash} className="text-[11px]" />
                  ) : (
                    <span className="text-text-dim text-[11px]">No live owner</span>
                  )}
                </div>
                <AppLink href={`/blocks/${item.createdAtBlock}`} className="text-[11px]">
                  #{item.createdAtBlock.toLocaleString('en-US')}
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
