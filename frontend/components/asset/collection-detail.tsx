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
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CollectionItems } from '@/components/asset/collection-gallery';
import { CollectionHolders } from '@/components/asset/collection-holders';
import { CurrentPatternEvidence } from '@/components/asset/genetics';
import { CollectionObjects, DecodeCoverage } from '@/components/asset/collection-objects';
import { MnftPermissions } from '@/components/asset/identity-document';
import { CapacityHistory } from '@/components/asset/capacity-history';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { Hash } from '@/components/ui/hash';
import { AppLink } from '@/components/ui/app-link';
import { Address } from '@/components/ui/address';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { parseSporeClusterDescription } from '@/lib/spore-cluster-description';
import { isDidCkbCollectionAlias } from '@/lib/detail-routes';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';
import { CompositionBadge } from '@/components/ui/composition-mark';
import type { CompositionTier } from '@/lib/asset-utils';
import { collectionFeedHeadline } from '@/lib/feed-headlines';
import { readErrorText } from '@/lib/read-error';
import { IdentityPositioning, type IdentityStandard } from '@/components/identity/identity-header';

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
// ── F2 objects (round 2, OB-C4): the Collection board's opening — the on-chain description
// with its source, the composition badge with one sentence, and a summary of who holds it. ──
function collectionCompositionSentence(tier: CompositionTier, onchain: number, live: number) {
  if (live > 0 && onchain === live && tier === 'btc_ckb')
    return `All ${live.toLocaleString('en-US')} objects are fully on-chain: records in CKB cells, pictures in Bitcoin inscriptions.`;
  if (live > 0 && onchain === live && tier === 'pure_ckb')
    return `All ${live.toLocaleString('en-US')} objects are fully on-chain: records and content in CKB cells.`;
  if (onchain > 0)
    return `${onchain.toLocaleString('en-US')} of ${live.toLocaleString('en-US')} objects are fully on-chain.`;
  return `None of its ${live.toLocaleString('en-US')} objects is fully on-chain.`;
}
function CollectionIntro({
  description,
  source,
  composition,
  live,
  holders,
  exactlyOne,
  held,
}: {
  description: string | null;
  source: string;
  composition?: ObjectCollection['composition'];
  live: number;
  holders: number;
  exactlyOne: number | undefined;
  held: bigint | null;
}) {
  return (
    <div className="space-y-3 pt-1">
      <p className="text-text text-[13px]">
        {description ?? 'No collection description recorded.'}
        {description && <span className="text-text-dim"> · {source}</span>}
      </p>
      {composition && (
        <div className="flex flex-wrap items-center gap-3">
          <CompositionBadge tier={composition.tier} />
          <span className="text-text text-[12.5px]">
            {collectionCompositionSentence(composition.tier, composition.onchainCount, live)}
          </span>
        </div>
      )}
      <p className="text-text text-[15px] leading-relaxed">
        {`${live.toLocaleString('en-US')} objects.`}
        {exactlyOne !== undefined &&
          ` ${exactlyOne.toLocaleString('en-US')} of their ${holders.toLocaleString('en-US')} holders own exactly one.`}
        {held !== null &&
          ` Their cells hold ${(held / BigInt(100000000)).toLocaleString('en-US')}${held % BigInt(100000000) === BigInt(0) ? '' : `.${(held % BigInt(100000000)).toString().padStart(8, '0').replace(/0+$/, '')}`} CKB.`}
      </p>
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
    retry: false,
  });
  const insights = useQuery({
    queryKey: ['collection-insights', kind, id],
    queryFn: () => api.getCollectionInsights(id, kind),
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
  // ── F2 objects (round 2, OB-C8): the Collection board's rail for Spore clusters and m-NFT
  // classes; identity collections keep their own facts below. ──
  const weekly = insights.data?.weeklyActivity;
  const mints = weekly?.reduce((n, w) => n + w.mintTransactions, 0);
  const transfers = weekly?.reduce((n, w) => n + w.transferTransactions, 0);
  const melted = data.totalCount - data.liveCount;
  const objectFacts = [
    {
      label: 'Objects',
      value: (
        <>
          {data.liveCount.toLocaleString('en-US')}
          <span className="text-text-dim mt-1 block text-[12px]">
            {melted === 0
              ? 'all live · none melted'
              : `live · ${melted.toLocaleString('en-US')} of ${data.totalCount.toLocaleString('en-US')} ${kind === 'spore' ? 'melted' : 'burned'}`}
          </span>
        </>
      ),
    },
    { label: 'Holders', value: data.holdersCount.toLocaleString('en-US') },
    {
      label: 'CKB held in their cells',
      value:
        held !== null && occupied !== null ? (
          <CapacityPartition
            subject={title}
            total={held.toString()}
            occupied={occupied.toString()}
            free={(held - occupied).toString()}
          />
        ) : (
          'Not recorded'
        ),
    },
    {
      label: 'Activity',
      value: (
        <>
          {data.activitiesCount.toLocaleString('en-US')}
          {mints !== undefined && transfers !== undefined && (
            <span className="text-text-dim text-[12px]">
              {' '}
              · {mints.toLocaleString('en-US')} mint{mints === 1 ? '' : 's'},{' '}
              {transfers.toLocaleString('en-US')} transfer{transfers === 1 ? '' : 's'}
              {insights.data?.completeThrough ? ` through ${insights.data.completeThrough}` : ''}
            </span>
          )}
        </>
      ),
    },
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
            value: protocol.data ? (
              protocol.data.current ? (
                <>
                  <Address address={protocol.data.current.cell.ownerAddress} />
                  <span className="text-text-dim mt-1 block text-[12px]">
                    {protocol.data.current.cell.lockName ?? 'Unregistered lock'}
                  </span>
                </>
              ) : (
                'No current cluster cell'
              )
            ) : (
              'Current cell evidence unavailable'
            ),
          },
        ]
      : []),
  ];
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
                  {formatBlockNumber(cluster.data.createdAtBlock)}
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
      // F2 identities (round 2, ID-COL-5): an identity collection's id is a route alias, and it
      // has no description to record; it opens with what the standard is instead.
      identity={
        kind === 'identity' ? undefined : (
          <>
            {/* F2 objects (OB-C4): what kind of collection this is, then its id. */}
            <span className="text-text-dim mr-2 text-[13px]">
              {kind === 'spore'
                ? `Spore cluster${insights.data?.currentPattern.status === 'dob' && insights.data.currentPattern.version !== null ? ` · DOB/${insights.data.currentPattern.version}` : ''}`
                : 'm-NFT class'}
            </span>
            <Hash hash={id} truncate={false} className="text-[11px]" />
          </>
        )
      }
      facts={kind === 'identity' ? facts : objectFacts}
      intro={
        kind === 'identity' ? (
          <IdentityPositioning standard={identityStandard(data.standard)} scope="collection" />
        ) : (
          <CollectionIntro
            description={description?.summary ?? object?.classDetail?.description ?? null}
            source={
              kind === 'spore'
                ? 'the cluster’s description, on chain'
                : 'the class description, on chain'
            }
            composition={object?.composition}
            live={data.liveCount}
            holders={data.holdersCount}
            exactlyOne={insights.data?.holderDistribution.find((b) => b.range === '1')?.holders}
            held={held}
          />
        )
      }
      feed={
        <AssetFeed
          queryKey={`${kind}:${id}`}
          scope="This collection"
          collection
          plain={kind !== 'identity'}
          // visual pass: every row of a Spore cluster / m-NFT class feed is about objects
          mark={kind === 'identity' ? undefined : 'objects'}
          loadActivities={(p) => api.getCollectionActivityFeed(kind, id, p)}
          loadTransactions={(p) => api.getCollectionTransactions(kind, id, p)}
          context={{
            label: 'For this collection',
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
                    href={`/cell/${protocol.data.current.cell.txHash}-${protocol.data.current.cell.outputIndex}`}
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
          {kind === 'spore' && insights.data && (
            <>
              <DecodeCoverage genetics={insights.data.genetics} />
              <CurrentPatternEvidence pattern={insights.data.currentPattern} />
            </>
          )}
          {/* F2 objects (OB-K5): the configure decoding is developer evidence (Mnft board tier 2). */}
          {object?.classDetail && (
            <MnftPermissions
              permissions={object.classDetail.permissions}
              configure={object.classDetail.configure}
            />
          )}
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
      {kind === 'identity' &&
        (held !== null && occupied !== null ? (
          <CapacityPartition
            subject={title}
            total={held.toString()}
            occupied={occupied.toString()}
            free={(held - occupied).toString()}
          />
        ) : (
          <p className="text-text-dim text-[11px]">Collection capacity is not recorded.</p>
        ))}
      {/* F2 objects (OB-C5): Objects first, then History, then Who holds it (Collection board). */}
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
        {kind !== 'identity' && (
          <>
            {mints !== undefined && transfers !== undefined && (
              <p className="text-text-dim text-[12px]">
                Minted in {mints.toLocaleString('en-US')} transaction{mints === 1 ? '' : 's'} ·{' '}
                {transfers.toLocaleString('en-US')} transfer{transfers === 1 ? '' : 's'} since
                {held !== null && occupied !== null && occupied > BigInt(0)
                  ? ` · H-multiplier ×${formatScaledInteger(held, occupied, 2)}`
                  : ''}
              </p>
            )}
          </>
        )}
      </div>
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
    </AssetDetailShell>
  );
}
