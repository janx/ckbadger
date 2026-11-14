'use client';
import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, isNotFoundApiError } from '@/lib/api';
import { isBadRequestApiError } from '@/lib/api-errors';
import { useRouter } from '@/src/navigation';
import { resolveObjectRouteTarget } from '@/lib/detail-routes';
import { AssetDetailShell } from '@/components/asset/detail-shell';
import { DetailSkeletonPage } from '@/components/ui/detail-skeleton';
import { NotFoundPage } from '@/components/not-found-page';
import { ReadFailure } from '@/components/read-failure';
import { SporeDetailView } from '@/components/asset/spore-detail';
import { CollectionDetail } from '@/components/asset/collection-detail';
import { retryTransientOnly } from '@/lib/query-client';
/** What `resolveObjectRouteTarget` routes (an identity alias redirects), as the not-an-id page says. */
const OBJECT_ID_RULE =
  'an object id is 64 hex digits (a Spore or a cluster) or 48 (an m-NFT class).';
export interface SporeDetailPageProps {
  sporeId: string;
}
export default function SporeDetailPage({ sporeId }: SporeDetailPageProps) {
  const router = useRouter(),
    route = resolveObjectRouteTarget(sporeId);
  const redirect = route.kind === 'redirect' ? route.href : null;
  useEffect(() => {
    if (redirect) router.replace(redirect);
  }, [redirect, router]);
  const id = route.kind === 'spore-object' ? route.assetId : sporeId;
  const object = useQuery({
    queryKey: ['spore', id],
    queryFn: () => api.getSporeObject(id),
    enabled: route.kind === 'spore-object',
    retry: retryTransientOnly,
  });
  // A 32-byte ID names either namespace; only an explicit absent object permits collection lookup.
  const collectionEnabled =
    route.kind === 'spore-object' && !object.data && isNotFoundApiError(object.error);
  const collection = useQuery({
    queryKey: ['object-collection', id],
    queryFn: () => api.getObjectCollection(id),
    enabled: collectionEnabled,
    retry: retryTransientOnly,
  });
  if (redirect) return null;
  if (object.data) return <SporeDetailView key={id} object={object.data} error={object.error} />;
  if (collection.data)
    return (
      <CollectionDetail
        id={id}
        kind={collection.data.standard === 'spore' ? 'spore' : 'object'}
        initial={collection.data}
      />
    );
  if (route.kind === 'unroutable')
    return <NotFoundPage notAnId={{ kind: 'object', reason: OBJECT_ID_RULE }} />;
  // The collection is asked only once the object lookup said 404, so its 404 means neither is.
  if (collectionEnabled && isNotFoundApiError(collection.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'object',
          identifier: id,
          checkedEndpoint: `/assets/objects/${encodeURIComponent(id)}`,
        }}
      />
    );
  }
  const failed = collectionEnabled ? collection.error : object.error;
  if (isBadRequestApiError(failed))
    return <NotFoundPage notAnId={{ kind: 'object', reason: failed.apiMessage }} />;
  if (!failed) return <DetailSkeletonPage />;
  return (
    <AssetDetailShell
      title="Object"
      kind="objects"
      breadcrumb={[{ label: 'Inventory', href: '/inventory/objects' }]}
    >
      <ReadFailure what="object" error={failed} />
    </AssetDetailShell>
  );
}
