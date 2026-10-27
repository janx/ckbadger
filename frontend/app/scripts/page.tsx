'use client';

import { useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { AppLink } from '@/components/ui/app-link';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import {
  ScriptCodeClassSummary,
  scriptClassLabel,
} from '@/components/scripts/script-code-evidence';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type KnownScript } from '@/lib/api';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { getScriptRefBadgeLabel, normalizeScriptRefHashType } from '@/lib/script-ref';
import { formatCkbCompact, truncateHash } from '@/lib/utils';

type SortKey = 'name' | 'usedAs' | 'description' | 'used' | 'capacity' | 'liveCells' | 'cells';
type SortDirection = 'asc' | 'desc';
const UNKNOWN_SCRIPT_NAME = 'unknown';

function hasName(script: KnownScript): boolean {
  return Boolean(script.name?.trim() && script.name.trim().toLowerCase() !== UNKNOWN_SCRIPT_NAME);
}

function scriptHref(script: KnownScript): string {
  const kind = script.scriptKind === 'lock+type' ? 'both' : script.scriptKind;
  return getScriptDetailHref({
    name: script.name,
    codeHash: script.codeHash,
    hashType: normalizeScriptRefHashType(script.hashType),
    scriptKind: kind === 'lock' || kind === 'type' || kind === 'both' ? kind : undefined,
  });
}

function codeDescription(script: KnownScript): string {
  const classes = script.codeClasses ?? [];
  if (!classes.length) return 'Code evidence unavailable';
  if (classes.length > 1) return `${classes.length} code classes`;
  return scriptClassLabel(classes[0].class);
}

function occupiedPercent(script: KnownScript): string {
  const total = BigInt(script.ownedCapacitySum!);
  if (total === BigInt(0)) return '0.00%';
  const occupied = BigInt(script.ownedKnowledgeSum!);
  if (occupied < BigInt(0) || occupied > total) {
    throw new Error(`Script ${script.codeHash} has invalid occupied capacity`);
  }
  const hundredths = (occupied * BigInt(10000)) / total;
  return `${hundredths / BigInt(100)}.${String(hundredths % BigInt(100)).padStart(2, '0')}%`;
}

function capacityText(value: string) {
  const compact = formatCkbCompact(value);
  return <span title={`${compact.full} CKB`}>{compact.value} CKB</span>;
}

function countText(value: number | undefined): string {
  return value === undefined ? 'unavailable' : value.toLocaleString();
}

function ScriptName({ script }: { script: KnownScript }) {
  const name = hasName(script) ? script.name!.trim() : 'Unlabeled';
  const ref = `${getScriptRefBadgeLabel(script.hashType)} · ${truncateHash(script.codeHash, 10, 8)}`;
  return (
    <div className="min-w-0">
      <AppLink
        href={scriptHref(script)}
        title={!hasName(script) ? `${script.hashType} ref:${script.codeHash}` : undefined}
        className="text-text-bright hover:text-emphasis font-medium hover:underline"
      >
        {name}
      </AppLink>
      {script.deprecated && <span className="text-negative ml-2 text-[11px]">Deprecated</span>}
      <div className="text-text-dim mt-1 truncate text-[11px]" title={script.description ?? ref}>
        {hasName(script) ? script.description : ref}
      </div>
    </div>
  );
}

export default function ScriptsPage() {
  const pagination = useCursorPagination();
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>();
  const [role, setRole] = useState<'lock' | 'type' | undefined>();
  const [sortKey, setSortKey] = useState<SortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['scripts', pagination.cursor, search, role, sortKey, sortDirection],
    queryFn: () =>
      api.getScripts({
        limit: DEFAULT_PAGE_SIZE,
        cursor: pagination.cursor,
        decoderType: undefined,
        search,
        role,
        sortKey,
        sortDirection,
      }),
    placeholderData: keepPreviousData,
  });

  function toggleSort(next: SortKey) {
    setSortDirection(
      next === sortKey
        ? sortDirection === 'asc'
          ? 'desc'
          : 'asc'
        : next === 'name' || next === 'description' || next === 'usedAs'
          ? 'asc'
          : 'desc'
    );
    setSortKey(next);
    pagination.reset();
  }

  const columns = [
    {
      key: 'script',
      label: 'Script',
      minWidth: 205,
      width: '24%',
      render: (s: KnownScript) => <ScriptName script={s} />,
    },
    {
      key: 'role',
      label: 'Role',
      minWidth: 85,
      width: '9%',
      render: (s: KnownScript) => (
        <span className="text-text-dim">{s.scriptKind ?? 'unregistered'}</span>
      ),
    },
    {
      key: 'code',
      label: 'Code',
      minWidth: 210,
      width: '23%',
      render: (s: KnownScript) => <span className="text-text-dim">{codeDescription(s)}</span>,
    },
    {
      key: 'cells',
      label: 'Live cells',
      minWidth: 105,
      width: '11%',
      numeric: true,
      render: (s: KnownScript) => (
        <span title={`${countText(s.cellsCount)} ever`}>{countText(s.liveCellsCount)}</span>
      ),
    },
    {
      key: 'capacity',
      label: 'CKB in its cells · occupied',
      minWidth: 225,
      width: '24%',
      render: (s: KnownScript) => (
        <div className="min-w-0">
          <div>{capacityText(s.ownedCapacitySum!)}</div>
          <div className="bg-base-elevated mt-1 h-1 w-full overflow-hidden rounded">
            <div className="bg-positive h-full" style={{ width: occupiedPercent(s) }} />
          </div>
        </div>
      ),
    },
    {
      key: 'occupied',
      label: 'Occupied',
      minWidth: 95,
      width: '9%',
      numeric: true,
      render: (s: KnownScript) => <span className="text-text-dim">{occupiedPercent(s)}</span>,
    },
  ];

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <DetailLayout
          kind="index"
          title="Scripts"
          identity="Known script families and their registered code references"
          actions={
            <form
              onSubmit={(event) => {
                event.preventDefault();
                setSearch(searchInput.trim() || undefined);
                pagination.reset();
              }}
              className="flex min-w-0 gap-2"
            >
              <input
                value={searchInput}
                onChange={(event) => setSearchInput(event.target.value)}
                placeholder="Search by script family name..."
                aria-label="Search scripts"
                className="border-base-border bg-base-surface text-text-bright min-w-0 rounded border px-3 py-1.5 text-xs sm:w-64"
              />
              {search && (
                <button
                  type="button"
                  onClick={() => {
                    setSearchInput('');
                    setSearch(undefined);
                    pagination.reset();
                  }}
                  aria-label="Clear search"
                  className="text-text-dim"
                >
                  ×
                </button>
              )}
              <button
                type="submit"
                className="border-base-border text-text-bright rounded border px-3 py-1.5 text-xs"
              >
                Search
              </button>
            </form>
          }
        >
          {data?.classSummary && (
            <ScriptCodeClassSummary
              classes={data.classSummary}
              title="Who can change the code · lock scripts"
            />
          )}
          <DetailSection
            title="Script families"
            actions={
              <span className="text-text-dim text-xs">
                {data?.total?.toLocaleString() ?? '—'} families
              </span>
            }
          >
            <div
              role="group"
              aria-label="Filter scripts by role"
              className="mb-4 flex flex-wrap gap-2 text-xs"
            >
              {(
                [
                  [undefined, 'All', data?.roleCounts?.all],
                  ['lock', 'Locks', data?.roleCounts?.locks],
                  ['type', 'Types', data?.roleCounts?.types],
                ] as const
              ).map(([value, label, count]) => (
                <button
                  key={label}
                  type="button"
                  aria-pressed={role === value}
                  onClick={() => {
                    setRole(value);
                    pagination.reset();
                  }}
                  className={`border-base-border rounded border px-3 py-1 ${role === value ? 'text-text-bright bg-base-elevated' : 'text-text-dim'}`}
                >
                  {label} {count === undefined ? '' : count.toLocaleString()}
                </button>
              ))}
            </div>
            <div className="text-text-dim mb-3 flex flex-wrap gap-x-4 gap-y-2 text-[11px]">
              {(
                [
                  ['name', 'Script'],
                  ['usedAs', 'Used as'],
                  ['description', 'Description'],
                  ['liveCells', 'Live Cells'],
                  ['cells', 'Total Cells'],
                  ['used', 'Used (CKB)'],
                  ['capacity', 'Capacity (CKB)'],
                ] as const
              ).map(([key, label]) => (
                <button
                  key={key}
                  type="button"
                  aria-label={`Sort by ${label}`}
                  onClick={() => toggleSort(key)}
                  className={sortKey === key ? 'text-text-bright' : 'hover:text-text-bright'}
                >
                  <span>{label}</span>{' '}
                  {sortKey === key ? (sortDirection === 'asc' ? '↑' : '↓') : '↕'}
                </button>
              ))}
            </div>
            {isLoading ? (
              <div className="text-text-dim py-8 text-xs">Loading scripts...</div>
            ) : isError ? (
              <div role="alert" className="text-negative py-8 text-xs">
                <strong>Failed to load scripts</strong>:{' '}
                {error instanceof Error ? error.message : 'Unknown error'}
              </div>
            ) : (
              <ListTable
                label="Scripts"
                columns={columns}
                rows={data?.data ?? []}
                rowKey={(s) => s.codeHash}
                empty="No scripts found"
                mobileRow={(s) => (
                  <div className="min-w-0 space-y-2 text-xs">
                    <ScriptName script={s} />
                    <div className="text-text-dim">
                      {s.scriptKind ?? 'unregistered'} · {codeDescription(s)}
                    </div>
                    <div className="flex flex-wrap justify-between gap-2">
                      <span>{countText(s.liveCellsCount)} live cells</span>
                      <span>{capacityText(s.ownedCapacitySum!)}</span>
                    </div>
                    <div className="text-text-dim">
                      Common Knowledge: {formatCkbCompact(s.ownedKnowledgeSum!).value} · Occupied{' '}
                      {occupiedPercent(s)}
                    </div>
                  </div>
                )}
              />
            )}
            {data && data.data.length > 0 && (
              <div className="border-base-border mt-4 border-t pt-4">
                <CursorPagination
                  total={data.total ?? undefined}
                  totalLabel="scripts"
                  pageSize={DEFAULT_PAGE_SIZE}
                  page={pagination.page}
                  hasMore={data.hasMore}
                  hasPrevious={pagination.hasPrevious}
                  onNext={() => pagination.goToNext(data.nextCursor)}
                  onPrevious={pagination.goToPrevious}
                />
              </div>
            )}
          </DetailSection>
        </DetailLayout>
      </main>
    </div>
  );
}
