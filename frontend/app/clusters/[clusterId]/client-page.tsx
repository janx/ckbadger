'use client';
import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueries, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CapacityStatisticsSection } from '@/components/ui/capacity-statistics-section';
import { ObjectActivityCard } from '@/components/object/object-activity-card';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { getCapacityRangeParams, CapacityRangeKey } from '@/lib/capacity-range';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { ClusterDescription, Tooltip } from '@/components/spore/cluster-description';
import { parseSporeClusterDescription, type DobInfo } from '@/lib/spore-cluster-description';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { formatNumber } from '@/lib/utils';
import { formatActivityTimestamp, formatStorageTier } from '@/lib/asset-utils';
type ListContentFilter = 'all' | 'image' | 'video' | 'audio' | 'text' | 'other';
type ListSort = 'createdDesc' | 'createdAsc' | 'sizeDesc' | 'sizeAsc';
type CollectionSectionTab = 'activities' | 'objects' | 'holders';
const LIST_FILTER_VALUES: ListContentFilter[] = ['all', 'image', 'video', 'audio', 'text', 'other'];
const LIST_SORT_VALUES: ListSort[] = ['createdDesc', 'createdAsc', 'sizeDesc', 'sizeAsc'];
function isListContentFilter(value: string | null): value is ListContentFilter {
  return !!value && LIST_FILTER_VALUES.includes(value as ListContentFilter);
}
function isListSort(value: string | null): value is ListSort {
  return !!value && LIST_SORT_VALUES.includes(value as ListSort);
}
function isCollectionSectionTab(value: string | null): value is CollectionSectionTab {
  return value === 'activities' || value === 'objects' || value === 'holders';
}
function safeString(value: unknown, fallback = ''): string {
  if (typeof value !== 'string') {
    return fallback;
  }
  return value;
}
function safeNumber(value: unknown, fallback = 0): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return fallback;
  }
  return value;
}
function getSortIndicator(direction: 'asc' | 'desc' | null): string {
  if (direction === 'asc') return '↑';
  if (direction === 'desc') return '↓';
  return '↕';
}
const STORAGE_TIER_DESCRIPTIONS: Record<string, string> = {
  fully_on_ckb:
    'All content is stored directly on the CKB blockchain (on-chain data or ckbfs://). Fully verifiable and permanent.',
  fully_on_btc:
    'Content is inscribed on Bitcoin via btcfs:// and bridged to CKB. Data permanence depends on Bitcoin.',
  fully_on_ckb_and_btc:
    'Content is stored across both CKB (on-chain data or ckbfs://) and Bitcoin (btcfs://). Fully verifiable and permanent.',
  decentralized_external:
    'Some content references external decentralized storage (e.g. IPFS, Arweave). Data persists as long as the external network hosts it.',
  centralized_dependent:
    'Some content depends on centralized servers (http/https). Data availability relies on the server operator.',
  unknown:
    'Storage profile could not be determined. The content storage method for objects in this cluster is unverified.',
};

function storageTierColor(tier: string): {
  text: string;
  textClass?: string;
  accent: string;
  bg: string;
  cardClass?: string;
} {
  if (tier === 'fully_on_ckb') {
    return {
      text: '',
      textClass: 'storage-text-ckb',
      accent: 'border-l-[#2edba3] shadow-[inset_1px_0_8px_-4px_#2edba3]',
      bg: '',
      cardClass: 'storage-card-ckb',
    };
  }
  if (tier === 'fully_on_ckb_and_btc') {
    return {
      text: '',
      textClass: 'storage-text-both',
      accent: 'border-l-[#8ca050] shadow-[inset_1px_0_6px_-3px_rgba(140,160,80,0.5)]',
      bg: '',
      cardClass: 'storage-card-both',
    };
  }
  if (tier === 'fully_on_btc') {
    return {
      text: '',
      textClass: 'storage-text-btc',
      accent: 'border-l-[#b8872a] shadow-[inset_1px_0_6px_-3px_rgba(184,135,42,0.5)]',
      bg: '',
      cardClass: 'storage-card-btc',
    };
  }
  if (tier === 'centralized_dependent') {
    return {
      text: 'text-negative',
      accent: 'border-l-negative shadow-[inset_1px_0_8px_-4px_theme(colors.negative)]',
      bg: 'bg-negative/5 border-negative/20',
    };
  }
  return {
    text: 'text-warning',
    accent: 'border-l-warning shadow-[inset_1px_0_8px_-4px_theme(colors.warning)]',
    bg: 'bg-warning/5 border-warning/20',
  };
}

function StorageTierTooltip({ tier }: { tier: string }) {
  const text = STORAGE_TIER_DESCRIPTIONS[tier] || STORAGE_TIER_DESCRIPTIONS.unknown;
  return <Tooltip text={text} />;
}

/** Simple JSON syntax highlighter — no dependencies. */
function JsonHighlight({ json }: { json: string }) {
  const parts: { text: string; cls: string }[] = [];
  const re =
    /("(?:[^"\\]|\\.)*")\s*:|("(?:[^"\\]|\\.)*")|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(\btrue\b|\bfalse\b|\bnull\b)|([{}[\],:])/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(json)) !== null) {
    if (m.index > last) {
      parts.push({ text: json.slice(last, m.index), cls: '' });
    }
    if (m[1] !== undefined) {
      parts.push({ text: m[1], cls: 'text-info' });
      parts.push({
        text: json.slice(m.index + m[1].length, m.index + m[0].length),
        cls: 'text-text-dim',
      });
    } else if (m[2] !== undefined) {
      parts.push({ text: m[2], cls: 'text-positive' });
    } else if (m[3] !== undefined) {
      parts.push({ text: m[3], cls: 'text-warning' });
    } else if (m[4] !== undefined) {
      parts.push({ text: m[4], cls: 'text-negative' });
    } else if (m[5] !== undefined) {
      parts.push({ text: m[5], cls: 'text-text-dim' });
    }
    last = m.index + m[0].length;
  }
  if (last < json.length) {
    parts.push({ text: json.slice(last), cls: '' });
  }

  return (
    <>
      {parts.map((p, i) => (
        <span key={i} className={p.cls}>
          {p.text}
        </span>
      ))}
    </>
  );
}

function describeTraitItem(item: import('@/lib/spore-cluster-description').DobPatternItem): string {
  const name = item.traitName || 'unnamed';
  const decode = item.patternType || 'unknown';

  if (decode === 'options' && item.optionsCount !== null) {
    return `"${name}" is decoded by selecting from ${item.optionsCount} possible values based on the spore's DNA bytes.`;
  }
  if (decode === 'range') {
    return `"${name}" is decoded as a numeric value mapped from DNA bytes to a continuous range.`;
  }
  if (decode === 'utf8') {
    return `"${name}" is decoded by reading DNA bytes as UTF-8 text.`;
  }
  if (decode === 'raw' || decode === 'rawnumber' || decode === 'rawstring') {
    return `"${name}" is decoded by passing DNA bytes directly as ${decode === 'rawnumber' ? 'a number' : decode === 'rawstring' ? 'a hex string' : 'raw data'}.`;
  }
  return `"${name}" — a trait decoded from the spore's on-chain DNA using the "${decode}" method.`;
}

export interface ClusterDetailPageProps {
  clusterId: string;
}
export default function ClusterDetailPage({ clusterId }: ClusterDetailPageProps) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const tabFromQuery = searchParams.get('tab');
  const [capacityRange, setCapacityRange] = useState<CapacityRangeKey>('all');
  const [activeCollectionTab, setActiveCollectionTab] = useState<CollectionSectionTab>(() =>
    isCollectionSectionTab(tabFromQuery) ? tabFromQuery : 'activities'
  );
  const [listContentFilter, setListContentFilter] = useState<ListContentFilter>(() => {
    const value = searchParams.get('content');
    return isListContentFilter(value) ? value : 'all';
  });
  const [listSort, setListSort] = useState<ListSort>(() => {
    const value = searchParams.get('sort');
    return isListSort(value) ? value : 'createdDesc';
  });
  const [listQuery, setListQuery] = useState(() => searchParams.get('q') ?? '');
  const capacityRangeParams = getCapacityRangeParams(capacityRange);
  const sporesPagination = useCursorPagination();
  const clusterHoldersPagination = useCursorPagination();
  const clusterActivitiesPagination = useCursorPagination();
  const { reset: resetClusterHoldersPagination } = clusterHoldersPagination;
  const { reset: resetClusterActivitiesPagination } = clusterActivitiesPagination;
  const {
    data: cluster,
    isLoading: clusterLoading,
    error: clusterError,
  } = useQuery({
    queryKey: ['cluster', clusterId],
    queryFn: () => api.getSporeCluster(clusterId),
  });
  const { data: sporesData, isLoading: sporesLoading } = useQuery({
    queryKey: ['cluster-spores', clusterId, sporesPagination.cursor],
    queryFn: () =>
      api.getSporesByCluster(clusterId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: sporesPagination.cursor,
      }),
    enabled: !!clusterId,
    placeholderData: keepPreviousData,
  });
  const {
    data: clusterHolders,
    isLoading: isClusterHoldersLoading,
    isError: isClusterHoldersError,
  } = useQuery({
    queryKey: ['cluster-holders', clusterId, clusterHoldersPagination.cursor],
    queryFn: () =>
      api.getSporeClusterHolders(clusterId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: clusterHoldersPagination.cursor,
      }),
    enabled: !!clusterId && activeCollectionTab === 'holders',
    placeholderData: keepPreviousData,
  });
  const {
    data: clusterActivities,
    isLoading: isClusterActivitiesLoading,
    isError: isClusterActivitiesError,
  } = useQuery({
    queryKey: ['cluster-activities', clusterId, clusterActivitiesPagination.cursor],
    queryFn: () =>
      api.getSporeClusterActivities(clusterId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: clusterActivitiesPagination.cursor,
      }),
    enabled: !!clusterId && activeCollectionTab === 'activities',
    placeholderData: keepPreviousData,
  });
  const { data: capacityChart, isLoading: isCapacityChartLoading } = useQuery({
    queryKey: ['cluster-capacity-chart', clusterId, capacityRange],
    queryFn: () =>
      capacityRangeParams
        ? api.getSporeClusterCapacityChart(clusterId, capacityRangeParams)
        : api.getSporeClusterCapacityChart(clusterId),
    enabled: !!clusterId,
  });
  const { data: creatorAddressRecord } = useQuery({
    queryKey: ['cluster-creator-address', cluster?.ownerLockHash],
    queryFn: () => api.getAddress(cluster!.ownerLockHash),
    enabled: !!cluster?.ownerLockHash && !cluster?.ownerAddress,
    retry: false,
  });
  const getContentTypeIcon = (contentType: string | null | undefined) => {
    const normalized = safeString(contentType);
    if (!normalized) return '📦';
    if (normalized.startsWith('image/') || normalized.startsWith('ipfs/image')) return '🖼️';
    if (normalized.startsWith('video/') || normalized.startsWith('ipfs/video')) return '🎬';
    if (normalized.startsWith('audio/') || normalized.startsWith('ipfs/audio')) return '🎵';
    if (normalized.startsWith('text/')) return '📄';
    return '📦';
  };
  const summarizeContentType = (contentType: string | null | undefined): string => {
    const normalized = safeString(contentType);
    if (!normalized) {
      return 'unknown';
    }
    const [primary] = normalized.toLowerCase().split('/');
    if (!primary) {
      return 'unknown';
    }
    return primary;
  };
  const isKnownPrimaryType = (primary: string): boolean => {
    return primary === 'image' || primary === 'video' || primary === 'audio' || primary === 'text';
  };
  const normalizedQuery = listQuery.trim().toLowerCase();
  const sizeSortDirection =
    listSort === 'sizeAsc' ? 'asc' : listSort === 'sizeDesc' ? 'desc' : null;
  const blockSortDirection =
    listSort === 'createdAsc' ? 'asc' : listSort === 'createdDesc' ? 'desc' : null;
  useEffect(() => {
    const currentQuery = searchParams.toString();
    const nextParams = new URLSearchParams(currentQuery);
    if (listContentFilter === 'all') {
      nextParams.delete('content');
    } else {
      nextParams.set('content', listContentFilter);
    }
    if (listSort === 'createdDesc') {
      nextParams.delete('sort');
    } else {
      nextParams.set('sort', listSort);
    }
    if (!normalizedQuery) {
      nextParams.delete('q');
    } else {
      nextParams.set('q', normalizedQuery);
    }
    if (activeCollectionTab === 'activities') {
      nextParams.delete('tab');
    } else {
      nextParams.set('tab', activeCollectionTab);
    }
    const nextQuery = nextParams.toString();
    if (nextQuery === currentQuery) {
      return;
    }
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  }, [
    activeCollectionTab,
    listContentFilter,
    listSort,
    normalizedQuery,
    pathname,
    router,
    searchParams,
  ]);
  useEffect(() => {
    resetClusterHoldersPagination();
    resetClusterActivitiesPagination();
  }, [clusterId, resetClusterActivitiesPagination, resetClusterHoldersPagination]);
  const filteredAndSortedSpores = useMemo(() => {
    if (!sporesData?.data?.length) {
      return [];
    }
    const filtered = sporesData.data.filter((spore) => {
      if (listContentFilter === 'all') {
        return true;
      }
      const primary = summarizeContentType(spore.contentType);
      if (listContentFilter === 'other') {
        return !isKnownPrimaryType(primary);
      }
      if (primary !== listContentFilter) {
        return false;
      }
      return true;
    });
    const queryFiltered = normalizedQuery
      ? filtered.filter((spore) => {
          const sporeId = safeString(spore.sporeId);
          const contentType = safeString(spore.contentType);
          const ownerLockHash = safeString(spore.ownerLockHash);
          const ownerAddress = safeString(spore.ownerAddress);
          return (
            sporeId.toLowerCase().includes(normalizedQuery) ||
            contentType.toLowerCase().includes(normalizedQuery) ||
            ownerLockHash.toLowerCase().includes(normalizedQuery) ||
            ownerAddress.toLowerCase().includes(normalizedQuery)
          );
        })
      : filtered;
    const sorted = [...queryFiltered];
    sorted.sort((a, b) => {
      const createdAtA = safeNumber(a.createdAtBlock);
      const createdAtB = safeNumber(b.createdAtBlock);
      const contentSizeA = safeNumber(a.contentSize);
      const contentSizeB = safeNumber(b.contentSize);
      if (listSort === 'createdDesc') {
        return createdAtB - createdAtA;
      }
      if (listSort === 'createdAsc') {
        return createdAtA - createdAtB;
      }
      if (listSort === 'sizeDesc') {
        return contentSizeB - contentSizeA;
      }
      if (listSort === 'sizeAsc') {
        return contentSizeA - contentSizeB;
      }
      return 0;
    });
    return sorted;
  }, [sporesData?.data, listContentFilter, listSort, normalizedQuery]);
  const missingSporeOwnerLockHashes = useMemo(() => {
    if (!filteredAndSortedSpores.length) {
      return [];
    }
    const unique = new Map<string, string>();
    for (const spore of filteredAndSortedSpores) {
      const ownerAddress = safeString(spore.ownerAddress);
      const ownerLockHash = safeString(spore.ownerLockHash);
      if (ownerAddress || !ownerLockHash) {
        continue;
      }
      const normalized = ownerLockHash.toLowerCase();
      if (!unique.has(normalized)) {
        unique.set(normalized, ownerLockHash);
      }
    }
    return Array.from(unique.values());
  }, [filteredAndSortedSpores]);
  const sporeOwnerAddressQueries = useQueries({
    queries: missingSporeOwnerLockHashes.map((ownerLockHash) => ({
      queryKey: ['spore-owner-address', ownerLockHash],
      queryFn: () => api.getAddress(ownerLockHash),
      retry: false,
    })),
  });
  const sporeOwnerAddressByLockHash = useMemo(() => {
    const map = new Map<string, string>();
    missingSporeOwnerLockHashes.forEach((ownerLockHash, index) => {
      const address = sporeOwnerAddressQueries[index]?.data?.address;
      if (address) {
        map.set(ownerLockHash.toLowerCase(), address);
      }
    });
    return map;
  }, [missingSporeOwnerLockHashes, sporeOwnerAddressQueries]);
  const resolveSporeOwnerAddress = (
    ownerLockHash: string | null | undefined,
    ownerAddress?: string | null
  ) => {
    const normalizedOwnerAddress = safeString(ownerAddress);
    if (normalizedOwnerAddress) {
      return normalizedOwnerAddress;
    }
    const normalizedOwnerLockHash = safeString(ownerLockHash);
    if (!normalizedOwnerLockHash) {
      return null;
    }
    return sporeOwnerAddressByLockHash.get(normalizedOwnerLockHash.toLowerCase()) ?? null;
  };
  const creatorAddress = cluster?.ownerAddress || creatorAddressRecord?.address || null;
  const parsedDescription = useMemo(
    () => parseSporeClusterDescription(cluster?.description),
    [cluster?.description]
  );
  const dobInfo: DobInfo | null = parsedDescription?.dob ?? null;
  if (clusterLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="bg-base-elevated mb-6 h-10 w-64 animate-pulse rounded" />
          <div className="grid gap-6 lg:grid-cols-3">
            <div className="border-base-border bg-base-surface/50 h-64 animate-pulse rounded border" />
            <div className="border-base-border bg-base-surface/50 h-96 animate-pulse rounded border lg:col-span-2" />
          </div>
        </main>
      </div>
    );
  }
  if (clusterError || !cluster) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">Spore Cluster not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/assets?type=object"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            ← Back to Objects
          </Link>
        </div>
        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Collection Overview</TerminalPanelHeader>
          <TerminalPanelContent>
            {/* Name + badge */}
            <div className="flex flex-wrap items-center gap-3">
              <h1 className="text-text-bright font-mono text-2xl font-bold">
                {cluster.name || 'Unnamed Collection'}
              </h1>
              <Badge variant="neutral">Spore Cluster</Badge>
            </div>

            {/* Cluster ID */}
            <div className="mt-3 flex flex-wrap items-baseline gap-2 font-mono text-sm">
              <span className="text-text-dim text-xs uppercase tracking-wider">cluster id</span>
              <HexDisplay value={cluster.clusterId} truncate={false} size="sm" />
            </div>

            {/* Creator */}
            <div className="mt-1.5 flex flex-wrap items-baseline gap-2 font-mono text-sm">
              <span className="text-text-dim text-xs uppercase tracking-wider">creator</span>
              {creatorAddress ? (
                <Address address={creatorAddress} truncate={false} />
              ) : (
                <span className="text-text-dim">unavailable</span>
              )}
            </div>

            {/* Stat cards row */}
            <div className="border-base-border mt-4 grid grid-cols-2 gap-3 border-t pt-4 sm:grid-cols-4">
              {/* Storage profile card */}
              {cluster.storageProfile?.tier &&
                (() => {
                  const colors = storageTierColor(cluster.storageProfile.tier);
                  return (
                    <div
                      className={`rounded border border-l-2 p-3 ${colors.bg} ${colors.accent} ${colors.cardClass || ''}`}
                    >
                      <div className="text-text-dim relative z-10 mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                        Storage Profile
                      </div>
                      <div className="relative z-10 flex items-center gap-1">
                        <span
                          className={`font-mono text-sm font-semibold leading-tight ${colors.textClass || colors.text}`}
                        >
                          {formatStorageTier(cluster.storageProfile.tier)}
                        </span>
                        <StorageTierTooltip tier={cluster.storageProfile.tier} />
                      </div>
                    </div>
                  );
                })()}

              {/* Supply card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Supply
                </div>
                <div className="text-warning font-mono text-sm font-semibold tabular-nums">
                  {formatNumber(cluster.sporesCount)}
                </div>
              </div>

              {/* Holders card */}
              {cluster.holdersCount !== undefined && (
                <div className="border-base-border rounded border p-3">
                  <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                    Holders
                  </div>
                  <div className="text-text-bright font-mono text-sm font-semibold tabular-nums">
                    {formatNumber(cluster.holdersCount)}
                  </div>
                </div>
              )}

              {/* Created card */}
              {cluster.createdAtBlock !== undefined && (
                <div className="border-base-border rounded border p-3">
                  <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                    Created
                  </div>
                  <Link
                    href={`/blocks/${cluster.createdAtBlock}`}
                    className="text-text-bright font-mono text-sm font-semibold tabular-nums hover:underline"
                  >
                    #{formatNumber(cluster.createdAtBlock)}
                  </Link>
                </div>
              )}
            </div>

            {/* Description */}
            {cluster.description && (
              <div className="border-base-border mt-3 border-t pt-3">
                <ClusterDescription
                  description={cluster.description}
                  clusterName={cluster.name}
                  hideDob
                />
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
        <CapacityStatisticsSection
          className="mb-6"
          capacityRange={capacityRange}
          onCapacityRangeChange={setCapacityRange}
          capacityChart={capacityChart}
          isCapacityChartLoading={isCapacityChartLoading}
          totalCapacity={cluster.ownedCapacity}
          commonKnowledgeSize={cluster.ownedKnowledge}
        />
        {dobInfo && (
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">DOB Blueprint</TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="flex flex-wrap gap-x-6 gap-y-2 font-mono text-sm">
                <div className="flex items-center gap-2">
                  <span className="text-text-dim text-xs uppercase tracking-wider">version</span>
                  <a
                    href={`https://github.com/sporeprotocol/spore-dob-${dobInfo.version ?? 0}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-text-bright tabular-nums hover:underline"
                    title="View DOB specification"
                  >
                    DOB/{dobInfo.version ?? '?'} ↗
                  </a>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-text-dim text-xs uppercase tracking-wider">traits</span>
                  <span className="text-text-bright tabular-nums">
                    {dobInfo.patternItems.length}
                  </span>
                  <Tooltip text="Number of trait definitions (pattern items) that define the visual attributes each spore in this cluster can have. Each trait is decoded from the spore's on-chain DNA." />
                </div>
                {dobInfo.decodersCount > 0 && (
                  <div className="flex items-center gap-2">
                    <span className="text-text-dim text-xs uppercase tracking-wider">decoders</span>
                    <span className="text-text-bright tabular-nums">{dobInfo.decodersCount}</span>
                    <Tooltip text="Number of decoders used to interpret on-chain DNA data and render it into visual elements (e.g. SVG images)." />
                  </div>
                )}
              </div>
              {dobInfo.patternItems.length > 0 && (
                <div className="mt-4">
                  <div className="text-text-dim mb-2 font-mono text-xs uppercase tracking-wider">
                    Trait Definitions
                  </div>
                  <div className="border-base-border overflow-hidden rounded border">
                    <div className="border-base-border bg-base-surface/50 text-text-dim hidden border-b px-3 py-1.5 font-mono text-[10px] uppercase tracking-wider sm:grid sm:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)_minmax(0,0.8fr)_minmax(0,0.8fr)]">
                      <div>Trait</div>
                      <div>Type</div>
                      <div>Decode</div>
                      <div className="text-right">Variants</div>
                    </div>
                    {dobInfo.patternItems.map((item, i) => (
                      <div
                        key={`${item.traitName}-${i}`}
                        className="border-base-border hover:bg-base-elevated/40 border-b px-3 py-2 font-mono text-xs transition-colors last:border-b-0 sm:grid sm:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)_minmax(0,0.8fr)_minmax(0,0.8fr)] sm:items-center"
                      >
                        <div className="text-text-bright flex items-center font-semibold">
                          {item.traitName || '—'}
                          <Tooltip text={describeTraitItem(item)} />
                        </div>
                        <div className="text-text-dim mt-0.5 sm:mt-0">{item.dobType || '—'}</div>
                        <div className="text-text-dim mt-0.5 sm:mt-0">{item.patternType}</div>
                        <div className="text-text mt-0.5 text-right tabular-nums sm:mt-0">
                          {item.optionsCount !== null ? item.optionsCount : '—'}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
              {parsedDescription?.rawJson && (
                <details className="border-base-border bg-base-bg/40 mt-4 w-full overflow-hidden rounded border">
                  <summary className="text-text-dim cursor-pointer px-3 py-2 text-left font-mono text-xs uppercase tracking-wider">
                    View Raw Cluster Metadata JSON
                  </summary>
                  <pre className="border-base-border bg-base-bg/90 max-h-80 overflow-auto border-t px-3 py-2 text-left font-mono text-xs leading-relaxed">
                    <JsonHighlight json={parsedDescription.rawJson} />
                  </pre>
                </details>
              )}
            </TerminalPanelContent>
          </TerminalPanel>
        )}
        <TerminalPanel>
          <Tabs
            value={activeCollectionTab}
            onValueChange={(nextValue) => {
              if (isCollectionSectionTab(nextValue)) {
                setActiveCollectionTab(nextValue);
              }
            }}
          >
            <TerminalPanelHeader
              indicator="active"
              actions={
                <div className="flex w-full flex-wrap items-center justify-between gap-3">
                  {activeCollectionTab === 'objects' && (
                    <div
                      data-testid="spore-list-controls"
                      className="flex flex-1 flex-wrap items-center gap-2"
                    >
                      <label className="sr-only" htmlFor="spore-list-query">
                        Search spores
                      </label>
                      <input
                        id="spore-list-query"
                        aria-label="Search spores"
                        type="text"
                        value={listQuery}
                        onChange={(event) => setListQuery(event.target.value)}
                        placeholder="Search hash / owner / type"
                        className="border-base-border bg-base-surface text-text-bright placeholder:text-text-dim w-full rounded border px-2 py-1 font-mono text-xs sm:w-48"
                      />
                      <label className="sr-only" htmlFor="spore-content-filter">
                        Filter spores by content type
                      </label>
                      <select
                        id="spore-content-filter"
                        aria-label="Filter spores by content type"
                        value={listContentFilter}
                        onChange={(event) =>
                          setListContentFilter(
                            event.target.value as
                              | 'all'
                              | 'image'
                              | 'video'
                              | 'audio'
                              | 'text'
                              | 'other'
                          )
                        }
                        className="border-base-border bg-base-surface text-text-bright rounded border px-2 py-1 font-mono text-xs"
                      >
                        <option value="all">All Types</option>
                        <option value="image">Image</option>
                        <option value="video">Video</option>
                        <option value="audio">Audio</option>
                        <option value="text">Text</option>
                        <option value="other">Other</option>
                      </select>
                      <div className="text-text-dim font-mono text-xs">
                        {filteredAndSortedSpores.length} shown / {formatNumber(cluster.sporesCount)}{' '}
                        total
                      </div>
                    </div>
                  )}
                  <TabsList className="border-b-0">
                    <TabsTrigger value="activities">
                      Activities ({formatNumber(cluster.activitiesCount)})
                    </TabsTrigger>
                    <TabsTrigger value="objects">
                      Objects ({formatNumber(cluster.sporesCount)})
                    </TabsTrigger>
                    <TabsTrigger value="holders">
                      Holders ({formatNumber(cluster.holdersCount)})
                    </TabsTrigger>
                  </TabsList>
                </div>
              }
            >
              {activeCollectionTab === 'activities'
                ? 'Activities'
                : activeCollectionTab === 'holders'
                  ? 'Holders'
                  : 'Objects'}
            </TerminalPanelHeader>
            <TabsContent value="activities" className="py-0">
              <TerminalPanelContent>
                {isClusterActivitiesLoading && !clusterActivities ? (
                  <div className="text-text-dim py-8 text-center">Loading activities...</div>
                ) : isClusterActivitiesError ? (
                  <div className="text-rouge py-8 text-center">
                    Failed to load activities. Please refresh and try again.
                  </div>
                ) : !clusterActivities?.data?.length ? (
                  <div className="text-text-dim py-8 text-center">
                    No activities in this collection
                  </div>
                ) : (
                  <div className="space-y-2">
                    {clusterActivities.data.map((activity) => (
                      <ObjectActivityCard
                        key={`${activity.txHash}-${activity.txIndex}`}
                        txHash={activity.txHash}
                        blockNumber={activity.blockNumber}
                        txIndex={activity.txIndex}
                        timestamp={formatActivityTimestamp(activity.timestamp)}
                        actions={activity.actions}
                        badgeActions
                      />
                    ))}
                  </div>
                )}
              </TerminalPanelContent>
              <TerminalPanelFooter>
                <CursorPagination
                  totalLabel="Activities"
                  pageSize={DEFAULT_PAGE_SIZE}
                  page={clusterActivitiesPagination.page}
                  currentCount={clusterActivities?.data?.length ?? 0}
                  hasMore={clusterActivities?.hasMore ?? false}
                  hasPrevious={clusterActivitiesPagination.hasPrevious}
                  onNext={() => clusterActivitiesPagination.goToNext(clusterActivities?.nextCursor)}
                  onPrevious={clusterActivitiesPagination.goToPrevious}
                />
              </TerminalPanelFooter>
            </TabsContent>
            <TabsContent value="objects" className="py-0">
              <TerminalPanelContent padding="none">
                {sporesLoading ? (
                  <div className="text-text-dim py-8 text-center">Loading spores...</div>
                ) : filteredAndSortedSpores.length ? (
                  <>
                    <div className="border-base-border bg-base-surface/50 text-text-dim hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider md:block">
                      <div className="grid grid-cols-[minmax(0,1.8fr)_minmax(0,1fr)_90px_80px_minmax(0,1.2fr)_110px] items-center gap-3">
                        <div>Spore ID</div>
                        <div>Content</div>
                        <div className="text-right">
                          <button
                            type="button"
                            onClick={() =>
                              setListSort((current) =>
                                current === 'sizeDesc' ? 'sizeAsc' : 'sizeDesc'
                              )
                            }
                            aria-label="Sort spores by size"
                            className="text-text-dim hover:text-text ml-auto inline-flex items-center gap-1 text-right font-mono text-xs uppercase tracking-wider transition"
                          >
                            <span>Size</span>
                            <span aria-hidden>{getSortIndicator(sizeSortDirection)}</span>
                          </button>
                        </div>
                        <div className="text-center">Status</div>
                        <div className="text-right">Owner</div>
                        <div className="text-right">
                          <button
                            type="button"
                            onClick={() =>
                              setListSort((current) =>
                                current === 'createdDesc' ? 'createdAsc' : 'createdDesc'
                              )
                            }
                            aria-label="Sort spores by block"
                            className="text-text-dim hover:text-text ml-auto inline-flex items-center gap-1 text-right font-mono text-xs uppercase tracking-wider transition"
                          >
                            <span>Block</span>
                            <span aria-hidden>{getSortIndicator(blockSortDirection)}</span>
                          </button>
                        </div>
                      </div>
                    </div>
                    {filteredAndSortedSpores.map((spore) => {
                      const sporeId = safeString(spore.sporeId);
                      const contentType = safeString(spore.contentType, 'unknown');
                      const contentSize = safeNumber(spore.contentSize);
                      const createdAtBlock = safeNumber(spore.createdAtBlock);
                      const ownerLockHash = safeString(spore.ownerLockHash);
                      const ownerAddress = safeString(spore.ownerAddress);
                      const resolvedOwnerAddress = resolveSporeOwnerAddress(
                        ownerLockHash,
                        ownerAddress
                      );
                      const rowKey =
                        sporeId ||
                        `${safeString(spore.txHash, 'unknown-tx')}:${safeNumber(spore.outputIndex)}`;
                      const isLive = spore.isLive !== false;
                      return (
                        <TerminalRow key={rowKey}>
                          <div className="hidden md:grid md:grid-cols-[minmax(0,1.8fr)_minmax(0,1fr)_90px_80px_minmax(0,1.2fr)_110px] md:items-center md:gap-3">
                            <div className="flex items-center gap-2">
                              <span className="text-base">{getContentTypeIcon(contentType)}</span>
                              {sporeId ? (
                                <Link href={`/objects/${sporeId}`} className="hover:underline">
                                  <HexDisplay value={sporeId} size="sm" />
                                </Link>
                              ) : (
                                <span className="text-text-dim font-mono text-xs">
                                  Unknown spore ID
                                </span>
                              )}
                            </div>
                            <div
                              className="text-text truncate font-mono text-xs"
                              title={contentType}
                            >
                              {contentType}
                            </div>
                            <div className="text-text text-right font-mono text-xs">
                              {formatNumber(contentSize)} B
                            </div>
                            <div className="text-center">
                              {isLive ? (
                                <Badge variant="green">Live</Badge>
                              ) : (
                                <Badge variant="red">Burned</Badge>
                              )}
                            </div>
                            <div className="text-right">
                              {resolvedOwnerAddress ? (
                                <Address address={resolvedOwnerAddress} truncate />
                              ) : (
                                <span className="text-text-dim font-mono text-xs">
                                  Address unavailable
                                </span>
                              )}
                            </div>
                            <div className="text-right">
                              <Link
                                href={`/blocks/${createdAtBlock}`}
                                className="text-emphasis font-mono text-xs hover:underline"
                              >
                                #{formatNumber(createdAtBlock)}
                              </Link>
                            </div>
                          </div>
                          <div className="space-y-2 md:hidden">
                            <div className="flex items-start justify-between gap-3">
                              <div className="flex items-center gap-2">
                                <span className="text-base">{getContentTypeIcon(contentType)}</span>
                                {sporeId ? (
                                  <Link href={`/objects/${sporeId}`} className="hover:underline">
                                    <HexDisplay value={sporeId} size="sm" />
                                  </Link>
                                ) : (
                                  <span className="text-text-dim font-mono text-xs">
                                    Unknown spore ID
                                  </span>
                                )}
                              </div>
                              {isLive ? (
                                <Badge variant="green">Live</Badge>
                              ) : (
                                <Badge variant="red">Burned</Badge>
                              )}
                            </div>
                            <div className="flex items-center justify-between gap-3 text-xs">
                              <span
                                className="text-text-dim truncate font-mono"
                                title={contentType}
                              >
                                {contentType}
                              </span>
                              <span className="text-text font-mono">
                                {formatNumber(contentSize)} B
                              </span>
                            </div>
                            <div className="flex items-center justify-between gap-3 text-xs">
                              <span className="text-text-dim font-mono">
                                Block #{formatNumber(createdAtBlock)}
                              </span>
                              {resolvedOwnerAddress ? (
                                <Address address={resolvedOwnerAddress} truncate />
                              ) : (
                                <span className="text-text-dim font-mono text-xs">
                                  Address unavailable
                                </span>
                              )}
                            </div>
                          </div>
                        </TerminalRow>
                      );
                    })}
                  </>
                ) : (sporesData?.data?.length ?? 0) > 0 ? (
                  <div className="text-text-dim py-8 text-center">
                    No spores match current filters
                  </div>
                ) : (
                  <div className="text-text-dim py-8 text-center">No spores in this collection</div>
                )}
              </TerminalPanelContent>
              {sporesData && filteredAndSortedSpores.length > 0 && (
                <TerminalPanelFooter>
                  <CursorPagination
                    total={cluster.sporesCount}
                    totalLabel="Spores"
                    pageSize={DEFAULT_PAGE_SIZE}
                    page={sporesPagination.page}
                    currentCount={filteredAndSortedSpores.length}
                    hasMore={sporesData.hasMore}
                    hasPrevious={sporesPagination.hasPrevious}
                    onNext={() => sporesPagination.goToNext(sporesData.nextCursor)}
                    onPrevious={sporesPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              )}
            </TabsContent>
            <TabsContent value="holders" className="py-0">
              <TerminalPanelContent>
                {isClusterHoldersLoading && !clusterHolders ? (
                  <div className="text-text-dim py-8 text-center">Loading holders...</div>
                ) : isClusterHoldersError ? (
                  <div className="text-rouge py-8 text-center">
                    Failed to load holders. Please refresh and try again.
                  </div>
                ) : !clusterHolders?.data?.length ? (
                  <div className="text-text-dim py-8 text-center">
                    No holders in this collection
                  </div>
                ) : (
                  <div className="border-base-border bg-base-surface/30 overflow-hidden rounded border">
                    {clusterHolders.data.map((holder) => (
                      <div
                        key={holder.lockScriptHash}
                        className="row-scan hover:bg-base-elevated/40 border-base-border flex items-center justify-between gap-3 border-b px-3 py-2.5 transition-colors last:border-b-0"
                      >
                        <div className="min-w-0">
                          <Link
                            href={`/address/${holder.address ?? holder.lockScriptHash}`}
                            className="text-text font-mono text-xs hover:underline"
                          >
                            {holder.address ? (
                              holder.address
                            ) : (
                              <HexDisplay
                                value={holder.lockScriptHash}
                                size="sm"
                                startChars={12}
                                endChars={10}
                              />
                            )}
                          </Link>
                        </div>
                        <div className="text-text-bright shrink-0 font-mono text-sm">
                          {formatNumber(holder.itemCount)}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </TerminalPanelContent>
              <TerminalPanelFooter>
                <CursorPagination
                  total={clusterHolders?.total}
                  totalLabel="Holders"
                  pageSize={DEFAULT_PAGE_SIZE}
                  page={clusterHoldersPagination.page}
                  currentCount={clusterHolders?.data?.length ?? 0}
                  hasMore={clusterHolders?.hasMore ?? false}
                  hasPrevious={clusterHoldersPagination.hasPrevious}
                  onNext={() => clusterHoldersPagination.goToNext(clusterHolders?.nextCursor)}
                  onPrevious={clusterHoldersPagination.goToPrevious}
                />
              </TerminalPanelFooter>
            </TabsContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}
