'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import { useParams, usePathname, useRouter, useSearchParams } from 'next/navigation';
import { keepPreviousData, useQuery } from '@tanstack/react-query';

import { Header } from '@/components/layout/header';
import { NftActivityCard } from '@/components/nft/nft-activity-card';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { api } from '@/lib/api';
import { normalizeNftId, parseActivityCursor } from '@/lib/nft-utils';
import { formatNumber } from '@/lib/utils';

function decodeTokenState(state: number): string {
  switch (state) {
    case 0:
      return 'normal';
    case 1:
      return 'locked';
    case 2:
      return 'frozen';
    default:
      return `unknown(${state})`;
  }
}

function decodeTokenConfigure(configure: number): string {
  const flags: string[] = [];
  if ((configure & 0b00000001) !== 0) flags.push('transferable');
  if ((configure & 0b00000010) !== 0) flags.push('burnable');
  if ((configure & 0b00000100) !== 0) flags.push('mutable');
  if ((configure & 0b00001000) !== 0) flags.push('reserved_3');
  return flags.length > 0 ? flags.join(', ') : 'none';
}

export default function MnftItemDetailPage() {
  const params = useParams();
  const pathname = usePathname();
  const router = useRouter();
  const searchParams = useSearchParams();
  const rawNftId = params.nftId as string;
  const nftId = normalizeNftId(rawNftId);
  const [activityCursor, setActivityCursor] = useState<string | undefined>(() =>
    parseActivityCursor(searchParams.get('activity_cursor'))
  );
  const [activityCursorHistory, setActivityCursorHistory] = useState<string[]>([]);

  const detailQuery = useQuery({
    queryKey: ['mnft-item-detail', nftId],
    queryFn: () => api.getMnftItemDetail(nftId),
    retry: false,
  });

  const detail = detailQuery.data;

  const { data: ownerAddressRecord } = useQuery({
    queryKey: ['address-by-lock-hash', detail?.ownerLockHash],
    queryFn: () => api.getAddress(detail!.ownerLockHash!),
    enabled: !!detail?.ownerLockHash,
    retry: false,
  });

  const { data: itemActivities, isLoading: isActivitiesLoading } = useQuery({
    queryKey: ['mnft-item-activities', detail?.nftId, activityCursor],
    queryFn: () => {
      const queryParams: { limit: number; cursor?: string } = { limit: 20 };
      if (activityCursor) {
        queryParams.cursor = activityCursor;
      }
      return api.getMnftItemActivities(detail!.nftId, queryParams);
    },
    enabled: !!detail?.nftId,
    retry: false,
    placeholderData: keepPreviousData,
  });

  const goToNextActivityPage = useCallback(
    (nextCursor: string | null | undefined) => {
      if (!nextCursor) return;
      setActivityCursorHistory((prev) => [...prev, activityCursor || '']);
      setActivityCursor(nextCursor);
    },
    [activityCursor]
  );

  const goToPreviousActivityPage = useCallback(() => {
    if (activityCursorHistory.length === 0) return;
    const prev = activityCursorHistory[activityCursorHistory.length - 1];
    setActivityCursorHistory((history) => history.slice(0, -1));
    setActivityCursor(prev || undefined);
  }, [activityCursorHistory]);

  useEffect(() => {
    const nextParams = new URLSearchParams(searchParams.toString());
    if (activityCursor) {
      nextParams.set('activity_cursor', activityCursor);
    } else {
      nextParams.delete('activity_cursor');
    }
    const current = searchParams.toString();
    const next = nextParams.toString();
    if (next === current) return;
    router.replace(next ? `${pathname}?${next}` : pathname, { scroll: false });
  }, [activityCursor, pathname, router, searchParams]);

  if (detailQuery.isLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="mb-6 h-10 w-48 animate-pulse rounded bg-slate-800" />
          <div className="space-y-6">
            <div className="h-40 animate-pulse rounded border border-slate-800 bg-slate-900/40" />
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/40" />
          </div>
        </main>
      </div>
    );
  }

  if (!detail) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">mNFT item not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  const ownerAddress = ownerAddressRecord?.address || null;

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6 flex items-center gap-4">
          <Link
            href={`/nfts/${detail.class.classId}`}
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← Back to Class
          </Link>
          <Link
            href="/assets?type=nft"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            Back to NFTs
          </Link>
        </div>

        <PageHeader
          title={
            detail.class.name
              ? `${detail.class.name} #${formatNumber(detail.tokenIndex)}`
              : `mNFT #${formatNumber(detail.tokenIndex)}`
          }
          badge={
            detail.isLive ? (
              <Badge variant="green">Live</Badge>
            ) : (
              <Badge variant="red">Burned</Badge>
            )
          }
        />

        <div className="space-y-6">
          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Asset Snapshot</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={2}>
                <DataField label="Standard">
                  <span className="font-mono text-slate-200">{detail.standard.toUpperCase()}</span>
                </DataField>
                <DataField label="Token Index">
                  <span className="font-mono text-slate-200">
                    #{formatNumber(detail.tokenIndex)}
                  </span>
                </DataField>
                <DataField label="Status">
                  {detail.isLive ? (
                    <Badge variant="green">Live</Badge>
                  ) : (
                    <Badge variant="red">Burned</Badge>
                  )}
                </DataField>
                <DataField label="Created Block">
                  <Link
                    href={`/blocks/${detail.createdAtBlock}`}
                    className="text-terminal-green font-mono hover:underline"
                  >
                    #{formatNumber(detail.createdAtBlock)}
                  </Link>
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Identity Graph</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField label="Issuer ID" layout="vertical" valueClassName="w-full">
                  <HexDisplay value={detail.issuer.issuerId} truncate={false} color="accent" />
                </DataField>
                <DataField label="Class ID" layout="vertical" valueClassName="w-full">
                  <Link href={`/nfts/${detail.class.classId}`} className="hover:underline">
                    <HexDisplay value={detail.class.classId} truncate={false} color="accent" />
                  </Link>
                </DataField>
                <DataField label="Token ID" layout="vertical" valueClassName="w-full">
                  <HexDisplay value={detail.nftId} truncate={false} color="accent" />
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">On-chain State</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField label="State">
                  <span className="font-mono text-slate-200">{decodeTokenState(detail.state)}</span>
                </DataField>
                <DataField label="Configure">
                  <span className="font-mono text-slate-200">
                    {decodeTokenConfigure(detail.configure)}
                  </span>
                </DataField>
                <DataField label="Characteristic" layout="vertical" valueClassName="w-full">
                  <HexDisplay value={detail.characteristicHex} truncate={false} color="accent" />
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Ownership & Live Cell</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField label="Owner" layout="vertical" valueClassName="w-full">
                  {ownerAddress ? (
                    <Address address={ownerAddress} truncate={false} />
                  ) : detail.ownerLockHash ? (
                    <Link href={`/address/${detail.ownerLockHash}`} className="hover:underline">
                      <HexDisplay value={detail.ownerLockHash} truncate={false} color="accent" />
                    </Link>
                  ) : (
                    <span className="font-mono text-slate-500">Unavailable</span>
                  )}
                </DataField>
                <DataField label="Owner Lock Hash" layout="vertical" valueClassName="w-full">
                  {detail.ownerLockHash ? (
                    <Link href={`/address/${detail.ownerLockHash}`} className="hover:underline">
                      <HexDisplay value={detail.ownerLockHash} truncate={false} color="accent" />
                    </Link>
                  ) : (
                    <span className="font-mono text-slate-500">Unavailable</span>
                  )}
                </DataField>
                <DataField label="Live Outpoint">
                  {detail.txHash !== null && detail.outputIndex !== null ? (
                    <Link
                      href={`/cell/${detail.txHash}-${detail.outputIndex}`}
                      className="text-terminal-green font-mono hover:underline"
                    >
                      <HexDisplay value={detail.txHash} color="accent" size="sm" />-
                      {detail.outputIndex}
                    </Link>
                  ) : (
                    <span className="font-mono text-slate-500">No live outpoint</span>
                  )}
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Class Context</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField label="Class Name">
                  <span className="font-mono text-slate-200">
                    {detail.class.name || 'Unnamed Class'}
                  </span>
                </DataField>
                <DataField label="Class Description">
                  <span className="text-slate-300">{detail.class.description || 'None'}</span>
                </DataField>
                <DataField label="Renderer">
                  <span className="font-mono text-slate-300">
                    {detail.class.renderer || 'None'}
                  </span>
                </DataField>
                <DataField label="Issued / Total">
                  <span className="font-mono text-slate-200">
                    {formatNumber(detail.class.issued)} / {formatNumber(detail.class.total)}
                  </span>
                </DataField>
                <DataField label="Issuer Name">
                  <span className="font-mono text-slate-200">
                    {detail.issuer.name || 'Unnamed Issuer'}
                  </span>
                </DataField>
                <DataField label="Issuer Counts">
                  <span className="font-mono text-slate-200">
                    classes {formatNumber(detail.issuer.classCount)} / sets{' '}
                    {formatNumber(detail.issuer.setCount)}
                  </span>
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Lifecycle</TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="space-y-2">
                {detail.lifecycle.map((event, index) => (
                  <div
                    key={`${event.event}-${index}`}
                    className="rounded border border-slate-800 bg-slate-900/40 p-3"
                  >
                    <div className="mb-1 flex items-center justify-between gap-3">
                      <span className="font-mono text-xs uppercase tracking-wider text-slate-400">
                        {event.event}
                      </span>
                      {event.blockNumber !== null && (
                        <Link
                          href={`/blocks/${event.blockNumber}`}
                          className="text-terminal-green font-mono text-xs hover:underline"
                        >
                          #{formatNumber(event.blockNumber)}
                        </Link>
                      )}
                    </div>
                    {event.txHash !== null && event.outputIndex !== null && (
                      <Link
                        href={`/cell/${event.txHash}-${event.outputIndex}`}
                        className="text-terminal-green font-mono text-xs hover:underline"
                      >
                        <HexDisplay value={event.txHash} color="accent" size="sm" />-
                        {event.outputIndex}
                      </Link>
                    )}
                    {event.note && <div className="mt-1 text-xs text-slate-400">{event.note}</div>}
                  </div>
                ))}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Activities</TerminalPanelHeader>
            <TerminalPanelContent>
              {isActivitiesLoading ? (
                <div className="py-2 text-sm text-slate-500">Loading activities...</div>
              ) : !itemActivities?.data?.length ? (
                <div className="py-2 text-sm text-slate-500">No related activities found.</div>
              ) : (
                <div className="space-y-2">
                  {itemActivities.data.map((activity) => (
                    <NftActivityCard
                      key={`${activity.blockNumber}-${activity.txIndex}-${activity.txHash}`}
                      txHash={activity.txHash}
                      blockNumber={activity.blockNumber}
                      txIndex={activity.txIndex}
                      actions={activity.actions}
                      badgeActions
                    />
                  ))}
                </div>
              )}
            </TerminalPanelContent>
            <TerminalPanelFooter>
              <CursorPagination
                total={itemActivities?.total ?? undefined}
                totalLabel="activities"
                pageSize={20}
                page={activityCursorHistory.length + 1}
                currentCount={itemActivities?.data?.length ?? 0}
                hasMore={itemActivities?.hasMore ?? false}
                hasPrevious={activityCursorHistory.length > 0}
                onNext={() => goToNextActivityPage(itemActivities?.nextCursor)}
                onPrevious={goToPreviousActivityPage}
              />
            </TerminalPanelFooter>
          </TerminalPanel>
        </div>
      </main>
    </div>
  );
}
