'use client';
import { useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, type SporeDetail } from '@/lib/api';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CellVersions } from '@/components/asset/cell-versions';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import { ObjectGeneticsView } from '@/components/asset/genetics';
import { ObjectMedia } from '@/components/asset/object-media';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { ExactCkb } from '@/components/activity-event-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { formatUtc8Date } from '@/lib/utils';
import { exactUnsignedInteger } from '@/lib/exact-integer';
export interface SporeDetailViewProps {
  object: SporeDetail;
  error: Error | null;
}
export function SporeDetailView({ object, error }: SporeDetailViewProps) {
  const id = object.sporeId,
    isDob = object.contentType === 'dob/0' || object.contentType === 'dob/1';
  const client = useQueryClient();
  const versions = useQuery({
    queryKey: ['item-versions', 'spore', id],
    queryFn: () => api.getItemCellVersions('spore', id),
  });
  const protocol = useQuery({
    queryKey: ['item-protocol', 'spore', id],
    queryFn: () => api.getItemProtocol('spore', id),
    retry: false,
  });
  const cluster = useQuery({
    queryKey: ['cluster', object.clusterId],
    queryFn: () => api.getSporeCluster(object.clusterId!),
    enabled: object.clusterId !== null,
  });
  const collectionGenes = useQuery({
    queryKey: ['collection-insights', object.clusterId],
    queryFn: () => api.getCollectionInsights(object.clusterId!),
    enabled: isDob && object.clusterId !== null,
    retry: false,
  });
  const genetics = useQuery({
    queryKey: ['object-genetics', id],
    queryFn: () => api.getObjectGenetics(id),
    enabled: isDob,
    retry: false,
  });
  const decoded = useQuery({
    queryKey: ['spore-dob-decoded', id],
    queryFn: () => api.getSporeObjectDecoded(id),
    enabled: isDob,
    retry: false,
    refetchInterval: (q) =>
      q.state.status !== 'error' && q.state.data?.status === 'pending' ? 2000 : false,
  });
  useEffect(() => {
    if (decoded.data?.status === 'decoded' || decoded.data?.status === 'failed') {
      void client.invalidateQueries({ queryKey: ['spore', id], exact: true });
      void client.invalidateQueries({ queryKey: ['object-genetics', id], exact: true });
    }
  }, [decoded.data?.status, client, id]);
  const data = protocol.data;
  if (data && data.kind !== 'spore')
    throw new Error(`${id}: expected spore protocol, got ${data.kind}`);
  const first = versions.data?.data[0],
    last = versions.data?.data.at(-1);
  const held = exactUnsignedInteger(object.ownedCapacity, `${id}: held capacity`),
    occupied = exactUnsignedInteger(object.ownedKnowledge, `${id}: occupied capacity`);
  if (occupied > held) throw new Error(`${id}: occupied capacity exceeds held capacity`);
  const title =
    cluster.data?.name ??
    (genetics.data?.number !== null && genetics.data?.number !== undefined
      ? `Object #${genetics.data.number}`
      : 'Spore object');
  const facts = [
    {
      label: 'Current owner',
      value: object.ownerLockHash ? (
        <Address address={object.ownerAddress ?? object.ownerLockHash} />
      ) : (
        'No live owner'
      ),
    },
    { label: 'Owner lock', value: last?.lockName ?? 'Unregistered / unavailable' },
    {
      label: 'Created',
      value: first ? (
        <>
          <span>{formatUtc8Date(first.timestamp)} · UTC+8</span>
          <AppLink className="block" href={`/blocks/${first.blockNumber}`}>
            #{first.blockNumber.toLocaleString('en-US')}
          </AppLink>
        </>
      ) : (
        <AppLink href={`/blocks/${object.createdAtBlock}`}>
          #{object.createdAtBlock.toLocaleString('en-US')}
        </AppLink>
      ),
    },
    {
      label: 'Current / final cell',
      value: last ? (
        <AppLink href={`/cell/${last.txHash}-${last.outputIndex}`}>
          Block #{last.blockNumber.toLocaleString('en-US')} · output {last.outputIndex}
        </AppLink>
      ) : (
        'Loading cell history'
      ),
    },
    { label: 'CKB held now', value: <ExactCkb shannons={held} signed={false} tone="plain" wrap /> },
    {
      label: 'Content',
      value: (
        <span className="break-all">
          {object.contentType} · {object.contentSize} bytes
        </span>
      ),
    },
  ];
  return (
    <AssetDetailShell
      title={title}
      kind="objects"
      breadcrumb={[
        { label: 'Inventory', href: '/inventory/objects' },
        { label: 'Objects', href: '/inventory/objects' },
        ...(object.clusterId
          ? [{ label: cluster.data?.name ?? 'Collection', href: `/clusters/${object.clusterId}` }]
          : []),
      ]}
      status={
        <span className="text-text-dim text-[11px]">{object.isLive ? 'Live' : 'Melted'}</span>
      }
      identity={<Hash hash={id} truncate={false} className="text-[11px]" />}
      facts={facts}
      feed={
        <AssetFeed
          key={id}
          queryKey={`spore:${id}`}
          scope="This object's whole life"
          loadActivities={(p) => api.getItemActivityFeed('spore', id, p)}
          loadTransactions={(p) => api.getItemTransactions('spore', id, p)}
          context={{
            label: 'Object · cell after',
            render: (t) => <ItemTransactionContext transaction={t} />,
          }}
        />
      }
      tier2={
        <>
          {data ? (
            <>
              <AssetReadState label="raw object data" error={protocol.error} retained />
              <ItemEvidence protocol={data} />
            </>
          ) : (
            <AssetReadState
              label="raw object data"
              error={protocol.error}
              loading={protocol.isLoading}
            />
          )}
        </>
      }
    >
      <AssetReadState label="object" error={error} retained />
      <ObjectMedia
        object={object}
        contentHex={data?.versions.at(-1)?.decoded.contentHex ?? null}
        decoded={decoded.data}
      />
      {isDob && (
        <>
          <AssetReadState
            label="DOB execution"
            error={decoded.error}
            retained={!!decoded.data}
            loading={decoded.isLoading}
          />
          {genetics.data ? (
            <>
              <AssetReadState label="DNA evidence" error={genetics.error} retained />
              <ObjectGeneticsView
                evidence={genetics.data}
                subject={title}
                collection={collectionGenes.data?.genetics}
              />
              {object.clusterId && (
                <AssetReadState
                  label="collection variants"
                  error={collectionGenes.error}
                  retained={!!collectionGenes.data}
                  loading={collectionGenes.isLoading}
                />
              )}
              {collectionGenes.data && !collectionGenes.data.genetics.complete && (
                <p className="text-text-dim text-[11px]">
                  Collection variant frequencies await complete decoding.
                </p>
              )}
            </>
          ) : (
            <AssetReadState
              label="DNA evidence"
              error={genetics.error}
              loading={genetics.isLoading}
            />
          )}
        </>
      )}
      <section className="space-y-4">
        <h2 className="text-text-bright text-[15px]">CKB held in its cell</h2>
        {!object.isLive && (
          <p className="text-text-dim text-[11px]">
            No live cell holds capacity for this melted object.
          </p>
        )}
        <CapacityPartition
          subject={`Object ${id}`}
          total={object.ownedCapacity}
          occupied={object.ownedKnowledge}
          free={(held - occupied).toString()}
        />
        {versions.data ? (
          <>
            <AssetReadState label="cell versions" error={versions.error} retained />
            <CellVersions subject={title} history={versions.data} />
          </>
        ) : (
          <AssetReadState
            label="cell versions"
            error={versions.error}
            loading={versions.isLoading}
          />
        )}
      </section>
      {object.clusterId && (
        <section className="space-y-4">
          <h2 className="text-text-bright text-[15px]">In this collection</h2>
          {cluster.data ? (
            <>
              <AssetReadState label="collection" error={cluster.error} retained />
              <AppLink href={`/clusters/${object.clusterId}`}>
                {cluster.data.name ?? 'Unnamed collection'} →
              </AppLink>
              <p>
                {cluster.data.sporesCount} objects · {cluster.data.holdersCount} holders
              </p>
              {cluster.data.ownedCapacity !== null && (
                <p>
                  <ExactCkb
                    shannons={BigInt(cluster.data.ownedCapacity)}
                    signed={false}
                    tone="plain"
                    wrap
                  />{' '}
                  held in their cells
                </p>
              )}
            </>
          ) : (
            <AssetReadState label="collection" error={cluster.error} loading={cluster.isLoading} />
          )}
        </section>
      )}
    </AssetDetailShell>
  );
}
