'use client';
import React, { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueries, keepPreviousData } from '@tanstack/react-query';
import { useSearchParams } from '@/src/navigation';
import Link from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { NotFoundPage } from '@/components/not-found-page';
import { Pager } from '@/components/ui/pager';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { parseShannons } from '@/lib/ckb-decimal';
import { CellBar } from '@/components/ui/cell-bar';
import { RangeSelect } from '@/components/ui/range-select';
import { DetailSection, DetailLayout } from '@/components/ui/detail-layout';
import { DetailSkeleton } from '@/components/ui/detail-skeleton';
import { CELL_GAP_PX, ListTable } from '@/components/ui/list-table';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import {
  UNAVAILABLE,
  WIDEST_BLOCK_NUMBER,
  WIDEST_CELL_AMOUNT_LINE,
  WIDEST_CKB_HELD_COMPACT,
  WIDEST_ROW_COUNT,
  WIDEST_VERSION_CODE_CELLS,
} from '@/lib/list-bounds';
import {
  ScriptCustodyEvidence,
  ScriptCapacityEvidenceChart,
} from '@/components/scripts/script-code-evidence';
import { ScriptFamilyPage } from '@/components/scripts/script-family-page';
import type { NewestCellsState } from '@/components/scripts/script-newest-cells';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, isNotFoundApiError } from '@/lib/api';
import { isBadRequestApiError } from '@/lib/api-errors';
import { ReadFailure } from '@/components/read-failure';
import { isAwaited } from '@/lib/query-state';
import {
  CAPACITY_RANGE_OPTIONS,
  getCapacityRangeParams,
  CapacityRangeKey,
} from '@/lib/capacity-range';
import { ENTITY_PAGE_SIZE, mergeAsked, rankedPageAt } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import {
  encodeCellsByScriptCursor,
  getScriptRefQueryHashType,
  normalizeScriptRefHashType,
  type ScriptRefHashType,
} from '@/lib/script-ref';
import { formatCapacity, formatNumber } from '@/lib/utils';
import type { Cell, KnownScript, ScriptFamilyDetail, ScriptLookupInfo } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { NEWEST_CELLS_COUNT, mergeNewestFirst, newestCellsKind } from '@/lib/script-newest-cells';

/**
 * What a code reference's tables hold (readability F3-e): each column's widest value measured at
 * its step through columnPx, with the 16 px gap after every column but the last. A full id here
 * breaks anywhere (`break-all`), so its column is a flexible one: at its minimum a line of 22
 * characters (a hash in three), the rest of the container beyond the table's sum.
 */
const small = (text: string) => textPx(text.length, fontPx('small'));
const meta = (text: string) => textPx(text.length, fontPx('meta'));
const ID_LINE_PX = columnPx(textPx(22, fontPx('small')) + CELL_GAP_PX);
const BLOCK_PX = columnPx(small(WIDEST_BLOCK_NUMBER) + CELL_GAP_PX);
/**
 * An amount on one line with its unit (WIDEST_CELL_AMOUNT_LINE, "999,999.99999999 CKB"); a larger
 * one breaks before its decimals (`wrap`). Lane P2's third round (item 11): the bound is
 * lib/list-bounds.ts's, as is a version's compact CKB (WIDEST_CKB_HELD_COMPACT, "999.99M").
 */
const AMOUNT_LINE_PX = columnPx(small(WIDEST_CELL_AMOUNT_LINE) + meta(' CKB') + CELL_GAP_PX);

function CellOccupiedValue({
  cell,
  form,
}: {
  cell: Cell;
  /**
   * The table's numeric column aligns its points (`column`); a phone row writes the summary (lane
   * P2's follow-up, item 5: one form for both was the column's).
   */
  form: 'column' | 'summary';
}) {
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
        <CkbAmount
          form={form}
          shannons={BigInt(cell.virtualCommonKnowledgeSize)}
          signed={false}
          tone="plain"
          wrap
        />
      </span>
    );
  }
  return (
    <CkbAmount
      form={form}
      shannons={BigInt(cell.commonKnowledgeSize)}
      signed={false}
      tone="plain"
      wrap
    />
  );
}

function CellOwner({ cell }: { cell: Cell }) {
  return cell.address ? (
    <Link href={`/address/${cell.address}`} look="inline" className="break-all">
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
      look="inline"
      className="break-all"
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
  // A column of a table of versions: compact, cut (R5), the exact value in its title.
  return (
    <span data-testid="version-usage" data-state="ready">
      <CkbAmount
        form="compact"
        shannons={parseShannons(usage.ownedCapacitySum, 'script version CKB')}
        signed={false}
        tone="inherit"
      />
    </span>
  );
}

const UNKNOWN_SCRIPT_NAME = 'unknown';
/** A script page is a topic page: its cells list shows 5 rows, paged (hierarchy plan R8). */
const FAMILY_CELLS_PAGE_SIZE = 5;

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
  // The board pages a family's cells 12 at a time; a raw reference page keeps the list size.
  const listPageSize = usePageSize(ENTITY_PAGE_SIZE);
  const cellsPageSize = isCodeHashMode ? listPageSize : FAMILY_CELLS_PAGE_SIZE;
  // a change of page size at md starts the walk again (lane c2-finish, 1d), and so does a change of the list it pages (lane cd-fixes-controls): the version
  // (what the cells' query key holds of the list: a code-hash page's version and its kind, a
  // family page's name)
  const cellsPagination = useCursorPagination(cellsPageSize, {
    list: isCodeHashMode
      ? [
          selectedVersion?.codeHash,
          selectedVersion?.scriptKind === 'lock' || selectedVersion?.scriptKind === 'type'
            ? selectedVersion.scriptKind
            : undefined,
        ]
      : [name],
  });
  const capacityRangeParams = getCapacityRangeParams(capacityRange);
  const namedScriptDetailRead = useQuery({
    queryKey: ['script-family-detail', name],
    queryFn: () => {
      if (!name) throw new Error('script name is required');
      return api.getScriptFamilyDetail(name);
    },
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const { data: namedScriptDetail, error: namedScriptDetailError } = namedScriptDetailRead;
  const usageRead = useQuery({
    queryKey: ['script-usage', name],
    queryFn: () => {
      if (!name) throw new Error('script name is required');
      return api.getScriptUsage(name);
    },
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const { data: usage, error: usageError } = usageRead;
  const familyCapacityChartRead = useQuery({
    queryKey: ['script-capacity-chart', 'family', name, capacityRange],
    queryFn: () => api.getScriptCapacityChart(name!, capacityRangeParams ?? undefined),
    enabled: Boolean(name) && !isCodeHashMode,
  });
  const { data: familyCapacityChart, error: familyCapacityChartError } = familyCapacityChartRead;
  const codeHashLookupRead = useQuery({
    queryKey: ['script-lookup-detail', normalizedCodeHash],
    queryFn: async () => {
      const result = await api.lookupScripts([normalizedCodeHash!]);
      return result[normalizedCodeHash!] ?? null;
    },
    enabled: isCodeHashMode,
    staleTime: Infinity,
  });
  const { data: codeHashLookup, error: codeHashLookupError } = codeHashLookupRead;
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
  const codeHashCodeCellsRead = useQuery({
    queryKey: ['script-code-cells-unified', normalizedCodeHash, codeHashDeploymentQueryHashType],
    queryFn: () => api.getCodeCells(normalizedCodeHash!, codeHashDeploymentQueryHashType!),
    enabled: Boolean(isCodeHashMode && normalizedCodeHash && codeHashDeploymentQueryHashType),
    staleTime: Infinity,
  });
  const { data: codeHashCodeCells, error: codeHashCodeCellsError } = codeHashCodeCellsRead;
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
  // Named family: "Its cells" lists every cell of every registered reference (SC-S10), never only
  // the selected version's. A raw code-hash page keeps the reference(s) of its one version.
  const cellReferences = useMemo<VersionUsageReference[]>(() => {
    if (isCodeHashMode) return versionUsageReferences;
    const references = new Map<string, VersionUsageReference>();
    for (const reference of namedScriptDetail?.references ?? []) {
      const hashType = normalizeScriptRefHashType(reference.hashType);
      if (!hashType)
        throw new Error(
          `${namedScriptDetail!.name}: registered reference ${reference.referenceHash} has hash type ${reference.hashType}`
        );
      const key = `${reference.referenceHash}:${hashType}`;
      references.set(key, { key, referenceHash: reference.referenceHash, hashType });
    }
    return Array.from(references.values()).sort((a, b) => a.key.localeCompare(b.key));
  }, [isCodeHashMode, namedScriptDetail, versionUsageReferences]);
  const cellsScriptKind = isCodeHashMode
    ? undefined
    : namedScriptDetail?.scriptKind === 'lock' || namedScriptDetail?.scriptKind === 'type'
      ? namedScriptDetail.scriptKind
      : undefined;
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
  const selectedCapacityChartRead = useQuery({
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
    enabled: isCodeHashMode && Boolean(selectedVersion),
  });
  const selectedCapacityChart = selectedCapacityChartRead.data;
  const cellsKind = isCodeHashMode ? selectedScriptKindForChart : cellsScriptKind;
  const selectedVersionCellsQueries = useQueries({
    queries: cellReferences.map((reference) => ({
      queryKey: [
        'script-version-cells',
        isCodeHashMode ? selectedVersion?.codeHash : `family:${name}`,
        reference.referenceHash,
        reference.hashType,
        cellsKind,
        versionCellsCursorState[reference.key] ?? null,
        cellsPageSize,
      ],
      // asked by the walk (lane cd-fixes-controls, item 1): each reference's page carries the
      // walk's page it was read for, and the merged page is asked where all of them were
      queryFn: cellsPagination.ask(() =>
        api.getCellsByScriptRef({
          codeHash: reference.referenceHash,
          hashType: reference.hashType,
          scriptKind: cellsKind,
          limit: cellsPageSize,
          cursor: versionCellsCursorState[reference.key],
        })
      ),
      enabled: isCodeHashMode ? Boolean(selectedVersion) : Boolean(namedScriptDetail),
      placeholderData: keepPreviousData,
    })),
  });
  // Lane S: the family's newest live cells — one index (ruling 2), each registered reference read
  // newest block first by the API, the pages merged on its key order.
  const newestKind = namedScriptDetail ? newestCellsKind(namedScriptDetail.scriptKind) : null;
  const newestQueries = useQueries({
    queries: cellReferences.map((reference) => ({
      queryKey: [
        'script-newest-cells',
        name,
        reference.referenceHash,
        reference.hashType,
        newestKind,
      ],
      queryFn: () =>
        api.getCellsByScriptRef({
          codeHash: reference.referenceHash,
          hashType: reference.hashType,
          scriptKind: newestKind!,
          limit: NEWEST_CELLS_COUNT,
          order: 'desc',
        }),
      enabled: !isCodeHashMode && Boolean(namedScriptDetail) && newestKind !== null,
    })),
  });
  const newest = ((): NewestCellsState => {
    const failed = newestQueries.find((query) => query.error)?.error ?? null;
    if (failed || newestKind === null || newestQueries.some((query) => !query.data))
      return { kind: newestKind, rows: [], loading: !failed && newestKind !== null, error: failed };
    try {
      const rows = mergeNewestFirst(
        newestKind,
        newestQueries.map((query, i) => ({
          reference: `${cellReferences[i].referenceHash} ${cellReferences[i].hashType}`,
          cells: query.data!.data,
        })),
        NEWEST_CELLS_COUNT
      );
      return { kind: newestKind, rows, loading: false, error: null };
    } catch (error) {
      // An invariant the API broke (a page out of order, a row from the other index): said in
      // the section, never shown as if it were the newest cells.
      return { kind: newestKind, rows: [], loading: false, error: error as Error };
    }
  })();
  const usageByCodeHash = new Map(
    versionUsageEntries.map((deployment) => [deployment.codeHash, deployment])
  );
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
  const selectedVersionCellsData = useMemo(() => {
    const merged = new Map<
      string,
      {
        cell: NonNullable<(typeof selectedVersionCellsQueries)[number]['data']>['data'][number];
        sourceKeys: Set<string>;
      }
    >();

    for (const [index, query] of selectedVersionCellsQueries.entries()) {
      const reference = cellReferences[index];
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
    const pageEntries = mergedEntries.slice(0, cellsPageSize);
    const includedOutpoints = new Set(
      pageEntries.map((entry) => `${entry.cell.txHash}:${entry.cell.outputIndex}`)
    );
    const nextCursorState: Record<string, string | undefined> = {
      ...versionCellsCursorState,
    };

    for (const [index, query] of selectedVersionCellsQueries.entries()) {
      const reference = cellReferences[index];
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
          phase: cellsKind ? undefined : lastConsumedCell.matchedScriptKind,
        });
      }
    }

    const hasMore =
      mergedEntries.length > cellsPageSize ||
      selectedVersionCellsQueries.some((query) => Boolean(query.data?.hasMore));

    return mergeAsked(
      selectedVersionCellsQueries.map((query) => query.data),
      {
        data: pageEntries.map((entry) => entry.cell),
        // the page the references' pages are merged into: this many rows, counting none (the
        // usage's live cells are another read, at another tip: lane c2-finish, 1c)
        limit: cellsPageSize,
        hasMore,
        nextCursor:
          hasMore && pageEntries.length > 0 ? encodeVersionCellsCursorState(nextCursorState) : null,
      }
    );
  }, [
    cellsKind,
    cellsPageSize,
    selectedVersionCellsQueries,
    versionCellsCursorState,
    cellReferences,
  ]);
  // The page waits while its reads are awaited: fetching, or paused (offline, or a hidden tab
  // between retries). A disabled read (the code cells of an ambiguous reference) is not awaited.
  // The version's cells, oldest first: the pager's pages mode by its rows and its "more" (its
  // merged page counts none), null for one page.
  const versionCellsAt =
    selectedVersionCellsData && rankedPageAt('cells', selectedVersionCellsData, cellsPagination);
  const isAwaiting = isCodeHashMode
    ? isAwaited(codeHashLookupRead) || isAwaited(codeHashCodeCellsRead)
    : isAwaited(namedScriptDetailRead) || isAwaited(usageRead);
  if (isAwaiting) {
    return (
      <div className="min-h-screen">
        <main className="container mx-auto py-8">
          <DetailSkeleton />
        </main>
      </div>
    );
  }
  const rawRequestFailures = [codeHashLookupError, codeHashCodeCellsError].filter(
    (error): error is Error => error !== null
  );
  const failedRequest = isCodeHashMode
    ? (rawRequestFailures.find((error) => !isNotFoundApiError(error)) ??
      (codeHashLookup ? rawRequestFailures[0] : null) ??
      null)
    : namedScriptDetailError && !isNotFoundApiError(namedScriptDetailError)
      ? namedScriptDetailError
      : null;
  if (isBadRequestApiError(failedRequest)) {
    return (
      <NotFoundPage
        notAnId={{
          kind: isCodeHashMode ? 'script' : 'script-family',
          reason: failedRequest.apiMessage,
        }}
      />
    );
  }
  if (failedRequest) {
    return (
      <div className="min-h-screen">
        <main className="container mx-auto py-8">
          <ReadFailure what="script" error={failedRequest} />
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
      <div className="min-h-screen">
        <main className="container mx-auto py-8">
          <DetailLayout title="Ambiguous script reference" identity={normalizedCodeHash}>
            <DetailSection
              title="Conflicting live bytecode"
              args={formatCount(codeHashAmbiguity.versionHashes.length, 'version')}
            >
              <p className="text-text text-small mb-4">
                Current live type resolution points to more than one bytecode version.
              </p>
              <ul className="text-small space-y-2">
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
    rawRequestFailures.some((error) => isNotFoundApiError(error))
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
      <div className="min-h-screen">
        <main className="container mx-auto py-8">
          <DetailLayout title="No script deployments">
            <p className="text-text-dim text-small py-8">
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
  const isVersionCellsLoading = selectedVersionCellsQueries.some((query) => isAwaited(query));

  const handleVersionClick = (versionRow: (typeof versionRows)[number]) => {
    if (selectedVersion?.codeHash !== versionRow.codeHash) {
      setSelectedVersion({
        codeHash: versionRow.codeHash,
        scriptKind: versionRow.scriptKind,
      });
      setSelectedDeploymentOutpoint(null);
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

  if (!isCodeHashMode && namedScriptDetail) {
    const codeCells = new Map(
      sortedDeployments.flatMap((deployment, index) => {
        const key = deploymentOutpointKey(
          deployment.codeCellTxHash,
          deployment.codeCellOutputIndex
        );
        const query = deploymentCellQueries[index];
        return key ? [[key, { cell: query?.data, error: query?.error ?? null }] as const] : [];
      })
    );
    return (
      <ScriptFamilyPage
        family={namedScriptDetail}
        deprecated={pageDeprecated}
        chart={familyCapacityChart}
        chartLoading={isAwaited(familyCapacityChartRead)}
        chartError={familyCapacityChartError}
        capacityRange={capacityRange}
        onCapacityRange={setCapacityRange}
        versionUsage={(versionHash) => (
          <VersionUsageCell
            usage={versionUsageOf(usageByCodeHash.get(versionHash), versionHash)}
            field="cells"
          />
        )}
        codeCells={codeCells}
        newest={newest}
        cells={{
          read: selectedVersionCellsData,
          walk: cellsPagination,
          loading: isVersionCellsLoading,
        }}
      />
    );
  }
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
    <div className="min-h-screen">
      <main className="container mx-auto py-8">
        <DetailLayout
          title={pageTitle}
          documentTitle={[pageTitle, 'script']}
          identity={isCodeHashMode ? pageHash : pageSubtitle}
          status={
            <span className="text-text-dim text-small">
              {inferredScriptKind ?? namedScriptDetail?.scriptKind ?? 'role unregistered'}
              {pageDeprecated ? ' · Deprecated' : ''}
            </span>
          }
          actions={
            pageWebsite && (
              <ExternalLink href={pageWebsite} look="onward" className="text-small">
                Website
              </ExternalLink>
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
              // A stat tile: the summary form (R5), the exact value in its title.
              value:
                !isCodeHashMode || usage ? (
                  <CkbAmount
                    form="summary"
                    shannons={parseShannons(
                      !isCodeHashMode
                        ? namedScriptDetail!.ownedCapacitySum
                        : usage!.ownedCapacitySum,
                      'script CKB in its cells'
                    )}
                    signed={false}
                    tone="inherit"
                    unit={false}
                  />
                ) : (
                  'unavailable'
                ),
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
                  <span className="text-text-dim text-small">
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
                      // the full hash, breaking anywhere: a flexible column (F3-e: it was 225 and
                      // 30 %)
                      minWidth: ID_LINE_PX,
                      width: 'auto',
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
                      // the block; "No deployment observed" breaks between its words (F3-e: it was
                      // 130 and 17 %)
                      minWidth: BLOCK_PX,
                      render: (row) =>
                        row.firstDeploymentCell ? (
                          <Link
                            href={`/blocks/${row.firstDeploymentCell.createdAtBlock}`}
                            look="inline"
                          >
                            {formatBlockNumber(row.firstDeploymentCell.createdAtBlock)}
                          </Link>
                        ) : (
                          'No deployment observed'
                        ),
                    },
                    {
                      key: 'deployments',
                      label: 'Code cells',
                      // a version's code cells, two digits (WIDEST_VERSION_CODE_CELLS; lane P2's
                      // follow-up, item 7: it read WIDEST_COUNT, a block's or a transaction's
                      // count), under its head's words (F3-e: it was 110 and 12 %)
                      minWidth: columnPx(
                        Math.max(small(WIDEST_VERSION_CODE_CELLS), meta('cells')) + CELL_GAP_PX
                      ),
                      numeric: true,
                      render: (row) => row.deploymentsCount.toLocaleString(),
                    },
                    {
                      key: 'cells',
                      label: 'Live cells using it',
                      // eight digits of live cells, or "unavailable" (F3-e: it was 120 and 17 %)
                      minWidth: columnPx(
                        Math.max(small(WIDEST_ROW_COUNT), small(UNAVAILABLE)) + CELL_GAP_PX
                      ),
                      numeric: true,
                      render: (row) => <VersionUsageCell usage={row.usage} field="cells" />,
                    },
                    {
                      key: 'capacity',
                      label: 'CKB in its cells',
                      // the compact CKB and its unit, "999.99M CKB", or "unavailable" (the wider);
                      // the last column, no gap after it (F3-e: it was 150 and 24 %)
                      minWidth: columnPx(
                        Math.max(small(WIDEST_CKB_HELD_COMPACT) + meta(' CKB'), small(UNAVAILABLE))
                      ),
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
                      className="text-small w-full space-y-2 text-left"
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
                        // the full out point, breaking anywhere: a flexible column (F3-e: it was
                        // 235 and 28 %)
                        minWidth: ID_LINE_PX,
                        width: 'auto',
                        render: (row) =>
                          row.outpointKey ? (
                            <Link
                              href={`/cell/${row.deployment.codeCellTxHash}-${row.deployment.codeCellOutputIndex}`}
                              look="key"
                              className="break-all"
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
                        // "Consumed", or "Status unavailable" breaking between its words (F3-e: it
                        // was 90 and 12 %)
                        minWidth: columnPx(
                          Math.max(small('Consumed'), small(UNAVAILABLE)) + CELL_GAP_PX
                        ),
                        render: (row) => renderDeploymentStatus(row.codeCell),
                      },
                      {
                        key: 'holder',
                        label: 'Code cell holder',
                        // the holder's evidence, sentences and ids that break: a flexible column
                        // in lines of 22 characters (F3-e: it was 220 and 36 %)
                        minWidth: ID_LINE_PX,
                        width: 'auto',
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
                        // the block, or "unavailable"; the last column, no gap after it (F3-e: it
                        // was 120 and 24 %)
                        minWidth: columnPx(
                          Math.max(small(WIDEST_BLOCK_NUMBER), small(UNAVAILABLE))
                        ),
                        render: (row) =>
                          row.codeCell ? (
                            <Link href={`/blocks/${row.codeCell.createdAtBlock}`} look="inline">
                              {formatBlockNumber(row.codeCell.createdAtBlock)}
                            </Link>
                          ) : (
                            'unavailable'
                          ),
                      },
                    ]}
                    mobileRow={(row) => (
                      <div className="text-small space-y-2">
                        <Link
                          href={`/cell/${row.deployment.codeCellTxHash}-${row.deployment.codeCellOutputIndex}`}
                          look="key"
                          className="break-all"
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
                    empty="No deployments observed for this code version."
                  />
                </DetailSection>
              )}
              {selectedVersion && (
                <DetailSection
                  title="Its cells"
                  actions={
                    <span className="text-text-dim text-small">
                      Live cells using the selected code version
                    </span>
                  }
                >
                  {isVersionCellsLoading ? (
                    <p className="text-text-dim text-small py-6">Loading cells…</p>
                  ) : (
                    <ListTable
                      label="Its cells"
                      rows={selectedVersionCellsData?.data ?? []}
                      rowKey={(cell) => `${cell.txHash}:${cell.outputIndex}`}
                      columns={[
                        {
                          key: 'cell',
                          label: 'Cell',
                          // the full out point, breaking anywhere: a flexible column (F3-e: it was
                          // 220 and 26 %)
                          minWidth: ID_LINE_PX,
                          width: 'auto',
                          render: (cell) => (
                            <Link
                              href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                              look="key"
                              className="break-all"
                            >
                              {cell.txHash}
                              <OutpointIndex index={cell.outputIndex} />
                            </Link>
                          ),
                        },
                        {
                          key: 'owner',
                          label: 'Owner',
                          // the full address, breaking anywhere: a flexible column (F3-e: it was
                          // 200 and 18 %)
                          minWidth: ID_LINE_PX,
                          width: 'auto',
                          render: (cell) => <CellOwner cell={cell} />,
                        },
                        {
                          key: 'type',
                          label: 'Type reference',
                          // a protocol's name or the full code hash, breaking: a flexible column
                          // (F3-e: it was 165 and 15 %)
                          minWidth: ID_LINE_PX,
                          width: 'auto',
                          render: (cell) => <CellTypeReference cell={cell} />,
                        },
                        {
                          key: 'capacity',
                          label: 'Capacity',
                          // one line under a million CKB with its unit (F3-e: it was 120 and 11 %)
                          minWidth: AMOUNT_LINE_PX,
                          numeric: true,
                          // Material pass v2 leftovers: the shared exact figure (ink, dim unit), not gold.
                          render: (cell) => (
                            <CkbAmount
                              form="column"
                              shannons={BigInt(cell.capacity)}
                              signed={false}
                              tone="plain"
                              wrap
                            />
                          ),
                        },
                        {
                          key: 'knowledge',
                          label: 'Occupied',
                          // as Capacity (F3-e: it was 120 and 11 %)
                          minWidth: AMOUNT_LINE_PX,
                          numeric: true,
                          render: (cell) => <CellOccupiedValue cell={cell} form="column" />,
                        },
                        {
                          key: 'created',
                          label: 'Created',
                          // the block (F3-e: it was 95 and 9 %)
                          minWidth: BLOCK_PX,
                          numeric: true,
                          render: (cell) => (
                            <Link href={`/blocks/${cell.createdAtBlock}`} look="inline">
                              {formatBlockNumber(cell.createdAtBlock)}
                            </Link>
                          ),
                        },
                        {
                          key: 'anatomy',
                          label: 'Cell anatomy',
                          // The bar over its legend, whose widest part is its 8 px swatch, its 6 px
                          // gap and "data 9,999,999 B" at 12 px (115.2); the last column, no gap
                          // after it (F3-e: it was 180 and 10 %).
                          minWidth: columnPx(
                            8 + 6 + meta(`data ${(9_999_999).toLocaleString('en-US')} B`)
                          ),
                          render: (cell) => <CellAnatomyBar cell={cell} />,
                        },
                      ]}
                      mobileRow={(cell) => (
                        <div className="text-small min-w-0 space-y-2">
                          <Link
                            href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                            look="key"
                            className="break-all"
                          >
                            {cell.txHash}
                            <OutpointIndex index={cell.outputIndex} />
                          </Link>
                          <div>
                            Owner: <CellOwner cell={cell} />
                          </div>
                          <div>
                            Type: <CellTypeReference cell={cell} />
                          </div>
                          <div>
                            <CkbAmount
                              form="summary"
                              shannons={BigInt(cell.capacity)}
                              signed={false}
                              tone="plain"
                            />{' '}
                            · occupied <CellOccupiedValue cell={cell} form="summary" />
                          </div>
                          <div>
                            Created{' '}
                            <Link href={`/blocks/${cell.createdAtBlock}`} look="inline">
                              {formatBlockNumber(cell.createdAtBlock)}
                            </Link>
                          </div>
                          <CellAnatomyBar cell={cell} />
                        </div>
                      )}
                      empty="No live cells currently use this version."
                    />
                  )}
                  {/* The cells oldest first, paged by their rows and their "more" (the usage
                      states their count: lane c2-finish, 1c). */}
                  {versionCellsAt && <Pager mode="pages" label="cells" {...versionCellsAt} />}
                </DetailSection>
              )}
            </>
          }
        >
          <DetailSection title="Capacity history">
            <p className="text-text text-small max-w-measure mb-3">
              Live capacity and common knowledge across{' '}
              {isCodeHashMode ? 'this code reference' : 'all registered family references'}.
            </p>
            <RangeSelect
              label="range"
              options={CAPACITY_RANGE_OPTIONS}
              value={capacityRange}
              onChange={setCapacityRange}
            />
            {isAwaited(isCodeHashMode ? selectedCapacityChartRead : familyCapacityChartRead) ? (
              <p className="text-text-dim text-small py-6">Loading capacity history…</p>
            ) : (isCodeHashMode ? selectedCapacityChart : familyCapacityChart)?.data.length ? (
              <ScriptCapacityEvidenceChart
                chart={(isCodeHashMode ? selectedCapacityChart : familyCapacityChart)!}
                markers={namedScriptDetail?.history ?? []}
              />
            ) : (
              <p className="text-text-dim text-small py-6">No capacity history.</p>
            )}
            {!isCodeHashMode && familyCapacityChartError && (
              <p role="alert" className="text-negative text-small max-w-measure">
                Capacity history unavailable: {String(familyCapacityChartError)}
              </p>
            )}
          </DetailSection>
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
                      <span className="text-text [overflow-wrap:anywhere]">
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
                  <div className="text-small space-y-2">
                    <div className="break-all">
                      {row.hashType} · {row.referenceHash}
                    </div>
                    <div className="[overflow-wrap:anywhere]">
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
