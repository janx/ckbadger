'use client';
import { useState, useCallback, type ReactNode } from 'react';
import { useSearchParams, usePathname, useRouter } from '@/src/navigation';
import { parseActivityCursor } from '@/lib/asset-utils';
import { useQuery } from '@tanstack/react-query';
import { api, type ItemKind } from '@/lib/api';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { IdentityDocument } from '@/components/asset/identity-document';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import { CellVersions } from '@/components/asset/cell-versions';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { AppLink } from '@/components/ui/app-link';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { getIdentityCollectionHref } from '@/lib/detail-routes';
import { formatUtc8Date } from '@/lib/utils';
export interface CanonicalIdentityDetailProps {
  kind: Extract<ItemKind, 'dotbit' | 'bit-cell' | 'did' | 'dotcell'>;
  id: string;
  title: string;
  isLive: boolean;
  createdAtBlock: number;
  error: Error | null;
  children?: ReactNode;
  extraFacts?: { label: string; value: ReactNode }[];
}
export function CanonicalIdentityDetail({
  kind,
  id,
  title,
  isLive,
  createdAtBlock,
  error,
  children,
  extraFacts = [],
}: CanonicalIdentityDetailProps) {
  const search = useSearchParams(),
    pathname = usePathname(),
    router = useRouter();
  const [initialActivityCursor] = useState(() =>
    parseActivityCursor(search.get('activity_cursor'))
  );
  const cursorChanged = useCallback(
    (cursor: string | undefined) => {
      const params = new URLSearchParams(search.toString());
      if (cursor) params.set('activity_cursor', cursor);
      else params.delete('activity_cursor');
      if (params.toString() !== search.toString())
        router.replace(params.size ? `${pathname}?${params}` : pathname, { scroll: false });
    },
    [search, pathname, router]
  );
  const versions = useQuery({
    queryKey: ['item-versions', kind, id],
    queryFn: () => api.getItemCellVersions(kind, id),
  });
  const protocol = useQuery({
    queryKey: ['item-protocol', kind, id],
    queryFn: () => api.getItemProtocol(kind, id),
    retry: false,
  });
  const expected = { dotbit: 'dotbit', 'bit-cell': 'bit_cell', did: 'did_ckb', dotcell: 'dotcell' }[
    kind
  ];
  if (protocol.data && protocol.data.kind !== expected)
    throw new Error(`${id}: expected ${expected} protocol, got ${protocol.data.kind}`);
  const current = versions.data?.data.at(-1),
    first = versions.data?.data[0];
  const displayTitle =
    protocol.data?.kind === 'did_ckb' ? protocol.data.versions.at(-1)!.decoded.did : title;
  const standard = kind === 'did' ? 'did_ckb' : kind === 'bit-cell' ? 'bit_cell' : kind;
  const held = current ? (current.isLive ? current.capacity : '0') : null,
    occupied = current ? (current.isLive ? current.occupiedCapacity : '0') : null;
  const facts = [
    { label: 'Standard', value: formatStandard(standard) },
    {
      label: 'Physical cell owner',
      value: current?.isLive ? (
        <Address address={current.ownerAddress} className="text-[11px]" />
      ) : current ? (
        'No live cell'
      ) : (
        'Loading cell history'
      ),
    },
    { label: 'Physical lock', value: current?.lockName ?? 'Unregistered / unavailable' },
    {
      label: 'Created',
      value: (
        <>
          <AppLink href={`/blocks/${createdAtBlock}`}>
            #{createdAtBlock.toLocaleString('en-US')}
          </AppLink>
          {first && (
            <span className="text-text-dim block text-[11px]">
              {formatUtc8Date(first.timestamp)} UTC+8
            </span>
          )}
        </>
      ),
    },
    {
      label: 'Current / final cell',
      value: current ? (
        <AppLink href={`/cell/${current.txHash}-${current.outputIndex}`}>
          Block #{current.blockNumber.toLocaleString('en-US')} · output {current.outputIndex}
        </AppLink>
      ) : (
        'Loading cell history'
      ),
    },
    {
      label: 'CKB held now',
      value:
        held !== null ? (
          <ExactCkb shannons={BigInt(held)} signed={false} tone="plain" wrap />
        ) : (
          'Loading cell history'
        ),
    },
    ...extraFacts,
  ];
  return (
    <AssetDetailShell
      title={displayTitle}
      kind="identities"
      breadcrumb={[
        { label: 'Inventory', href: '/inventory/identities' },
        { label: formatStandard(standard), href: getIdentityCollectionHref(standard, kind) },
      ]}
      status={
        <span className="text-text-dim text-[11px]">
          {isLive ? 'Live cell' : 'Recycled / inactive'}
        </span>
      }
      identity={<Hash hash={id} truncate={false} className="text-[11px]" />}
      facts={facts}
      feed={
        <AssetFeed
          key={`${kind}:${id}`}
          queryKey={`${kind}:${id}`}
          scope="This identity's whole life"
          itemKind="identity"
          initialActivityCursor={initialActivityCursor}
          onActivityCursorChange={cursorChanged}
          loadActivities={(p) => api.getItemActivityFeed(kind, id, p)}
          loadTransactions={(p) => api.getItemTransactions(kind, id, p)}
          context={{
            label: 'Identity · cell after',
            render: (tx) => <ItemTransactionContext transaction={tx} />,
          }}
        />
      }
      tier2={
        protocol.data ? (
          <>
            <AssetReadState label="raw identity evidence" error={protocol.error} retained />
            <ItemEvidence protocol={protocol.data} />
          </>
        ) : (
          <AssetReadState
            label="raw identity evidence"
            error={protocol.error}
            loading={protocol.isLoading}
          />
        )
      }
    >
      <AssetReadState label="identity" error={error} retained />
      {protocol.data ? (
        <>
          <AssetReadState label="identity document" error={protocol.error} retained />
          <IdentityDocument protocol={protocol.data} />
        </>
      ) : (
        <AssetReadState
          label="identity document"
          error={protocol.error}
          loading={protocol.isLoading}
        />
      )}
      {children}
      <section className="space-y-4">
        <h2 className="text-text-bright text-[15px]">CKB held in its cell</h2>
        {held !== null && occupied !== null && (
          <CapacityPartition
            subject={displayTitle}
            total={held}
            occupied={occupied}
            free={(BigInt(held) - BigInt(occupied)).toString()}
          />
        )}{' '}
        {versions.data ? (
          <>
            <AssetReadState label="cell history" error={versions.error} retained />
            <CellVersions subject={displayTitle} history={versions.data} />
          </>
        ) : (
          <AssetReadState
            label="cell history"
            error={versions.error}
            loading={versions.isLoading}
          />
        )}
      </section>
    </AssetDetailShell>
  );
}
