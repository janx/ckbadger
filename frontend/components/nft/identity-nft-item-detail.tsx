'use client';

import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useState } from 'react';

import { Header } from '@/components/layout/header';
import { NftActivityCard } from '@/components/nft/nft-activity-card';
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { HexDisplay } from '@/components/ui/hex-display';
import { PageHeader, Badge } from '@/components/ui/page-header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import {
  api,
  type CursorPaginatedResponse,
  type MnftItemActivity,
  type NftCollectionItem,
} from '@/lib/api';
import {
  normalizeNftId,
  parseActivityCursor,
  formatActivityTimestamp,
  normalizeActivityAction,
  formatExpiry,
} from '@/lib/nft-utils';
import { formatNumber } from '@/lib/utils';

export interface IdentityNftItemDetailConfig {
  standard: 'dotbit' | 'did_ckb';
  fetchDetail: (nftId: string) => Promise<NftCollectionItem>;
  fetchActivities: (
    nftId: string,
    params: { limit: number; cursor?: string }
  ) => Promise<CursorPaginatedResponse<MnftItemActivity>>;
  labels: {
    standardDisplay: string;
    nameLabel: string;
    idLabel: string;
    backLabel: string;
    backHref: string;
    defaultTitle: string;
    notFoundMsg: string;
    recycledMsg: string;
    showExpiry: boolean;
  };
}

interface Props {
  config: IdentityNftItemDetailConfig;
  nftId: string;
}

export function IdentityNftItemDetail({ config, nftId: routeNftId }: Props) {
  const { labels, fetchDetail, fetchActivities } = config;
  const pathname = usePathname();
  const router = useRouter();
  const searchParams = useSearchParams();
  const nftId = normalizeNftId(routeNftId);
  const [activityCursor, setActivityCursor] = useState<string | undefined>(() =>
    parseActivityCursor(searchParams.get('activity_cursor'))
  );
  const [activityCursorHistory, setActivityCursorHistory] = useState<string[]>([]);

  const detailQuery = useQuery({
    queryKey: [`${config.standard}-item-detail`, nftId],
    queryFn: () => fetchDetail(nftId),
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
    queryKey: [`${config.standard}-item-activities`, detail?.nftId, activityCursor],
    queryFn: () => {
      const queryParams: { limit: number; cursor?: string } = { limit: 20 };
      if (activityCursor) {
        queryParams.cursor = activityCursor;
      }
      return fetchActivities(detail!.nftId, queryParams);
    },
    enabled: !!detail?.nftId,
    retry: false,
  });

  const goToNextActivityPage = useCallback(
    (nextCursor: string | null | undefined) => {
      if (!nextCursor) {
        return;
      }
      setActivityCursorHistory((prev) => [...prev, activityCursor || '']);
      setActivityCursor(nextCursor);
    },
    [activityCursor]
  );

  const goToPreviousActivityPage = useCallback(() => {
    if (activityCursorHistory.length === 0) {
      return;
    }
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
    if (next === current) {
      return;
    }
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
            <div className="h-52 animate-pulse rounded border border-slate-800 bg-slate-900/40" />
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
              <h2 className="text-xl text-slate-400">{labels.notFoundMsg}</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  const ownerAddress = ownerAddressRecord?.address || null;
  const liveTxHash = detail.txHash;
  const liveOutputIndex = detail.outputIndex;

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6 flex items-center gap-4">
          <Link
            href={labels.backHref}
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← {labels.backLabel}
          </Link>
          <Link
            href="/assets?type=nft"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            Back to NFTs
          </Link>
        </div>

        <PageHeader
          title={detail.name || labels.defaultTitle}
          badge={
            detail.isLive ? (
              <Badge variant="green">Live</Badge>
            ) : (
              <Badge variant="red">Recycled</Badge>
            )
          }
        />

        <div className="space-y-6">
          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Asset Snapshot</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={2}>
                <DataField label="Standard">
                  <span className="font-mono text-slate-200">{labels.standardDisplay}</span>
                </DataField>
                <DataField label="Status">
                  {detail.isLive ? (
                    <Badge variant="green">Live</Badge>
                  ) : (
                    <Badge variant="red">Recycled</Badge>
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
                {labels.showExpiry && (
                  <DataField label="Expires At">
                    <span className="font-mono text-slate-200">
                      {formatExpiry(detail.expiredAt)}
                    </span>
                  </DataField>
                )}
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Identity & Ownership</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField label={labels.nameLabel}>
                  <span className="font-mono text-slate-200">{detail.name || 'Unavailable'}</span>
                </DataField>
                <DataField label={labels.idLabel} layout="vertical" valueClassName="w-full">
                  <HexDisplay value={detail.nftId} truncate={false} color="accent" />
                </DataField>
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
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Cell Status</TerminalPanelHeader>
            <TerminalPanelContent>
              {detail.isLive && liveTxHash != null && liveOutputIndex != null ? (
                <DataField label="Live Cell">
                  <Link
                    href={`/cell/${liveTxHash}-${liveOutputIndex}`}
                    className="text-terminal-green font-mono hover:underline"
                  >
                    <HexDisplay value={liveTxHash} color="accent" size="sm" />-{liveOutputIndex}
                  </Link>
                </DataField>
              ) : (
                <div className="font-mono text-sm text-slate-400">{labels.recycledMsg}</div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Activities</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <div className="flex items-center gap-1.5 border-b border-slate-800 px-4 py-2">
                <span className="bg-terminal-green/15 text-terminal-green rounded px-2.5 py-1 font-mono text-xs">
                  Activities
                </span>
              </div>
              <div className="p-4">
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
                        timestamp={formatActivityTimestamp(activity.timestamp)}
                        actions={activity.actions}
                        normalizeAction={normalizeActivityAction}
                      />
                    ))}
                  </div>
                )}
              </div>
            </TerminalPanelContent>
            <TerminalPanelFooter>
              <CursorPagination
                total={itemActivities?.total ?? undefined}
                totalLabel="activities"
                pageSize={20}
                page={activityCursorHistory.length + 1}
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
