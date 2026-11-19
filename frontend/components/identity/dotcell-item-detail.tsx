'use client';
import { useState } from 'react';
import { useQuery, useInfiniteQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api, isNotFoundApiError, type DotCellItem } from '@/lib/api';
import { isBadRequestApiError } from '@/lib/api-errors';
import { NotFoundPage } from '@/components/not-found-page';
import { ReadFailure } from '@/components/read-failure';
import { AppLink } from '@/components/ui/app-link';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { CanonicalIdentityDetail } from '@/components/asset/identity-detail';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { DetailSkeletonPage } from '@/components/ui/detail-skeleton';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { Pager } from '@/components/ui/pager';
import { NamedParty } from '@/components/asset/identity-document';
import { ENTITY_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { retryTransientOnly } from '@/lib/query-client';
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
  const pageSize = usePageSize(ENTITY_PAGE_SIZE);
  const more = useInfiniteQuery({
    queryKey: ['dotcell-item-children', item.identityId, item.childrenNextCursor, pageSize],
    queryFn: ({ pageParam }) => {
      if (pageParam === null) {
        throw new Error(`.cell name ${item.identityId}: no sub-name cursor to page from`);
      }
      return api.getDotCellItemChildren(item.identityId, {
        limit: pageSize,
        cursor: pageParam,
      });
    },
    initialPageParam: item.childrenNextCursor,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled: wantsMore,
    retry: retryTransientOnly,
  });

  const appended = more.data ? more.data.pages.flatMap((page) => page.data) : [];
  const children = [...item.children, ...appended];
  const hasMore = more.data ? more.hasNextPage : item.childrenHasMore;

  return (
    <div className="min-w-0 space-y-2">
      {/* The endpoint declares no total, so a count is shown only once the whole list is here. */}
      {children.length === 0 && !hasMore ? (
        // C16: nothing to list is a line, not a heading over an empty list.
        <p className="text-text-dim text-small">Sub-names · none</p>
      ) : (
        <h3 className="text-text-dim text-small font-normal">
          {hasMore ? 'Sub-names' : `Sub-names · ${children.length.toLocaleString('en-US')}`}
        </h3>
      )}
      {children.length > 0 && (
        <ul className="text-small space-y-1">
          {children.map((child) => (
            <li key={child.identityId}>
              <Link href={nameHref(child.identityId)} className="text-emphasis hover:underline">
                {child.name}
              </Link>
            </li>
          ))}
        </ul>
      )}
      {more.isError && (
        <p className="text-negative text-small mt-2">Failed to load more sub-names</p>
      )}
      {hasMore && (
        // the controls board's short-list form, "next 20 ↓": the rows the next load asks for; a
        // load in flight is not asked for again
        <div className="mt-2">
          <Pager
            mode="more"
            next={
              more.isFetching
                ? undefined
                : {
                    count: pageSize,
                    onShow: () => (wantsMore ? void more.fetchNextPage() : setWantsMore(true)),
                  }
            }
          />
        </div>
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
    retry: retryTransientOnly,
  });
  const ring = useQuery({
    queryKey: ['dotcell-ring'],
    queryFn: () => api.getDotCellRing(),
    retry: retryTransientOnly,
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
  if (!item && isBadRequestApiError(detail.error))
    return <NotFoundPage notAnId={{ kind: 'identity', reason: detail.error.apiMessage }} />;
  if (!item) {
    if (!detail.error) return <DetailSkeletonPage />;
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
        <ReadFailure what=".cell name" error={detail.error} />
      </AssetDetailShell>
    );
  }
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
        <section aria-label="Name relationships" className="text-small min-w-0 space-y-3">
          <h2 className="text-text text-body font-medium">Name relationships</h2>
          {item.parent ? (
            <p className="max-w-measure">
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
            <AssetReadState label="name ring" error={ring.error} read={ring} />
          )}
        </section>
      }
    >
      {item.sale && (
        <section aria-label="For sale" className="space-y-3">
          <PromptHeading command="For sale" />
          <CkbAmount
            form="column"
            shannons={BigInt(item.sale.priceShannons)}
            signed={false}
            tone="plain"
            wrap
          />
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
