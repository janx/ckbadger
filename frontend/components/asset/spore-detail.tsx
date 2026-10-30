'use client';
import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, type SporeDetail } from '@/lib/api';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CellVersions } from '@/components/asset/cell-versions';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import { ObjectGeneticsEvidence, ObjectGeneticsView } from '@/components/asset/genetics';
import { ObjectMedia } from '@/components/asset/object-media';
import {
  CompositionLine,
  LiveBadge,
  ObjectTraits,
  cellLifeSentence,
  ckbText,
  itemLifeSentence,
  lockFootnote,
} from '@/components/asset/object-hero';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { ExactCkb } from '@/components/activity-event-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { formatUtc8DateTime } from '@/lib/utils';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';
import { sequencingGenes } from '@/lib/dob-genes';
import { COMPOSITION_BADGES } from '@/lib/design-tokens';
import { formatPercent } from '@/lib/ckb-decimal';

export interface SporeDetailViewProps {
  object: SporeDetail;
  error: Error | null;
}
export function SporeDetailView({ object, error }: SporeDetailViewProps) {
  const id = object.sporeId,
    isDob = object.contentType === 'dob/0' || object.contentType === 'dob/1';
  const client = useQueryClient();
  // F2 objects (OB-5): one gene lit across the trait list, the sequencing gel and the allele gel.
  const [activeGene, setActiveGene] = useState<string | null>(null);
  const [now] = useState(() => Date.now());
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
  const collection = useQuery({
    queryKey: ['object-collection', object.clusterId],
    queryFn: () => api.getObjectCollection(object.clusterId!),
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
  const history = versions.data?.data;
  const first = history?.[0],
    last = history?.at(-1);
  const held = exactUnsignedInteger(object.ownedCapacity, `${id}: held capacity`),
    occupied = exactUnsignedInteger(object.ownedKnowledge, `${id}: occupied capacity`);
  if (occupied > held) throw new Error(`${id}: occupied capacity exceeds held capacity`);
  const title =
    cluster.data?.name ??
    (genetics.data?.number !== null && genetics.data?.number !== undefined
      ? `Object #${genetics.data.number}`
      : 'Spore object');
  const collectionName = cluster.data?.name ?? null;
  const collectionHref = object.clusterId ? `/clusters/${object.clusterId}` : undefined;
  const tier = object.mediaProfile?.tier;
  const geneColors = useMemo(() => {
    const colors = new Map<string, string>();
    if (genetics.data?.dnaHex)
      for (const g of sequencingGenes(genetics.data, title).genes)
        if (!colors.has(g.name)) colors.set(g.name, g.color);
    return colors;
  }, [genetics.data, title]);
  const moves = history ? history.length - 1 : null;
  const facts = [
    {
      label: 'Owner',
      value: object.ownerLockHash ? (
        <>
          <Address address={object.ownerAddress ?? object.ownerLockHash} />
          <span className="text-text-dim mt-1 block text-[12px]">
            {last?.lockName ?? (history ? 'Unregistered lock' : 'Loading lock…')}
          </span>
        </>
      ) : (
        'No live owner'
      ),
    },
    ...(object.clusterId
      ? [
          {
            label: 'Collection',
            value: (
              <>
                <AppLink href={collectionHref!}>{collectionName ?? 'Unnamed collection'}</AppLink>
                {collection.data && (
                  <span className="text-text-dim text-[12px]">
                    {' '}
                    · {collection.data.liveCount.toLocaleString('en-US')} objects
                  </span>
                )}
              </>
            ),
          },
        ]
      : []),
    {
      label: 'Minted',
      value: first ? (
        <>
          <span>{formatUtc8DateTime(first.timestamp)}</span>
          <span className="text-text-dim mt-1 block text-[12px]">
            <AppLink href={`/blocks/${first.blockNumber}`}>
              {formatBlockNumber(first.blockNumber)}
            </AppLink>{' '}
            · by{' '}
            <Address address={first.ownerAddress} startChars={8} endChars={6} className="inline" />
          </span>
        </>
      ) : (
        <AppLink href={`/blocks/${object.createdAtBlock}`}>
          {formatBlockNumber(object.createdAtBlock)}
        </AppLink>
      ),
    },
    {
      label: 'Last moved',
      value:
        last && moves !== null ? (
          moves === 0 ? (
            <span className="text-text-dim">Never moved since minting</span>
          ) : (
            <>
              <span>{formatUtc8DateTime(last.timestamp)}</span>
              <span className="text-text-dim mt-1 block text-[12px]">
                <AppLink href={`/blocks/${last.blockNumber}`}>
                  {formatBlockNumber(last.blockNumber)}
                </AppLink>{' '}
                · <span>{`${moves} transfer${moves === 1 ? '' : 's'} since minting`}</span>
              </span>
            </>
          )
        ) : (
          <span className="text-text-dim">Loading cell history…</span>
        ),
    },
    {
      label: 'CKB held in its cell',
      value: (
        <>
          <ExactCkb shannons={held} signed={false} tone="plain" wrap />
          <span className="text-text-dim mt-1 block text-[12px]">
            <span className="text-jade">{ckbText(occupied)}</span> occupied ·{' '}
            {ckbText(held - occupied)} free
          </span>
        </>
      ),
    },
    {
      label: 'Content',
      value: (
        <span className="break-all">
          {object.contentType} · {object.contentSize.toLocaleString('en-US')} bytes
        </span>
      ),
    },
  ];
  const coll = collection.data;
  const onchain = coll?.composition?.onchainCount;
  return (
    <AssetDetailShell
      title={title}
      kind="objects"
      breadcrumb={[
        { label: 'Inventory', href: '/inventory/objects' },
        { label: 'Objects', href: '/inventory/objects' },
        ...(object.clusterId
          ? [{ label: collectionName ?? 'Collection', href: collectionHref! }]
          : []),
      ]}
      status={<LiveBadge live={object.isLive} gone="Melted" />}
      identity={<Hash hash={id} truncate={false} className="text-[11px]" />}
      intro={
        <div className="space-y-4 pt-1">
          {tier ? (
            <CompositionLine tier={tier} />
          ) : (
            <span className="text-text-dim text-[11px]">Composition not recorded</span>
          )}
          {(history || coll) && (
            <p className="text-text text-[15px] leading-relaxed">
              {history && history.length > 0 && <span>{itemLifeSentence(history, now)}</span>}
              {coll && collectionName && (
                <span>
                  {' '}
                  {`One of ${coll.liveCount.toLocaleString('en-US')} in ${collectionName}, ${coll.holdersCount === coll.liveCount ? 'each held by a different address' : `held by ${coll.holdersCount.toLocaleString('en-US')} addresses`}.`}
                </span>
              )}
            </p>
          )}
          {genetics.data && (
            <ObjectTraits
              traits={genetics.data.cachedVmTraits.traits}
              colors={geneColors}
              active={activeGene}
              onActive={setActiveGene}
            />
          )}
        </div>
      }
      facts={facts}
      feed={
        <AssetFeed
          key={id}
          queryKey={`spore:${id}`}
          scope="This object's whole life"
          item={{ kind: 'object' }}
          plain
          focusItemId={id}
          loadActivities={(p) => api.getItemActivityFeed('spore', id, p)}
          loadTransactions={(p) => api.getItemTransactions('spore', id, p)}
          context={{
            label: 'Object · cell after',
            render: (t) => <ItemTransactionContext transaction={t} item={{ kind: 'object' }} />,
          }}
        />
      }
      tier2={
        <>
          {genetics.data && <ObjectGeneticsEvidence evidence={genetics.data} subject={title} />}
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
                collection={collectionGenes.data}
                collectionName={collectionName}
                collectionHref={collectionHref}
                tier={tier}
                active={activeGene}
                onActive={setActiveGene}
              />
              {object.clusterId && (
                <AssetReadState
                  label="collection variants"
                  error={collectionGenes.error}
                  retained={!!collectionGenes.data}
                  loading={collectionGenes.isLoading}
                />
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
      <div role="region" aria-label="CKB held in its cell" className="space-y-4">
        {!object.isLive && (
          <p className="text-text-dim text-[11px]">
            No live cell holds capacity for this melted object.
          </p>
        )}
        {versions.data ? (
          <>
            <AssetReadState label="cell versions" error={versions.error} retained />
            <CellVersions
              subject={title}
              history={versions.data}
              life
              title="CKB held in its cell"
              actions={
                <span className="text-text-dim text-[12px]">
                  1 CKB per byte ·{' '}
                  {held === occupied ? (
                    <>
                      <span className="text-jade">all occupied</span>, none free
                    </>
                  ) : (
                    <>
                      <span className="text-jade">{ckbText(occupied)} occupied</span>,{' '}
                      {ckbText(held - occupied)} free
                    </>
                  )}
                </span>
              }
              intro={
                versions.data.data.length > 0 && (
                  <p className="mb-4 text-[13px] leading-relaxed">
                    {cellLifeSentence(versions.data.data, 'object')}
                  </p>
                )
              }
            />
            {lockFootnote(versions.data.data) && (
              <p className="text-text-dim text-[12px]">{lockFootnote(versions.data.data)}</p>
            )}
          </>
        ) : (
          <AssetReadState
            label="cell versions"
            error={versions.error}
            loading={versions.isLoading}
          />
        )}
        <CapacityPartition
          subject={`Object ${id}`}
          total={object.ownedCapacity}
          occupied={object.ownedKnowledge}
          free={(held - occupied).toString()}
        />
      </div>
      {object.clusterId && (
        <section aria-label={`In ${collectionName ?? 'this collection'}`} className="space-y-3">
          <div className="flex flex-wrap items-baseline justify-between gap-4">
            <h2 className="text-text-bright text-[15px]">
              In {collectionName ?? 'this collection'}
            </h2>
            <AppLink href={collectionHref!} className="text-[12px]">
              Open the collection →
            </AppLink>
          </div>
          {coll ? (
            <>
              <AssetReadState label="collection" error={collection.error} retained />
              <div className="flex flex-wrap items-baseline gap-x-10 gap-y-2">
                <span>
                  <span className="text-text-bright text-[15px]">
                    {coll.liveCount.toLocaleString('en-US')}
                  </span>{' '}
                  <span className="text-text-dim">
                    {coll.liveCount === coll.totalCount
                      ? 'objects, all live'
                      : `live objects of ${coll.totalCount.toLocaleString('en-US')} minted`}
                  </span>
                </span>
                <span>
                  <span className="text-text-bright text-[15px]">
                    {coll.holdersCount.toLocaleString('en-US')}
                  </span>{' '}
                  <span className="text-text-dim">holders</span>
                </span>
                {coll.composition && onchain !== undefined && (
                  <span className="flex items-center gap-2">
                    <span className="text-text-bright text-[15px]">
                      {onchain.toLocaleString('en-US')} of {coll.liveCount.toLocaleString('en-US')}
                    </span>
                    <CompositionBadge tier={coll.composition.tier} size="sm" />
                  </span>
                )}
                {coll.ownedCapacity !== null && (
                  <span>
                    <span className="text-text-bright text-[15px]">
                      {ckbText(BigInt(coll.ownedCapacity))}
                    </span>{' '}
                    <span className="text-text-dim">CKB held in their cells</span>
                  </span>
                )}
              </div>
              {coll.composition &&
                onchain !== undefined &&
                coll.liveCount > 0 &&
                (coll.composition.tier === 'btc_ckb' || coll.composition.tier === 'pure_ckb') && (
                  <div
                    aria-label={`${formatPercent(BigInt(onchain), BigInt(coll.liveCount))}% fully on-chain`}
                    role="img"
                    className="bg-base-elevated h-2 overflow-hidden rounded"
                  >
                    <span
                      className="block h-full rounded"
                      style={{
                        width: `${formatPercent(BigInt(onchain), BigInt(coll.liveCount))}%`,
                        backgroundImage: COMPOSITION_BADGES[coll.composition.tier].bar,
                      }}
                    />
                  </div>
                )}
              {cluster.data && (
                <p className="text-text-dim text-[12px]">
                  Its cluster cell is held by{' '}
                  {cluster.data.ownerAddress || cluster.data.ownerLockHash ? (
                    <Address
                      address={cluster.data.ownerAddress ?? cluster.data.ownerLockHash!}
                      startChars={10}
                      endChars={6}
                      className="inline"
                    />
                  ) : (
                    'no live owner'
                  )}
                  {cluster.data.createdAtBlock !== null &&
                    ` · created at ${formatBlockNumber(cluster.data.createdAtBlock)}`}
                </p>
              )}
            </>
          ) : (
            <AssetReadState
              label="collection"
              error={collection.error}
              loading={collection.isLoading}
            />
          )}
        </section>
      )}
    </AssetDetailShell>
  );
}
