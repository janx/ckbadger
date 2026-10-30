'use client';
import { useState } from 'react';
import { useQuery, useInfiniteQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api, isNotFoundApiError, type DotCellItem } from '@/lib/api';
import { NotFoundPage } from '@/components/not-found-page';
import { AppLink } from '@/components/ui/app-link';
import { ExactCkb } from '@/components/activity-event-row';
import { CanonicalIdentityDetail } from '@/components/asset/identity-detail';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { NamedParty } from '@/components/asset/identity-document';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
function nameHref(id: string) {
  return `/identities/dotcell/${encodeURIComponent(id)}`;
}
function DotCellChildren({ item }: { item: DotCellItem }) {
  if (item.childrenHasMore !== (item.childrenNextCursor !== null)) {
    throw new Error(
      `.cell name ${item.identityId}: childrenHasMore=${item.childrenHasMore} disagrees with childrenNextCursor=${item.childrenNextCursor}`
    );
  }
  const [wantsMore, setWantsMore] = useState(false);
  const more = useInfiniteQuery({
    queryKey: ['dotcell-item-children', item.identityId, item.childrenNextCursor],
    queryFn: ({ pageParam }) => {
      if (pageParam === null) {
        throw new Error(`.cell name ${item.identityId}: no sub-name cursor to page from`);
      }
      return api.getDotCellItemChildren(item.identityId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: pageParam,
      });
    },
    initialPageParam: item.childrenNextCursor,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled: wantsMore,
    retry: false,
  });

  const appended = more.data ? more.data.pages.flatMap((page) => page.data) : [];
  const children = [...item.children, ...appended];
  const hasMore = more.data ? more.hasNextPage : item.childrenHasMore;

  return (
    <div className="min-w-0 space-y-2">
      {/* The endpoint declares no total, so a count is shown only once the whole list is here. */}
      <h3 className="text-text-dim text-[12px] font-normal">
        {hasMore
          ? 'Sub-names'
          : children.length
            ? `Sub-names · ${children.length.toLocaleString('en-US')}`
            : 'Sub-names · none'}
      </h3>
      <ul className="space-y-1 text-[12px]">
        {children.map((child) => (
          <li key={child.identityId}>
            <Link href={nameHref(child.identityId)} className="text-emphasis hover:underline">
              {child.name}
            </Link>
          </li>
        ))}
      </ul>
      {more.isError && <p className="text-negative mt-2 text-xs">Failed to load more sub-names</p>}
      {hasMore && (
        <button
          type="button"
          disabled={more.isFetching}
          onClick={() => (wantsMore ? void more.fetchNextPage() : setWantsMore(true))}
          className="hover:border-jade hover:text-jade border-base-border bg-base-elevated text-text mt-2 rounded border px-3 py-1 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          Show more sub-names
        </button>
      )}
    </div>
  );
}

export function DotCellItemDetail({ identityId }: { identityId: string }) {
  return <ScopedName key={identityId} itemRef={decodeURIComponent(identityId)} />;
}
function ScopedName({ itemRef }: { itemRef: string }) {
  const detail = useQuery({
    queryKey: ['dotcell-item-detail', itemRef],
    queryFn: () => api.getDotCellItemDetail(itemRef),
    retry: false,
  });
  const ring = useQuery({
    queryKey: ['dotcell-ring'],
    queryFn: () => api.getDotCellRing(),
    retry: false,
  });
  const item = detail.data;
  if (!item && isNotFoundApiError(detail.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'identity',
          identifier: itemRef,
          checkedEndpoint: `/assets/identities/dotcell/items/${encodeURIComponent(itemRef)}`,
        }}
      />
    );
  }
  if (!item)
    return (
      <AssetDetailShell
        title=".cell name"
        kind="identities"
        breadcrumb={[
          { label: 'Inventory', href: '/inventory/tokens' },
          { label: 'Identities', href: '/inventory/identities' },
          { label: '.cell', href: '/identities/dotcell' },
        ]}
      >
        <AssetReadState label=".cell name" error={detail.error} loading={detail.isLoading} />
      </AssetDetailShell>
    );
  return (
    <CanonicalIdentityDetail
      kind="dotcell"
      id={item.identityId}
      title={item.name}
      isLive={item.isLive}
      createdAtBlock={item.createdAtBlock}
      error={detail.error}
      dotcell={item}
      tier2Extra={
        <section aria-label="Name relationships" className="min-w-0 space-y-3 text-[12px]">
          <h2 className="text-text text-[13px] font-medium">Name relationships</h2>
          {item.parent ? (
            <p>
              Parent <AppLink href={nameHref(item.parent.identityId)}>{item.parent.name}</AppLink>
            </p>
          ) : (
            <p className="text-text-dim">Top-level name</p>
          )}
          <DotCellChildren item={item} />
          {ring.data ? (
            <>
              <AssetReadState label="name ring" error={ring.error} retained />
              <AppLink
                href={`/cell/${ring.data.rootOutPoint.txHash}-${ring.data.rootOutPoint.index}`}
              >
                Namespace root cell · {ring.data.liveCount.toLocaleString('en-US')} live names
              </AppLink>
            </>
          ) : (
            <AssetReadState label="name ring" error={ring.error} loading={ring.isLoading} />
          )}
        </section>
      }
    >
      {item.sale && (
        <section aria-label="For sale" className="space-y-3">
          <h2 className="text-text-bright text-[15px] font-medium">For sale</h2>
          <ExactCkb shannons={BigInt(item.sale.priceShannons)} signed={false} tone="plain" wrap />
          <dl>
            <NamedParty party={item.sale.seller} label="Seller" />
          </dl>
          {item.sale.offerOutPoint && (
            <AppLink
              href={`/cell/${item.sale.offerOutPoint.txHash}-${item.sale.offerOutPoint.index}`}
            >
              Sale offer cell
            </AppLink>
          )}
        </section>
      )}
    </CanonicalIdentityDetail>
  );
}
