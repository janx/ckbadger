'use client';

import { useState, useEffect } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { useRouter, useSearchParams } from '@/src/navigation';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { Capacity } from '@/components/ui/capacity';
import { CapacityUtilization } from '@/components/ui/capacity-utilization';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import {
  getScriptRefBadgeLabel,
  getScriptRefQueryHashType,
  normalizeScriptRefHashType,
  type ScriptRefHashType,
} from '@/lib/script-ref';

type ScriptKind = 'lock' | 'type' | 'both';
type HashType = ScriptRefHashType;
const UNKNOWN_SCRIPT_NAME = 'unknown';

function isValidScriptKind(value: string | null): value is ScriptKind {
  return value === 'lock' || value === 'type' || value === 'both';
}

function normalizeHash(value: string | null | undefined): string | null {
  if (!value) return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function hasKnownScriptName(name: string | null | undefined): boolean {
  return Boolean(name && name.trim() && name.trim().toLowerCase() !== UNKNOWN_SCRIPT_NAME);
}

function isHexScriptHash(value: string): boolean {
  return /^0x[0-9a-fA-F]{64}$/.test(value);
}

function normalizeScriptKind(value: string | null | undefined): ScriptKind | null {
  if (value === 'lock' || value === 'type' || value === 'both') return value;
  if (value === 'lock+type') return 'both';
  return null;
}

export interface ScriptByCodeHashPageProps {
  codeHash: string;
}

export default function ScriptByCodeHashPage({
  codeHash: routeCodeHash,
}: ScriptByCodeHashPageProps) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const rawIdentifier = decodeURIComponent(routeCodeHash);
  const scriptIdentifier = rawIdentifier.trim();
  const isCodeHashIdentifier = isHexScriptHash(scriptIdentifier);
  const codeHash = isCodeHashIdentifier
    ? `0x${scriptIdentifier.slice(2).toLowerCase()}`
    : scriptIdentifier;

  const initialHashType = searchParams.get('hashType');
  const initialKind = searchParams.get('kind');
  const explicitHashType = normalizeScriptRefHashType(initialHashType);
  const hasExplicitHashType = explicitHashType !== null;

  const [hashType, setHashType] = useState<HashType>(explicitHashType ?? 'type');
  const [scriptKind, setScriptKind] = useState<ScriptKind>(
    isValidScriptKind(initialKind) ? initialKind : 'both'
  );
  const cellsPagination = useCursorPagination();

  useEffect(() => {
    if (explicitHashType) {
      setHashType(explicitHashType);
    }
  }, [explicitHashType]);

  useEffect(() => {
    if (isValidScriptKind(initialKind)) {
      setScriptKind(initialKind);
    }
  }, [initialKind]);

  useEffect(() => {
    if (!scriptIdentifier || isCodeHashIdentifier) return;
    router.replace(`/scripts/${encodeURIComponent(scriptIdentifier)}`);
  }, [isCodeHashIdentifier, router, scriptIdentifier]);

  const { data: lookupResult } = useQuery({
    queryKey: ['script-lookup', codeHash],
    queryFn: () => api.lookupScripts([codeHash]),
    enabled: isCodeHashIdentifier,
    staleTime: Infinity,
  });

  const knownScript = lookupResult?.[codeHash];
  const deploymentKind: ScriptKind =
    knownScript?.scriptKind === 'lock'
      ? 'lock'
      : knownScript?.scriptKind === 'type'
        ? 'type'
        : 'both';
  const deploymentTypeHash =
    normalizeHash(knownScript?.deploymentTypeHash) ??
    ((knownScript?.hashType === 'type' && normalizeHash(knownScript?.codeHash)) || null);
  const deploymentDataHash =
    normalizeHash(knownScript?.deploymentDataHash) ??
    ((knownScript?.hashType !== 'type' && normalizeHash(knownScript?.codeHash)) || null);
  const dataRefHashType: HashType =
    knownScript?.hashType &&
    knownScript.hashType !== 'type' &&
    normalizeScriptRefHashType(knownScript.hashType)
      ? getScriptRefQueryHashType(knownScript.hashType, 'data')
      : 'data';
  const supportsTypeRef = Boolean(deploymentTypeHash);
  const defaultHashType: HashType = supportsTypeRef ? 'type' : dataRefHashType;

  useEffect(() => {
    if (!hasExplicitHashType) {
      setHashType(defaultHashType);
    }
  }, [defaultHashType, hasExplicitHashType]);

  useEffect(() => {
    if (!isCodeHashIdentifier || !knownScript || !hasKnownScriptName(knownScript.name)) return;
    const targetName = knownScript.name.trim();
    const query = new URLSearchParams();
    query.set('ref', codeHash);
    const redirectHashType = explicitHashType ?? normalizeScriptRefHashType(knownScript.hashType);
    if (redirectHashType) query.set('hashType', redirectHashType);
    const redirectKind =
      normalizeScriptKind(initialKind) ?? normalizeScriptKind(knownScript.scriptKind);
    if (redirectKind) query.set('kind', redirectKind);
    const suffix = query.toString();
    router.replace(`/scripts/${encodeURIComponent(targetName)}${suffix ? `?${suffix}` : ''}`);
  }, [codeHash, explicitHashType, initialKind, isCodeHashIdentifier, knownScript, router]);

  const { data: codeCellData } = useQuery({
    queryKey: ['code-cell', codeHash, hashType],
    queryFn: () => api.getCodeCell(codeHash, hashType),
    enabled: isCodeHashIdentifier && !knownScript?.codeCellTxHash,
    staleTime: Infinity,
  });

  const codeCellTxHash = knownScript?.codeCellTxHash || codeCellData?.txHash;
  const codeCellOutputIndex = knownScript?.codeCellOutputIndex ?? codeCellData?.outputIndex ?? null;

  const { data: cellsData, isLoading: isCellsLoading } = useQuery({
    queryKey: ['script-cells-by-hash', codeHash, hashType, scriptKind, cellsPagination.cursor],
    queryFn: () =>
      api.getCellsByScriptRef({
        codeHash,
        hashType,
        scriptKind,
        limit: 20,
        cursor: cellsPagination.cursor,
      }),
    enabled: isCodeHashIdentifier,
    placeholderData: keepPreviousData,
  });
  const { data: occupationChart, isLoading: isOccupationChartLoading } = useQuery({
    queryKey: ['script-occupation-by-code-hash', codeHash, scriptKind],
    queryFn: () => api.getScriptOccupationChartByCodeHash(codeHash, scriptKind),
    enabled: isCodeHashIdentifier,
  });

  if (!isCodeHashIdentifier || (knownScript && hasKnownScriptName(knownScript.name))) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="text-text-muted text-sm">Resolving script page...</div>
        </main>
      </div>
    );
  }

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Script"
          subtitle={
            knownScript && hasKnownScriptName(knownScript.name) ? (
              <Link
                href={`/scripts/${encodeURIComponent(knownScript.name)}`}
                className="text-emphasis hover:underline"
              >
                {knownScript.name}
              </Link>
            ) : undefined
          }
        />

        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Deployment</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="border-base-border bg-base-surface/50 text-text-muted flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
              <div className="w-40">Code Cell</div>
              <div className="flex-1">Code Hash</div>
              <div className="w-20 text-center">Hash Type</div>
              <div className="w-20 text-center">Kind</div>
              <div className="w-24 text-right">Cells</div>
              <div className="w-32 text-right">Capacity</div>
            </div>

            <TerminalRow hoverable={false}>
              <div className="flex items-center">
                <div className="w-40">
                  {codeCellTxHash && codeCellOutputIndex !== null ? (
                    <Link
                      href={`/cell/${codeCellTxHash}-${codeCellOutputIndex}`}
                      className="hover:underline"
                    >
                      <HexDisplay
                        value={`${codeCellTxHash}:${codeCellOutputIndex}`}
                        color="accent"
                        size="sm"
                        startChars={8}
                        endChars={8}
                      />
                    </Link>
                  ) : (
                    <span className="text-text-muted">-</span>
                  )}
                </div>
                <div className="flex-1">
                  <HexDisplay value={codeHash} truncate={false} color="accent" size="sm" />
                </div>
                <div className="w-20 text-center">
                  <Badge variant="gray">
                    {getScriptRefBadgeLabel(knownScript?.hashType || hashType)}
                  </Badge>
                </div>
                <div className="w-20 text-center">
                  {knownScript?.scriptKind || (scriptKind !== 'both' ? scriptKind : null) ? (
                    <Badge variant="neutral">{knownScript?.scriptKind || scriptKind}</Badge>
                  ) : (
                    <span className="text-text-muted">-</span>
                  )}
                </div>
                <div className="text-text-muted w-24 text-right font-mono">
                  {knownScript ? knownScript.liveCellsCount.toLocaleString() : '-'}
                </div>
                <div className="text-text-muted w-32 text-right">
                  {knownScript ? <Capacity value={knownScript.liveCapacitySum} /> : '-'}
                </div>
              </div>
            </TerminalRow>
            <div className="border-base-border border-t px-4 py-3">
              <div className="text-text-muted mb-2 text-[11px] uppercase tracking-wider">
                Same Deployment References
              </div>
              <div className="grid gap-2 md:grid-cols-2">
                <div className="flex items-center gap-2">
                  <Badge variant="gray">type</Badge>
                  {deploymentTypeHash ? (
                    <Link
                      href={`/script/${deploymentTypeHash}?hashType=type&kind=${deploymentKind}`}
                      className="text-emphasis font-mono text-xs hover:underline"
                    >
                      <HexDisplay
                        value={deploymentTypeHash}
                        color="accent"
                        size="sm"
                        startChars={10}
                        endChars={8}
                      />
                    </Link>
                  ) : (
                    <span className="text-text-muted font-mono text-xs">Unavailable</span>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  <Badge variant="gray">{getScriptRefBadgeLabel(dataRefHashType)}</Badge>
                  {deploymentDataHash ? (
                    <Link
                      href={`/script/${deploymentDataHash}?hashType=${dataRefHashType}&kind=${deploymentKind}`}
                      className="text-emphasis font-mono text-xs hover:underline"
                    >
                      <HexDisplay
                        value={deploymentDataHash}
                        color="accent"
                        size="sm"
                        startChars={10}
                        endChars={8}
                      />
                    </Link>
                  ) : (
                    <span className="text-text-muted font-mono text-xs">Unavailable</span>
                  )}
                </div>
              </div>
            </div>
            <div className="border-base-border border-t px-4 py-3">
              <div className="text-text-muted mb-2 text-[11px] uppercase tracking-wider">
                Reference Semantics
              </div>
              <div className="text-text-muted space-y-1 text-xs">
                <div>
                  <span className="text-text-secondary font-mono">type ref</span>
                  <span>
                    {' '}
                    resolves code by matching type script hash (upgradeable; runs latest VM
                    version).
                  </span>
                </div>
                <div>
                  <span className="text-text-secondary font-mono">
                    bytecode hash ref family (data/data1/data2)
                  </span>
                  <span>
                    {' '}
                    resolves by bytecode hash (immutable/reproducible; locks VM version to
                    data/data1/data2).
                  </span>
                </div>
                <div className="text-text-muted">
                  Tradeoff: choose type for upgradeability, choose data/data1/data2 for fixed code
                  behavior.
                </div>
                <div>
                  <a
                    href="https://docs.nervos.org/docs/tech-explanation/data-type-diff"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-emphasis hover:underline"
                  >
                    Reference doc: data vs type hash semantics
                  </a>
                </div>
              </div>
            </div>
            {knownScript && (
              <div className="border-base-border border-t px-4 py-4">
                <CapacityUtilization
                  totalCapacity={knownScript.liveCapacitySum}
                  occupiedCapacity={knownScript.liveOccupiedCapacitySum}
                />
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Occupation History</TerminalPanelHeader>
          <TerminalPanelContent>
            <div className="text-text-muted mb-3 text-xs">
              Daily cumulative live CKB occupation for this deployment.
            </div>
            {isOccupationChartLoading ? (
              <div className="text-text-muted py-8 text-center">Loading occupation history...</div>
            ) : occupationChart && occupationChart.data.length > 0 ? (
              <StackedAreaChart
                data={occupationChart.data}
                series={occupationChart.series}
                valueUnit="shannon"
              />
            ) : (
              <div className="text-text-muted py-8 text-center">No occupation history yet</div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel>
          <TerminalPanelHeader
            indicator="active"
            actions={
              <div className="flex flex-wrap gap-4 text-sm font-normal">
                <div className="flex items-center gap-2">
                  <span className="text-text-muted">Hash Type:</span>
                  <select
                    value={hashType}
                    onChange={(e) => {
                      setHashType(e.target.value as HashType);
                      cellsPagination.reset();
                    }}
                    className="border-base-border bg-base-elevated rounded border px-2 py-1 font-mono text-xs text-white"
                  >
                    <option value="type">
                      {supportsTypeRef ? 'type (upgradeable ref)' : 'type (unavailable)'}
                    </option>
                    <option value="data">data (immutable bytecode ref)</option>
                    <option value="data1">data1 (immutable bytecode ref)</option>
                    <option value="data2">data2 (immutable bytecode ref)</option>
                  </select>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-text-muted">Reference Mode:</span>
                  <Badge variant={supportsTypeRef ? 'green' : 'amber'}>
                    {supportsTypeRef ? 'Type + Data' : 'Data-only'}
                  </Badge>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-text-muted">Script Kind:</span>
                  <select
                    value={scriptKind}
                    onChange={(e) => {
                      setScriptKind(e.target.value as ScriptKind);
                      cellsPagination.reset();
                    }}
                    className="border-base-border bg-base-elevated rounded border px-2 py-1 font-mono text-xs text-white"
                  >
                    <option value="both">Both</option>
                    <option value="lock">Lock</option>
                    <option value="type">Type</option>
                  </select>
                </div>
              </div>
            }
          >
            Cells
          </TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {hashType === 'type' && !supportsTypeRef && (
              <div className="border-warning-500/30 bg-warning/10 text-warning border-b px-4 py-2 text-xs">
                This deployment has no type reference. Switch hash type to{' '}
                <span className="font-mono">{dataRefHashType}</span> to query by code hash.
              </div>
            )}
            {isCellsLoading ? (
              <div className="text-text-muted py-8 text-center">Loading cells...</div>
            ) : cellsData && cellsData.data.length > 0 ? (
              <>
                <div className="border-base-border bg-base-surface/50 text-text-muted flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                  <div className="flex-1">Cell</div>
                  <div className="w-32 text-right">Capacity</div>
                  <div className="w-28 text-right">Data Size</div>
                  <div className="w-28 text-right">Created At</div>
                </div>

                {cellsData.data.map((cell) => (
                  <TerminalRow key={`${cell.txHash}-${cell.outputIndex}`}>
                    <div className="flex items-center">
                      <div className="flex-1">
                        <Link
                          href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                          className="hover:underline"
                        >
                          <HexDisplay
                            value={`${cell.txHash}:${cell.outputIndex}`}
                            color="accent"
                            size="sm"
                            startChars={8}
                            endChars={8}
                          />
                        </Link>
                      </div>
                      <div className="text-text-muted w-32 text-right text-sm">
                        <Capacity value={cell.capacity} className="text-sm" />
                      </div>
                      <div className="text-text-muted w-28 text-right font-mono text-sm">
                        {cell.dataSize.toLocaleString()} bytes
                      </div>
                      <div className="w-28 text-right">
                        <Link
                          href={`/blocks/${cell.createdAtBlock}`}
                          className="text-emphasis font-mono text-sm hover:underline"
                        >
                          #{cell.createdAtBlock.toLocaleString()}
                        </Link>
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </>
            ) : (
              <div className="text-text-muted py-8 text-center">
                No cells found for this script with hash_type=&quot;{hashType}&quot;
              </div>
            )}
          </TerminalPanelContent>

          {cellsData && cellsData.data.length > 0 && (
            <TerminalPanelFooter>
              <CursorPagination
                total={cellsData.total}
                totalLabel="cells"
                pageSize={20}
                page={cellsPagination.page}
                hasMore={cellsData.hasMore}
                hasPrevious={cellsPagination.hasPrevious}
                onNext={() => cellsPagination.goToNext(cellsData.nextCursor)}
                onPrevious={cellsPagination.goToPrevious}
              />
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
