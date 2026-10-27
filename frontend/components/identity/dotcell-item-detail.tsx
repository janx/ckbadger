'use client';
import { useState } from 'react';
import { useQuery, useInfiniteQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api, type DotCellItem } from '@/lib/api';
import { DataField } from '@/components/ui/data-field';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ExactCkb } from '@/components/activity-event-row';
import { CanonicalIdentityDetail } from '@/components/asset/identity-detail';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { NamedParty } from '@/components/asset/identity-document';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatUtc8Date } from '@/lib/utils';
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
    <DataField
      // The endpoint declares no total, so a count is shown only once the
      // whole list is on screen.
      label={hasMore ? 'Children' : `Children (${children.length})`}
      layout="vertical"
      valueClassName="w-full"
    >
      <ul className="space-y-1">
        {children.map((child) => (
          <li key={child.identityId}>
            <Link
              href={nameHref(child.identityId)}
              className="text-emphasis font-mono hover:underline"
            >
              {child.name}
            </Link>
          </li>
        ))}
      </ul>
      {more.isError && (
        <p className="text-negative mt-2 font-mono text-xs">Failed to load more sub-names</p>
      )}
      {hasMore && (
        <button
          type="button"
          disabled={more.isFetching}
          onClick={() => (wantsMore ? void more.fetchNextPage() : setWantsMore(true))}
          className="hover:border-jade hover:text-jade border-base-border bg-base-elevated text-text mt-2 rounded border px-3 py-1 font-mono text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        >
          Show more sub-names
        </button>
      )}
    </DataField>
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
  if (!item)
    return (
      <AssetDetailShell
        title=".cell name"
        kind="identities"
        breadcrumb={[{ label: 'Names', href: '/identities/dotcell' }]}
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
      extraFacts={[
        { label: 'Name state', value: item.state },
        {
          label: 'Grace ends',
          value: `${formatUtc8Date((BigInt(item.graceEndsAt) * BigInt(1000)).toString())} UTC+8`,
        },
        {
          label: 'Namespace args',
          value: <Hash hash={item.namespaceArgs} truncate={false} className="text-[11px]" />,
        },
      ]}
    >
      {item.sale && (
        <section className="space-y-3">
          <h2 className="text-text-bright text-[15px]">For sale</h2>
          <ExactCkb shannons={BigInt(item.sale.priceShannons)} signed={false} tone="plain" wrap />
          <dl>
            <NamedParty party={item.sale.seller} label="Seller" />
          </dl>
          {item.sale.offerOutPoint && (
            <AppLink
              href={`/cell/${item.sale.offerOutPoint.txHash}/${item.sale.offerOutPoint.index}`}
            >
              Sale offer cell
            </AppLink>
          )}
        </section>
      )}
      <section className="space-y-4">
        <h2 className="text-text-bright text-[15px]">Name relationships</h2>
        {item.parent ? (
          <p>
            Parent <AppLink href={nameHref(item.parent.identityId)}>{item.parent.name}</AppLink>
          </p>
        ) : (
          <p className="text-text-dim text-[11px]">Top-level name</p>
        )}
        <DotCellChildren item={item} />
        <p className="min-w-0 break-all text-[11px]">
          Ring successor{' '}
          <AppLink href={nameHref(item.nextId)}>
            <Hash hash={item.nextId} className="text-[11px]" copyable={false} />
          </AppLink>
        </p>
        {ring.data ? (
          <>
            <AssetReadState label="name ring" error={ring.error} retained />
            <AppLink
              href={`/cell/${ring.data.rootOutPoint.txHash}/${ring.data.rootOutPoint.index}`}
              className="text-[11px]"
            >
              Namespace root cell · {ring.data.liveCount} live names
            </AppLink>
          </>
        ) : (
          <AssetReadState label="name ring" error={ring.error} loading={ring.isLoading} />
        )}
      </section>
    </CanonicalIdentityDetail>
  );
}
