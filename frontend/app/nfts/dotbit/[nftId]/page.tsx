'use client';

import Link from 'next/link';
import { useParams, usePathname, useRouter, useSearchParams } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useRef, useState } from 'react';

import { Header } from '@/components/layout/header';
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
import { api } from '@/lib/api';

function normalizeNftId(raw: string): string {
  const decoded = decodeURIComponent(raw);
  return decoded.startsWith('0x') ? decoded : `0x${decoded}`;
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value);
}

function formatExpiry(expiredAt: number | null | undefined): string {
  if (!expiredAt) return 'Not available';
  return new Date(expiredAt * 1000).toLocaleString();
}

function formatActivityTimestamp(timestamp: string): string {
  const numeric = Number(timestamp);
  if (Number.isFinite(numeric) && numeric > 0) {
    const milliseconds = numeric >= 1_000_000_000_000 ? numeric : numeric * 1000;
    return new Date(milliseconds).toLocaleString();
  }
  const parsed = Date.parse(timestamp);
  if (Number.isFinite(parsed)) {
    return new Date(parsed).toLocaleString();
  }
  return timestamp;
}

function normalizeActivityAction(action: string): string {
  if (action.toLowerCase() === 'burn') {
    return 'recycled';
  }
  return action.toLowerCase();
}

type DotbitActivityFilter = 'all' | 'mint' | 'transfer' | 'recycled';

function parseActivityFilter(raw: string | null): DotbitActivityFilter {
  switch (raw?.toLowerCase()) {
    case 'mint':
      return 'mint';
    case 'transfer':
      return 'transfer';
    case 'recycled':
      return 'recycled';
    default:
      return 'all';
  }
}

function parseActivityCursor(raw: string | null): string | undefined {
  if (!raw) return undefined;
  const trimmed = raw.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function toActivityActionParam(
  filter: DotbitActivityFilter
): 'mint' | 'transfer' | 'burn' | undefined {
  if (filter === 'all') return undefined;
  if (filter === 'recycled') return 'burn';
  return filter;
}

export default function DotbitItemDetailPage() {
  const params = useParams();
  const pathname = usePathname();
  const router = useRouter();
  const searchParams = useSearchParams();
  const rawNftId = params.nftId as string;
  const nftId = normalizeNftId(rawNftId);
  const [activityFilter, setActivityFilter] = useState<DotbitActivityFilter>(() =>
    parseActivityFilter(searchParams.get('activity'))
  );
  const [activityCursor, setActivityCursor] = useState<string | undefined>(() =>
    parseActivityCursor(searchParams.get('activity_cursor'))
  );
  const [activityCursorHistory, setActivityCursorHistory] = useState<string[]>([]);
  const hasActivityFilterMounted = useRef(false);

  const detailQuery = useQuery({
    queryKey: ['dotbit-item-detail', nftId],
    queryFn: () => api.getDotbitItemDetail(nftId),
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
    queryKey: ['dotbit-item-activities', detail?.nftId, activityFilter, activityCursor],
    queryFn: () => {
      const params: { limit: number; cursor?: string; action?: 'mint' | 'transfer' | 'burn' } = {
        limit: 20,
      };
      if (activityCursor) {
        params.cursor = activityCursor;
      }
      const action = toActivityActionParam(activityFilter);
      if (action) {
        params.action = action;
      }
      return api.getDotbitItemActivities(detail!.nftId, params);
    },
    enabled: !!detail?.nftId,
    retry: false,
  });

  const resetActivityPagination = useCallback(() => {
    setActivityCursor(undefined);
    setActivityCursorHistory([]);
  }, []);

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
    if (!hasActivityFilterMounted.current) {
      hasActivityFilterMounted.current = true;
      return;
    }
    resetActivityPagination();
  }, [activityFilter, resetActivityPagination]);

  useEffect(() => {
    const nextParams = new URLSearchParams(searchParams.toString());
    if (activityFilter === 'all') {
      nextParams.delete('activity');
    } else {
      nextParams.set('activity', activityFilter);
    }
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
  }, [activityCursor, activityFilter, pathname, router, searchParams]);

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
              <h2 className="text-xl text-slate-400">.bit item not found</h2>
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
            href="/nfts/dotbit"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← Back to .bit Collection
          </Link>
          <Link
            href="/assets?type=nft"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            Back to NFTs
          </Link>
        </div>

        <PageHeader
          title={detail.name || '.bit account'}
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
                  <span className="font-mono text-slate-200">DOTBIT</span>
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
                <DataField label="Expires At">
                  <span className="font-mono text-slate-200">{formatExpiry(detail.expiredAt)}</span>
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Identity & Ownership</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField label=".bit Name">
                  <span className="font-mono text-slate-200">{detail.name || 'Unavailable'}</span>
                </DataField>
                <DataField label="Account ID" layout="vertical" valueClassName="w-full">
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
                <div className="font-mono text-sm text-slate-400">
                  Recycled .bit account has no live cell.
                </div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader
              indicator="active"
              actions={
                <select
                  value={activityFilter}
                  onChange={(event) =>
                    setActivityFilter(event.target.value as DotbitActivityFilter)
                  }
                  aria-label="Activity Filter"
                  className="focus:border-terminal-green rounded border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs text-slate-200 outline-none transition-colors"
                >
                  <option value="all">All</option>
                  <option value="mint">Mint</option>
                  <option value="transfer">Transfer</option>
                  <option value="recycled">Recycled</option>
                </select>
              }
            >
              Activities
            </TerminalPanelHeader>
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
                      <div
                        key={`${activity.blockNumber}-${activity.txIndex}-${activity.txHash}`}
                        className="rounded border border-slate-800 bg-slate-900/40 p-3"
                      >
                        <div className="mb-1 flex items-center justify-between gap-3">
                          <Link
                            href={`/blocks/${activity.blockNumber}`}
                            className="text-terminal-green font-mono text-xs hover:underline"
                          >
                            #{formatNumber(activity.blockNumber)}
                          </Link>
                          <span className="font-mono text-xs text-slate-500">
                            {formatActivityTimestamp(activity.timestamp)}
                          </span>
                        </div>
                        <Link
                          href={`/tx/${activity.txHash}`}
                          className="text-terminal-green font-mono text-xs hover:underline"
                        >
                          <HexDisplay value={activity.txHash} color="accent" size="sm" />
                        </Link>
                        <div className="mt-1 font-mono text-xs text-slate-300">
                          {activity.actions.map(normalizeActivityAction).join(', ')}
                        </div>
                      </div>
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
