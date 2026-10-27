'use client';
import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, isNotFoundApiError } from '@/lib/api';
import { useRouter } from '@/src/navigation';
import { resolveObjectRouteTarget } from '@/lib/detail-routes';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { SporeDetailView } from '@/components/asset/spore-detail';
import { CollectionDetail } from '@/components/asset/collection-detail';
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
    retry: false,
  });
  // A 32-byte ID names either namespace; only an explicit absent object permits collection lookup.
  const collectionEnabled =
    route.kind === 'spore-object' && !object.data && isNotFoundApiError(object.error);
  const collection = useQuery({
    queryKey: ['object-collection', id],
    queryFn: () => api.getObjectCollection(id),
    enabled: collectionEnabled,
    retry: false,
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
  const absent =
    route.kind === 'unroutable' || (collectionEnabled && isNotFoundApiError(collection.error));
  return (
    <AssetDetailShell
      title={absent ? 'Asset not found' : 'Object'}
      kind="objects"
      breadcrumb={[{ label: 'Inventory', href: '/inventory/objects' }]}
    >
      {absent ? (
        <p className="text-text-dim">No object or collection matches this identifier.</p>
      ) : (
        <AssetReadState
          label="object"
          error={collectionEnabled ? collection.error : object.error}
          loading={object.isLoading || (collectionEnabled && collection.isLoading)}
        />
      )}
    </AssetDetailShell>
  );
}
