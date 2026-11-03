'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, isNotFoundApiError } from '@/lib/api';
import { hexBytes } from '@/lib/hex-bytes';
import { formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { getMnftClassDetailHref } from '@/lib/detail-routes';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { AssetDetailShell, AssetMetrics, AssetReadState } from '@/components/asset/detail-shell';
import { AssetCount } from '@/components/asset/inventory-table';
import { AssetFeed } from '@/components/asset/asset-feed';
import { CellVersions } from '@/components/asset/cell-versions';
import { MnftPermissions } from '@/components/asset/identity-document';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { ExactCkb } from '@/components/activity-event-row';
import { DetailSection } from '@/components/ui/detail-layout';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { formatBlockNumber } from '@/lib/format-block';
import { CompositionBadge } from '@/components/ui/composition-mark';
import {
  CompositionLine,
  LiveBadge,
  cellLifeSentence,
  ckbText,
  itemLifeSentence,
  itemLifeAges,
  lockFootnote,
  lockPhrase,
} from '@/components/asset/object-hero';

export function MnftTokenIdentity({ id }: { id: string }) {
  const bytes = hexBytes(id, 'm-NFT token ID');
  if (bytes.length < 28) throw new Error('m-NFT token ID needs at least 28 bytes');
  return (
    <ProtocolEvidence
      title="m-NFT token ID"
      dataHex={id}
      fields={[
        {
          name: 'Issuer ID',
          start: 0,
          end: 20,
          valueHex: bytes.slice(0, 20).join(''),
          meaning: 'The issuer that created the class.',
        },
        {
          // F2 objects (OB-M11): each index with its decoded value.
          name: `Class index · big-endian = ${parseInt(bytes.slice(20, 24).join(''), 16)}`,
          start: 20,
          end: 24,
          valueHex: bytes.slice(20, 24).join(''),
          meaning: 'Which of the issuer’s classes, a big-endian number.',
        },
        {
          name: `Token index · big-endian = ${parseInt(bytes.slice(24, 28).join(''), 16)}`,
          start: 24,
          end: 28,
          valueHex: bytes.slice(24, 28).join(''),
          meaning: 'Which token of the class, a big-endian number.',
        },
      ]}
    />
  );
}

/**
 * Where the class's picture lives (Mnft board, OB-M4): a plain frame on purpose — the crystal
 * frames are for fully on-chain objects. The scheme of the renderer reference says where it is.
 */
export function MnftRenderer({ renderer }: { renderer: string | null }) {
  const scheme = renderer?.match(/^([a-z][a-z0-9+.-]*):\/\//i)?.[1].toLowerCase() ?? null;
  const where =
    scheme === 'http' || scheme === 'https'
      ? 'a file on a web server'
      : scheme === 'ipfs' || scheme === 'ipns' || scheme === 'ar'
        ? 'a file on decentralized storage'
        : scheme === 'btcfs'
          ? 'a Bitcoin inscription'
          : null;
  return (
    <figure className="border-base-border min-w-0 space-y-3 rounded-md border p-5 text-center">
      {renderer ? (
        <>
          <div className="text-text-bright text-body">
            {where ? `Its image is ${where}` : 'Its class names a renderer'}
          </div>
          <p className="text-text-dim text-small break-all">{renderer}</p>
          {where && (
            <a href={renderer} rel="noreferrer" target="_blank" className="text-aqua text-small">
              Open the file ↗
            </a>
          )}
          <figcaption className="text-text-dim text-meta">
            One file for the whole class.
            {scheme === 'http' || scheme === 'https'
              ? ' If that server goes away, the picture goes with it; the token itself stays on CKB.'
              : ' Its availability is separate from ownership of this token, which stays on CKB.'}
          </figcaption>
        </>
      ) : (
        <figcaption className="text-text-dim text-meta">No class renderer recorded.</figcaption>
      )}
    </figure>
  );
}

export interface MnftDetailProps {
  objectId: string;
}

export function MnftDetail({ objectId }: MnftDetailProps) {
  const [now] = useState(() => Date.now());
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
      <div className="min-h-screen">
        <Header />
        <main className="container mx-auto min-w-0 py-8">
          <h1 className="text-text-bright text-entity">m-NFT</h1>
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
  const moves = history ? history.data.length - 1 : null;
  const characteristic = hexBytes(item.characteristicHex, 'm-NFT characteristic');
  const classHref = getMnftClassDetailHref(item.class.classId);
  const className = item.class.name ?? 'm-NFT class';
  // The m-NFT's index in its class (not a block height), as the h1 writes it.
  const tokenNumber = item.tokenIndex.toLocaleString('en-US');
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
      documentTitle={[`${className} #${tokenNumber}`, 'object']}
      breadcrumb={[
        { label: 'Inventory', href: '/inventory/objects' },
        { label: 'Objects', href: '/inventory/objects' },
        { label: className, href: classHref },
      ]}
      status={<LiveBadge live={item.isLive} gone="Burned" />}
      identity={<Hash hash={item.nftId} truncate={false} className="text-meta break-all" />}
      intro={
        <div className="space-y-4 pt-1">
          <AssetReadState error={detail.error} label="m-NFT" retained />
          {item.composition ? (
            <CompositionLine tier={item.composition.tier} />
          ) : (
            <p className="text-text-dim text-meta">Composition unavailable.</p>
          )}
          {history && history.data.length > 0 ? (
            <p className="text-text text-body leading-relaxed">
              <span title={itemLifeAges(history.data, now, 'Issued')}>
                {itemLifeSentence(history.data, now, 'Issued', item.issuer.name ?? 'its issuer')}
              </span>
            </p>
          ) : (
            <p className="text-text text-body">
              Issued in <AppLink href={classHref}>{className}</AppLink> at{' '}
              <AppLink href={`/blocks/${item.createdAtBlock}`}>
                block <AssetCount value={item.createdAtBlock} />
              </AppLink>
              .
            </p>
          )}
          {item.class.description && (
            <blockquote className="border-base-border border-l pl-4">
              <p className="break-words">{item.class.description}</p>
              <footer className="text-text-dim text-meta mt-2">
                the class’s description, on chain
              </footer>
            </blockquote>
          )}
          {/* F2 objects (OB-M6): the board's four properties; configure decoding is tier 2. */}
          {/* One column until md: at 640 the 2×2 grid broke "8 bytes" (responsive B#15). */}
          <dl aria-label="Properties" className="grid gap-x-7 md:grid-cols-2">
            <div className="border-base-elevated flex justify-between gap-3 border-b py-2">
              <dt className="text-text-dim">Token</dt>
              <dd className="text-text-bright text-right">
                #<AssetCount value={item.tokenIndex} />{' '}
                <span className="text-text-dim">
                  of <AssetCount value={item.class.issued} /> issued ·{' '}
                  <AssetCount value={item.class.total} /> planned
                </span>
              </dd>
            </div>
            <div className="border-base-elevated flex justify-between gap-3 border-b py-2">
              <dt className="text-text-dim">State</dt>
              <dd className="text-text-bright text-right">
                0x{item.state.toString(16).padStart(2, '0')} ·{' '}
                {item.claimed ? 'claimed' : 'unclaimed'} · {item.locked ? 'locked' : 'unlocked'}
              </dd>
            </div>
            <div className="border-base-elevated flex justify-between gap-3 border-b py-2">
              <dt className="text-text-dim">Issuer</dt>
              <dd className="text-text-bright text-right">
                {item.issuer.name ?? 'Unnamed issuer'}{' '}
                <span className="text-text-dim">
                  · <AssetCount value={item.issuer.classCount} /> classes
                </span>
              </dd>
            </div>
            <div className="border-base-elevated flex justify-between gap-3 border-b py-2">
              <dt className="text-text-dim">Characteristic</dt>
              <dd className="text-text-bright min-w-0 text-right [overflow-wrap:anywhere]">
                {characteristic.every((b) => b === '00') ? (
                  <>
                    none, all zero ·{' '}
                    <span className="whitespace-nowrap">{characteristic.length} bytes</span>
                  </>
                ) : (
                  item.characteristicHex
                )}
              </dd>
            </div>
          </dl>
        </div>
      }
      // Consistency round C7: the rail's facts as the metrics row under the hero; a time of day
      // stays in the metric's title.
      metrics={
        <AssetMetrics
          items={[
            {
              label: 'Owner',
              size: 'body',
              value: history ? (
                current ? (
                  <AppLink href={`/address/${current.ownerAddress}`}>
                    <Hash hash={current.ownerAddress} className="text-body" copyable={false} />
                  </AppLink>
                ) : (
                  'No live cell'
                )
              ) : (
                evidenceState
              ),
              note: current ? (
                <>
                  {current.lockName ?? 'Unregistered lock'} · since{' '}
                  {formatUtc8Date(current.timestamp)}
                </>
              ) : undefined,
            },
            {
              label: 'Class',
              size: 'body',
              value: <AppLink href={classHref}>{className}</AppLink>,
              note: `by ${item.issuer.name ?? 'an unnamed issuer'}`,
            },
            {
              label: 'Issued',
              title: first ? `issued ${formatUtc8DateTime(first.timestamp)}` : undefined,
              value: first ? formatUtc8Date(first.timestamp) : evidenceState,
              note: first ? (
                <>
                  <AppLink href={`/blocks/${first.blockNumber}`}>
                    {formatBlockNumber(first.blockNumber)}
                  </AppLink>{' '}
                  · to {lockPhrase(first.lockName)}
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
                    <AppLink href={`/cell/${last.txHash}-${last.outputIndex}`}>
                      {formatUtc8Date(last.timestamp)}
                    </AppLink>
                  )
                ) : (
                  evidenceState
                ),
              note:
                last && moves !== null ? (
                  moves === 0 ? (
                    'since issue'
                  ) : (
                    <>
                      <AppLink href={`/blocks/${last.blockNumber}`}>
                        {formatBlockNumber(last.blockNumber)}
                      </AppLink>{' '}
                      · {moves} transfer{moves === 1 ? '' : 's'} since issue
                    </>
                  )
                ) : undefined,
            },
            {
              label: 'CKB held in its cell',
              value: history ? (
                current ? (
                  <ExactCkb shannons={BigInt(current.capacity)} signed={false} tone="plain" wrap />
                ) : (
                  'No live cell'
                )
              ) : (
                evidenceState
              ),
              note: current ? (
                <>
                  <span className="text-text">{ckbText(BigInt(current.occupiedCapacity))}</span>{' '}
                  occupied · {ckbText(BigInt(current.capacity) - BigInt(current.occupiedCapacity))}{' '}
                  free
                </>
              ) : undefined,
            },
            { label: 'Standard', size: 'body', value: 'm-NFT' },
          ]}
        />
      }
      feed={
        <AssetFeed
          key={objectId}
          queryKey={`mnft:${objectId}`}
          scope="Its whole life"
          item={{ kind: 'object' }}
          plain
          mark="objects"
          focusItemId={objectId}
          loadActivities={(params) => api.getItemActivityFeed('mnft', objectId, params)}
          loadTransactions={(params) => api.getItemTransactions('mnft', objectId, params)}
          context={{
            label: 'This token',
            render: (transaction) => (
              <ItemTransactionContext transaction={transaction} item={{ kind: 'object' }} />
            ),
          }}
        />
      }
      tier2={
        <>
          <MnftTokenIdentity id={item.nftId} />
          <MnftPermissions
            permissions={item.permissions}
            configure={item.configure}
            state={{ claimed: item.claimed, locked: item.locked, value: item.state }}
          />
          <DetailSection title="Issuer record">
            <p className="text-meta">Name · {item.issuer.name ?? 'not recorded'}</p>
            <p className="text-meta">
              Issuer ID ·{' '}
              <Hash hash={item.issuer.issuerId} truncate={false} className="text-meta break-all" />
            </p>
            <p className="text-text-dim text-meta mt-2">
              <AssetCount value={item.issuer.classCount} /> classes ·{' '}
              <AssetCount value={item.issuer.setCount} /> sets
            </p>
            {item.issuer.infoHex !== null && (
              <details className="text-meta mt-3">
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
      <AssetReadState
        error={protocol.error}
        label="item cell evidence"
        retained={!!evidence}
        loading={protocol.isLoading}
      />
      {history && (
        <div role="region" aria-label="CKB held in its cell" className="space-y-3">
          <CellVersions
            history={history}
            subject="m-NFT"
            life
            firstLabel="Issued"
            title="CKB held in its cell"
            actions={
              current && (
                <span className="text-text-dim text-small">
                  1 CKB per byte ·{' '}
                  <span className="text-jade">
                    {ckbText(BigInt(current.occupiedCapacity))} occupied
                  </span>{' '}
                  · {ckbText(BigInt(current.capacity) - BigInt(current.occupiedCapacity))} free
                </span>
              )
            }
            intro={
              history.data.length > 0 && (
                <p className="text-body mb-4 leading-relaxed">
                  {cellLifeSentence(history.data, 'token')}
                </p>
              )
            }
          />
          {lockFootnote(history.data, 'token') && (
            <p className="text-text-dim text-small">{lockFootnote(history.data, 'token')}</p>
          )}
        </div>
      )}
      <section aria-label={`In ${className}`} className="space-y-3">
        <div className="flex flex-wrap items-baseline justify-between gap-4">
          <h2 className="text-text-bright text-section">In {className}</h2>
          <AppLink className="text-small" href={classHref}>
            Open the class →
          </AppLink>
        </div>
        {collection.data ? (
          <>
            <AssetReadState error={collection.error} label="class totals" retained />
            <div className="flex flex-wrap items-baseline gap-x-10 gap-y-2">
              <span>
                <span className="text-text-bright text-body">
                  <AssetCount value={collection.data.totalCount} />
                </span>{' '}
                <span className="text-text-dim">
                  issued of <AssetCount value={item.class.total} /> ·{' '}
                  <AssetCount value={collection.data.liveCount} /> live
                </span>
              </span>
              <span>
                <span className="text-text-bright text-body">
                  <AssetCount value={collection.data.holdersCount} />
                </span>{' '}
                <span className="text-text-dim">holders</span>
              </span>
              {collection.data.composition && (
                <span>
                  <span className="text-text-bright text-body">
                    <AssetCount value={collection.data.composition.onchainCount} /> of{' '}
                    <AssetCount value={collection.data.liveCount} />
                  </span>{' '}
                  <span className="text-text-dim">fully on-chain</span>
                </span>
              )}
              {collection.data.ownedCapacity !== null && (
                <span>
                  <span className="text-text-bright text-body">
                    <ExactCkb
                      shannons={BigInt(collection.data.ownedCapacity)}
                      signed={false}
                      tone="plain"
                      wrap
                    />
                  </span>{' '}
                  <span className="text-text-dim">held in their cells</span>
                </span>
              )}
            </div>
            <div className="text-text-dim text-small flex flex-wrap items-center gap-4">
              {collection.data.composition && (
                <CompositionBadge tier={collection.data.composition.tier} size="sm" />
              )}
              <span>issued by {item.issuer.name ?? 'an unnamed issuer'}</span>
            </div>
          </>
        ) : (
          <AssetReadState
            error={collection.error}
            loading={collection.isLoading}
            label="class totals"
          />
        )}
      </section>
    </AssetDetailShell>
  );
}
