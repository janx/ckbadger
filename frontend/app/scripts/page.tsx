'use client';

import { useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { AppLink } from '@/components/ui/app-link';
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
import { getScriptDetailHref } from '@/lib/detail-routes';
import {
  getScriptRefBadgeLabel,
  getScriptRefVerboseLabel,
  normalizeScriptRefHashType,
} from '@/lib/script-ref';
import { formatCkbCompact, truncateHash } from '@/lib/utils';

type SortDirection = 'asc' | 'desc';
type ScriptSortKey = 'name' | 'kind' | 'description' | 'occupied' | 'capacity';
const UNKNOWN_SCRIPT_NAME = 'unknown';
const UNLABELED_SCRIPT_LABEL = 'Unlabeled';

export default function ScriptsPage() {
  const pagination = useCursorPagination();
  const decoderType = undefined;
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>(undefined);
  const [sortKey, setSortKey] = useState<ScriptSortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');

  const { data, isLoading, isError, error } = useQuery({
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
      <span className={sortKey === key ? 'text-emphasis' : 'text-text-muted'}>
        {sortKey === key ? (sortDirection === 'asc' ? '↑' : '↓') : '↕'}
      </span>
    </button>
  );

  const scripts = data?.data ?? [];
  const errorMessage = error instanceof Error ? error.message : 'Unknown error';
  const hasKnownScriptName = (name: string | null | undefined): boolean =>
    Boolean(name && name.trim() && name.trim().toLowerCase() !== UNKNOWN_SCRIPT_NAME);
  const getScriptRefDisplay = (script: KnownScript): string =>
    `${getScriptRefBadgeLabel(script.hashType)} · ${truncateHash(script.codeHash, 10, 8)}`;
  const getScriptRefFull = (script: KnownScript): string => {
    const hashType = getScriptRefVerboseLabel(script.hashType);
    return `${hashType}:${script.codeHash}`;
  };
  const normalizeScriptKind = (value: string | null): 'lock' | 'type' | 'both' | undefined => {
    if (value === 'lock' || value === 'type' || value === 'both') return value;
    if (value === 'lock+type') return 'both';
    return undefined;
  };
  const getScriptHref = (script: KnownScript): string =>
    getScriptDetailHref({
      name: script.name,
      codeHash: script.codeHash,
      hashType: normalizeScriptRefHashType(script.hashType),
      scriptKind: normalizeScriptKind(script.scriptKind),
    });
  return (
    <div className="bg-base-bg min-h-screen">
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
                  className="focus:border-emphasis-dim focus:ring-emphasis-dim border-base-border bg-base-surface placeholder-text-dim text-text-primary w-64 rounded border px-3 py-1.5 font-mono text-sm transition-colors focus:outline-none focus:ring-1"
                />
                {search && (
                  <button
                    type="button"
                    onClick={clearSearch}
                    className="text-text-muted hover:text-text-secondary absolute right-2 top-1/2 -translate-y-1/2"
                  >
                    ×
                  </button>
                )}
              </div>
              <button
                type="submit"
                className="border-emphasis-dim bg-emphasis-dim/20 text-emphasis hover:bg-emphasis-dim/40 rounded border px-4 py-1.5 font-mono text-sm transition-colors"
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
                        <div className="bg-base-elevated h-4 w-32 rounded" />
                      </div>
                      <div className="w-20">
                        <div className="bg-base-elevated h-4 w-12 rounded" />
                      </div>
                      <div className="flex-1">
                        <div className="bg-base-elevated h-4 w-48 rounded" />
                      </div>
                      <div className="w-28">
                        <div className="bg-base-elevated ml-auto h-4 w-20 rounded" />
                      </div>
                      <div className="w-28">
                        <div className="bg-base-elevated ml-auto h-4 w-20 rounded" />
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </div>
            ) : isError ? (
              <div className="py-8 text-center">
                <p className="text-negative font-mono text-sm">Failed to load scripts</p>
                <p className="text-negative mt-2 font-mono text-xs">{errorMessage}</p>
              </div>
            ) : data?.data?.length ? (
              <>
                <div className="border-base-border bg-base-surface/50 text-text-muted flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                  {renderSortHeader('name', 'Script', 'w-44')}
                  {renderSortHeader('kind', 'Kind', 'w-16')}
                  {renderSortHeader('description', 'Description', 'flex-1 px-4')}
                  {renderSortHeader('occupied', 'Occupied (CKB)', 'w-28', 'right')}
                  {renderSortHeader('capacity', 'Capacity (CKB)', 'w-28', 'right')}
                </div>
                {scripts.map((script: KnownScript) => (
                  <TerminalRow key={script.codeHash}>
                    <div className="flex items-center">
                      <div className="w-44">
                        {hasKnownScriptName(script.name) ? (
                          <AppLink
                            href={getScriptHref(script)}
                            className="text-emphasis font-medium hover:underline"
                          >
                            {script.name!.trim()}
                          </AppLink>
                        ) : (
                          <AppLink
                            href={getScriptHref(script)}
                            className="hover:text-emphasis text-text-secondary font-medium hover:underline"
                            title={getScriptRefFull(script)}
                          >
                            {UNLABELED_SCRIPT_LABEL}
                          </AppLink>
                        )}
                      </div>
                      <div className="w-16">
                        {script.scriptKind ? (
                          <Badge variant={script.scriptKind === 'lock' ? 'blue' : 'purple'}>
                            {script.scriptKind}
                          </Badge>
                        ) : (
                          <span className="text-text-muted">-</span>
                        )}
                      </div>
                      <div className="text-text-muted flex-1 truncate px-4 text-sm">
                        {hasKnownScriptName(script.name) ? (
                          script.description
                        ) : (
                          <span
                            title={getScriptRefFull(script)}
                            className="text-text-muted font-mono text-xs"
                          >
                            {getScriptRefDisplay(script)}
                          </span>
                        )}
                      </div>
                      <div className="text-text-secondary w-28 text-right font-mono">
                        {(() => {
                          const occupied = script.liveOccupiedCapacitySum;
                          if (!occupied) {
                            return <span className="text-text-muted">-</span>;
                          }
                          const compact = formatCkbCompact(occupied);
                          return <span title={`${compact.full} CKB`}>{compact.value}</span>;
                        })()}
                      </div>
                      <div className="text-text-secondary w-28 text-right font-mono">
                        {(() => {
                          const capacity = script.liveCapacitySum;
                          if (!capacity) {
                            return <span className="text-text-muted">-</span>;
                          }
                          const compact = formatCkbCompact(capacity);
                          return <span title={`${compact.full} CKB`}>{compact.value}</span>;
                        })()}
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </>
            ) : (
              <div className="text-text-muted py-8 text-center">No scripts found</div>
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
