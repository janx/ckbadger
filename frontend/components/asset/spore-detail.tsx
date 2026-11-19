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
import { Prose } from '@/components/ui/prose';
import { DetailSection } from '@/components/ui/detail-layout';
import { ObjectMedia } from '@/components/asset/object-media';
import {
  CompositionLine,
  LiveBadge,
  ObjectTraits,
  cellLifeSentence,
  ckbText,
  ItemLife,
  LockFootnote,
} from '@/components/asset/object-hero';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { AppLink } from '@/components/ui/app-link';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { BlockLink } from '@/components/ui/block-link';
import { HeadId } from '@/components/ui/head-id';
import { Address } from '@/components/ui/address';
import { formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';
import { sequencingGenes } from '@/lib/dob-genes';
import { COMPOSITION_BADGES } from '@/lib/design-tokens';
import { formatPercent } from '@/lib/ckb-decimal';
import { retryTransientOnly } from '@/lib/query-client';
import { formatCount, countNoun } from '@/lib/format-count';
import { percentOf } from '@/lib/format-unit';
import { unitNode } from '@/components/ui/units';
import { cellOwnerLockPart } from '@/components/ui/lock-kind';
import { PartReadError } from '@/components/read-failure';

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
        <Address
          look="key"
          address={object.ownerAddress ?? object.ownerLockHash}
          size="row"
          className="text-body"
        />
      ) : (
        'No live owner'
      ),
      // the owner's lock by the one decision, off its cell's last version (B4): nothing for the
      // default lock; an owner with no cell version is a contract break, named; either refusal
      // fails this note in its place (D7)
      note: object.ownerLockHash ? (
        history === undefined ? (
          'Loading lock…'
        ) : last === undefined ? (
          <PartReadError
            what="the owner’s lock"
            where={`object ${id}`}
            error={new Error(`${id}: owner ${object.ownerLockHash} but no cell versions`)}
          />
        ) : (
          (cellOwnerLockPart(last) ?? undefined)
        )
      ) : undefined,
    },
    ...(object.clusterId
      ? [
          {
            label: 'Collection',
            size: 'body' as const,
            value: (
              <AppLink look="key" href={collectionHref!}>
                {collectionName ?? 'Unnamed collection'}
              </AppLink>
            ),
            note: collection.data ? formatCount(collection.data.liveCount, 'object') : undefined,
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
        <AppLink look="key" href={`/blocks/${object.createdAtBlock}`}>
          {formatBlockNumber(object.createdAtBlock)}
        </AppLink>
      ),
      note: first ? (
        <>
          <AppLink look="inline" href={`/blocks/${first.blockNumber}`}>
            {formatBlockNumber(first.blockNumber)}
          </AppLink>{' '}
          · by <Address look="inline" address={first.ownerAddress} size="row" className="inline" />
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
              <AppLink look="inline" href={`/blocks/${last.blockNumber}`}>
                {formatBlockNumber(last.blockNumber)}
              </AppLink>{' '}
              · <span>{`${moves} ${countNoun(moves, 'transfer')} since minting`}</span>
            </>
          )
        ) : undefined,
    },
    {
      label: 'CKB held in its cell',
      value: <CkbAmount form="summary" shannons={held} signed={false} tone="plain" />,
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
      note: formatCount(object.contentSize, 'byte'),
    },
  ];
  const coll = collection.data;
  const onchain = coll?.composition?.onchainCount;
  // Hierarchy v3 (F4): every sentence that left tier 1, in full; it heads the DNA column. Its life
  // story is the page's lede since readability R8 (what it is and whether it is live, said once).
  // the footnote is said where the cell has moved (`lockFootnote`: two versions or more)
  const words = (coll || genetics.data || (history && history.length > 1)) && (
    <DetailSection title="This object, in words">
      <Prose>
        {coll && collectionName && (
          <p>
            {unitNode(
              `One of ${coll.liveCount.toLocaleString('en-US')} in ${collectionName}, ${coll.holdersCount === coll.liveCount ? 'each held by a different address' : `held by ${formatCount(coll.holdersCount, 'address', 'addresses')}`}.`
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
        {history && <LockFootnote versions={history} noun="object" />}
      </Prose>
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
      identity={<HeadId value={id} kind="hash" copy="copy object id" />}
      // Readability R8: the lede says what the object is and whether it is live — its path to the
      // lock that holds it now, or that its cell is gone — the sentence "This object, in words"
      // told; then what it is made of (the composition, the owner's first-class rule)
      lede={
        <div className="space-y-2.5 pt-1">
          {history && history.length > 0 && (
            <p data-role="object-state" className="text-text text-body max-w-measure">
              <ItemLife versions={history} lede />
            </p>
          )}
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
                  // the sequencing gel stands here now (lane P2's follow-up, item 1): a trait's
                  // hover lights its gene in it, as it lights its lane in tier 1's allele gel
                  active={activeGene}
                  onActive={setActiveGene}
                  tier={tier}
                />
              ) : (
                <AssetReadState label="DNA evidence" error={genetics.error} read={genetics} />
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
              <AssetReadState label="raw object data" error={protocol.error} read={protocol} />
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
            read={decoded}
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
                  read={collectionGenes}
                />
              )}
            </>
          ) : (
            <AssetReadState label="DNA evidence" error={genetics.error} read={genetics} />
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
              title="CKB held"
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
          <AssetReadState label="cell versions" error={versions.error} read={versions} />
        )}
        <CapacityPartition
          subject={`Object ${id}`}
          total={object.ownedCapacity}
          occupied={object.ownedKnowledge}
          free={(held - occupied).toString()}
        />
        {/* Hierarchy v3 (F4): the cell's story is the caption under its evidence. */}
        {versions.data && versions.data.data.length > 0 && (
          <p data-role="caption" className="text-text text-body max-w-measure">
            {cellLifeSentence(versions.data.data, 'object')}
          </p>
        )}
      </div>
      {object.clusterId && (
        <section aria-label={`In ${collectionName ?? 'this collection'}`} className="space-y-3">
          {/* Rubbing × Song print, phase 5 (A11): the collection, run on its name; the link to it
              is the right-prompt note. */}
          <PromptHeading
            command="Collection"
            args={collectionName ?? 'this collection'}
            note={
              <AppLink look="onward" href={collectionHref!}>
                Open the collection
              </AppLink>
            }
          />
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
                    aria-label={`${percentOf(BigInt(onchain), BigInt(coll.liveCount))} fully on-chain`}
                    role="img"
                    className="bg-bar-track h-2 overflow-hidden"
                  >
                    <span
                      className="scan-fill block h-full"
                      style={{
                        width: `${formatPercent(BigInt(onchain), BigInt(coll.liveCount))}%`,
                        backgroundImage: COMPOSITION_BADGES[coll.composition.tier].bar,
                      }}
                    />
                  </div>
                )}
              {cluster.data && (
                <p className="text-text text-small max-w-measure">
                  Its cluster cell is held by{' '}
                  {cluster.data.ownerAddress || cluster.data.ownerLockHash ? (
                    <Address
                      look="inline"
                      address={cluster.data.ownerAddress ?? cluster.data.ownerLockHash!}
                      size="row"
                      className="inline"
                    />
                  ) : (
                    'no live owner'
                  )}
                  {cluster.data.createdAtBlock !== null && (
                    <>
                      {' '}
                      · created at <BlockLink look="inline" number={cluster.data.createdAtBlock} />
                    </>
                  )}
                </p>
              )}
            </>
          ) : (
            <AssetReadState label="collection" error={collection.error} read={collection} />
          )}
        </section>
      )}
    </AssetDetailShell>
  );
}
