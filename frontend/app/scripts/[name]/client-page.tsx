'use client';
import React, { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueries, keepPreviousData } from '@tanstack/react-query';
import { useSearchParams } from '@/src/navigation';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { Capacity } from '@/components/ui/capacity';
import { HMultiplier } from '@/components/ui/h-multiplier';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { CapacityRangeSelector } from '@/components/ui/capacity-range-selector';
import { HelpPopover } from '@/components/ui/help-popover';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api } from '@/lib/api';
import { getCapacityRangeParams, CapacityRangeKey } from '@/lib/capacity-range';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import {
  getScriptRefQueryHashType,
  normalizeScriptRefHashType,
  type ScriptRefHashType,
} from '@/lib/script-ref';
import { formatCkbCompact } from '@/lib/utils';
import type { KnownScript, ScriptLookupInfo } from '@/lib/api';

interface SelectedVersion {
  codeHash: string;
  scriptKind?: 'lock' | 'type';
}

interface ScriptVersionGroup {
  codeHash: string;
  deployments: KnownScript[];
  primaryDeployment: KnownScript;
}

interface VersionUsageReference {
  key: string;
  referenceHash: string;
  hashType: ScriptRefHashType;
}

interface VersionUsageStats {
  codeHash: string;
  scriptKind: string | null;
  liveCellsCount: number;
  liveCapacitySum: string;
  liveUsedCapacitySum: string;
}

const UNKNOWN_SCRIPT_NAME = 'unknown';
const ALL_ZERO_HASH = `0x${'0'.repeat(64)}`;
const MOBILE_BREAKPOINT = 768;
const COMPACT_SCRIPT_VERSIONS_WIDTH = 1080;
const COMPACT_VERSION_DEPLOYMENTS_WIDTH = 1240;

function compareDeploymentsByDeployedAt(a: KnownScript, b: KnownScript): number {
  const aTs = a.deployedAt ?? null;
  const bTs = b.deployedAt ?? null;
  if (aTs != null && bTs != null) {
    return bTs - aTs;
  }
  if (aTs != null) return -1;
  if (bTs != null) return 1;
  return a.codeHash.localeCompare(b.codeHash);
}
function normalizeHash(value: string | null | undefined): string | null {
  if (!value) return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}
function isHexScriptHash(value: string): boolean {
  return /^0x[0-9a-fA-F]{64}$/.test(value);
}

function deploymentOutpointKey(
  txHash: string | null | undefined,
  outputIndex: number | null | undefined
): string | null {
  if (!txHash || outputIndex == null) {
    return null;
  }

  return `${txHash}:${outputIndex}`;
}

function compareCellsByCreatedAt(
  a: { cell: { createdAtBlock: number; txHash: string; outputIndex: number } },
  b: { cell: { createdAtBlock: number; txHash: string; outputIndex: number } }
): number {
  if (a.cell.createdAtBlock !== b.cell.createdAtBlock) {
    return a.cell.createdAtBlock - b.cell.createdAtBlock;
  }
  if (a.cell.txHash !== b.cell.txHash) {
    return a.cell.txHash.localeCompare(b.cell.txHash);
  }
  return a.cell.outputIndex - b.cell.outputIndex;
}

function hexToBytes(hex: string): number[] {
  const normalized = hex.startsWith('0x') ? hex.slice(2) : hex;
  if (normalized.length % 2 !== 0) {
    throw new Error(`Invalid hex length for cursor encoding: ${hex}`);
  }

  const bytes: number[] = [];
  for (let index = 0; index < normalized.length; index += 2) {
    const pair = normalized.slice(index, index + 2);
    const value = Number.parseInt(pair, 16);
    if (Number.isNaN(value)) {
      throw new Error(`Invalid hex value for cursor encoding: ${hex}`);
    }
    bytes.push(value);
  }
  return bytes;
}

function encodeVersionCellCursor(
  referenceHash: string,
  createdAtBlock: number,
  txHash: string,
  outputIndex: number
): string {
  const bytes = [
    ...hexToBytes(referenceHash),
    ...Array.from(new Uint8Array(new BigInt64Array([BigInt(createdAtBlock)]).buffer).reverse()),
    ...hexToBytes(txHash),
    ...Array.from(new Uint8Array(new Int16Array([outputIndex]).buffer).reverse()),
  ];
  return bytes.map((value) => value.toString(16).padStart(2, '0')).join('');
}

function decodeVersionCellsCursorState(
  cursor: string | undefined
): Record<string, string | undefined> {
  if (!cursor) {
    return {};
  }

  const parsed = JSON.parse(cursor);
  if (parsed == null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return {};
  }

  return Object.fromEntries(
    Object.entries(parsed).filter(
      (entry): entry is [string, string] =>
        typeof entry[0] === 'string' && typeof entry[1] === 'string'
    )
  );
}

function encodeVersionCellsCursorState(state: Record<string, string | undefined>): string {
  return JSON.stringify(state);
}

function formatDeploymentTimestamp(timestamp: number | null | undefined): string {
  if (timestamp == null) {
    return '-';
  }

  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return '-';
  }

  return date.toLocaleString();
}

function compareDeploymentCreatedAt(
  left: { deployedAt?: number | null; createdAtBlock?: number | null },
  right: { deployedAt?: number | null; createdAtBlock?: number | null }
): number {
  const leftTimestamp = left.deployedAt ?? null;
  const rightTimestamp = right.deployedAt ?? null;
  if (leftTimestamp != null && rightTimestamp != null && leftTimestamp !== rightTimestamp) {
    return leftTimestamp - rightTimestamp;
  }
  if (leftTimestamp != null && rightTimestamp == null) {
    return -1;
  }
  if (leftTimestamp == null && rightTimestamp != null) {
    return 1;
  }

  const leftBlock = left.createdAtBlock ?? null;
  const rightBlock = right.createdAtBlock ?? null;
  if (leftBlock != null && rightBlock != null && leftBlock !== rightBlock) {
    return leftBlock - rightBlock;
  }
  if (leftBlock != null && rightBlock == null) {
    return -1;
  }
  if (leftBlock == null && rightBlock != null) {
    return 1;
  }

  return 0;
}

function hasKnownScriptName(name: string | null | undefined): boolean {
  if (!name) {
    return false;
  }

  const normalized = name.trim();
  return normalized.length > 0 && normalized.toLowerCase() !== UNKNOWN_SCRIPT_NAME;
}

function isAllZeroHash(hash: string | null | undefined): boolean {
  return hash != null && hash.toLowerCase() === ALL_ZERO_HASH;
}

function useViewportWidth(defaultWidth = 1280): number {
  const [width, setWidth] = useState(() =>
    typeof window === 'undefined' ? defaultWidth : window.innerWidth
  );

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    const handleResize = () => setWidth(window.innerWidth);
    handleResize();
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  return width;
}

function deploymentReferenceHashes(
  deployment: KnownScript,
  lookupInfo?: ScriptLookupInfo
): {
  typeRef: string | null;
  dataRef: string | null;
  dataRefType: ScriptRefHashType;
} {
  const normalizedHashType = normalizeScriptRefHashType(deployment.hashType);
  const lookupTypeRef = normalizeHash(lookupInfo?.deploymentTypeHash);
  const lookupDataRef = normalizeHash(lookupInfo?.deploymentDataHash);
  const typeRef =
    deployment.typeHash ??
    lookupTypeRef ??
    (normalizedHashType === 'type' ? deployment.codeHash : null);
  const dataRef =
    deployment.dataHash ??
    lookupDataRef ??
    (normalizedHashType !== 'type' ? deployment.codeHash : null);
  const dataRefType =
    normalizedHashType !== 'type' ? getScriptRefQueryHashType(deployment.hashType, 'data') : 'data';
  return { typeRef, dataRef, dataRefType };
}
export interface ScriptDetailPageProps {
  name?: string;
  codeHash?: string;
}
export default function ScriptDetailPage({
  name: routeName,
  codeHash: routeCodeHash,
}: ScriptDetailPageProps) {
  const searchParams = useSearchParams();
  const name = routeName ? decodeURIComponent(routeName) : null;
  const codeHashParam = routeCodeHash ? decodeURIComponent(routeCodeHash) : null;
  const normalizedCodeHash =
    codeHashParam && isHexScriptHash(codeHashParam.trim())
      ? `0x${codeHashParam.trim().slice(2).toLowerCase()}`
      : null;
  const isCodeHashMode = normalizedCodeHash != null;
  const viewportWidth = useViewportWidth();
  const selectedRefParam = searchParams.get('ref') ?? (isCodeHashMode ? normalizedCodeHash : null);
  const selectedRef =
    selectedRefParam && isHexScriptHash(selectedRefParam.trim())
      ? `0x${selectedRefParam.trim().slice(2).toLowerCase()}`
      : null;
  const selectedRefHashType = normalizeScriptRefHashType(searchParams.get('hashType'));
  const [capacityRange, setCapacityRange] = useState<CapacityRangeKey>('all');
  const [selectedVersion, setSelectedVersion] = useState<SelectedVersion | null>(null);
  const cellsPagination = useCursorPagination();
  const capacityRangeParams = getCapacityRangeParams(capacityRange);
  const {
    data: namedDeployments,
    isLoading: isNamedDeploymentsLoading,
    error: namedDeploymentsError,
  } = useQuery({
    queryKey: ['script', name],
    queryFn: () => {
      if (!name) throw new Error('script name is required');
      return api.getScript(name);
    },
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const { data: usage, isLoading: isUsageLoading } = useQuery({
    queryKey: ['script-usage', name],
    queryFn: () => {
      if (!name) throw new Error('script name is required');
      return api.getScriptUsage(name);
    },
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const { data: codeHashLookup, isLoading: isCodeHashLookupLoading } = useQuery({
    queryKey: ['script-lookup-detail', normalizedCodeHash],
    queryFn: async () => {
      const result = await api.lookupScripts([normalizedCodeHash!]);
      return result[normalizedCodeHash!] ?? null;
    },
    enabled: isCodeHashMode,
    staleTime: Infinity,
  });
  const codeHashDeploymentQueryHashType = useMemo<ScriptRefHashType | null>(() => {
    if (!isCodeHashMode) {
      return null;
    }

    if (codeHashLookup?.resolutionState === 'ambiguous') {
      return null;
    }

    if (selectedRefHashType) {
      return selectedRefHashType;
    }

    if (normalizeHash(codeHashLookup?.deploymentTypeHash)) {
      return 'type';
    }

    return normalizeScriptRefHashType(codeHashLookup?.hashType) ?? 'type';
  }, [
    codeHashLookup?.deploymentTypeHash,
    codeHashLookup?.hashType,
    codeHashLookup?.resolutionState,
    isCodeHashMode,
    selectedRefHashType,
  ]);
  const { data: codeHashCodeCells, isLoading: isCodeHashCodeCellsLoading } = useQuery({
    queryKey: ['script-code-cells-unified', normalizedCodeHash, codeHashDeploymentQueryHashType],
    queryFn: () => api.getCodeCells(normalizedCodeHash!, codeHashDeploymentQueryHashType!),
    enabled: Boolean(isCodeHashMode && normalizedCodeHash && codeHashDeploymentQueryHashType),
    staleTime: Infinity,
  });
  const deployments = useMemo<KnownScript[] | null>(() => {
    if (!isCodeHashMode) {
      return namedDeployments ?? null;
    }

    if (!normalizedCodeHash || !codeHashLookup) {
      return null;
    }

    const baseDeployment: KnownScript = {
      codeHash: codeHashLookup.codeHash,
      name: codeHashLookup.name,
      description: null,
      scriptKind: codeHashLookup.scriptKind,
      rfc: null,
      website: null,
      sourceUrl: null,
      decoderType: codeHashLookup.decoderType,
      network: '',
      hashType: codeHashLookup.hashType,
      dataHash: codeHashLookup.deploymentDataHash ?? null,
      typeHash: codeHashLookup.deploymentTypeHash ?? null,
      tag: null,
      deprecated: false,
      isSystem: false,
      codeCellTxHash: codeHashLookup.codeCellTxHash,
      codeCellOutputIndex: codeHashLookup.codeCellOutputIndex,
      deployedAt: null,
      liveCapacitySum: codeHashLookup.liveCapacitySum,
      liveUsedCapacitySum: codeHashLookup.liveUsedCapacitySum,
      liveCellsCount: codeHashLookup.liveCellsCount,
      codeCellsLiveCount: codeHashLookup.codeCellsLiveCount,
      codeCellsTotal: codeHashLookup.codeCellsTotal,
    };

    const deploymentEntries = codeHashCodeCells?.codeCells.length
      ? codeHashCodeCells.codeCells
      : [baseDeployment];

    return deploymentEntries.map((deploymentEntry) => {
      if ('codeHash' in deploymentEntry) {
        return deploymentEntry;
      }

      return {
        ...baseDeployment,
        codeCellTxHash: deploymentEntry.txHash,
        codeCellOutputIndex: deploymentEntry.outputIndex,
      };
    });
  }, [
    codeHashCodeCells?.codeCells,
    codeHashLookup,
    isCodeHashMode,
    namedDeployments,
    normalizedCodeHash,
  ]);
  const versionUsageEntries = useMemo<VersionUsageStats[]>(() => {
    if (isCodeHashMode) {
      if (!codeHashLookup) {
        return [];
      }

      return [
        {
          codeHash: codeHashLookup.codeHash,
          scriptKind: codeHashLookup.scriptKind,
          liveCellsCount: codeHashLookup.liveCellsCount,
          liveCapacitySum: codeHashLookup.liveCapacitySum,
          liveUsedCapacitySum: codeHashLookup.liveUsedCapacitySum,
        },
      ];
    }

    return (
      usage?.byDeployment.map((deployment) => ({
        codeHash: deployment.codeHash,
        scriptKind: deployment.scriptKind,
        liveCellsCount: deployment.liveCellsCount,
        liveCapacitySum: deployment.liveCapacitySum,
        liveUsedCapacitySum: deployment.liveUsedCapacitySum,
      })) ?? []
    );
  }, [codeHashLookup, isCodeHashMode, usage?.byDeployment]);
  const sortedDeployments = useMemo(
    () => (deployments ? [...deployments].sort(compareDeploymentsByDeployedAt) : []),
    [deployments]
  );
  const versionGroups = useMemo<ScriptVersionGroup[]>(() => {
    const groups = new Map<string, KnownScript[]>();
    for (const deployment of sortedDeployments) {
      const existing = groups.get(deployment.codeHash);
      if (existing) {
        existing.push(deployment);
      } else {
        groups.set(deployment.codeHash, [deployment]);
      }
    }

    return Array.from(groups.entries()).map(([codeHash, groupedDeployments]) => ({
      codeHash,
      deployments: groupedDeployments,
      primaryDeployment: groupedDeployments[0],
    }));
  }, [sortedDeployments]);
  const lookupCodeHashes = useMemo(() => {
    const refs = new Set<string>((deployments ?? []).map((deployment) => deployment.codeHash));
    if (selectedRef) {
      refs.add(selectedRef);
    }
    return Array.from(refs);
  }, [deployments, selectedRef]);
  const { data: deploymentLookup } = useQuery({
    queryKey: ['script-deployments-lookup', lookupCodeHashes],
    queryFn: () => api.lookupScripts(lookupCodeHashes),
    enabled: lookupCodeHashes.length > 0,
    staleTime: Infinity,
  });
  useEffect(() => {
    if (versionGroups.length > 0 && !selectedVersion) {
      const usageByCodeHash = new Map(versionUsageEntries.map((d) => [d.codeHash, d]));
      const selectedByRef = selectedRef
        ? versionGroups.find((group) => {
            if (group.codeHash === selectedRef) {
              return true;
            }

            return group.deployments.some((deployment) => {
              const refs = deploymentReferenceHashes(
                deployment,
                deploymentLookup?.[deployment.codeHash]
              );
              const matchesTypeRef =
                refs.typeRef === selectedRef &&
                (!selectedRefHashType || selectedRefHashType === 'type');
              const matchesDataRef =
                refs.dataRef === selectedRef &&
                (!selectedRefHashType || selectedRefHashType === refs.dataRefType);
              return matchesTypeRef || matchesDataRef;
            });
          })
        : null;
      const firstWithUsage = versionGroups.find((group) => {
        const stats = usageByCodeHash.get(group.codeHash);
        return Boolean(stats && stats.liveCellsCount > 0);
      });
      const target = selectedByRef || firstWithUsage || versionGroups[0];
      const stats = usageByCodeHash.get(target.codeHash);

      setSelectedVersion({
        codeHash: target.codeHash,
        scriptKind: stats?.scriptKind as 'lock' | 'type' | undefined,
      });
    }
  }, [
    deploymentLookup,
    selectedRef,
    selectedRefHashType,
    selectedVersion,
    versionGroups,
    versionUsageEntries,
  ]);
  const selectedVersionDeployments = useMemo(
    () =>
      sortedDeployments.filter((deployment) => deployment.codeHash === selectedVersion?.codeHash),
    [selectedVersion?.codeHash, sortedDeployments]
  );
  const versionUsageReferences = useMemo<VersionUsageReference[]>(() => {
    const references = new Map<string, VersionUsageReference>();

    for (const deployment of selectedVersionDeployments) {
      const resolved = deploymentReferenceHashes(
        deployment,
        deploymentLookup?.[deployment.codeHash]
      );

      if (resolved.typeRef) {
        references.set(`${resolved.typeRef}:type`, {
          key: `${resolved.typeRef}:type`,
          referenceHash: resolved.typeRef,
          hashType: 'type',
        });
      }

      if (resolved.dataRef) {
        const key = `${resolved.dataRef}:${resolved.dataRefType}`;
        references.set(key, {
          key,
          referenceHash: resolved.dataRef,
          hashType: resolved.dataRefType,
        });
      }
    }

    return Array.from(references.values()).sort((a, b) => a.key.localeCompare(b.key));
  }, [deploymentLookup, selectedVersionDeployments]);
  const versionCellsCursorState = useMemo(
    () => decodeVersionCellsCursorState(cellsPagination.cursor),
    [cellsPagination.cursor]
  );
  const deploymentCellQueries = useQueries({
    queries: sortedDeployments.map((deployment) => ({
      queryKey: ['cell', deployment.codeCellTxHash, deployment.codeCellOutputIndex],
      queryFn: () => api.getCell(deployment.codeCellTxHash!, deployment.codeCellOutputIndex!),
      enabled: Boolean(deployment.codeCellTxHash != null && deployment.codeCellOutputIndex != null),
      staleTime: Infinity,
    })),
  });
  const governanceCodeHashes = useMemo(() => {
    const hashes = new Set<string>();

    for (const query of deploymentCellQueries) {
      const codeHash = normalizeHash(query.data?.lock?.codeHash);
      if (codeHash && !isAllZeroHash(codeHash)) {
        hashes.add(codeHash);
      }
    }

    return Array.from(hashes);
  }, [deploymentCellQueries]);
  const { data: governanceLookup } = useQuery({
    queryKey: ['script-governance-lookup', governanceCodeHashes],
    queryFn: () => api.lookupScripts(governanceCodeHashes),
    enabled: governanceCodeHashes.length > 0,
    staleTime: Infinity,
  });
  const selectedScriptKindForChart =
    selectedVersion?.scriptKind === 'lock' || selectedVersion?.scriptKind === 'type'
      ? selectedVersion.scriptKind
      : undefined;
  const { data: selectedCapacityChart, isLoading: isSelectedCapacityChartLoading } = useQuery({
    queryKey: [
      'script-capacity-chart',
      'version',
      selectedVersion?.codeHash,
      selectedScriptKindForChart,
      capacityRange,
    ],
    queryFn: () =>
      capacityRangeParams
        ? api.getScriptCapacityChartByCodeHash(
            selectedVersion!.codeHash,
            selectedScriptKindForChart,
            capacityRangeParams
          )
        : api.getScriptCapacityChartByCodeHash(
            selectedVersion!.codeHash,
            selectedScriptKindForChart
          ),
    enabled: Boolean(selectedVersion),
  });
  const selectedVersionCellsQueries = useQueries({
    queries: versionUsageReferences.map((reference) => ({
      queryKey: [
        'script-version-cells',
        selectedVersion?.codeHash,
        reference.referenceHash,
        reference.hashType,
        selectedScriptKindForChart,
        versionCellsCursorState[reference.key] ?? null,
      ],
      queryFn: () =>
        api.getCellsByScriptRef({
          codeHash: reference.referenceHash,
          hashType: reference.hashType,
          scriptKind: selectedScriptKindForChart,
          limit: DEFAULT_PAGE_SIZE,
          cursor: versionCellsCursorState[reference.key],
        }),
      enabled: Boolean(selectedVersion),
      placeholderData: keepPreviousData,
    })),
  });
  const usageByCodeHash = new Map(
    versionUsageEntries.map((deployment) => [deployment.codeHash, deployment])
  );
  const selectedVersionUsage = selectedVersion
    ? usageByCodeHash.get(selectedVersion.codeHash)
    : undefined;
  const selectedVersionCellsData = useMemo(() => {
    const total = selectedVersionUsage?.liveCellsCount ?? 0;
    const merged = new Map<
      string,
      {
        cell: NonNullable<(typeof selectedVersionCellsQueries)[number]['data']>['data'][number];
        sourceKeys: Set<string>;
      }
    >();

    for (const [index, query] of selectedVersionCellsQueries.entries()) {
      const reference = versionUsageReferences[index];
      if (!reference || !query.data) {
        continue;
      }

      for (const cell of query.data.data) {
        const outpointKey = `${cell.txHash}:${cell.outputIndex}`;
        const existing = merged.get(outpointKey);
        if (existing) {
          existing.sourceKeys.add(reference.key);
          continue;
        }

        merged.set(outpointKey, {
          cell,
          sourceKeys: new Set([reference.key]),
        });
      }
    }

    const mergedEntries = Array.from(merged.values()).sort(compareCellsByCreatedAt);
    const pageEntries = mergedEntries.slice(0, DEFAULT_PAGE_SIZE);
    const includedOutpoints = new Set(
      pageEntries.map((entry) => `${entry.cell.txHash}:${entry.cell.outputIndex}`)
    );
    const nextCursorState: Record<string, string | undefined> = {
      ...versionCellsCursorState,
    };

    for (const [index, query] of selectedVersionCellsQueries.entries()) {
      const reference = versionUsageReferences[index];
      if (!reference || !query.data) {
        continue;
      }

      let lastConsumedCell: NonNullable<(typeof query.data)['data']>[number] | undefined;
      for (const cell of query.data.data) {
        const outpointKey = `${cell.txHash}:${cell.outputIndex}`;
        if (includedOutpoints.has(outpointKey)) {
          lastConsumedCell = cell;
        }
      }

      if (lastConsumedCell) {
        nextCursorState[reference.key] = encodeVersionCellCursor(
          reference.referenceHash,
          lastConsumedCell.createdAtBlock,
          lastConsumedCell.txHash,
          lastConsumedCell.outputIndex
        );
      }
    }

    const hasMore =
      mergedEntries.length > DEFAULT_PAGE_SIZE ||
      selectedVersionCellsQueries.some((query) => Boolean(query.data?.hasMore));

    return {
      data: pageEntries.map((entry) => entry.cell),
      total,
      hasMore,
      nextCursor:
        hasMore && pageEntries.length > 0 ? encodeVersionCellsCursorState(nextCursorState) : null,
      currentCount: pageEntries.length,
    };
  }, [
    selectedVersionCellsQueries,
    selectedVersionUsage?.liveCellsCount,
    versionCellsCursorState,
    versionUsageReferences,
  ]);
  const isLoading = isCodeHashMode
    ? isCodeHashLookupLoading || isCodeHashCodeCellsLoading
    : isNamedDeploymentsLoading || isUsageLoading;
  if (isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse space-y-8">
            <div className="bg-base-surface h-20 w-full rounded" />
            <div className="bg-base-surface h-64 rounded" />
            <div className="bg-base-surface h-96 rounded" />
          </div>
        </main>
      </div>
    );
  }
  const codeHashAmbiguity =
    isCodeHashMode && codeHashLookup?.resolutionState === 'ambiguous'
      ? codeHashLookup.ambiguity
      : (codeHashCodeCells?.ambiguity ?? null);
  if (isCodeHashMode && codeHashAmbiguity) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelHeader>
              <div>
                <div className="text-emphasis text-sm font-semibold">
                  Ambiguous Script Reference
                </div>
                <div className="text-text-dim text-xs">
                  Current live type resolution points to more than one bytecode version.
                </div>
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent className="space-y-4">
              <div className="space-y-2">
                <div className="text-text-dim text-xs uppercase tracking-[0.2em]">Reference</div>
                <HexDisplay
                  value={normalizedCodeHash ?? ''}
                  size="sm"
                  startChars={10}
                  endChars={8}
                />
              </div>
              <div className="space-y-2">
                <div className="text-text-dim text-xs uppercase tracking-[0.2em]">
                  Conflicting Versions
                </div>
                <div className="space-y-2">
                  {codeHashAmbiguity.versionHashes.map((versionHash) => (
                    <HexDisplay
                      key={versionHash}
                      value={versionHash}
                      size="sm"
                      startChars={10}
                      endChars={8}
                    />
                  ))}
                </div>
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  if (namedDeploymentsError || !deployments || deployments.length === 0) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">Script not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  const scriptInfo = deployments[0];
  const formatNumber = (num: number) => {
    return new Intl.NumberFormat().format(num);
  };
  const inferredScriptKind = versionUsageEntries.find((d) => d.scriptKind)?.scriptKind;
  const isKnownScript = hasKnownScriptName(scriptInfo.name);
  const pageTitle = isKnownScript ? scriptInfo.name : 'Unlabeled Script';
  const pageSubtitle = isKnownScript
    ? scriptInfo.description
    : 'No imported label for this script yet. The code hash below is the canonical identity.';
  const deploymentCellsByOutpoint = new Map(
    deploymentCellQueries
      .map((query) => query.data)
      .filter((cell): cell is NonNullable<typeof cell> => cell != null)
      .map((cell) => [`${cell.txHash}:${cell.outputIndex}`, cell])
  );
  const versionRows = versionGroups.map((group) => {
    const stats = usageByCodeHash.get(group.codeHash);
    const firstDeployment = group.deployments.reduce((earliest, deployment) => {
      const earliestOutpointKey = deploymentOutpointKey(
        earliest.codeCellTxHash,
        earliest.codeCellOutputIndex
      );
      const deploymentOutpointKeyValue = deploymentOutpointKey(
        deployment.codeCellTxHash,
        deployment.codeCellOutputIndex
      );
      const earliestCell = earliestOutpointKey
        ? deploymentCellsByOutpoint.get(earliestOutpointKey)
        : undefined;
      const deploymentCell = deploymentOutpointKeyValue
        ? deploymentCellsByOutpoint.get(deploymentOutpointKeyValue)
        : undefined;

      return compareDeploymentCreatedAt(
        {
          deployedAt: deployment.deployedAt,
          createdAtBlock: deploymentCell?.createdAtBlock,
        },
        {
          deployedAt: earliest.deployedAt,
          createdAtBlock: earliestCell?.createdAtBlock,
        }
      ) < 0
        ? deployment
        : earliest;
    }, group.deployments[0]);
    const firstDeploymentOutpointKey = deploymentOutpointKey(
      firstDeployment.codeCellTxHash,
      firstDeployment.codeCellOutputIndex
    );

    return {
      ...group,
      firstDeployment,
      firstDeploymentCell: firstDeploymentOutpointKey
        ? deploymentCellsByOutpoint.get(firstDeploymentOutpointKey)
        : undefined,
      deploymentsCount: group.deployments.length,
      liveCellsCount: stats?.liveCellsCount ?? 0,
      liveCapacitySum: stats?.liveCapacitySum ?? '0',
      scriptKind:
        (stats?.scriptKind as 'lock' | 'type' | undefined) ??
        (group.primaryDeployment.scriptKind as 'lock' | 'type' | undefined),
    };
  });
  const selectedVersionDeploymentRows = selectedVersionDeployments.map((deployment) => {
    const outpointKey = deploymentOutpointKey(
      deployment.codeCellTxHash,
      deployment.codeCellOutputIndex
    );

    return {
      deployment,
      outpointKey,
      codeCell: outpointKey ? deploymentCellsByOutpoint.get(outpointKey) : undefined,
      references: deploymentReferenceHashes(deployment, deploymentLookup?.[deployment.codeHash]),
    };
  });
  const isSelectedVersionCodeCellsLoading = selectedVersionDeployments.some((deployment) => {
    const outpointKey = deploymentOutpointKey(
      deployment.codeCellTxHash,
      deployment.codeCellOutputIndex
    );
    if (!outpointKey) {
      return false;
    }

    const queryIndex = sortedDeployments.findIndex(
      (candidate) =>
        candidate.codeCellTxHash === deployment.codeCellTxHash &&
        candidate.codeCellOutputIndex === deployment.codeCellOutputIndex
    );

    return queryIndex >= 0 ? Boolean(deploymentCellQueries[queryIndex]?.isLoading) : false;
  });
  const isVersionCellsLoading = selectedVersionCellsQueries.some((query) => query.isLoading);

  const handleVersionClick = (versionRow: (typeof versionRows)[number]) => {
    if (selectedVersion?.codeHash !== versionRow.codeHash) {
      setSelectedVersion({
        codeHash: versionRow.codeHash,
        scriptKind: versionRow.scriptKind,
      });
      cellsPagination.reset();
    }
  };
  const isSelected = (versionRow: (typeof versionRows)[number]) =>
    selectedVersion?.codeHash === versionRow.codeHash;
  const showMobileVersions = viewportWidth < MOBILE_BREAKPOINT;
  const showCompactVersions = !showMobileVersions && viewportWidth < COMPACT_SCRIPT_VERSIONS_WIDTH;
  const showMobileVersionDeployments = viewportWidth < MOBILE_BREAKPOINT;
  const showCompactVersionDeployments =
    !showMobileVersionDeployments && viewportWidth < COMPACT_VERSION_DEPLOYMENTS_WIDTH;

  const renderVersionIdentity = (versionRow: (typeof versionRows)[number], selected: boolean) => (
    <div className="flex flex-wrap items-center gap-2">
      <HexDisplay
        value={versionRow.codeHash}
        size="sm"
        startChars={8}
        endChars={6}
        copyable={false}
      />
      <button
        type="button"
        className="text-text-dim border-base-border hover:bg-base-elevated/60 rounded border px-2 py-1 font-mono text-[10px] uppercase tracking-wide"
        title={`Click to copy: ${versionRow.codeHash}`}
        onClick={(event) => {
          event.stopPropagation();
          if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
            void navigator.clipboard.writeText(versionRow.codeHash).catch(() => {
              // Ignore clipboard failures for the explicit copy affordance.
            });
          }
        }}
      >
        Copy
      </button>
      {selected && <Badge variant="green">Selected</Badge>}
    </div>
  );

  const renderVersionFirstDeployedAt = (versionRow: (typeof versionRows)[number]) =>
    versionRow.firstDeploymentCell ? (
      <Link
        href={`/blocks/${versionRow.firstDeploymentCell.createdAtBlock}`}
        className="block hover:underline"
      >
        <div className="text-emphasis font-mono text-xs">
          #{formatNumber(versionRow.firstDeploymentCell.createdAtBlock)}
        </div>
        <div className="text-text-dim font-mono text-xs">
          {formatDeploymentTimestamp(versionRow.firstDeployment.deployedAt)}
        </div>
      </Link>
    ) : (
      <span className="text-text-dim">-</span>
    );

  const renderVersionUsageBadge = (versionRow: (typeof versionRows)[number]) =>
    versionRow.scriptKind ? (
      <Badge variant="neutral" className="px-1.5 py-0.5 text-[10px]">
        {versionRow.scriptKind.toUpperCase()}
      </Badge>
    ) : (
      <span className="text-text-dim">-</span>
    );

  const renderDeploymentStatus = (
    codeCell: (typeof selectedVersionDeploymentRows)[number]['codeCell']
  ) =>
    codeCell ? (
      <Badge variant={codeCell.status === 'live' ? 'green' : 'gray'}>
        {codeCell.status === 'live' ? 'Live' : 'Consumed'}
      </Badge>
    ) : (
      <span className="text-text-dim font-mono text-xs">-</span>
    );

  const renderGovernance = (
    codeCell: (typeof selectedVersionDeploymentRows)[number]['codeCell']
  ) => {
    if (!codeCell?.lock) {
      return <span className="text-text-dim font-mono text-xs">-</span>;
    }

    if (isAllZeroHash(codeCell.lock.codeHash)) {
      return (
        <div className="space-y-1">
          <div className="text-text font-mono text-sm">Immutable (all-zero lock)</div>
          <div className="text-text-dim font-mono text-xs">No governance lock script</div>
        </div>
      );
    }

    if (hasKnownScriptName(governanceLookup?.[codeCell.lock.codeHash]?.name)) {
      return (
        <div className="space-y-1">
          <div className="text-text font-mono text-sm">
            {governanceLookup?.[codeCell.lock.codeHash]?.name}
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <Badge variant="gray">{codeCell.lock.hashType}</Badge>
            <div>
              <HexDisplay value={codeCell.lock.args} size="sm" startChars={8} endChars={6} />
            </div>
          </div>
        </div>
      );
    }

    return (
      <div className="space-y-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-text font-mono text-sm">Script Ref</span>
          <Badge variant="gray">{codeCell.lock.hashType}</Badge>
        </div>
        <div>
          <HexDisplay value={codeCell.lock.codeHash} size="sm" startChars={8} endChars={6} />
        </div>
        <div className="pl-1">
          <HexDisplay value={codeCell.lock.args} size="sm" startChars={8} endChars={6} />
        </div>
      </div>
    );
  };

  const renderDeploymentReferences = (
    references: (typeof selectedVersionDeploymentRows)[number]['references']
  ) => (
    <div className="min-w-0 space-y-2">
      <div className="flex items-start gap-2">
        <span className="text-text-dim font-mono text-xs">type:</span>
        {references.typeRef ? (
          <HexDisplay value={references.typeRef} size="sm" startChars={8} endChars={6} />
        ) : (
          <span className="text-text-dim font-mono text-xs">Unavailable</span>
        )}
      </div>
      <div className="flex items-start gap-2">
        <span className="text-text-dim font-mono text-xs">{references.dataRefType}:</span>
        {references.dataRef ? (
          <HexDisplay value={references.dataRef} size="sm" startChars={8} endChars={6} />
        ) : (
          <span className="text-text-dim font-mono text-xs">Unavailable</span>
        )}
      </div>
    </div>
  );

  const renderDeploymentTimestamp = (
    deployment: (typeof selectedVersionDeploymentRows)[number]['deployment'],
    codeCell: (typeof selectedVersionDeploymentRows)[number]['codeCell']
  ) => (
    <div className="space-y-1 text-right">
      <div>
        {codeCell ? (
          <Link
            href={`/blocks/${codeCell.createdAtBlock}`}
            className="text-emphasis font-mono text-xs hover:underline"
          >
            #{formatNumber(codeCell.createdAtBlock)}
          </Link>
        ) : (
          <span className="text-text-dim font-mono text-xs">-</span>
        )}
      </div>
      <div className="text-text-dim font-mono text-xs">
        {formatDeploymentTimestamp(deployment.deployedAt)}
      </div>
    </div>
  );

  const scriptVersionsHelp = (
    <>
      <div className="text-text font-mono text-xs">What this section shows</div>
      <div>One row = one script version, identified by `code_hash`.</div>
      <div>
        This table answers which versions exist for the script and how much current live usage each
        version has.
      </div>
      <div className="space-y-1">
        <div>
          <span className="text-text font-mono">Code Hash</span>: version identity.
        </div>
        <div>
          <span className="text-text font-mono">First Deployed At</span>: earliest known deployment
          block and timestamp for this version.
        </div>
        <div>
          <span className="text-text font-mono">Used As</span>: whether cells use this version as a
          `lock`, `type`, or both. In compact tables this appears as a badge inside the code-hash
          cell.
        </div>
        <div>
          <span className="text-text font-mono">Deployments</span>: how many code cells have
          deployed this same bytecode version.
        </div>
        <div>
          <span className="text-text font-mono">Cells Using It</span>: live cells currently using
          this version.
        </div>
        <div>
          <span className="text-text font-mono">Capacity Using It</span>: current live capacity held
          by cells using this version.
        </div>
      </div>
    </>
  );

  const versionDeploymentsHelp = (
    <>
      <div className="text-text font-mono text-xs">What this section shows</div>
      <div>One row = one deployment code cell for the currently selected version.</div>
      <div>
        This table answers where the selected version was deployed and which concrete code cell a
        given reference points to.
      </div>
      <div className="space-y-1">
        <div>
          <span className="text-text font-mono">Outpoint</span>: the concrete code cell identity. In
          compact tables, the status badge is shown in the same cell.
        </div>
        <div>
          <span className="text-text font-mono">Status</span>: whether that deployment code cell is
          still live or already consumed.
        </div>
        <div>
          <span className="text-text font-mono">Governance</span>: the code cell lock script that
          governs upgrades or proves immutability.
        </div>
        <div>
          <span className="text-text font-mono">References</span>: the actual refs bound to this
          deployment code cell.
        </div>
        <div>
          <span className="text-text font-mono">Deployed At</span>: block number and timestamp for
          this deployment.
        </div>
        <div>
          <span className="text-text font-mono">Used Capacity</span>: occupied capacity of the code
          cell itself.
        </div>
      </div>
    </>
  );

  const referencesHelp = (
    <>
      <div className="text-text font-mono text-xs">type ref</div>
      <div>Resolves by type script hash. Upgradable flow, executes on latest CKB-VM.</div>
      <div className="text-text pt-1 font-mono text-xs">data/data1/data2</div>
      <div>Resolves by bytecode hash. Immutable binary, VM version fixed to v0/v1/v2.</div>
      <div className="pt-1">
        `type` favors upgradability; `data` family favors deterministic, reproducible execution.
      </div>
      <div className="pt-1">
        <a
          href="https://docs.nervos.org/docs/tech-explanation/data-type-diff"
          target="_blank"
          rel="noopener noreferrer"
          className="text-emphasis hover:underline"
        >
          Reference doc: data vs type hash semantics
        </a>
      </div>
    </>
  );

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title={pageTitle}
          subtitle={pageSubtitle}
          hash={!isKnownScript ? scriptInfo.codeHash : undefined}
          badge={
            <div className="flex items-center gap-2">
              {!isKnownScript && <Badge variant="gray">UNLABELED</Badge>}
              {inferredScriptKind && (
                <Badge variant="neutral">{inferredScriptKind.toUpperCase()}</Badge>
              )}
              {scriptInfo.decoderType && (
                <Badge variant="gray">{scriptInfo.decoderType.toUpperCase()}</Badge>
              )}
            </div>
          }
          actions={
            <div className="flex gap-3">
              {scriptInfo.rfc && (
                <a
                  href={scriptInfo.rfc}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-emphasis border-base-border hover:border-base-border hover:bg-base-surface rounded border px-3 py-1 font-mono text-sm transition-colors"
                >
                  RFC
                </a>
              )}
              {scriptInfo.website && (
                <a
                  href={scriptInfo.website}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-emphasis border-base-border hover:border-base-border hover:bg-base-surface rounded border px-3 py-1 font-mono text-sm transition-colors"
                >
                  Website
                </a>
              )}
              {scriptInfo.sourceUrl && (
                <a
                  href={scriptInfo.sourceUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-emphasis border-base-border hover:border-base-border hover:bg-base-surface rounded border px-3 py-1 font-mono text-sm transition-colors"
                >
                  Source
                </a>
              )}
            </div>
          }
        />
        <TerminalPanel className="border-base-border/80 mb-6">
          <TerminalPanelHeader indicator="active">
            <div className="flex items-center gap-2">
              <span>Script Versions</span>
              <HelpPopover label="Explain Script Versions" title="Script Versions">
                {scriptVersionsHelp}
              </HelpPopover>
            </div>
          </TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {showMobileVersions ? (
              <div data-testid="script-versions-compact">
                {versionRows.map((versionRow) => {
                  const selected = isSelected(versionRow);
                  const compactCapacity = formatCkbCompact(versionRow.liveCapacitySum);

                  return (
                    <TerminalRow
                      key={versionRow.codeHash}
                      data-testid={`version-row-${versionRow.codeHash}`}
                      onClick={() => handleVersionClick(versionRow)}
                      className={`cursor-pointer ${selected ? 'bg-emphasis/10 ring-emphasis/30 ring-1 ring-inset' : ''}`}
                    >
                      <div className="space-y-3">
                        <div className="flex items-start justify-between gap-3">
                          <div className="min-w-0 space-y-2">
                            {renderVersionIdentity(versionRow, selected)}
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                                Used as
                              </span>
                              {renderVersionUsageBadge(versionRow)}
                            </div>
                          </div>
                          <div className="min-w-[9rem] text-right">
                            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                              First deployed
                            </div>
                            <div className="pt-1">{renderVersionFirstDeployedAt(versionRow)}</div>
                          </div>
                        </div>
                        <div className="border-base-border/50 grid grid-cols-3 gap-3 border-t pt-3">
                          <div>
                            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                              Deployments
                            </div>
                            <div className="text-text pt-1 font-mono tabular-nums">
                              {formatNumber(versionRow.deploymentsCount)}
                            </div>
                          </div>
                          <div>
                            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                              Cells using it
                            </div>
                            <div className="text-text pt-1 font-mono tabular-nums">
                              {versionRow.liveCellsCount > 0
                                ? formatNumber(versionRow.liveCellsCount)
                                : '-'}
                            </div>
                          </div>
                          <div className="text-right">
                            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                              Capacity using it
                            </div>
                            <div className="text-text pt-1 font-mono tabular-nums">
                              {versionRow.liveCapacitySum !== '0'
                                ? `${compactCapacity.value} CKB`
                                : '-'}
                            </div>
                          </div>
                        </div>
                      </div>
                    </TerminalRow>
                  );
                })}
                {usage && (
                  <div className="border-base-border bg-base-bg/95 border-t px-4 py-3 backdrop-blur">
                    <div className="flex items-center justify-between gap-4">
                      <div className="text-text-dim font-medium">Total</div>
                      <div className="grid grid-cols-2 gap-4 text-right">
                        <div>
                          <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                            Cells using it
                          </div>
                          <div className="text-emphasis font-mono tabular-nums">
                            <span title={`Total: ${formatNumber(usage.cellsCount)}`}>
                              {formatNumber(usage.liveCellsCount)}
                            </span>
                          </div>
                        </div>
                        <div>
                          <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                            Capacity using it
                          </div>
                          <div className="text-emphasis font-mono tabular-nums">
                            <span title={`${formatCkbCompact(usage.liveCapacitySum).full} CKB`}>
                              {formatCkbCompact(usage.liveCapacitySum).value} CKB
                            </span>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            ) : showCompactVersions ? (
              <div className="overflow-x-auto">
                <div className="border-base-border bg-base-surface/50 text-text-dim grid min-w-[760px] grid-cols-[minmax(0,1fr)_9rem_6rem_8rem_9rem] gap-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                  <div>Code Hash</div>
                  <div className="text-right">First Deployed At</div>
                  <div className="text-right">Deployments</div>
                  <div className="text-right">Cells Using It</div>
                  <div className="text-right">Capacity Using It</div>
                </div>
                {versionRows.map((versionRow) => {
                  const selected = isSelected(versionRow);
                  const compactCapacity = formatCkbCompact(versionRow.liveCapacitySum);

                  return (
                    <TerminalRow
                      key={versionRow.codeHash}
                      data-testid={`version-row-${versionRow.codeHash}`}
                      onClick={() => handleVersionClick(versionRow)}
                      className={`min-w-[760px] cursor-pointer ${selected ? 'bg-emphasis/10 ring-emphasis/30 ring-1 ring-inset' : ''}`}
                    >
                      <div className="grid w-full min-w-[760px] grid-cols-[minmax(0,1fr)_9rem_6rem_8rem_9rem] items-start gap-4">
                        <div className="min-w-0 space-y-2 py-0.5">
                          {renderVersionIdentity(versionRow, selected)}
                          <div>{renderVersionUsageBadge(versionRow)}</div>
                        </div>
                        <div className="py-0.5 text-right">
                          {renderVersionFirstDeployedAt(versionRow)}
                        </div>
                        <div className="text-text py-0.5 text-right font-mono tabular-nums">
                          {formatNumber(versionRow.deploymentsCount)}
                        </div>
                        <div className="text-text py-0.5 text-right font-mono tabular-nums">
                          {versionRow.liveCellsCount > 0 ? (
                            <span title={`Total: ${formatNumber(versionRow.liveCellsCount)}`}>
                              {formatNumber(versionRow.liveCellsCount)}
                            </span>
                          ) : (
                            '-'
                          )}
                        </div>
                        <div className="text-text py-0.5 text-right font-mono tabular-nums">
                          {versionRow.liveCapacitySum !== '0' ? (
                            <span title={`${compactCapacity.full} CKB`}>
                              {compactCapacity.value} CKB
                            </span>
                          ) : (
                            '-'
                          )}
                        </div>
                      </div>
                    </TerminalRow>
                  );
                })}
                {usage && (
                  <div className="border-base-border bg-base-bg/95 sticky bottom-0 z-10 grid min-w-[760px] grid-cols-[minmax(0,1fr)_9rem_6rem_8rem_9rem] gap-4 border-t px-4 py-3 font-medium backdrop-blur">
                    <div className="text-text-dim">Total</div>
                    <div />
                    <div />
                    <div className="text-emphasis text-right font-mono tabular-nums">
                      <span title={`Total: ${formatNumber(usage.cellsCount)}`}>
                        {formatNumber(usage.liveCellsCount)}
                      </span>
                    </div>
                    <div className="text-emphasis text-right font-mono tabular-nums">
                      <span title={`${formatCkbCompact(usage.liveCapacitySum).full} CKB`}>
                        {formatCkbCompact(usage.liveCapacitySum).value} CKB
                      </span>
                    </div>
                  </div>
                )}
              </div>
            ) : (
              <div className="overflow-x-auto">
                <div className="border-base-border bg-base-surface/50 text-text-dim flex min-w-[980px] border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                  <div className="flex-1">Code Hash</div>
                  <div className="w-44 shrink-0 text-right">First Deployed At</div>
                  <div className="w-24 shrink-0 text-center">Used As</div>
                  <div className="w-28 shrink-0 text-right">Deployments</div>
                  <div className="w-40 shrink-0 text-right">Cells Using It</div>
                  <div className="w-44 shrink-0 text-right">Capacity Using It</div>
                </div>
                {versionRows.map((versionRow) => {
                  const selected = isSelected(versionRow);
                  const compactCapacity = formatCkbCompact(versionRow.liveCapacitySum);

                  return (
                    <React.Fragment key={versionRow.codeHash}>
                      <TerminalRow
                        data-testid={`version-row-${versionRow.codeHash}`}
                        onClick={() => handleVersionClick(versionRow)}
                        className={`min-w-[980px] cursor-pointer ${selected ? 'bg-emphasis/10 ring-emphasis/30 ring-1 ring-inset' : ''}`}
                      >
                        <div className="grid w-full min-w-[980px] grid-cols-[minmax(0,1fr)_11rem_6rem_7rem_9rem_11rem] items-center gap-4">
                          <div className="min-w-0 py-0.5">
                            {renderVersionIdentity(versionRow, selected)}
                          </div>
                          <div className="py-0.5 text-right">
                            {renderVersionFirstDeployedAt(versionRow)}
                          </div>
                          <div className="text-text-dim py-0.5 text-center">
                            {renderVersionUsageBadge(versionRow)}
                          </div>
                          <div className="text-text py-0.5 text-right font-mono tabular-nums">
                            {formatNumber(versionRow.deploymentsCount)}
                          </div>
                          <div className="text-text py-0.5 text-right font-mono tabular-nums">
                            {versionRow.liveCellsCount > 0 ? (
                              <span title={`Total: ${formatNumber(versionRow.liveCellsCount)}`}>
                                {formatNumber(versionRow.liveCellsCount)}
                              </span>
                            ) : (
                              '-'
                            )}
                          </div>
                          <div className="text-text py-0.5 text-right font-mono tabular-nums">
                            {versionRow.liveCapacitySum !== '0' ? (
                              <span title={`${compactCapacity.full} CKB`}>
                                {compactCapacity.value} CKB
                              </span>
                            ) : (
                              '-'
                            )}
                          </div>
                        </div>
                      </TerminalRow>
                    </React.Fragment>
                  );
                })}
                {usage && (
                  <div className="border-base-border bg-base-bg/95 sticky bottom-0 z-10 flex min-w-[980px] border-t px-4 py-3 font-medium backdrop-blur">
                    <div className="text-text-dim flex-1">Total</div>
                    <div className="w-44 shrink-0" />
                    <div className="w-24 shrink-0" />
                    <div className="w-28 shrink-0" />
                    <div className="text-emphasis w-40 shrink-0 text-right font-mono tabular-nums">
                      <span title={`Total: ${formatNumber(usage.cellsCount)}`}>
                        {formatNumber(usage.liveCellsCount)}
                      </span>
                    </div>
                    <div className="text-emphasis w-44 shrink-0 text-right font-mono tabular-nums">
                      <span title={`${formatCkbCompact(usage.liveCapacitySum).full} CKB`}>
                        {formatCkbCompact(usage.liveCapacitySum).value} CKB
                      </span>
                    </div>
                  </div>
                )}
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
        {selectedVersion && (
          <>
            <TerminalPanel className="mb-6">
              <TerminalPanelHeader indicator="none">
                <div className="flex items-center gap-2">
                  <span>Version Deployments</span>
                  <HelpPopover label="Explain Version Deployments" title="Version Deployments">
                    {versionDeploymentsHelp}
                  </HelpPopover>
                </div>
              </TerminalPanelHeader>
              <TerminalPanelContent padding="none">
                {showMobileVersionDeployments ? (
                  <div data-testid="version-deployments-compact">
                    {selectedVersionDeploymentRows.length > 0 ? (
                      selectedVersionDeploymentRows.map(
                        ({ deployment, outpointKey, codeCell, references }) => (
                          <TerminalRow key={outpointKey ?? deployment.codeHash}>
                            <div className="space-y-3">
                              <div className="flex items-start justify-between gap-3">
                                <div className="min-w-0">
                                  <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                                    Outpoint
                                  </div>
                                  <div className="pt-1">
                                    {deployment.codeCellTxHash != null &&
                                    deployment.codeCellOutputIndex != null ? (
                                      <Link
                                        href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                                        className="text-emphasis hover:underline"
                                      >
                                        <HexDisplay
                                          value={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
                                          size="sm"
                                          startChars={8}
                                          endChars={8}
                                          copyable={false}
                                        />
                                      </Link>
                                    ) : (
                                      <span className="text-text-dim font-mono text-xs">
                                        Unavailable
                                      </span>
                                    )}
                                  </div>
                                </div>
                                <div className="pt-4">{renderDeploymentStatus(codeCell)}</div>
                              </div>
                              <div>
                                <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                                  Governance
                                </div>
                                <div className="pt-1">{renderGovernance(codeCell)}</div>
                              </div>
                              <div>
                                <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                                  References
                                </div>
                                <div className="pt-1">{renderDeploymentReferences(references)}</div>
                              </div>
                              <div className="border-base-border/50 grid grid-cols-2 gap-3 border-t pt-3">
                                <div>
                                  <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                                    Deployed at
                                  </div>
                                  <div className="pt-1 text-left">
                                    {renderDeploymentTimestamp(deployment, codeCell)}
                                  </div>
                                </div>
                                <div className="text-right">
                                  <div className="text-text-dim font-mono text-[10px] uppercase tracking-wide">
                                    Used capacity
                                  </div>
                                  <div className="pt-1">
                                    {codeCell?.usedCapacity != null ? (
                                      <Capacity
                                        value={String(codeCell.usedCapacity)}
                                        className="text-sm"
                                      />
                                    ) : (
                                      <span className="text-text-dim font-mono text-xs">-</span>
                                    )}
                                  </div>
                                </div>
                              </div>
                            </div>
                          </TerminalRow>
                        )
                      )
                    ) : (
                      <div className="text-text-dim px-4 py-8 text-center">
                        No deployments found for this version
                      </div>
                    )}
                    {isSelectedVersionCodeCellsLoading &&
                      selectedVersionDeploymentRows.length > 0 && (
                        <div className="text-text-dim border-base-border border-t px-4 py-3 text-xs">
                          Loading deployment status, block, and capacity...
                        </div>
                      )}
                  </div>
                ) : showCompactVersionDeployments ? (
                  <div className="overflow-x-auto" data-testid="version-deployments-scroll">
                    <div className="border-base-border bg-base-surface/50 text-text-dim grid min-w-[860px] grid-cols-[10rem_minmax(10rem,1fr)_minmax(11rem,1fr)_8rem_9rem] gap-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                      <div>Outpoint</div>
                      <div>Governance</div>
                      <div className="flex items-center gap-1">
                        <span>References</span>
                        <HelpPopover
                          label="Explain References"
                          title="Reference Semantics"
                          contentClassName="w-[24rem]"
                        >
                          {referencesHelp}
                        </HelpPopover>
                      </div>
                      <div className="text-right">Deployed At</div>
                      <div className="text-right">Used Capacity</div>
                    </div>
                    {selectedVersionDeploymentRows.length > 0 ? (
                      selectedVersionDeploymentRows.map(
                        ({ deployment, outpointKey, codeCell, references }) => (
                          <TerminalRow
                            key={outpointKey ?? deployment.codeHash}
                            className="min-w-[860px]"
                          >
                            <div className="grid w-full min-w-[860px] grid-cols-[10rem_minmax(10rem,1fr)_minmax(11rem,1fr)_8rem_9rem] items-start gap-4">
                              <div
                                className="min-w-0 space-y-2"
                                title={outpointKey ? `Click to copy: ${outpointKey}` : undefined}
                              >
                                {deployment.codeCellTxHash != null &&
                                deployment.codeCellOutputIndex != null ? (
                                  <Link
                                    href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                                    className="text-emphasis hover:underline"
                                  >
                                    <HexDisplay
                                      value={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
                                      size="sm"
                                      startChars={8}
                                      endChars={8}
                                      copyable={false}
                                    />
                                  </Link>
                                ) : (
                                  <span className="text-text-dim font-mono text-xs">
                                    Unavailable
                                  </span>
                                )}
                                <div>{renderDeploymentStatus(codeCell)}</div>
                              </div>
                              <div className="min-w-0 space-y-2">{renderGovernance(codeCell)}</div>
                              {renderDeploymentReferences(references)}
                              {renderDeploymentTimestamp(deployment, codeCell)}
                              <div className="text-right">
                                {codeCell?.usedCapacity != null ? (
                                  <Capacity
                                    value={String(codeCell.usedCapacity)}
                                    className="text-sm"
                                  />
                                ) : (
                                  <span className="text-text-dim font-mono text-xs">-</span>
                                )}
                              </div>
                            </div>
                          </TerminalRow>
                        )
                      )
                    ) : (
                      <div className="text-text-dim px-4 py-8 text-center">
                        No deployments found for this version
                      </div>
                    )}
                    {isSelectedVersionCodeCellsLoading &&
                      selectedVersionDeploymentRows.length > 0 && (
                        <div className="text-text-dim border-base-border border-t px-4 py-3 text-xs">
                          Loading deployment status, block, and capacity...
                        </div>
                      )}
                  </div>
                ) : (
                  <div className="overflow-x-auto" data-testid="version-deployments-scroll">
                    <div className="border-base-border bg-base-surface/50 text-text-dim grid min-w-[1120px] grid-cols-[11rem_6rem_minmax(14rem,1fr)_minmax(16rem,1fr)_11rem_11rem] gap-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                      <div>Outpoint</div>
                      <div>Status</div>
                      <div>Governance</div>
                      <div className="flex items-center gap-1">
                        <span>References</span>
                        <HelpPopover
                          label="Explain References"
                          title="Reference Semantics"
                          contentClassName="w-[24rem]"
                        >
                          {referencesHelp}
                        </HelpPopover>
                      </div>
                      <div className="text-right">Deployed At</div>
                      <div className="text-right">Used Capacity</div>
                    </div>
                    {selectedVersionDeploymentRows.length > 0 ? (
                      selectedVersionDeploymentRows.map(
                        ({ deployment, outpointKey, codeCell, references }) => (
                          <TerminalRow
                            key={outpointKey ?? deployment.codeHash}
                            className="min-w-[1120px]"
                          >
                            <div className="grid w-full min-w-[1120px] grid-cols-[11rem_6rem_minmax(14rem,1fr)_minmax(16rem,1fr)_11rem_11rem] items-start gap-4">
                              <div
                                className="min-w-0"
                                title={outpointKey ? `Click to copy: ${outpointKey}` : undefined}
                              >
                                {deployment.codeCellTxHash != null &&
                                deployment.codeCellOutputIndex != null ? (
                                  <Link
                                    href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                                    className="text-emphasis hover:underline"
                                  >
                                    <HexDisplay
                                      value={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
                                      size="sm"
                                      startChars={8}
                                      endChars={8}
                                      copyable={false}
                                    />
                                  </Link>
                                ) : (
                                  <span className="text-text-dim font-mono text-xs">
                                    Unavailable
                                  </span>
                                )}
                              </div>
                              <div className="pt-0.5">{renderDeploymentStatus(codeCell)}</div>
                              <div className="min-w-0 space-y-2">{renderGovernance(codeCell)}</div>
                              {renderDeploymentReferences(references)}
                              {renderDeploymentTimestamp(deployment, codeCell)}
                              <div className="text-right">
                                {codeCell?.usedCapacity != null ? (
                                  <Capacity
                                    value={String(codeCell.usedCapacity)}
                                    className="text-sm"
                                  />
                                ) : (
                                  <span className="text-text-dim font-mono text-xs">-</span>
                                )}
                              </div>
                            </div>
                          </TerminalRow>
                        )
                      )
                    ) : (
                      <div className="text-text-dim px-4 py-8 text-center">
                        No deployments found for this version
                      </div>
                    )}
                    {isSelectedVersionCodeCellsLoading &&
                      selectedVersionDeploymentRows.length > 0 && (
                        <div className="text-text-dim border-base-border border-t px-4 py-3 text-xs">
                          Loading deployment status, block, and capacity...
                        </div>
                      )}
                  </div>
                )}
              </TerminalPanelContent>
            </TerminalPanel>
            <TerminalPanel>
              <TerminalPanelHeader indicator="none">Usage</TerminalPanelHeader>
              <TerminalPanelContent padding="none">
                {selectedVersionUsage && (
                  <div className="border-base-border border-b px-4 py-4">
                    <HMultiplier
                      totalCapacity={selectedVersionUsage.liveCapacitySum}
                      usedCapacity={selectedVersionUsage.liveUsedCapacitySum}
                    />
                  </div>
                )}
                <div className="border-base-border border-b px-4 py-4">
                  <div className="text-text-dim mb-3 text-xs">
                    Historical used/unused live capacity for the selected version.
                  </div>
                  <CapacityRangeSelector value={capacityRange} onChange={setCapacityRange} />
                  {isSelectedCapacityChartLoading ? (
                    <div className="text-text-dim py-6 text-center">Loading version history...</div>
                  ) : selectedCapacityChart && selectedCapacityChart.data.length > 0 ? (
                    <StackedAreaChart
                      data={selectedCapacityChart.data}
                      series={selectedCapacityChart.series}
                      height={220}
                      valueUnit="shannon"
                    />
                  ) : (
                    <div className="text-text-dim py-6 text-center">No version history yet</div>
                  )}
                </div>
                <div className="px-4 py-4">
                  <div className="text-text-dim mb-3 text-xs">
                    Live cells currently using the selected version.
                  </div>
                </div>
                {isVersionCellsLoading ? (
                  <div className="text-text-dim py-8 text-center">Loading cells...</div>
                ) : selectedVersionCellsData.data.length > 0 ? (
                  <>
                    <div className="border-base-border bg-base-surface/50 text-text-dim flex border-y px-4 py-2 font-mono text-xs uppercase tracking-wider">
                      <div className="flex-1">Cell</div>
                      <div className="w-52 shrink-0 text-right">Capacity</div>
                      <div className="w-24 shrink-0 text-right">Data Size</div>
                      <div className="w-28 shrink-0 text-right">Created At</div>
                    </div>
                    {selectedVersionCellsData.data.map((cell) => (
                      <TerminalRow key={`${cell.txHash}-${cell.outputIndex}`}>
                        <div className="flex items-center">
                          <div className="flex-1">
                            <Link
                              href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                              className="text-emphasis hover:underline"
                            >
                              <HexDisplay value={`${cell.txHash}:${cell.outputIndex}`} />
                            </Link>
                          </div>
                          <div className="text-text-bright w-52 shrink-0 text-right">
                            <Capacity value={cell.capacity} />
                          </div>
                          <div className="text-text-dim w-24 shrink-0 text-right font-mono">
                            {cell.cellType === 'genesis_special_burn' ? (
                              <span
                                className="border-base-border cursor-help border-b border-dashed"
                                title="Virtual used capacity: 5.04B CKB"
                              >
                                <Capacity value={cell.virtualUsedCapacity || '0'} />
                              </span>
                            ) : (
                              <>{cell.dataSize.toLocaleString()} bytes</>
                            )}
                          </div>
                          <div className="w-28 shrink-0 text-right">
                            <Link
                              href={`/blocks/${cell.createdAtBlock}`}
                              className="text-emphasis hover:underline"
                            >
                              #{cell.createdAtBlock.toLocaleString()}
                            </Link>
                          </div>
                        </div>
                      </TerminalRow>
                    ))}
                    <div className="border-base-border border-t p-4">
                      <CursorPagination
                        total={selectedVersionCellsData.total}
                        totalLabel="cells"
                        pageSize={DEFAULT_PAGE_SIZE}
                        page={cellsPagination.page}
                        currentCount={selectedVersionCellsData.currentCount}
                        hasMore={selectedVersionCellsData.hasMore}
                        hasPrevious={cellsPagination.hasPrevious}
                        onNext={() => cellsPagination.goToNext(selectedVersionCellsData.nextCursor)}
                        onPrevious={cellsPagination.goToPrevious}
                      />
                    </div>
                  </>
                ) : (
                  <div className="text-text-dim py-8 text-center">
                    No cells found for this script
                  </div>
                )}
              </TerminalPanelContent>
            </TerminalPanel>
          </>
        )}
      </main>
    </div>
  );
}
