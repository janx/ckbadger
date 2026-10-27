'use client';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type ObjectCollection, type IdentityCollection } from '@/lib/api';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CollectionGallery, CollectionItems } from '@/components/asset/collection-gallery';
import { CollectionHolders } from '@/components/asset/collection-holders';
import { CollectionGenes, CurrentPatternEvidence } from '@/components/asset/genetics';
import { MnftPermissions } from '@/components/asset/identity-document';
import { CapacityHistory } from '@/components/asset/capacity-history';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { ObjectCompositionLabel } from '@/components/address/address-holdings';
import { Hash } from '@/components/ui/hash';
import { AppLink } from '@/components/ui/app-link';
import { Address } from '@/components/ui/address';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { parseSporeClusterDescription } from '@/lib/spore-cluster-description';
import { isDidCkbCollectionAlias } from '@/lib/detail-routes';
import { exactUnsignedInteger } from '@/lib/exact-integer';
export interface CollectionDetailProps {
  id: string;
  kind: 'spore' | 'object' | 'identity';
  initial?: ObjectCollection;
}
export function CollectionDetail(props: CollectionDetailProps) {
  return <ScopedCollectionDetail key={`${props.kind}:${props.id}`} {...props} />;
}
function ScopedCollectionDetail({ id, kind, initial }: CollectionDetailProps) {
  const [traits, setTraits] = useState<Record<string, string>>({});
  const detail = useQuery<ObjectCollection | IdentityCollection>({
    queryKey: [kind === 'identity' ? 'identity-collection' : 'object-collection', id],
    queryFn: () =>
      kind === 'identity' ? api.getIdentityCollection(id) : api.getObjectCollection(id),
    initialData: initial,
  });
  const cluster = useQuery({
    queryKey: ['cluster', id],
    queryFn: () => api.getSporeCluster(id),
    enabled: kind === 'spore',
  });
  const protocol = useQuery({
    queryKey: ['cluster-protocol', id],
    queryFn: () => api.getSporeClusterProtocol(id),
    enabled: kind === 'spore',
    retry: false,
  });
  const insights = useQuery({
    queryKey: ['collection-insights', id],
    queryFn: () => api.getCollectionInsights(id),
    enabled: kind === 'spore',
    retry: false,
  });
  const chart = useQuery({
    queryKey: ['collection-capacity', kind, id],
    queryFn: () =>
      kind === 'spore'
        ? api.getSporeClusterCapacityChart(id)
        : kind === 'identity'
          ? api.getIdentityCollectionCapacityChart(id)
          : api.getObjectCollectionCapacityChart(id),
  });
  const pds = useQuery({
    queryKey: ['did-pds'],
    queryFn: () => api.getDidPdsDistribution(),
    enabled: kind === 'identity' && isDidCkbCollectionAlias(id),
  });
  const data = detail.data,
    object = data && 'composition' in data ? data : undefined;
  const description =
    cluster.data?.description === null || cluster.data?.description === undefined
      ? null
      : parseSporeClusterDescription(cluster.data.description);
  const held =
    data?.ownedCapacity === null || data?.ownedCapacity === undefined
      ? null
      : exactUnsignedInteger(data.ownedCapacity, `${id}: collection held`);
  const occupied =
    data?.ownedKnowledge === null || data?.ownedKnowledge === undefined
      ? null
      : exactUnsignedInteger(data.ownedKnowledge, `${id}: collection occupied`);
  if (held !== null && occupied !== null && occupied > held)
    throw new Error(`${id}: occupied exceeds collection capacity`);
  if (data && data.liveCount > data.totalCount)
    throw new Error(`${id}: live collection count exceeds total`);
  const title =
    kind === 'spore'
      ? (cluster.data?.name ?? 'Unnamed collection')
      : (data?.name ??
        (kind === 'identity' ? formatStandard(data?.standard ?? id) : 'Object collection'));
  if (!data)
    return (
      <AssetDetailShell
        title={title}
        kind={kind === 'identity' ? 'identities' : 'objects'}
        breadcrumb={[
          {
            label: 'Inventory',
            href: `/inventory/${kind === 'identity' ? 'identities' : 'objects'}`,
          },
        ]}
      >
        <AssetReadState label="collection" error={detail.error} loading={detail.isLoading} />
      </AssetDetailShell>
    );
  const facts = [
    { label: 'Standard', value: formatStandard(data.standard) },
    { label: 'Live', value: data.liveCount.toLocaleString('en-US') },
    { label: 'Minted / created', value: data.totalCount.toLocaleString('en-US') },
    {
      label: 'Inactive / burned',
      value: (data.totalCount - data.liveCount).toLocaleString('en-US'),
    },
    { label: 'Holders', value: data.holdersCount.toLocaleString('en-US') },
    { label: 'Activities', value: data.activitiesCount.toLocaleString('en-US') },
    {
      label: 'CKB held',
      value:
        held !== null ? (
          <ExactCkb shannons={held} signed={false} tone="plain" wrap />
        ) : (
          'Not recorded'
        ),
    },
    ...(cluster.data
      ? [
          {
            label: 'Cluster cell owner',
            value: protocol.data ? (
              protocol.data.current ? (
                <Address address={protocol.data.current.cell.ownerAddress} />
              ) : (
                'No current cluster cell'
              )
            ) : (
              'Current cell evidence unavailable'
            ),
          },
          {
            label: 'Created',
            value:
              cluster.data.createdAtBlock === null ? (
                'Not recorded'
              ) : (
                <AppLink href={`/blocks/${cluster.data.createdAtBlock}`}>
                  #{cluster.data.createdAtBlock.toLocaleString('en-US')}
                </AppLink>
              ),
          },
        ]
      : []),
  ];
  return (
    <AssetDetailShell
      title={title}
      kind={kind === 'identity' ? 'identities' : 'objects'}
      breadcrumb={[
        {
          label: 'Inventory',
          href: `/inventory/${kind === 'identity' ? 'identities' : 'objects'}`,
        },
        {
          label: kind === 'identity' ? 'Identities' : 'Objects',
          href: `/inventory/${kind === 'identity' ? 'identities' : 'objects'}`,
        },
      ]}
      identity={<Hash hash={id} truncate={false} className="text-[11px]" />}
      facts={facts}
      intro={
        <>
          {object?.composition && <ObjectCompositionLabel tier={object.composition.tier} />}
          <p className="text-text-dim mt-3 text-[13px]">
            {description?.summary ??
              object?.classDetail?.description ??
              'No collection description recorded.'}
          </p>
        </>
      }
      feed={
        <AssetFeed
          queryKey={`${kind}:${id}`}
          scope="This collection"
          loadActivities={(p) => api.getCollectionActivityFeed(kind, id, p)}
          loadTransactions={(p) => api.getCollectionTransactions(kind, id, p)}
        />
      }
      tier2={
        <>
          {kind === 'spore' && (
            <section aria-label="Cluster cell evidence" className="space-y-4">
              <AssetReadState
                label="cluster cell"
                error={protocol.error}
                retained={!!protocol.data}
                loading={protocol.isLoading}
              />
              {protocol.data?.current ? (
                <>
                  <AppLink
                    href={`/cell/${protocol.data.current.cell.txHash}/${protocol.data.current.cell.outputIndex}`}
                  >
                    Current cluster cell →
                  </AppLink>
                  <ProtocolEvidence
                    title="Raw cluster cell data"
                    dataHex={protocol.data.current.dataHex}
                    fields={protocol.data.current.fields}
                    decoded={protocol.data.current.decoded}
                  />
                </>
              ) : (
                protocol.data && (
                  <p className="text-text-dim text-[11px]">
                    No current cell carries this cluster type.
                  </p>
                )
              )}
            </section>
          )}
          {insights.data && <CurrentPatternEvidence pattern={insights.data.currentPattern} />}
          <section className="space-y-4">
            <h2 className="text-text-bright text-[15px]">Collection metadata</h2>
            {cluster.data && (
              <pre className="whitespace-pre-wrap break-all text-[11px]">
                {cluster.data.description ?? 'No description bytes recorded.'}
              </pre>
            )}
            {object?.classDetail && (
              <>
                <div className="break-all text-[11px]">
                  Class ID <Hash hash={object.classDetail.classId} truncate={false} />
                </div>
                <div className="break-all text-[11px]">
                  Issuer ID <Hash hash={object.classDetail.issuerId} truncate={false} />
                </div>
                <p className="text-[11px]">
                  Issued {object.classDetail.issued} / planned {object.classDetail.total}
                </p>
                <p className="break-all text-[11px]">
                  Renderer {object.classDetail.renderer ?? 'not recorded'}
                </p>
                {object.issuerDetail && (
                  <pre className="whitespace-pre-wrap break-all text-[11px]">
                    {JSON.stringify(object.issuerDetail, null, 2)}
                  </pre>
                )}
              </>
            )}
          </section>
        </>
      }
    >
      <AssetReadState label="collection" error={detail.error} retained />
      {kind === 'spore' && (
        <AssetReadState
          label="cluster metadata"
          error={cluster.error}
          retained={!!cluster.data}
          loading={cluster.isLoading}
        />
      )}
      {held !== null && occupied !== null ? (
        <CapacityPartition
          subject={title}
          total={held.toString()}
          occupied={occupied.toString()}
          free={(held - occupied).toString()}
        />
      ) : (
        <p className="text-text-dim text-[11px]">Collection capacity is not recorded.</p>
      )}
      {chart.data ? (
        <>
          <AssetReadState label="capacity history" error={chart.error} retained />
          <CapacityHistory
            history={chart.data}
            subject={title}
            weekly={insights.data?.weeklyActivity}
          />
        </>
      ) : (
        <AssetReadState label="capacity history" error={chart.error} loading={chart.isLoading} />
      )}
      {kind === 'spore' && (
        <>
          {insights.data ? (
            <>
              <AssetReadState label="collection insights" error={insights.error} retained />
              <CollectionGenes
                evidence={insights.data.genetics}
                selected={traits}
                onSelect={(name, value) =>
                  setTraits((prev) => {
                    const next = { ...prev };
                    if (value === null) delete next[name];
                    else next[name] = value;
                    return next;
                  })
                }
              />
            </>
          ) : (
            <AssetReadState
              label="collection insights"
              error={insights.error}
              loading={insights.isLoading}
            />
          )}
          <CollectionGallery key={`${id}:${JSON.stringify(traits)}`} id={id} traits={traits} />
        </>
      )}
      {kind !== 'spore' && <CollectionItems id={id} kind={kind} />}
      {object?.classDetail && (
        <MnftPermissions
          permissions={object.classDetail.permissions}
          configure={object.classDetail.configure}
        />
      )}
      {kind === 'identity' && isDidCkbCollectionAlias(id) && (
        <section aria-label="PDS distribution" className="space-y-4">
          <h2 className="text-text-bright text-[15px]">Personal data servers</h2>
          {pds.data ? (
            <>
              <AssetReadState label="PDS distribution" error={pds.error} retained />
              <p className="text-text-dim text-[11px]">
                {pds.data.totalLiveIdentities} live identities · {pds.data.withoutPds} without a
                PDS. An identity can name more than one endpoint.
              </p>
              <dl className="space-y-3">
                {pds.data.groups.map((g) => (
                  <div key={g.endpoint} className="flex flex-wrap justify-between gap-3">
                    <dt className="min-w-0 break-all">{g.endpoint}</dt>
                    <dd>{g.count} identities</dd>
                  </div>
                ))}
              </dl>
            </>
          ) : (
            <AssetReadState label="PDS distribution" error={pds.error} loading={pds.isLoading} />
          )}
        </section>
      )}
      <CollectionHolders
        queryKey={`${kind}:${id}`}
        distribution={insights.data?.holderDistribution}
        load={(p) =>
          kind === 'spore'
            ? api.getSporeClusterHolders(id, p)
            : kind === 'object'
              ? api.getObjectCollectionHolders(id, p)
              : api.getIdentityCollectionHolders(id, p)
        }
      />
    </AssetDetailShell>
  );
}
