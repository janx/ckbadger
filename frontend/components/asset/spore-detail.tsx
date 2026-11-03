'use client';
import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, type SporeDetail } from '@/lib/api';
import {
  AssetDetailShell,
  AssetMetrics,
  AssetReadState,
  type AssetMetric,
} from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CellVersions } from '@/components/asset/cell-versions';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import {
  DnaParagraph,
  ObjectGeneticsEvidence,
  ObjectGeneticsView,
} from '@/components/asset/genetics';
import { DetailSection } from '@/components/ui/detail-layout';
import { ObjectMedia } from '@/components/asset/object-media';
import {
  CompositionLine,
  LiveBadge,
  ObjectTraits,
  cellLifeSentence,
  ckbText,
  itemLifeSentence,
  itemLifeAges,
  lockFootnote,
} from '@/components/asset/object-hero';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { ExactCkb } from '@/components/activity-event-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';
import { sequencingGenes } from '@/lib/dob-genes';
import { COMPOSITION_BADGES } from '@/lib/design-tokens';
import { formatPercent } from '@/lib/ckb-decimal';
import { retryTransientOnly } from '@/lib/query-client';
import { omigaDobHref } from '@/lib/btcfs';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';

export interface SporeDetailViewProps {
  object: SporeDetail;
  error: Error | null;
}
export function SporeDetailView({ object, error }: SporeDetailViewProps) {
  const id = object.sporeId,
    isDob = object.contentType === 'dob/0' || object.contentType === 'dob/1';
  const client = useQueryClient();
  const network = useActiveNetwork();
  const omiga = omigaDobHref(object.typeScriptHash, network);
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
    retry: retryTransientOnly,
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
    retry: retryTransientOnly,
  });
  const genetics = useQuery({
    queryKey: ['object-genetics', id],
    queryFn: () => api.getObjectGenetics(id),
    enabled: isDob,
    retry: retryTransientOnly,
  });
  const decoded = useQuery({
    queryKey: ['spore-dob-decoded', id],
    queryFn: () => api.getSporeObjectDecoded(id),
    enabled: isDob,
    retry: retryTransientOnly,
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
      for (const g of sequencingGenes(genetics.data, title, cluster.data?.description).genes)
        if (!colors.has(g.name)) colors.set(g.name, g.color);
    return colors;
  }, [genetics.data, title, cluster.data?.description]);
  const moves = history ? history.length - 1 : null;
  // Consistency round C7: the rail's facts as the metrics row under the hero (R13 — a value and
  // its inline note; a time of day and the minter's full address stay in the item's title).
  const metrics: AssetMetric[] = [
    {
      label: 'Owner',
      size: 'body',
      value: object.ownerLockHash ? (
        <Address address={object.ownerAddress ?? object.ownerLockHash} className="text-body" />
      ) : (
        'No live owner'
      ),
      note: object.ownerLockHash
        ? (last?.lockName ?? (history ? 'Unregistered lock' : 'Loading lock…'))
        : undefined,
    },
    ...(object.clusterId
      ? [
          {
            label: 'Collection',
            size: 'body' as const,
            value: (
              <AppLink href={collectionHref!}>{collectionName ?? 'Unnamed collection'}</AppLink>
            ),
            note: collection.data
              ? `${collection.data.liveCount.toLocaleString('en-US')} objects`
              : undefined,
          },
        ]
      : []),
    {
      label: 'Minted',
      title: first
        ? `minted ${formatUtc8DateTime(first.timestamp)} by ${first.ownerAddress}`
        : undefined,
      value: first ? (
        formatUtc8Date(first.timestamp)
      ) : (
        <AppLink href={`/blocks/${object.createdAtBlock}`}>
          {formatBlockNumber(object.createdAtBlock)}
        </AppLink>
      ),
      note: first ? (
        <>
          <AppLink href={`/blocks/${first.blockNumber}`}>
            {formatBlockNumber(first.blockNumber)}
          </AppLink>{' '}
          · by{' '}
          <Address address={first.ownerAddress} startChars={8} endChars={6} className="inline" />
        </>
      ) : undefined,
    },
    {
      label: 'Last moved',
      title: last && moves ? `last moved ${formatUtc8DateTime(last.timestamp)}` : undefined,
      value:
        last && moves !== null ? (
          moves === 0 ? (
            <span className="text-text-dim">never</span>
          ) : (
            formatUtc8Date(last.timestamp)
          )
        ) : (
          <span className="text-text-dim">Loading cell history…</span>
        ),
      note:
        last && moves !== null ? (
          moves === 0 ? (
            'since minting'
          ) : (
            <>
              <AppLink href={`/blocks/${last.blockNumber}`}>
                {formatBlockNumber(last.blockNumber)}
              </AppLink>{' '}
              · <span>{`${moves} transfer${moves === 1 ? '' : 's'} since minting`}</span>
            </>
          )
        ) : undefined,
    },
    {
      label: 'CKB held in its cell',
      value: <ExactCkb shannons={held} signed={false} tone="plain" wrap />,
      // R11: tier 1's words stay uncoloured; the bars keep the occupied colour.
      note: (
        <>
          <span className="text-text">{ckbText(occupied)}</span> occupied ·{' '}
          {ckbText(held - occupied)} free
        </>
      ),
    },
    {
      label: 'Content',
      size: 'body',
      value: <span className="break-all">{object.contentType}</span>,
      note: `${object.contentSize.toLocaleString('en-US')} bytes`,
    },
  ];
  const coll = collection.data;
  const onchain = coll?.composition?.onchainCount;
  // Hierarchy v3 (F4): every sentence that left tier 1, in full; it heads the DNA column.
  const words = (history || coll || genetics.data) && (
    <DetailSection title="This object, in words">
      <div className="flex flex-col gap-3">
        {(history || coll) && (
          <p className="text-text-dim text-body">
            {history && history.length > 0 && (
              <span title={itemLifeAges(history, now)}>{itemLifeSentence(history, now)}</span>
            )}
            {coll && collectionName && (
              <span>
                {' '}
                {`One of ${coll.liveCount.toLocaleString('en-US')} in ${collectionName}, ${coll.holdersCount === coll.liveCount ? 'each held by a different address' : `held by ${coll.holdersCount.toLocaleString('en-US')} addresses`}.`}
              </span>
            )}
          </p>
        )}
        {genetics.data && (
          <DnaParagraph
            evidence={genetics.data}
            subject={title}
            tier={tier}
            description={cluster.data?.description}
          />
        )}
        {history && lockFootnote(history, 'object') && (
          <p className="text-text-dim text-body">{lockFootnote(history, 'object')}</p>
        )}
      </div>
    </DetailSection>
  );
  return (
    <AssetDetailShell
      title={title}
      documentTitle={[title, 'object']}
      kind="objects"
      breadcrumb={[
        { label: 'Inventory', href: '/inventory/objects' },
        { label: 'Objects', href: '/inventory/objects' },
        ...(object.clusterId
          ? [{ label: collectionName ?? 'Collection', href: collectionHref! }]
          : []),
      ]}
      status={<LiveBadge live={object.isLive} gone="Burned" />}
      // Lane U (U5): Omiga as the page's one action, an outlined button-link right of the h1 row,
      // as tall as the range buttons (28 px); the media column's quiet text link gives way to it.
      actions={
        omiga && (
          <a
            href={omiga}
            target="_blank"
            rel="noopener noreferrer"
            title="This object on Omiga, a DOB marketplace (opens a new tab)"
            className="border-base-border text-text hover:text-text-bright hover:border-text-dim text-small inline-flex h-7 items-center rounded-[4px] border px-2.5 font-mono transition-colors"
          >
            Omiga ↗
          </a>
        )
      }
      identity={<Hash hash={id} truncate={false} className="text-meta" />}
      // Hierarchy v3 (F4, R13): the composition sentence is the page's one lede; its life story
      // moved to tier 2 ("This object, in words").
      lede={
        <div className="pt-1">
          {tier ? (
            <CompositionLine tier={tier} lede />
          ) : (
            <span className="text-text-dim text-meta">Composition not recorded</span>
          )}
        </div>
      }
      intro={
        genetics.data && (
          <div className="pt-1">
            <ObjectTraits
              traits={genetics.data.cachedVmTraits.traits}
              colors={geneColors}
              active={activeGene}
              onActive={setActiveGene}
            />
          </div>
        )
      }
      metrics={<AssetMetrics items={metrics} />}
      feed={
        <AssetFeed
          key={id}
          queryKey={`spore:${id}`}
          scope="This object's whole life"
          item={{ kind: 'object' }}
          plain
          mark="objects"
          focusItemId={id}
          loadActivities={(p) => api.getItemActivityFeed('spore', id, p)}
          loadTransactions={(p) => api.getItemTransactions('spore', id, p)}
          context={{
            label: 'Object · cell after',
            render: (t) => <ItemTransactionContext transaction={t} item={{ kind: 'object' }} />,
          }}
        />
      }
      media={
        <ObjectMedia
          object={object}
          contentHex={data?.versions.at(-1)?.decoded.contentHex ?? null}
          decoded={decoded.data}
          omigaLink={false}
        />
      }
      tier2={
        // OB-14 (Object.dc.html tier 2): the DNA evidence and the cell's raw record side by side.
        <div
          data-tier2-columns
          className="grid min-w-0 items-start gap-x-14 gap-y-10 md:grid-cols-2"
        >
          {/* Only a DOB object has DNA evidence; otherwise the raw record spans both columns. */}
          {isDob && (
            <div className="min-w-0 space-y-10">
              {words}
              {genetics.data ? (
                <ObjectGeneticsEvidence
                  evidence={genetics.data}
                  subject={title}
                  decoded={decoded.data}
                  description={cluster.data?.description}
                />
              ) : (
                <AssetReadState
                  label="DNA evidence"
                  error={genetics.error}
                  loading={genetics.isLoading}
                />
              )}
            </div>
          )}
          <div className={isDob ? 'min-w-0 space-y-10' : 'min-w-0 space-y-10 md:col-span-2'}>
            {!isDob && words}
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
          </div>
        </div>
      }
    >
      <AssetReadState label="object" error={error} retained />
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
                description={cluster.data?.description}
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
          <p className="text-text-dim text-meta" title="melted, in Spore’s words">
            No live cell holds capacity for this burned object.
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
                <span className="text-text-dim text-small">
                  1 CKB per byte ·{' '}
                  {held === occupied ? (
                    <>
                      <span className="text-text">all occupied</span>, none free
                    </>
                  ) : (
                    <>
                      <span className="text-text">{ckbText(occupied)} occupied</span>,{' '}
                      {ckbText(held - occupied)} free
                    </>
                  )}
                </span>
              }
            />
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
        {/* Hierarchy v3 (F4): the cell's story is the caption under its evidence. */}
        {versions.data && versions.data.data.length > 0 && (
          <p data-role="caption" className="text-text text-body max-w-[640px]">
            {cellLifeSentence(versions.data.data, 'object')}
          </p>
        )}
      </div>
      {object.clusterId && (
        <section aria-label={`In ${collectionName ?? 'this collection'}`} className="space-y-3">
          <div className="flex flex-wrap items-baseline justify-between gap-4">
            <h2 className="text-text-bright text-section">
              In {collectionName ?? 'this collection'}
            </h2>
            <AppLink href={collectionHref!} className="text-small">
              Open the collection →
            </AppLink>
          </div>
          {coll ? (
            <>
              <AssetReadState label="collection" error={collection.error} retained />
              <div className="flex flex-wrap items-baseline gap-x-10 gap-y-2">
                <span>
                  <span className="text-text-bright text-body">
                    {coll.liveCount.toLocaleString('en-US')}
                  </span>{' '}
                  <span className="text-text-dim">
                    {coll.liveCount === coll.totalCount
                      ? 'objects, all live'
                      : `live objects of ${coll.totalCount.toLocaleString('en-US')} minted`}
                  </span>
                </span>
                <span>
                  <span className="text-text-bright text-body">
                    {coll.holdersCount.toLocaleString('en-US')}
                  </span>{' '}
                  <span className="text-text-dim">holders</span>
                </span>
                {coll.composition && onchain !== undefined && (
                  <span className="flex items-center gap-2">
                    <span className="text-text-bright text-body">
                      {onchain.toLocaleString('en-US')} of {coll.liveCount.toLocaleString('en-US')}
                    </span>
                    <CompositionBadge tier={coll.composition.tier} size="sm" />
                  </span>
                )}
                {coll.ownedCapacity !== null && (
                  <span>
                    <span className="text-text-bright text-body">
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
                <p className="text-text-dim text-small">
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
