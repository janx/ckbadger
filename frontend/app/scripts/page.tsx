'use client';

import { useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from 'next/link';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, KnownScript } from '@/lib/api';
import { formatCkbCompact, truncateHash } from '@/lib/utils';

type SortDirection = 'asc' | 'desc';
type ScriptSortKey = 'name' | 'kind' | 'description' | 'occupied' | 'capacity' | 'occupiedRatio';
const UNKNOWN_SCRIPT_NAME = 'unknown';
const UNLABELED_SCRIPT_LABEL = 'Unlabeled';

export default function ScriptsPage() {
  const pagination = useCursorPagination();
  const decoderType = undefined;
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>(undefined);
  const [sortKey, setSortKey] = useState<ScriptSortKey>('name');
  const [sortDirection, setSortDirection] = useState<SortDirection>('asc');

  const { data, isLoading } = useQuery({
    queryKey: ['scripts', pagination.cursor, decoderType, search, sortKey, sortDirection],
    queryFn: () =>
      api.getScripts({
        limit: 20,
        cursor: pagination.cursor,
        decoderType,
        search,
        sortKey,
        sortDirection,
      }),
    placeholderData: keepPreviousData,
  });

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    setSearch(searchInput.trim() || undefined);
    pagination.reset();
  };

  const clearSearch = () => {
    setSearchInput('');
    setSearch(undefined);
    pagination.reset();
  };

  const parseBigInt = (value: string | null | undefined): bigint | null => {
    if (!value) return null;
    try {
      return BigInt(value);
    } catch {
      return null;
    }
  };

  const parseOccupiedRatioBasisPoints = (
    occupied: string | null | undefined,
    capacity: string | null | undefined
  ): bigint | null => {
    const occupiedValue = parseBigInt(occupied);
    const capacityValue = parseBigInt(capacity);
    if (occupiedValue === null || capacityValue === null || capacityValue <= BigInt(0)) return null;
    return (occupiedValue * BigInt(10_000)) / capacityValue;
  };

  const formatOccupiedRatio = (
    occupied: string | null | undefined,
    capacity: string | null | undefined
  ): string | null => {
    const basisPoints = parseOccupiedRatioBasisPoints(occupied, capacity);
    if (basisPoints === null) return null;

    const integerPart = basisPoints / BigInt(100);
    const decimalPart = (basisPoints % BigInt(100)).toString().padStart(2, '0');
    return `${integerPart.toString()}.${decimalPart}%`;
  };

  const toggleSort = (nextKey: ScriptSortKey) => {
    if (nextKey === sortKey) {
      setSortDirection((prev) => (prev === 'asc' ? 'desc' : 'asc'));
    } else {
      setSortKey(nextKey);
      setSortDirection(
        nextKey === 'name' || nextKey === 'description' || nextKey === 'kind' ? 'asc' : 'desc'
      );
    }
    pagination.reset();
  };

  const renderSortHeader = (
    key: ScriptSortKey,
    label: string,
    className: string,
    align: 'left' | 'right' = 'left'
  ) => (
    <button
      type="button"
      className={`${className} flex items-center gap-1 ${align === 'right' ? 'justify-end text-right' : ''}`}
      onClick={() => toggleSort(key)}
      aria-label={`Sort by ${label}`}
    >
      <span>{label}</span>
      <span className={sortKey === key ? 'text-terminal-green' : 'text-slate-700'}>
        {sortKey === key ? (sortDirection === 'asc' ? '↑' : '↓') : '↕'}
      </span>
    </button>
  );

  const scripts = data?.data ?? [];
  const hasKnownScriptName = (name: string | null | undefined): boolean =>
    Boolean(name && name.trim() && name.trim().toLowerCase() !== UNKNOWN_SCRIPT_NAME);
  const getScriptHashTypeLabel = (script: KnownScript): string => script.hashType ?? 'type';
  const getScriptRefDisplay = (script: KnownScript): string =>
    `${getScriptHashTypeLabel(script)} · ${truncateHash(script.codeHash, 10, 8)}`;
  const getScriptRefFull = (script: KnownScript): string => {
    const hashType = getScriptHashTypeLabel(script);
    return `${hashType}:${script.codeHash}`;
  };
  const isScriptHashType = (value: string | null): value is 'type' | 'data' | 'data1' | 'data2' =>
    value === 'type' || value === 'data' || value === 'data1' || value === 'data2';
  const normalizeScriptKind = (value: string | null): 'lock' | 'type' | 'both' | undefined => {
    if (value === 'lock' || value === 'type' || value === 'both') return value;
    if (value === 'lock+type') return 'both';
    return undefined;
  };
  const getScriptHref = (script: KnownScript): string => {
    if (hasKnownScriptName(script.name)) {
      return `/scripts/${encodeURIComponent(script.name!.trim())}`;
    }

    const query = new URLSearchParams();
    const hashType = isScriptHashType(script.hashType) ? script.hashType : null;
    const kind = normalizeScriptKind(script.scriptKind);
    if (hashType) query.set('hashType', hashType);
    if (kind) query.set('kind', kind);

    const suffix = query.toString();
    return `/script/${encodeURIComponent(script.codeHash)}${suffix ? `?${suffix}` : ''}`;
  };
  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Scripts"
          subtitle="Browse known scripts deployed on the CKB network"
          actions={
            <form onSubmit={handleSearch} className="flex gap-2">
              <div className="relative">
                <input
                  type="text"
                  value={searchInput}
                  onChange={(e) => setSearchInput(e.target.value)}
                  placeholder="Search by name or code hash..."
                  className="focus:border-terminal-dark focus:ring-terminal-dark w-64 rounded border border-slate-700 bg-slate-900 px-3 py-1.5 font-mono text-sm text-white placeholder-slate-600 transition-colors focus:outline-none focus:ring-1"
                />
                {search && (
                  <button
                    type="button"
                    onClick={clearSearch}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300"
                  >
                    ×
                  </button>
                )}
              </div>
              <button
                type="submit"
                className="border-terminal-dark bg-terminal-dark/20 text-terminal-green hover:bg-terminal-dark/40 rounded border px-4 py-1.5 font-mono text-sm transition-colors"
              >
                Search
              </button>
            </form>
          }
        />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Script List</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {isLoading ? (
              <div className="space-y-2 py-4">
                {Array.from({ length: 5 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="flex animate-pulse items-center">
                      <div className="flex-1">
                        <div className="h-4 w-32 rounded bg-slate-800" />
                      </div>
                      <div className="w-20">
                        <div className="h-4 w-12 rounded bg-slate-800" />
                      </div>
                      <div className="flex-1">
                        <div className="h-4 w-48 rounded bg-slate-800" />
                      </div>
                      <div className="w-28">
                        <div className="ml-auto h-4 w-20 rounded bg-slate-800" />
                      </div>
                      <div className="w-28">
                        <div className="ml-auto h-4 w-20 rounded bg-slate-800" />
                      </div>
                      <div className="w-24">
                        <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </div>
            ) : data?.data?.length ? (
              <>
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  {renderSortHeader('name', 'Script', 'w-44')}
                  {renderSortHeader('kind', 'Kind', 'w-16')}
                  {renderSortHeader('description', 'Description', 'flex-1 px-4')}
                  {renderSortHeader('occupied', 'Occupied (CKB)', 'w-28', 'right')}
                  {renderSortHeader('capacity', 'Capacity (CKB)', 'w-28', 'right')}
                  {renderSortHeader('occupiedRatio', 'Occupied Ratio', 'w-24', 'right')}
                </div>
                {scripts.map((script: KnownScript) => (
                  <TerminalRow key={script.codeHash}>
                    <div className="flex items-center">
                      <div className="w-44">
                        {hasKnownScriptName(script.name) ? (
                          <Link
                            href={getScriptHref(script)}
                            className="text-terminal-green font-medium hover:underline"
                          >
                            {script.name!.trim()}
                          </Link>
                        ) : (
                          <Link
                            href={getScriptHref(script)}
                            className="hover:text-terminal-green font-medium text-slate-300 hover:underline"
                            title={getScriptRefFull(script)}
                          >
                            {UNLABELED_SCRIPT_LABEL}
                          </Link>
                        )}
                      </div>
                      <div className="w-16">
                        {script.scriptKind ? (
                          <Badge variant={script.scriptKind === 'lock' ? 'blue' : 'purple'}>
                            {script.scriptKind}
                          </Badge>
                        ) : (
                          <span className="text-slate-600">-</span>
                        )}
                      </div>
                      <div className="flex-1 truncate px-4 text-sm text-slate-400">
                        {hasKnownScriptName(script.name) ? (
                          script.description
                        ) : (
                          <span
                            title={getScriptRefFull(script)}
                            className="font-mono text-xs text-slate-500"
                          >
                            {getScriptRefDisplay(script)}
                          </span>
                        )}
                      </div>
                      <div className="w-28 text-right font-mono text-slate-300">
                        {(() => {
                          const occupied = script.liveOccupiedCapacitySum;
                          if (!occupied) {
                            return <span className="text-slate-600">-</span>;
                          }
                          const compact = formatCkbCompact(occupied);
                          return <span title={`${compact.full} CKB`}>{compact.value}</span>;
                        })()}
                      </div>
                      <div className="w-28 text-right font-mono text-slate-300">
                        {(() => {
                          const capacity = script.liveCapacitySum;
                          if (!capacity) {
                            return <span className="text-slate-600">-</span>;
                          }
                          const compact = formatCkbCompact(capacity);
                          return <span title={`${compact.full} CKB`}>{compact.value}</span>;
                        })()}
                      </div>
                      <div className="w-24 text-right font-mono text-slate-300">
                        {formatOccupiedRatio(
                          script.liveOccupiedCapacitySum,
                          script.liveCapacitySum
                        ) ?? <span className="text-slate-600">-</span>}
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </>
            ) : (
              <div className="py-8 text-center text-slate-500">No scripts found</div>
            )}
          </TerminalPanelContent>

          {data && data.data?.length > 0 && (
            <TerminalPanelFooter>
              <CursorPagination
                total={data.total ?? undefined}
                totalLabel="scripts"
                pageSize={20}
                page={pagination.page}
                hasMore={data.hasMore}
                hasPrevious={pagination.hasPrevious}
                onNext={() => pagination.goToNext(data.nextCursor)}
                onPrevious={pagination.goToPrevious}
              />
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
