'use client';
import React, { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueries, keepPreviousData } from '@tanstack/react-query';
import { useSearchParams } from '@/src/navigation';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { Capacity } from '@/components/ui/capacity';
import { CellBar } from '@/components/ui/cell-bar';
import { CapacityRangeSelector } from '@/components/ui/capacity-range-selector';
import { DetailSection, DetailLayout } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import {
  ScriptCodeClassSummary,
  ScriptCustodyEvidence,
  ScriptCapacityEvidenceChart,
} from '@/components/scripts/script-code-evidence';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, isNotFoundApiError } from '@/lib/api';
import { getCapacityRangeParams, CapacityRangeKey } from '@/lib/capacity-range';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import {
  encodeCellsByScriptCursor,
  getScriptRefQueryHashType,
  normalizeScriptRefHashType,
  type ScriptRefHashType,
} from '@/lib/script-ref';
import { formatCapacity, formatCkbCompact, formatNumber } from '@/lib/utils';
import type { Cell, KnownScript, ScriptFamilyDetail, ScriptLookupInfo } from '@/lib/api';

function CellOccupiedValue({ cell }: { cell: Cell }) {
  if (cell.cellType === 'genesis_special_burn') {
    return cell.virtualCommonKnowledgeSize === undefined ? (
      <span
        role="alert"
        data-testid="burn-cell-virtual-size-error"
        title={`Genesis burn cell ${cell.txHash}:${cell.outputIndex} has no virtualCommonKnowledgeSize`}
        className="text-negative"
      >
        unavailable
      </span>
    ) : (
      <span
        title={`Virtual common knowledge size: ${formatCapacity(cell.virtualCommonKnowledgeSize)}`}
      >
        <Capacity value={cell.virtualCommonKnowledgeSize} />
      </span>
    );
  }
  return <Capacity value={cell.commonKnowledgeSize} />;
}

function CellOwner({ cell }: { cell: Cell }) {
  return cell.address ? (
    <Link href={`/address/${cell.address}`} className="text-emphasis break-all hover:underline">
      {cell.address}
    </Link>
  ) : (
    <span role="alert" className="text-negative">
      Owner address unavailable
    </span>
  );
}

function CellTypeReference({ cell }: { cell: Cell }) {
  if (!cell.typeScriptHash) return <span className="text-text-dim">No type script</span>;
  if (!cell.typeCodeHash || !cell.typeHashType)
    return (
      <span role="alert" className="text-negative">
        Type reference unavailable
      </span>
    );
  return (
    <Link
      href={`/script/${cell.typeCodeHash}?hashType=${cell.typeHashType}`}
      className="text-emphasis break-all hover:underline"
    >
      {cell.typeProtocolName ?? cell.typeCodeHash}
    </Link>
  );
}

function CellAnatomyBar({ cell }: { cell: Cell }) {
  const bytes = cell.commonKnowledgeSizeBreakdown;
  if (!bytes)
    return (
      <span role="alert" className="text-negative">
        Cell anatomy unavailable
      </span>
    );
  return (
    <CellBar
      subject={`${cell.txHash}:${cell.outputIndex}`}
      anatomy={{
        lock: String(bytes.lockScriptBytes),
        type: String(bytes.typeScriptBytes),
        data: String(bytes.dataBytes),
      }}
    />
  );
}

interface SelectedVersion {
  codeHash: string;
  scriptKind?: 'lock' | 'type';
}

interface ScriptVersionGroup {
  codeHash: string;
  name: string;
  description: string | null;
  scriptKind: string | null;
  deprecated: boolean;
  deployments: KnownScript[];
  observedReferences: ScriptFamilyDetail['versions'][number]['references'];
  deployedAt: number | null;
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
  ownedCapacitySum: string;
  ownedKnowledgeSum: string;
}

/**
 * A version's usage: its counts, or why there are none to show — the usage request failed, or
 * its response left this version out. Neither reads as "0 cells, 0 CKB".
 */
type VersionUsage =
  | { state: 'ready'; liveCellsCount: number; ownedCapacitySum: string; ownedKnowledgeSum: string }
  | { state: 'failed' | 'missing'; message: string };

/** The cells or capacity a version's cells use — or, explicitly, why they cannot be shown. */
function VersionUsageCell({ usage, field }: { usage: VersionUsage; field: 'cells' | 'capacity' }) {
  if (usage.state !== 'ready') {
    return (
      <span
        data-testid="version-usage"
        data-state={usage.state}
        role="alert"
        title={usage.message}
        className="text-rouge"
      >
        unavailable
      </span>
    );
  }
  // A version with no live cells reads '-' (a real zero, not a missing value).
  if (field === 'cells') {
    const count = usage.liveCellsCount;
    return (
      <span
        data-testid="version-usage"
        data-state="ready"
        title={count > 0 ? `Total: ${formatNumber(count)}` : undefined}
      >
        {count > 0 ? formatNumber(count) : '-'}
      </span>
    );
  }
  if (usage.ownedCapacitySum === '0') {
    return (
      <span data-testid="version-usage" data-state="ready">
        -
      </span>
    );
  }
  const compact = formatCkbCompact(usage.ownedCapacitySum);
  return (
    <span data-testid="version-usage" data-state="ready" title={`${compact.full} CKB`}>
      {compact.value} CKB
    </span>
  );
}

const UNKNOWN_SCRIPT_NAME = 'unknown';

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

function collectObservedUsageReferences(
  references: Map<string, VersionUsageReference>,
  deployment: KnownScript
): void {
  for (const reference of deployment.observedReferences ?? []) {
    const normalizedHashType = normalizeScriptRefHashType(reference.hashType);
    if (!normalizedHashType) {
      continue;
    }

    references.set(`${reference.referenceHash}:${normalizedHashType}`, {
      key: `${reference.referenceHash}:${normalizedHashType}`,
      referenceHash: reference.referenceHash,
      hashType: normalizedHashType,
    });
  }
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
  const selectedRefParam = searchParams.get('ref') ?? (isCodeHashMode ? normalizedCodeHash : null);
  const selectedRef =
    selectedRefParam && isHexScriptHash(selectedRefParam.trim())
      ? `0x${selectedRefParam.trim().slice(2).toLowerCase()}`
      : null;
  const selectedRefHashType = normalizeScriptRefHashType(searchParams.get('hashType'));
  const [capacityRange, setCapacityRange] = useState<CapacityRangeKey>('all');
  const [selectedVersion, setSelectedVersion] = useState<SelectedVersion | null>(null);
  const [selectedDeploymentOutpoint, setSelectedDeploymentOutpoint] = useState<string | null>(null);
  const cellsPagination = useCursorPagination();
  const capacityRangeParams = getCapacityRangeParams(capacityRange);
  const {
    data: namedScriptDetail,
    isLoading: isNamedScriptDetailLoading,
    error: namedScriptDetailError,
  } = useQuery({
    queryKey: ['script-family-detail', name],
    queryFn: () => {
      if (!name) throw new Error('script name is required');
      return api.getScriptFamilyDetail(name);
    },
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const {
    data: usage,
    isLoading: isUsageLoading,
    error: usageError,
  } = useQuery({
    queryKey: ['script-usage', name],
    queryFn: () => {
      if (!name) throw new Error('script name is required');
      return api.getScriptUsage(name);
    },
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const {
    data: familyCapacityChart,
    isLoading: isFamilyCapacityChartLoading,
    error: familyCapacityChartError,
  } = useQuery({
    queryKey: ['script-capacity-chart', 'family', name, capacityRange],
    queryFn: () => api.getScriptCapacityChart(name!, capacityRangeParams ?? undefined),
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const {
    data: codeHashLookup,
    isLoading: isCodeHashLookupLoading,
    error: codeHashLookupError,
  } = useQuery({
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
  const {
    data: codeHashCodeCells,
    isLoading: isCodeHashCodeCellsLoading,
    error: codeHashCodeCellsError,
  } = useQuery({
    queryKey: ['script-code-cells-unified', normalizedCodeHash, codeHashDeploymentQueryHashType],
    queryFn: () => api.getCodeCells(normalizedCodeHash!, codeHashDeploymentQueryHashType!),
    enabled: Boolean(isCodeHashMode && normalizedCodeHash && codeHashDeploymentQueryHashType),
    staleTime: Infinity,
  });
  const deployments = useMemo<KnownScript[] | null>(() => {
    if (!isCodeHashMode) {
      if (!namedScriptDetail) {
        return null;
      }

      return namedScriptDetail.versions.flatMap((version) =>
        version.deployments.map((deployment) => ({
          codeHash: version.versionHash,
          name: version.name || namedScriptDetail.name,
          description: version.description ?? namedScriptDetail.description,
          scriptKind: version.scriptKind ?? namedScriptDetail.scriptKind,
          rfc: null,
          website: version.website ?? namedScriptDetail.website,
          sourceUrl: null,
          decoderType: null,
          network: '',
          hashType: deployment.hashType,
          dataHash: deployment.dataReferenceHash,
          typeHash:
            deployment.typeReferenceHash ??
            (version.canonicalHashType === 'type' ? version.canonicalReferenceHash : null),
          tag: null,
          deprecated: version.deprecated,
          isSystem: false,
          codeCellTxHash: deployment.codeCellTxHash,
          codeCellOutputIndex: deployment.codeCellOutputIndex,
          deployedAt: deployment.deployedAt,
          ownedCapacitySum: version.ownedCapacitySum,
          ownedKnowledgeSum: version.ownedKnowledgeSum,
          liveCellsCount: version.liveCellsCount,
          cellsCount: version.cellsCount,
          codeCellsLiveCount: version.codeCellsLiveCount,
          codeCellsTotal: version.codeCellsTotal,
        }))
      );
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
      deprecated: codeHashLookup.deprecated ?? false,
      isSystem: false,
      codeCellTxHash: codeHashLookup.codeCellTxHash,
      codeCellOutputIndex: codeHashLookup.codeCellOutputIndex,
      deployedAt: null,
      ownedCapacitySum: codeHashLookup.ownedCapacitySum,
      ownedKnowledgeSum: codeHashLookup.ownedKnowledgeSum,
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
    namedScriptDetail,
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
          ownedCapacitySum: codeHashLookup.ownedCapacitySum,
          ownedKnowledgeSum: codeHashLookup.ownedKnowledgeSum,
        },
      ];
    }

    return (
      usage?.byDeployment.map((deployment) => ({
        codeHash: deployment.codeHash,
        scriptKind: deployment.scriptKind,
        liveCellsCount: deployment.liveCellsCount,
        ownedCapacitySum: deployment.ownedCapacitySum,
        ownedKnowledgeSum: deployment.ownedKnowledgeSum,
      })) ?? []
    );
  }, [codeHashLookup, isCodeHashMode, usage?.byDeployment]);
  const sortedDeployments = useMemo(
    () => (deployments ? [...deployments].sort(compareDeploymentsByDeployedAt) : []),
    [deployments]
  );
  const versionGroups = useMemo<ScriptVersionGroup[]>(() => {
    if (!isCodeHashMode) {
      if (!namedScriptDetail) {
        return [];
      }

      return namedScriptDetail.versions.map((version) => ({
        codeHash: version.versionHash,
        name: version.name || namedScriptDetail.name,
        description: version.description ?? namedScriptDetail.description,
        scriptKind: version.scriptKind ?? namedScriptDetail.scriptKind,
        deprecated: version.deprecated,
        deployments: sortedDeployments.filter(
          (deployment) => deployment.codeHash === version.versionHash
        ),
        observedReferences: version.references,
        deployedAt: version.deployedAt,
      }));
    }

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
      name: groupedDeployments[0]?.name ?? codeHash,
      description: groupedDeployments[0]?.description ?? null,
      scriptKind: groupedDeployments[0]?.scriptKind ?? null,
      deprecated: groupedDeployments.some((deployment) => deployment.deprecated),
      deployments: groupedDeployments,
      observedReferences: [],
      deployedAt: groupedDeployments[0]?.deployedAt ?? null,
    }));
  }, [isCodeHashMode, namedScriptDetail, sortedDeployments]);
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

            return (
              group.deployments.some((deployment) => {
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
                const matchesObservedRef = (deployment.observedReferences ?? []).some(
                  (reference) => {
                    const normalizedHashType = normalizeScriptRefHashType(reference.hashType);
                    if (!normalizedHashType) {
                      return false;
                    }

                    return (
                      reference.referenceHash === selectedRef &&
                      (!selectedRefHashType || selectedRefHashType === normalizedHashType)
                    );
                  }
                );
                return matchesTypeRef || matchesDataRef || matchesObservedRef;
              }) ||
              group.observedReferences.some((reference) => {
                const normalizedHashType = normalizeScriptRefHashType(reference.hashType);
                if (!normalizedHashType) {
                  return false;
                }

                return (
                  reference.referenceHash === selectedRef &&
                  (!selectedRefHashType || selectedRefHashType === normalizedHashType)
                );
              })
            );
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
  const selectedVersionGroup = useMemo(
    () => versionGroups.find((group) => group.codeHash === selectedVersion?.codeHash) ?? null,
    [selectedVersion?.codeHash, versionGroups]
  );
  const deploymentsForReferences = useMemo(
    () =>
      selectedDeploymentOutpoint
        ? selectedVersionDeployments.filter(
            (d) =>
              deploymentOutpointKey(d.codeCellTxHash, d.codeCellOutputIndex) ===
              selectedDeploymentOutpoint
          )
        : selectedVersionDeployments,
    [selectedDeploymentOutpoint, selectedVersionDeployments]
  );
  const versionUsageReferences = useMemo<VersionUsageReference[]>(() => {
    const references = new Map<string, VersionUsageReference>();

    for (const deployment of deploymentsForReferences) {
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

    if (!selectedDeploymentOutpoint) {
      for (const deployment of selectedVersionDeployments) {
        collectObservedUsageReferences(references, deployment);
      }

      for (const reference of selectedVersionGroup?.observedReferences ?? []) {
        const normalizedHashType = normalizeScriptRefHashType(reference.hashType);
        if (!normalizedHashType) {
          continue;
        }

        references.set(`${reference.referenceHash}:${normalizedHashType}`, {
          key: `${reference.referenceHash}:${normalizedHashType}`,
          referenceHash: reference.referenceHash,
          hashType: normalizedHashType,
        });
      }
    }

    return Array.from(references.values()).sort((a, b) => a.key.localeCompare(b.key));
  }, [
    deploymentLookup,
    deploymentsForReferences,
    selectedDeploymentOutpoint,
    selectedVersionDeployments,
    selectedVersionGroup,
  ]);
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
  // Usage lists every family version (by_deployment) with its counts; the page waits for it
  // before rendering. A failed request, or a version the response leaves out (a contract
  // violation), is shown as such — never as "0 cells, 0 CKB".
  const versionUsageOf = (stats: VersionUsageStats | undefined, codeHash: string): VersionUsage => {
    if (!isCodeHashMode && usage === undefined) {
      const reason = usageError ? `failed to load: ${usageError.message}` : 'has not loaded';
      return { state: 'failed', message: `The usage of ${name} ${reason}` };
    }
    if (!stats) {
      const source = isCodeHashMode ? 'The code-hash lookup' : `The usage of ${name}`;
      return { state: 'missing', message: `${source} has no entry for version ${codeHash}` };
    }
    return {
      state: 'ready',
      liveCellsCount: stats.liveCellsCount,
      ownedCapacitySum: stats.ownedCapacitySum,
      ownedKnowledgeSum: stats.ownedKnowledgeSum,
    };
  };
  const selectedVersionUsageState = selectedVersion
    ? versionUsageOf(selectedVersionUsage, selectedVersion.codeHash)
    : null;
  // The cell list's total comes from the usage; without it the list states no total.
  const selectedVersionLiveCellsCount =
    selectedVersionUsageState?.state === 'ready'
      ? selectedVersionUsageState.liveCellsCount
      : undefined;
  const selectedVersionCellsData = useMemo(() => {
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
        nextCursorState[reference.key] = encodeCellsByScriptCursor({
          referenceHash: reference.referenceHash,
          hashType: reference.hashType,
          createdAtBlock: lastConsumedCell.createdAtBlock,
          txHash: lastConsumedCell.txHash,
          outputIndex: lastConsumedCell.outputIndex,
          // Without an explicit script kind the request is script_kind=both,
          // whose cursor is phase-composite.
          phase: selectedScriptKindForChart ? undefined : lastConsumedCell.matchedScriptKind,
        });
      }
    }

    const hasMore =
      mergedEntries.length > DEFAULT_PAGE_SIZE ||
      selectedVersionCellsQueries.some((query) => Boolean(query.data?.hasMore));

    return {
      data: pageEntries.map((entry) => entry.cell),
      total: selectedVersionLiveCellsCount,
      hasMore,
      nextCursor:
        hasMore && pageEntries.length > 0 ? encodeVersionCellsCursorState(nextCursorState) : null,
      currentCount: pageEntries.length,
    };
  }, [
    selectedScriptKindForChart,
    selectedVersionCellsQueries,
    selectedVersionLiveCellsCount,
    versionCellsCursorState,
    versionUsageReferences,
  ]);
  const isLoading = isCodeHashMode
    ? isCodeHashLookupLoading || isCodeHashCodeCellsLoading
    : isNamedScriptDetailLoading || isUsageLoading;
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
  const rawRequestFailures = [
    codeHashLookupError ? { source: 'reference lookup', error: codeHashLookupError } : null,
    codeHashCodeCellsError ? { source: 'code-cell query', error: codeHashCodeCellsError } : null,
  ].filter((failure): failure is { source: string; error: Error } => failure !== null);
  const failedRequest = isCodeHashMode
    ? (rawRequestFailures.find((failure) => !isNotFoundApiError(failure.error)) ??
      (codeHashLookup ? rawRequestFailures[0] : null) ??
      null)
    : namedScriptDetailError && !isNotFoundApiError(namedScriptDetailError)
      ? { source: 'script family detail', error: namedScriptDetailError }
      : null;
  if (failedRequest) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <DetailLayout title="Script data unavailable" identity={normalizedCodeHash ?? name}>
            <p role="alert" className="text-negative break-words py-8 text-xs">
              Failed to load {failedRequest.source}: {failedRequest.error.message}
            </p>
          </DetailLayout>
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
          <DetailLayout title="Ambiguous script reference" identity={normalizedCodeHash}>
            <DetailSection title="Conflicting live bytecode">
              <p className="text-text-dim mb-4 text-xs">
                Current live type resolution points to more than one bytecode version.
              </p>
              <ul className="space-y-2 text-xs">
                {codeHashAmbiguity.versionHashes.map((hash) => (
                  <li key={hash} className="break-all">
                    {hash}
                  </li>
                ))}
              </ul>
            </DetailSection>
          </DetailLayout>
        </main>
      </div>
    );
  }
  if (!isCodeHashMode && !namedScriptDetail && isNotFoundApiError(namedScriptDetailError)) {
    if (!name) throw new Error('Script family request failed without a route name');
    return (
      <NotFoundPage
        entity={{
          kind: 'script-family',
          identifier: name,
          checkedEndpoint: `/scripts/${encodeURIComponent(name)}`,
        }}
      />
    );
  }
  if (
    isCodeHashMode &&
    (!deployments || deployments.length === 0) &&
    rawRequestFailures.some((failure) => isNotFoundApiError(failure.error))
  ) {
    const failedEndpoint = isNotFoundApiError(codeHashCodeCellsError)
      ? `/scripts/code-cells?${new URLSearchParams({ code_hash: normalizedCodeHash!, hash_type: codeHashDeploymentQueryHashType! })}`
      : '/scripts/lookup';
    return (
      <NotFoundPage
        entity={{
          kind: 'script',
          identifier: normalizedCodeHash ?? name,
          checkedEndpoint: failedEndpoint,
        }}
      />
    );
  }
  if (
    (!isCodeHashMode && !namedScriptDetail) ||
    (isCodeHashMode && (!deployments || deployments.length === 0))
  ) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <DetailLayout title="No script deployments">
            <p className="text-text-dim py-8 text-xs">
              The script request returned no code-cell deployments.
            </p>
          </DetailLayout>
        </main>
      </div>
    );
  }
  const scriptInfo = isCodeHashMode ? (deployments?.[0] ?? null) : namedScriptDetail;
  const inferredScriptKind = versionUsageEntries.find((d) => d.scriptKind)?.scriptKind;
  const isKnownScript = hasKnownScriptName(scriptInfo?.name);
  const pageTitle = isKnownScript ? (scriptInfo?.name ?? 'Unlabeled Script') : 'Unlabeled Script';
  const pageSubtitle = isKnownScript
    ? scriptInfo?.description
    : 'No imported label for this script yet. The code hash below is the canonical identity.';
  const pageDeprecated = isCodeHashMode
    ? Boolean(scriptInfo && 'deprecated' in scriptInfo && scriptInfo.deprecated)
    : versionGroups.some((group) => group.deprecated);
  const pageHash =
    isCodeHashMode && scriptInfo && 'codeHash' in scriptInfo ? scriptInfo.codeHash : undefined;
  const pageWebsite = scriptInfo?.website ?? null;
  const deploymentCellsByOutpoint = new Map(
    deploymentCellQueries
      .map((query) => query.data)
      .filter((cell): cell is NonNullable<typeof cell> => cell != null)
      .map((cell) => [`${cell.txHash}:${cell.outputIndex}`, cell])
  );
  const versionRows = versionGroups.map((group) => {
    const stats = usageByCodeHash.get(group.codeHash);
    const firstDeployment =
      group.deployments.length > 0
        ? group.deployments.reduce((earliest, deployment) => {
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
          }, group.deployments[0])
        : null;
    const firstDeploymentOutpointKey = deploymentOutpointKey(
      firstDeployment?.codeCellTxHash,
      firstDeployment?.codeCellOutputIndex
    );

    return {
      ...group,
      firstDeployment,
      firstDeploymentCell: firstDeploymentOutpointKey
        ? deploymentCellsByOutpoint.get(firstDeploymentOutpointKey)
        : undefined,
      deploymentsCount: group.deployments.length,
      usage: versionUsageOf(stats, group.codeHash),
      scriptKind:
        (stats?.scriptKind as 'lock' | 'type' | undefined) ??
        (group.scriptKind as 'lock' | 'type' | undefined),
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
  const isVersionCellsLoading = selectedVersionCellsQueries.some((query) => query.isLoading);

  const handleVersionClick = (versionRow: (typeof versionRows)[number]) => {
    if (selectedVersion?.codeHash !== versionRow.codeHash) {
      setSelectedVersion({
        codeHash: versionRow.codeHash,
        scriptKind: versionRow.scriptKind,
      });
      setSelectedDeploymentOutpoint(null);
      cellsPagination.reset();
    }
  };
  const isSelected = (versionRow: (typeof versionRows)[number]) =>
    selectedVersion?.codeHash === versionRow.codeHash;
  const renderDeploymentStatus = (
    codeCell: (typeof selectedVersionDeploymentRows)[number]['codeCell']
  ) =>
    codeCell ? (
      <span className="text-text-dim">{codeCell.status === 'live' ? 'Live' : 'Consumed'}</span>
    ) : (
      <span className="text-text-dim">Status unavailable</span>
    );

  const namedVersion = namedScriptDetail?.versions.find(
    (version) => version.versionHash === selectedVersion?.codeHash
  );
  const custodyByOutpoint = new Map(
    namedVersion?.deployments.map((deployment) => [
      `${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`,
      deployment.custody,
    ]) ?? []
  );
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <DetailLayout
          title={pageTitle}
          identity={isCodeHashMode ? pageHash : pageSubtitle}
          status={
            <span className="text-text-dim text-xs">
              {inferredScriptKind ?? namedScriptDetail?.scriptKind ?? 'role unregistered'}
              {pageDeprecated ? ' · Deprecated' : ''}
            </span>
          }
          actions={
            pageWebsite && (
              <a href={pageWebsite} className="text-emphasis text-xs hover:underline">
                Website ↗
              </a>
            )
          }
          facts={[
            {
              label: 'Live cells',
              value: !isCodeHashMode
                ? formatNumber(namedScriptDetail!.liveCellsCount)
                : usage
                  ? formatNumber(usage.liveCellsCount)
                  : 'unavailable',
            },
            {
              label: 'CKB in its cells',
              value: !isCodeHashMode
                ? formatCkbCompact(namedScriptDetail!.ownedCapacitySum).value
                : usage
                  ? formatCkbCompact(usage.ownedCapacitySum).value
                  : 'unavailable',
            },
            {
              label: 'Code versions',
              value: !isCodeHashMode
                ? formatNumber(namedScriptDetail!.versionsCount)
                : formatNumber(versionRows.length),
            },
          ]}
          tier2={
            <>
              <DetailSection
                title="Code versions"
                actions={
                  <span className="text-text-dim text-xs">
                    Select a version to inspect its deployment and cells
                  </span>
                }
              >
                <ListTable
                  label="Code versions"
                  columns={[
                    {
                      key: 'version',
                      label: 'Version · bytecode hash',
                      minWidth: 225,
                      width: '30%',
                      render: (row) => (
                        <button
                          type="button"
                          onClick={() => handleVersionClick(row)}
                          className="text-emphasis break-all text-left font-mono hover:underline"
                        >
                          {row.codeHash}
                        </button>
                      ),
                    },
                    {
                      key: 'deployed',
                      label: 'First observed',
                      minWidth: 130,
                      width: '17%',
                      render: (row) =>
                        row.firstDeploymentCell ? (
                          <Link
                            href={`/blocks/${row.firstDeploymentCell.createdAtBlock}`}
                            className="text-emphasis"
                          >
                            #{formatNumber(row.firstDeploymentCell.createdAtBlock)}
                          </Link>
                        ) : (
                          'No deployment observed'
                        ),
                    },
                    {
                      key: 'deployments',
                      label: 'Code cells',
                      minWidth: 110,
                      width: '12%',
                      numeric: true,
                      render: (row) => row.deploymentsCount.toLocaleString(),
                    },
                    {
                      key: 'cells',
                      label: 'Live cells using it',
                      minWidth: 120,
                      width: '17%',
                      numeric: true,
                      render: (row) => <VersionUsageCell usage={row.usage} field="cells" />,
                    },
                    {
                      key: 'capacity',
                      label: 'CKB in its cells',
                      minWidth: 150,
                      width: '24%',
                      numeric: true,
                      render: (row) => <VersionUsageCell usage={row.usage} field="capacity" />,
                    },
                  ]}
                  rows={versionRows}
                  rowKey={(row) => row.codeHash}
                  rowClassName={(row) => (isSelected(row) ? 'bg-base-elevated/30' : '')}
                  mobileRow={(row) => (
                    <button
                      type="button"
                      onClick={() => handleVersionClick(row)}
                      className="w-full space-y-2 text-left text-xs"
                    >
                      <span className="text-emphasis break-all">{row.codeHash}</span>
                      <span className="text-text-dim block">
                        {row.deploymentsCount} code cells ·{' '}
                        <VersionUsageCell usage={row.usage} field="cells" /> live cells
                      </span>
                    </button>
                  )}
                />
              </DetailSection>
              {selectedVersion && (
                <DetailSection title="Code cell deployments">
                  <ListTable
                    label="Code cell deployments"
                    rows={selectedVersionDeploymentRows}
                    rowKey={(row) => row.outpointKey ?? row.deployment.codeHash}
                    columns={[
                      {
                        key: 'outpoint',
                        label: 'Outpoint',
                        minWidth: 235,
                        width: '28%',
                        render: (row) =>
                          row.outpointKey ? (
                            <Link
                              href={`/cell/${row.deployment.codeCellTxHash}-${row.deployment.codeCellOutputIndex}`}
                              className="text-emphasis break-all"
                            >
                              {row.outpointKey}
                            </Link>
                          ) : (
                            'No observed outpoint'
                          ),
                      },
                      {
                        key: 'status',
                        label: 'Status',
                        minWidth: 90,
                        width: '12%',
                        render: (row) => renderDeploymentStatus(row.codeCell),
                      },
                      {
                        key: 'holder',
                        label: 'Code cell holder',
                        minWidth: 220,
                        width: '36%',
                        render: (row) => {
                          const custody = row.outpointKey
                            ? custodyByOutpoint.get(row.outpointKey)
                            : undefined;
                          return custody ? (
                            <ScriptCustodyEvidence custody={custody} />
                          ) : (
                            <span className="text-text-dim">Custody evidence unavailable</span>
                          );
                        },
                      },
                      {
                        key: 'block',
                        label: 'Created at',
                        minWidth: 120,
                        width: '24%',
                        render: (row) =>
                          row.codeCell ? (
                            <Link
                              href={`/blocks/${row.codeCell.createdAtBlock}`}
                              className="text-emphasis"
                            >
                              #{formatNumber(row.codeCell.createdAtBlock)}
                            </Link>
                          ) : (
                            'unavailable'
                          ),
                      },
                    ]}
                    mobileRow={(row) => (
                      <div className="space-y-2 text-xs">
                        <Link
                          href={`/cell/${row.deployment.codeCellTxHash}-${row.deployment.codeCellOutputIndex}`}
                          className="text-emphasis break-all"
                        >
                          {row.outpointKey}
                        </Link>
                        <div>{renderDeploymentStatus(row.codeCell)}</div>
                        {row.outpointKey && custodyByOutpoint.get(row.outpointKey) && (
                          <ScriptCustodyEvidence
                            custody={custodyByOutpoint.get(row.outpointKey)!}
                          />
                        )}
                      </div>
                    )}
                    empty="No deployments observed for this code version"
                  />
                </DetailSection>
              )}
              {selectedVersion && (
                <DetailSection
                  title="Its cells"
                  actions={
                    <span className="text-text-dim text-xs">
                      Live cells using the selected code version
                    </span>
                  }
                >
                  {isVersionCellsLoading ? (
                    <p className="text-text-dim py-6 text-xs">Loading cells...</p>
                  ) : (
                    <ListTable
                      label="Its cells"
                      rows={selectedVersionCellsData.data}
                      rowKey={(cell) => `${cell.txHash}:${cell.outputIndex}`}
                      columns={[
                        {
                          key: 'cell',
                          label: 'Cell',
                          minWidth: 220,
                          width: '26%',
                          render: (cell) => (
                            <Link
                              href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                              className="text-emphasis break-all"
                            >
                              {cell.txHash}:{cell.outputIndex}
                            </Link>
                          ),
                        },
                        {
                          key: 'owner',
                          label: 'Owner',
                          minWidth: 200,
                          width: '18%',
                          render: (cell) => <CellOwner cell={cell} />,
                        },
                        {
                          key: 'type',
                          label: 'Type reference',
                          minWidth: 165,
                          width: '15%',
                          render: (cell) => <CellTypeReference cell={cell} />,
                        },
                        {
                          key: 'capacity',
                          label: 'Capacity',
                          minWidth: 120,
                          width: '11%',
                          numeric: true,
                          render: (cell) => <Capacity value={cell.capacity} />,
                        },
                        {
                          key: 'knowledge',
                          label: 'Occupied',
                          minWidth: 120,
                          width: '11%',
                          numeric: true,
                          render: (cell) => <CellOccupiedValue cell={cell} />,
                        },
                        {
                          key: 'created',
                          label: 'Created',
                          minWidth: 95,
                          width: '9%',
                          numeric: true,
                          render: (cell) => (
                            <Link href={`/blocks/${cell.createdAtBlock}`} className="text-emphasis">
                              #{cell.createdAtBlock.toLocaleString()}
                            </Link>
                          ),
                        },
                        {
                          key: 'anatomy',
                          label: 'Cell anatomy',
                          minWidth: 180,
                          width: '10%',
                          render: (cell) => <CellAnatomyBar cell={cell} />,
                        },
                      ]}
                      mobileRow={(cell) => (
                        <div className="min-w-0 space-y-2 text-xs">
                          <Link
                            href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                            className="text-emphasis break-all"
                          >
                            {cell.txHash}:{cell.outputIndex}
                          </Link>
                          <div>
                            Owner: <CellOwner cell={cell} />
                          </div>
                          <div>
                            Type: <CellTypeReference cell={cell} />
                          </div>
                          <div>
                            <Capacity value={cell.capacity} /> · occupied{' '}
                            <CellOccupiedValue cell={cell} />
                          </div>
                          <div>
                            Created{' '}
                            <Link href={`/blocks/${cell.createdAtBlock}`} className="text-emphasis">
                              #{cell.createdAtBlock.toLocaleString()}
                            </Link>
                          </div>
                          <CellAnatomyBar cell={cell} />
                        </div>
                      )}
                      empty="No live cells currently use this version"
                    />
                  )}
                  {selectedVersionCellsData.data.length > 0 && (
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
                  )}
                </DetailSection>
              )}
            </>
          }
        >
          <DetailSection title="Capacity history">
            <p className="text-text-dim mb-3 text-xs">
              Live capacity and common knowledge across{' '}
              {isCodeHashMode ? 'this code reference' : 'all registered family references'}.
            </p>
            <CapacityRangeSelector value={capacityRange} onChange={setCapacityRange} />
            {(isCodeHashMode ? isSelectedCapacityChartLoading : isFamilyCapacityChartLoading) ? (
              <p className="text-text-dim py-6 text-xs">Loading capacity history...</p>
            ) : (isCodeHashMode ? selectedCapacityChart : familyCapacityChart)?.data.length ? (
              <ScriptCapacityEvidenceChart
                chart={(isCodeHashMode ? selectedCapacityChart : familyCapacityChart)!}
                markers={namedScriptDetail?.history ?? []}
              />
            ) : (
              <p className="text-text-dim py-6 text-xs">No capacity history yet</p>
            )}
            {!isCodeHashMode && familyCapacityChartError && (
              <p role="alert" className="text-negative text-xs">
                Capacity history unavailable: {String(familyCapacityChartError)}
              </p>
            )}
          </DetailSection>
          {!isCodeHashMode && namedScriptDetail?.codeClasses && (
            <ScriptCodeClassSummary classes={namedScriptDetail.codeClasses} />
          )}
          {!isCodeHashMode && namedScriptDetail?.references && (
            <DetailSection title="Registered references">
              <ListTable
                label="Registered references"
                rows={namedScriptDetail.references}
                rowKey={(row) => `${row.hashType}:${row.referenceHash}`}
                columns={[
                  {
                    key: 'reference',
                    label: 'Reference',
                    minWidth: 250,
                    width: '35%',
                    render: (row) => (
                      <span className="text-text-bright break-all">
                        {row.hashType} · {row.referenceHash}
                      </span>
                    ),
                  },
                  {
                    key: 'code',
                    label: 'Current code version',
                    minWidth: 210,
                    width: '30%',
                    render: (row) => (
                      <span className="text-text-dim break-all">
                        {row.currentVersionHash ?? 'Retired · no current code cell'}
                      </span>
                    ),
                  },
                  {
                    key: 'custody',
                    label: 'Code cell holder',
                    minWidth: 235,
                    width: '35%',
                    render: (row) =>
                      row.custody ? (
                        <ScriptCustodyEvidence custody={row.custody} />
                      ) : (
                        <span className="text-text-dim">Evidence unavailable</span>
                      ),
                  },
                ]}
                mobileRow={(row) => (
                  <div className="space-y-2 text-xs">
                    <div className="break-all">
                      {row.hashType} · {row.referenceHash}
                    </div>
                    <div className="break-all">
                      Current code: {row.currentVersionHash ?? 'retired'}
                    </div>
                    {row.custody ? (
                      <ScriptCustodyEvidence custody={row.custody} />
                    ) : (
                      'Evidence unavailable'
                    )}
                  </div>
                )}
              />
            </DetailSection>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
