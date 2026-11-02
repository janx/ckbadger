'use client';
import { useQuery } from '@tanstack/react-query';
import {
  api,
  isNotFoundApiError,
  type CollectionFeedTransaction,
  type ObjectCollection,
  type IdentityCollection,
} from '@/lib/api';
import { NotFoundPage } from '@/components/not-found-page';
import {
  AssetDetailShell,
  AssetMetrics,
  AssetReadState,
  type AssetMetric,
} from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CollectionItems } from '@/components/asset/collection-gallery';
import { CollectionHolders } from '@/components/asset/collection-holders';
import { CollectionObjects } from '@/components/asset/collection-objects';
import {
  ClusterCellEvidence,
  ClusterPatternReading,
  MnftClassEvidence,
  StoredDescription,
} from '@/components/asset/collection-evidence';
import { MnftPermissions } from '@/components/asset/identity-document';
import { CapacityHistory } from '@/components/asset/capacity-history';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { Hash } from '@/components/ui/hash';
import { AppLink } from '@/components/ui/app-link';
import { Address } from '@/components/ui/address';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { parseSporeClusterDescription } from '@/lib/spore-cluster-description';
import { isDidCkbCollectionAlias } from '@/lib/detail-routes';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { HMultiplierReading } from '@/components/ui/h-multiplier';
import { formatBlockNumber } from '@/lib/format-block';
import { CompositionBadge } from '@/components/ui/composition-mark';
import {
  CompositionSet,
  compositionCountsFromCollection,
  type CompositionCounts,
} from '@/components/ui/composition-set';
import { RowTag } from '@/components/ui/row-tag';
import { formatCount } from '@/lib/format-count';
import type { CompositionTier } from '@/lib/asset-utils';
import { collectionFeedHeadline } from '@/lib/feed-headlines';
import { readErrorText } from '@/lib/read-error';
import { IdentityPositioning, type IdentityStandard } from '@/components/identity/identity-header';
import { retryTransientOnly } from '@/lib/query-client';

// ── F2 identities (round 2) ──
const IDENTITY_STANDARDS: readonly IdentityStandard[] = [
  'dotbit',
  'bit_cell',
  'did_ckb',
  'dotcell',
];
function identityStandard(standard: string): IdentityStandard {
  if (!(IDENTITY_STANDARDS as readonly string[]).includes(standard))
    throw new Error(`identity collection has no identity standard: ${JSON.stringify(standard)}`);
  return standard as IdentityStandard;
}
// ── Consistency round (Main.dc.html): the cluster / class hero — the kind and what its objects
// are made of in one lede, the description as a quote, the display number of live objects with
// the CKB their cells hold, the composition block and the history; the facts are the metrics row.
// (F2 OB-C4's opening, every fact of it kept: the description, the composition, the counts.)
function CollectionLede({
  kind,
  dobVersion,
  counts,
}: {
  kind: 'spore' | 'object';
  dobVersion: number | null;
  counts: CompositionCounts | null;
}) {
  const live = counts ? Object.values(counts).reduce((a, b) => a + b, 0) : 0;
  const only = counts
    ? (Object.keys(counts) as CompositionTier[]).find((t) => counts[t] === live && live > 0)
    : undefined;
  const onchain = counts ? counts.btc_ckb + counts.pure_ckb : 0;
  return (
    <p className="text-text-dim text-body max-w-[640px]">
      {/* Responsive B#21: the kind is one unit, so its separator never dangles at a line end. */}
      <span data-role="collection-kind" className="whitespace-nowrap">
        {kind === 'spore' ? 'Spore cluster' : 'm-NFT class'}
        {kind === 'spore' && dobVersion !== null && (
          <>
            {' · '}
            <RowTag>DOB/{dobVersion}</RowTag>
          </>
        )}
      </span>
      {counts &&
        live > 0 &&
        (only ? (
          <>
            {' · every object is '}
            <CompositionBadge tier={only} size="sm" />
            {only === 'btc_ckb'
              ? ': its record in a CKB cell, its picture in a Bitcoin inscription.'
              : only === 'pure_ckb'
                ? ': its record and its content in CKB cells.'
                : '.'}
          </>
        ) : onchain > 0 ? (
          ' · some of its objects are fully on-chain, the rest are not.'
        ) : (
          ' · none of its objects is fully on-chain.'
        ))}
    </p>
  );
}
/** The dim sentence after "N of M fully on-chain": how long the on-chain parts last. */
function permanence(counts: CompositionCounts): string | null {
  if (counts.btc_ckb > 0 && counts.pure_ckb === 0)
    return 'permanent and verifiable, as long as CKB and Bitcoin exist';
  if (counts.pure_ckb > 0 && counts.btc_ckb === 0)
    return 'permanent and verifiable, as long as CKB exists';
  if (counts.pure_ckb > 0) return 'permanent and verifiable, as long as CKB (and Bitcoin) exist';
  return null;
}
/** The page's one figure (R13): the live objects, with the CKB their cells hold beside it. */
function CollectionFigure({
  live,
  total,
  held,
  spore,
}: {
  live: number;
  total: number;
  held: bigint | null;
  spore: boolean;
}) {
  const burned = total - live;
  return (
    <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
      <div data-role="figure" className="min-w-0">
        <div className="text-text-dim text-meta mb-1.5 uppercase tracking-[0.08em]">
          objects live
        </div>
        <div className="text-index sm:text-display text-text-bright font-medium tabular-nums">
          {live.toLocaleString('en-US')}{' '}
          {/* C12: the feed's word, "burned", for every kind; Spore's own word stays in the title. */}
          <span
            className="text-body text-text-dim font-normal"
            title={spore ? 'melted, in Spore’s words' : undefined}
          >
            {burned === 0
              ? 'all live · none burned'
              : `of ${total.toLocaleString('en-US')} minted · ${burned.toLocaleString('en-US')} burned`}
          </span>
        </div>
      </div>
      <div className="text-section text-text-bright tabular-nums">
        {held !== null ? (
          <>
            <ExactCkb shannons={held} signed={false} tone="plain" wrap />{' '}
            <span className="text-small text-text-dim">held in their cells</span>
          </>
        ) : (
          <span className="text-small text-text-dim">CKB held in their cells not recorded</span>
        )}
      </div>
    </div>
  );
}
export interface CollectionDetailProps {
  id: string;
  kind: 'spore' | 'object' | 'identity';
  initial?: ObjectCollection;
}
export function CollectionDetail(props: CollectionDetailProps) {
  return <ScopedCollectionDetail key={`${props.kind}:${props.id}`} {...props} />;
}
function ScopedCollectionDetail({ id, kind, initial }: CollectionDetailProps) {
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
    retry: retryTransientOnly,
  });
  const insights = useQuery({
    queryKey: ['collection-insights', kind, id],
    queryFn: () => api.getCollectionInsights(id, kind),
    retry: retryTransientOnly,
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
  const clusterDescription =
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
  // C1: the tab names the collection by what it is — a Spore cluster, an m-NFT class, an
  // identity collection.
  const documentTitle = [
    title,
    kind === 'spore' ? 'cluster' : kind === 'object' ? 'class' : 'collection',
  ];
  if (!data && isNotFoundApiError(detail.error)) {
    const path = kind === 'identity' ? 'identities' : 'objects';
    return (
      <NotFoundPage
        entity={{
          kind: kind === 'identity' ? 'identity' : kind === 'spore' ? 'cluster' : 'class',
          identifier: id,
          checkedEndpoint: `/assets/${path}/${encodeURIComponent(id)}`,
        }}
      />
    );
  }
  if (!data)
    return (
      <AssetDetailShell
        title={title}
        documentTitle={documentTitle}
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
  // ── Consistency round C7 (Main.dc.html): the rail's facts as the metrics row. For a cluster or
  // class the live objects and the CKB their cells hold are the hero's figure; holders,
  // transactions, the cluster's creation and its cell's holder are the row. ──
  const weekly = insights.data?.weeklyActivity;
  const mints = weekly?.reduce((n, w) => n + w.mintTransactions, 0);
  const transfers = weekly?.reduce((n, w) => n + w.transferTransactions, 0);
  const burns = weekly?.reduce((n, w) => n + w.burnTransactions, 0);
  const exactlyOne = insights.data?.holderDistribution.find((b) => b.range === '1')?.holders;
  const weeklyNote =
    mints !== undefined && transfers !== undefined && burns !== undefined
      ? `${formatCount(mints, 'mint')} · ${formatCount(transfers, 'transfer')} · ${formatCount(burns, 'burn')}`
      : undefined;
  const objectMetrics: AssetMetric[] = [
    {
      label: 'Holders',
      value: data.holdersCount.toLocaleString('en-US'),
      note: exactlyOne === undefined ? undefined : `${exactlyOne.toLocaleString('en-US')} hold one`,
    },
    // Round 3 (L2): the count is null while the indexer has not derived it — say nothing of it.
    ...(data.activitiesCount === null
      ? weeklyNote === undefined
        ? []
        : [{ label: 'Mints · transfers · burns', value: weeklyNote, size: 'body' as const }]
      : [
          {
            label: 'Transactions',
            value: data.activitiesCount.toLocaleString('en-US'),
            note: weeklyNote,
            title: insights.data?.completeThrough
              ? `mints, transfers and burns counted by complete week through ${insights.data.completeThrough}`
              : undefined,
          },
        ]),
    ...(cluster.data
      ? [
          {
            label: 'Created',
            value:
              cluster.data.createdAtBlock === null ? (
                'Not recorded'
              ) : (
                <AppLink href={`/blocks/${cluster.data.createdAtBlock}`}>
                  {formatBlockNumber(cluster.data.createdAtBlock)}
                </AppLink>
              ),
          },
          {
            label: 'Cluster cell held by',
            size: 'body' as const,
            value: protocol.data ? (
              protocol.data.current ? (
                <Address address={protocol.data.current.cell.ownerAddress} className="text-body" />
              ) : (
                'No current cluster cell'
              )
            ) : (
              'Current cell evidence unavailable'
            ),
            note: protocol.data?.current ? (
              <RowTag>{protocol.data.current.cell.lockName ?? 'Unregistered lock'}</RowTag>
            ) : undefined,
          },
        ]
      : []),
  ];
  const identityMetrics: AssetMetric[] = [
    { label: 'Standard', size: 'body', value: formatStandard(data.standard) },
    {
      label: 'Live',
      value: data.liveCount.toLocaleString('en-US'),
      note: `of ${data.totalCount.toLocaleString('en-US')} minted / created · ${(data.totalCount - data.liveCount).toLocaleString('en-US')} inactive / burned`,
    },
    { label: 'Holders', value: data.holdersCount.toLocaleString('en-US') },
    ...(data.activitiesCount === null
      ? []
      : [{ label: 'Activities', value: data.activitiesCount.toLocaleString('en-US') }]),
    {
      label: 'CKB held',
      value:
        held !== null ? (
          <ExactCkb shannons={held} signed={false} tone="plain" wrap />
        ) : (
          'Not recorded'
        ),
      note:
        held !== null && occupied !== null && held > BigInt(0)
          ? `${formatScaledInteger(occupied * BigInt(100), held, 2)}% occupied`
          : undefined,
    },
  ];
  const counts = object?.composition ? compositionCountsFromCollection(object.composition) : null;
  const description =
    kind === 'spore'
      ? (clusterDescription?.summary ?? null)
      : (object?.classDetail?.description ?? null);
  const history = (
    <div role="region" aria-label="History" className="min-w-0 space-y-3">
      {chart.data ? (
        <>
          <AssetReadState label="capacity history" error={chart.error} retained />
          <CapacityHistory
            history={chart.data}
            subject={title}
            weekly={
              insights.data
                ? insights.data.weeklyActivity
                : {
                    unavailable: insights.error
                      ? `Weekly transfers unavailable: ${readErrorText(insights.error)}`
                      : 'Loading weekly transfers…',
                  }
            }
          />
        </>
      ) : (
        <AssetReadState label="capacity history" error={chart.error} loading={chart.isLoading} />
      )}
      {kind !== 'identity' && mints !== undefined && transfers !== undefined && (
        <p className="text-text-dim text-small">
          Minted in {mints.toLocaleString('en-US')} transaction{mints === 1 ? '' : 's'} ·{' '}
          {transfers.toLocaleString('en-US')} transfer{transfers === 1 ? '' : 's'} since
          {held !== null && occupied !== null && occupied > BigInt(0) && (
            <>
              {' · '}
              <HMultiplierReading held={held} occupied={occupied} />
            </>
          )}
        </p>
      )}
    </div>
  );
  const objectsKind = kind !== 'identity';
  const holdersRegion = (
    <div role="region" aria-label="Who holds it" className="min-w-0">
      <CollectionHolders
        queryKey={`${kind}:${id}`}
        distribution={insights.data?.holderDistribution}
        assetClass={kind === 'identity' ? undefined : ('objects' as const)}
        load={(p) =>
          kind === 'spore'
            ? api.getSporeClusterHolders(id, p)
            : kind === 'object'
              ? api.getObjectCollectionHolders(id, p)
              : api.getIdentityCollectionHolders(id, p)
        }
      />
    </div>
  );
  return (
    <AssetDetailShell
      title={title}
      documentTitle={documentTitle}
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
      // F2 identities (round 2, ID-COL-5): an identity collection's id is a route alias, and it
      // has no description to record; it opens with what the standard is instead. A cluster or
      // class is named by its id under the title; its kind leads the lede.
      identity={
        kind === 'identity' ? undefined : <Hash hash={id} truncate={false} className="text-meta" />
      }
      lede={
        kind === 'identity' ? undefined : (
          <div className="space-y-2.5">
            <CollectionLede
              kind={kind}
              dobVersion={
                insights.data?.currentPattern.status === 'dob'
                  ? insights.data.currentPattern.version
                  : null
              }
              counts={counts}
            />
            {description ? (
              <blockquote
                className="border-base-border text-text text-body max-w-[640px] border-l pl-3"
                title={
                  kind === 'spore'
                    ? 'the cluster’s description, on chain'
                    : 'the class description, on chain'
                }
              >
                {description}
              </blockquote>
            ) : (
              <p className="text-text-dim text-small">No collection description recorded.</p>
            )}
          </div>
        )
      }
      hero={
        objectsKind ? (
          <div className="space-y-[22px]">
            <CollectionFigure
              live={data.liveCount}
              total={data.totalCount}
              held={held}
              spore={kind === 'spore'}
            />
            {counts && (
              <CompositionSet counts={counts} total={data.liveCount} caption={permanence(counts)} />
            )}
            {history}
          </div>
        ) : undefined
      }
      metrics={<AssetMetrics items={objectsKind ? objectMetrics : identityMetrics} />}
      intro={
        kind === 'identity' ? (
          <IdentityPositioning standard={identityStandard(data.standard)} scope="collection" />
        ) : undefined
      }
      // Main.dc.html screen 3: who holds it beside the recent activity — the shell's feed slot,
      // after the tier-1 body (lists of rows follow the domain, as on every asset page).
      feed={
        objectsKind ? (
          <div className="grid min-w-0 items-start gap-x-16 gap-y-12 lg:grid-cols-2">
            {holdersRegion}
            <AssetFeed
              queryKey={`${kind}:${id}`}
              scope="This collection"
              collection
              plain
              // visual pass: every row of a Spore cluster / m-NFT class feed is about objects
              mark="objects"
              recent={{ rows: 5, transactions: data.activitiesCount }}
              loadActivities={(p) => api.getCollectionActivityFeed(kind, id, p)}
              loadTransactions={(p) => api.getCollectionTransactions(kind, id, p)}
              // OB-C9 (CollectionTxs board): the collection's own column, named by it, after Cells.
              context={{
                label: title,
                placement: 'after-cells',
                render: (tx) => (
                  <span className="text-text-bright">
                    {collectionFeedHeadline(
                      (tx as CollectionFeedTransaction).collectionContext,
                      tx.hash
                    )}
                  </span>
                ),
              }}
            />
          </div>
        ) : (
          <AssetFeed
            queryKey={`${kind}:${id}`}
            scope="This collection"
            collection
            plain={false}
            loadActivities={(p) => api.getCollectionActivityFeed(kind, id, p)}
            loadTransactions={(p) => api.getCollectionTransactions(kind, id, p)}
            context={{
              label: title,
              placement: 'after-cells',
              render: (tx) => (
                <span className="text-text-bright">
                  {collectionFeedHeadline(
                    (tx as CollectionFeedTransaction).collectionContext,
                    tx.hash
                  )}
                </span>
              ),
            }}
          />
        )
      }
      tier2={
        // Lane E: tier 2 only where there is evidence (a Spore cluster, an m-NFT class).
        (kind === 'spore' || object?.classDetail) && (
          <>
            {kind === 'spore' && (
              // Lane E (C5, Main board): the cluster cell byte by byte beside how an object is read.
              <div className="grid min-w-0 items-start gap-x-16 gap-y-10 xl:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)]">
                <section aria-label="Cluster cell evidence" className="min-w-0 space-y-4">
                  <AssetReadState
                    label="cluster cell"
                    error={protocol.error}
                    retained={!!protocol.data}
                    loading={protocol.isLoading}
                  />
                  {protocol.data?.current ? (
                    <ClusterCellEvidence clusterId={id} current={protocol.data.current} />
                  ) : (
                    protocol.data && (
                      <p className="text-text-dim text-meta">
                        No current cell carries this cluster type.
                      </p>
                    )
                  )}
                  {cluster.data && <StoredDescription description={cluster.data.description} />}
                </section>
                {insights.data && (
                  <ClusterPatternReading
                    clusterId={id}
                    insights={insights.data}
                    description={cluster.data?.description}
                  />
                )}
              </div>
            )}
            {/* F2 objects (OB-K5): the configure decoding is developer evidence (Mnft board tier 2). */}
            {object?.classDetail && (
              <MnftPermissions
                permissions={object.classDetail.permissions}
                configure={object.classDetail.configure}
              />
            )}
            {object?.classDetail && (
              <MnftClassEvidence
                classDetail={object.classDetail}
                issuerDetail={object.issuerDetail}
              />
            )}
          </>
        )
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
      {kind === 'identity' &&
        (held !== null && occupied !== null ? (
          <CapacityPartition
            subject={title}
            total={held.toString()}
            occupied={occupied.toString()}
            free={(held - occupied).toString()}
          />
        ) : (
          <p className="text-text-dim text-meta">Collection capacity is not recorded.</p>
        ))}
      {/* Main.dc.html: Objects (the genes that vary and the objects), then who holds it beside the
          recent activity. */}
      {kind === 'spore' &&
        (insights.data ? (
          <>
            <AssetReadState label="collection insights" error={insights.error} retained />
            <CollectionObjects
              id={id}
              title={title}
              insights={insights.data}
              tier={object?.composition?.tier}
            />
          </>
        ) : (
          <AssetReadState
            label="collection insights"
            error={insights.error}
            loading={insights.isLoading}
          />
        ))}
      {kind === 'object' && <CollectionItems id={id} kind={kind} />}
      {kind === 'identity' && history}
      {kind !== 'spore' && (
        <AssetReadState
          label="collection insights"
          error={insights.error}
          retained={!!insights.data}
          loading={insights.isLoading}
        />
      )}
      {kind === 'identity' && <CollectionItems id={id} kind={kind} />}
      {kind === 'identity' && isDidCkbCollectionAlias(id) && (
        <section aria-label="PDS distribution" className="space-y-4">
          <h2 className="text-text-bright text-section">Personal data servers</h2>
          {pds.data ? (
            <>
              <AssetReadState label="PDS distribution" error={pds.error} retained />
              <p className="text-text-dim text-meta">
                {pds.data.totalLiveIdentities} live identities · {pds.data.withoutPds} without a
                PDS. An identity can name more than one endpoint.
              </p>
              <dl className="space-y-3">
                {pds.data.groups.map((g) => (
                  <div key={g.endpoint} className="flex flex-wrap justify-between gap-3">
                    <dt className="min-w-0 break-all">{g.endpoint}</dt>
                    {/* F2 identities (round 2): one identity, many identities. */}
                    <dd>
                      {g.count.toLocaleString('en-US')} {g.count === 1 ? 'identity' : 'identities'}
                    </dd>
                  </div>
                ))}
              </dl>
            </>
          ) : (
            <AssetReadState label="PDS distribution" error={pds.error} loading={pds.isLoading} />
          )}
        </section>
      )}
      {!objectsKind && holdersRegion}
    </AssetDetailShell>
  );
}
