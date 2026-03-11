'use client';

import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useState } from 'react';

import { Header } from '@/components/layout/header';
import { IdentityActivityCard } from '@/components/identity/identity-activity-card';
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
  type CollectionItem,
} from '@/lib/api';
import {
  normalizeAssetId,
  parseActivityCursor,
  formatActivityTimestamp,
  normalizeActivityAction,
  formatExpiry,
} from '@/lib/asset-utils';
import { formatNumber } from '@/lib/utils';

export interface IdentityItemDetailConfig {
  standard: 'dotbit' | 'did_ckb';
  fetchDetail: (identityId: string) => Promise<CollectionItem>;
  fetchActivities: (
    identityId: string,
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
  config: IdentityItemDetailConfig;
  identityId: string;
}

export function IdentityItemDetail({ config, identityId: routeIdentityId }: Props) {
  const { labels, fetchDetail, fetchActivities } = config;
  const pathname = usePathname();
  const router = useRouter();
  const searchParams = useSearchParams();
  const identityId = normalizeAssetId(routeIdentityId);
  const [activityCursor, setActivityCursor] = useState<string | undefined>(() =>
    parseActivityCursor(searchParams.get('activity_cursor'))
  );
  const [activityCursorHistory, setActivityCursorHistory] = useState<string[]>([]);

  const detailQuery = useQuery({
    queryKey: [`${config.standard}-item-detail`, identityId],
    queryFn: () => fetchDetail(identityId),
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
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="bg-base-elevated mb-6 h-10 w-48 animate-pulse rounded" />
          <div className="space-y-6">
            <div className="border-base-border bg-base-surface/40 h-40 animate-pulse rounded border" />
            <div className="border-base-border bg-base-surface/40 h-52 animate-pulse rounded border" />
          </div>
        </main>
      </div>
    );
  }

  if (!detail) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">{labels.notFoundMsg}</h2>
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
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6 flex items-center gap-4">
          <Link
            href={labels.backHref}
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            ← {labels.backLabel}
          </Link>
          <Link
            href="/identities"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            Back to Identities
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
                  <span className="text-text-bright font-mono">{labels.standardDisplay}</span>
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
                    className="text-emphasis font-mono hover:underline"
                  >
                    #{formatNumber(detail.createdAtBlock)}
                  </Link>
                </DataField>
                {labels.showExpiry && (
                  <DataField label="Expires At">
                    <span className="text-text-bright font-mono">
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
                  <span className="text-text-bright font-mono">{detail.name || 'Unavailable'}</span>
                </DataField>
                <DataField label={labels.idLabel} layout="vertical" valueClassName="w-full">
                  <HexDisplay value={detail.nftId} truncate={false} />
                </DataField>
                <DataField label="Owner" layout="vertical" valueClassName="w-full">
                  {ownerAddress ? (
                    <Address address={ownerAddress} truncate={false} />
                  ) : detail.ownerLockHash ? (
                    <Link href={`/address/${detail.ownerLockHash}`} className="hover:underline">
                      <HexDisplay value={detail.ownerLockHash} truncate={false} />
                    </Link>
                  ) : (
                    <span className="text-text-dim font-mono">Unavailable</span>
                  )}
                </DataField>
                <DataField label="Owner Lock Hash" layout="vertical" valueClassName="w-full">
                  {detail.ownerLockHash ? (
                    <Link href={`/address/${detail.ownerLockHash}`} className="hover:underline">
                      <HexDisplay value={detail.ownerLockHash} truncate={false} />
                    </Link>
                  ) : (
                    <span className="text-text-dim font-mono">Unavailable</span>
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
                    className="text-emphasis font-mono hover:underline"
                  >
                    <HexDisplay value={liveTxHash} size="sm" />-{liveOutputIndex}
                  </Link>
                </DataField>
              ) : (
                <div className="text-text-dim font-mono text-sm">{labels.recycledMsg}</div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Activities</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <div className="border-base-border flex items-center gap-1.5 border-b px-4 py-2">
                <span className="bg-emphasis/15 text-emphasis rounded px-2.5 py-1 font-mono text-xs">
                  Activities
                </span>
              </div>
              <div className="p-4">
                {isActivitiesLoading ? (
                  <div className="text-text-dim py-2 text-sm">Loading activities...</div>
                ) : !itemActivities?.data?.length ? (
                  <div className="text-text-dim py-2 text-sm">No related activities found.</div>
                ) : (
                  <div className="space-y-2">
                    {itemActivities.data.map((activity) => (
                      <IdentityActivityCard
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
