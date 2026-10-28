'use client';

import { useQuery } from '@tanstack/react-query';
import { api, isNotFoundApiError } from '@/lib/api';
import { hexBytes } from '@/lib/hex-bytes';
import { formatUtc8Date } from '@/lib/utils';
import { getMnftClassDetailHref } from '@/lib/detail-routes';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetCount } from '@/components/asset/inventory-table';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CellVersions } from '@/components/asset/cell-versions';
import { MnftPermissions } from '@/components/asset/identity-document';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { ObjectCompositionLabel } from '@/components/address/address-holdings';
import { ExactCkb } from '@/components/activity-event-row';
import { DetailSection } from '@/components/ui/detail-layout';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';

export function MnftTokenIdentity({ id }: { id: string }) {
  const bytes = hexBytes(id, 'm-NFT token ID');
  if (bytes.length < 28) throw new Error('m-NFT token ID needs at least 28 bytes');
  return (
    <ProtocolEvidence
      title="m-NFT token ID"
      dataHex={id}
      fields={[
        { name: 'Issuer ID', start: 0, end: 20, valueHex: bytes.slice(0, 20).join('') },
        {
          name: 'Class index · big-endian',
          start: 20,
          end: 24,
          valueHex: bytes.slice(20, 24).join(''),
        },
        {
          name: 'Token index · big-endian',
          start: 24,
          end: 28,
          valueHex: bytes.slice(24, 28).join(''),
        },
      ]}
    />
  );
}

export function MnftRenderer({ renderer }: { renderer: string | null }) {
  return (
    <figure className="border-base-border min-w-0 space-y-3 border p-5">
      <div className="text-text-bright text-[18px]">Class renderer</div>
      {renderer ? (
        <>
          <p className="text-text-dim break-all text-[11px]">{renderer}</p>
          {/^(?:https?|ipfs|ipns|ar|btcfs):\/\//i.test(renderer) && (
            <a href={renderer} rel="noreferrer" className="text-aqua text-xs">
              Open the renderer ↗
            </a>
          )}
          <figcaption className="text-text-dim text-[11px]">
            The class records this renderer reference on CKB. Availability of its external content
            is separate from ownership of this token.
          </figcaption>
        </>
      ) : (
        <figcaption className="text-text-dim text-[11px]">No class renderer recorded.</figcaption>
      )}
    </figure>
  );
}

export interface MnftDetailProps {
  objectId: string;
}

export function MnftDetail({ objectId }: MnftDetailProps) {
  const detail = useQuery({
    queryKey: ['mnft-item-detail', objectId],
    queryFn: () => api.getMnftItemDetail(objectId),
  });
  const protocol = useQuery({
    queryKey: ['item-protocol', 'mnft', objectId],
    queryFn: () => api.getItemProtocol('mnft', objectId),
    enabled: !!detail.data,
  });
  const collection = useQuery({
    queryKey: ['object-collection', detail.data?.class.classId],
    queryFn: () => api.getObjectCollection(detail.data!.class.classId),
    enabled: !!detail.data,
  });
  if (!detail.data && isNotFoundApiError(detail.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'object',
          identifier: objectId,
          checkedEndpoint: `/assets/objects/items/${encodeURIComponent(objectId)}`,
        }}
      />
    );
  }
  if (!detail.data) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto min-w-0 px-4 py-8">
          <h1 className="text-text-bright text-[26px]">m-NFT</h1>
          <AssetReadState label="m-NFT" error={detail.error} loading={detail.isLoading} />
        </main>
      </div>
    );
  }
  const item = detail.data;
  const evidence = protocol.data;
  if (evidence && evidence.kind !== 'm-nft')
    throw new Error(`${objectId}: expected m-NFT protocol evidence, got ${evidence.kind}`);
  const history = evidence && {
    data: evidence.versions.map((version) => version.cell),
    total: evidence.versions.length,
  };
  const first = history?.data[0];
  const last = history?.data.at(-1);
  const current = history?.data.find((cell) => cell.isLive);
  const classHref = getMnftClassDetailHref(item.class.classId);
  const className = item.class.name ?? 'm-NFT class';
  const evidenceState = (
    <AssetReadState
      error={protocol.error}
      loading={protocol.isLoading}
      label="item cell evidence"
    />
  );
  return (
    <AssetDetailShell
      kind="objects"
      title={
        <>
          {className} #<AssetCount value={item.tokenIndex} />
        </>
      }
      breadcrumb={[
        { label: 'Inventory', href: '/inventory' },
        { label: 'Objects', href: '/inventory/objects' },
        { label: className, href: classHref },
      ]}
      status={
        <span className={item.isLive ? 'text-jade text-[11px]' : 'text-text-dim text-[11px]'}>
          {item.isLive ? 'Live' : 'Burned'}
        </span>
      }
      identity={<Hash hash={item.nftId} truncate={false} className="break-all text-[11px]" />}
      intro={
        <div className="space-y-4">
          <AssetReadState error={detail.error} label="m-NFT" retained />
          {item.composition ? (
            <ObjectCompositionLabel tier={item.composition.tier} />
          ) : (
            <p className="text-text-dim text-[11px]">Composition unavailable.</p>
          )}
          <p>
            Issued in <AppLink href={classHref}>{className}</AppLink> by{' '}
            {item.issuer.name ?? 'the on-chain issuer'} at{' '}
            <AppLink href={`/blocks/${item.createdAtBlock}`}>
              block <AssetCount value={item.createdAtBlock} />
            </AppLink>
            .
          </p>
          {item.class.description && (
            <blockquote className="border-base-border border-l pl-4">
              <p className="break-words">{item.class.description}</p>
              <footer className="text-text-dim mt-2 text-[11px]">
                The class description recorded on chain
              </footer>
            </blockquote>
          )}
        </div>
      }
      facts={[
        {
          label: 'Owner',
          value: history ? (
            current ? (
              <AppLink href={`/address/${current.ownerAddress}`}>
                <Hash hash={current.ownerAddress} className="text-xs" copyable={false} />
              </AppLink>
            ) : (
              'No live cell'
            )
          ) : (
            evidenceState
          ),
        },
        { label: 'Class', value: <AppLink href={classHref}>{className}</AppLink> },
        {
          label: 'Issued',
          value: first ? (
            <>
              <span className="block text-[11px]">{formatUtc8Date(first.timestamp)} UTC+8</span>
              <AppLink href={`/blocks/${first.blockNumber}`}>
                #<AssetCount value={first.blockNumber} />
              </AppLink>
            </>
          ) : (
            evidenceState
          ),
        },
        {
          label: 'Latest cell version',
          value: last ? (
            <AppLink href={`/cell/${last.txHash}/${last.outputIndex}`}>
              <span className="block text-[11px]">{formatUtc8Date(last.timestamp)} UTC+8</span>Block{' '}
              <AssetCount value={last.blockNumber} /> · output {last.outputIndex}
            </AppLink>
          ) : (
            evidenceState
          ),
        },
        {
          label: 'CKB held in its cell',
          value: history ? (
            current ? (
              <>
                <ExactCkb shannons={BigInt(current.capacity)} signed={false} tone="plain" wrap />
                <div className="text-text-dim mt-1 text-[11px]">
                  <ExactCkb
                    shannons={BigInt(current.occupiedCapacity)}
                    signed={false}
                    tone="plain"
                    wrap
                  />{' '}
                  occupied
                </div>
              </>
            ) : (
              'No live cell'
            )
          ) : (
            evidenceState
          ),
        },
        { label: 'Standard', value: 'm-NFT' },
      ]}
      feed={
        <AssetFeed
          key={objectId}
          queryKey={`mnft:${objectId}`}
          scope="Its whole life"
          loadActivities={(params) => api.getItemActivityFeed('mnft', objectId, params)}
          loadTransactions={(params) => api.getItemTransactions('mnft', objectId, params)}
          context={{
            label: 'This token',
            render: (transaction) => <ItemTransactionContext transaction={transaction} />,
          }}
        />
      }
      tier2={
        <>
          <MnftTokenIdentity id={item.nftId} />
          <DetailSection title="Issuer record">
            <p className="text-[11px]">
              Issuer ID ·{' '}
              <Hash
                hash={item.issuer.issuerId}
                truncate={false}
                className="break-all text-[11px]"
              />
            </p>
            <p className="text-text-dim mt-2 text-[11px]">
              <AssetCount value={item.issuer.classCount} /> classes ·{' '}
              <AssetCount value={item.issuer.setCount} /> sets
            </p>
            {item.issuer.infoHex !== null && (
              <details className="mt-3 text-[11px]">
                <summary className="cursor-pointer">Raw issuer metadata</summary>
                <pre className="mt-3 whitespace-pre-wrap break-all">{item.issuer.infoHex}</pre>
              </details>
            )}
          </DetailSection>
          <AssetReadState
            error={protocol.error}
            label="item cell evidence"
            retained={!!evidence}
            loading={protocol.isLoading}
          />
          {evidence && <ItemEvidence protocol={evidence} />}
        </>
      }
    >
      <MnftRenderer renderer={item.class.renderer} />
      <DetailSection title="Token properties">
        <dl className="grid gap-4 sm:grid-cols-2">
          <div>
            <dt className="text-text-dim text-[11px]">Token</dt>
            <dd>
              #<AssetCount value={item.tokenIndex} /> · <AssetCount value={item.class.issued} />{' '}
              issued of <AssetCount value={item.class.total} /> planned
            </dd>
          </div>
          <div>
            <dt className="text-text-dim text-[11px]">Characteristic</dt>
            <dd className="break-all text-xs">
              {item.characteristicHex}
              <span className="text-text-dim block text-[11px]">
                {hexBytes(item.characteristicHex, 'm-NFT characteristic').length} bytes
              </span>
            </dd>
          </div>
        </dl>
      </DetailSection>
      <MnftPermissions
        permissions={item.permissions}
        configure={item.configure}
        state={{ claimed: item.claimed, locked: item.locked, value: item.state }}
      />
      <AssetReadState
        error={protocol.error}
        label="item cell evidence"
        retained={!!evidence}
        loading={protocol.isLoading}
      />
      {history && <CellVersions history={history} subject="m-NFT" />}
      <DetailSection
        title={`In ${className}`}
        actions={
          <AppLink className="text-[11px]" href={classHref}>
            Open the class →
          </AppLink>
        }
      >
        {collection.data ? (
          <>
            <AssetReadState error={collection.error} label="class totals" retained />
            <dl className="grid gap-4 sm:grid-cols-2">
              <div>
                <dt className="text-text-dim text-[11px]">Live items</dt>
                <dd>
                  <AssetCount value={collection.data.liveCount} /> of{' '}
                  <AssetCount value={collection.data.totalCount} /> issued
                </dd>
              </div>
              <div>
                <dt className="text-text-dim text-[11px]">Holders</dt>
                <dd>
                  <AssetCount value={collection.data.holdersCount} />
                </dd>
              </div>
              <div>
                <dt className="text-text-dim text-[11px]">CKB held in class item cells</dt>
                <dd>
                  {collection.data.ownedCapacity === null ? (
                    'Unavailable'
                  ) : (
                    <ExactCkb
                      shannons={BigInt(collection.data.ownedCapacity)}
                      signed={false}
                      tone="plain"
                      wrap
                    />
                  )}
                </dd>
              </div>
              {collection.data.composition && (
                <div>
                  <dt className="text-text-dim text-[11px]">Fully on chain</dt>
                  <dd>
                    <AssetCount value={collection.data.composition.onchainCount} /> of{' '}
                    <AssetCount value={collection.data.liveCount} /> live items
                  </dd>
                </div>
              )}
            </dl>
          </>
        ) : (
          <AssetReadState
            error={collection.error}
            loading={collection.isLoading}
            label="class totals"
          />
        )}
      </DetailSection>
    </AssetDetailShell>
  );
}
