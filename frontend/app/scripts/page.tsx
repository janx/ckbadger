'use client';

import { useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { AppLink } from '@/components/ui/app-link';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import {
  ScriptCodeClassSummary,
  isImmutableClass,
  scriptClassLabel,
  scriptShare,
} from '@/components/scripts/script-code-evidence';
import { ExactCkb } from '@/components/activity-event-row';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type KnownScript } from '@/lib/api';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { getScriptRefBadgeLabel, normalizeScriptRefHashType } from '@/lib/script-ref';
import { formatUtc8Date, truncateHash } from '@/lib/utils';
import { formatCount } from '@/lib/format-count';
import { DATA_COLOR } from '@/lib/design-tokens';

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

const GOVERNOR: Partial<Record<NonNullable<KnownScript['codeClasses']>[number]['class'], string>> =
  {
    multisig: 'a multisig',
    one_key: 'one key',
  };

/** The board's Code column: "Immutable" or "Upgradable · N versions", then by whom. */
function ScriptCodeClass({ script }: { script: KnownScript }) {
  const classes = script.codeClasses ?? [];
  if (!classes.length)
    return (
      <span data-testid="script-code-class" className="text-text-dim">
        Code evidence unavailable
      </span>
    );
  const replaceable = classes.filter((row) => !isImmutableClass(row.class));
  const immutable = classes.filter((row) => isImmutableClass(row.class));
  let head: string;
  let sub: string;
  if (script.firstDeployedAt === undefined)
    throw new Error(`Script family ${script.name} row has no firstDeployedAt`);
  const first = script.firstDeployedAt;
  if (!replaceable.length) {
    head = 'Immutable';
    sub = [
      ...(first ? [first.blockNumber === 0 ? 'genesis' : formatUtc8Date(first.timestampMs)] : []),
      ...[...immutable]
        .sort((a, b) => Number(b.class === 'unspendable') - Number(a.class === 'unspendable'))
        .map((row) => (row.class === 'fixed' ? 'pinned by data hash' : 'code cell unspendable')),
    ].join(' · ');
  } else {
    if (script.versionsCount === undefined)
      throw new Error(`Script family ${script.name} has replaceable code but no versionsCount`);
    head = `Upgradable · ${formatCount(script.versionsCount, 'version')}`;
    sub = [
      `by ${replaceable.map((row) => GOVERNOR[row.class] ?? scriptClassLabel(row.class).toLowerCase()).join(' or ')}`,
      ...(first ? [`since ${formatUtc8Date(first.timestampMs)}`] : []),
      ...(immutable.length ? ['some references pinned'] : []),
    ].join(' · ');
  }
  return (
    <span data-testid="script-code-class" className="flex min-w-0 flex-col gap-0.5">
      <span
        className={`inline-flex items-center gap-2 ${replaceable.length ? 'text-text-bright' : 'text-text'}`}
      >
        <svg width="10" height="10" aria-hidden="true" className="shrink-0">
          <circle
            cx="5"
            cy="5"
            r="3.8"
            fill={replaceable.length ? '#0c0e15' : '#a0a8be'}
            fillOpacity={replaceable.length ? 1 : 0.55}
            stroke={replaceable.length ? '#dee2ec' : '#a0a8be'}
          />
        </svg>
        {head}
      </span>
      <span className="text-text-dim text-[11px]">{sub}</span>
    </span>
  );
}

function occupiedShare(script: KnownScript): { share: string; width: string } {
  const total = BigInt(script.ownedCapacitySum!);
  const occupied = BigInt(script.ownedKnowledgeSum!);
  if (occupied < BigInt(0) || occupied > total)
    throw new Error(`Script ${script.codeHash} has invalid occupied capacity`);
  if (total === BigInt(0)) return { share: '0%', width: '0%' };
  return {
    share: scriptShare(occupied, total),
    width: `${Number((occupied * BigInt(1000000)) / total) / 10000}%`,
  };
}

function capacityText(value: string) {
  return <ExactCkb shannons={BigInt(value)} signed={false} tone="plain" />;
}

function countText(value: number | undefined): string {
  return value === undefined ? 'unavailable' : value.toLocaleString('en-US');
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
      {script.deprecated && <span className="text-text-dim ml-2 text-[11px]">deprecated</span>}
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

  function sortBy(next: SortKey, direction: SortDirection) {
    setSortKey(next);
    setSortDirection(direction);
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
      width: '8%',
      render: (s: KnownScript) => (
        <span className="text-text-dim">
          {s.scriptKind ? s.scriptKind.replace('+', ' + ') : 'unregistered'}
        </span>
      ),
    },
    {
      key: 'code',
      label: 'Code',
      minWidth: 210,
      width: '22%',
      render: (s: KnownScript) => <ScriptCodeClass script={s} />,
    },
    {
      key: 'cells',
      label: 'Live cells',
      minWidth: 120,
      width: '12%',
      numeric: true,
      render: (s: KnownScript) => (
        <span className="tabular-nums">
          {countText(s.liveCellsCount)}
          <span className="text-text-ghost block text-[11px]">
            of {countText(s.cellsCount)} ever
          </span>
        </span>
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
          <div className="bg-base-elevated mt-1 flex h-1 w-full overflow-hidden rounded">
            <div
              className="h-full"
              style={{
                width: occupiedShare(s).width,
                minWidth: occupiedShare(s).width === '0%' ? 0 : 2,
                backgroundColor: DATA_COLOR,
              }}
            />
          </div>
        </div>
      ),
    },
    {
      key: 'occupied',
      label: 'Occupied',
      minWidth: 90,
      width: '10%',
      numeric: true,
      render: (s: KnownScript) => <span className="text-text-dim">{occupiedShare(s).share}</span>,
    },
  ];

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <DetailLayout
          kind="index"
          title="Scripts"
          identity="Every cell runs one lock script, which decides who can spend it, and may run one type script, which decides what the cell is and how it may change."
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
                placeholder="Search by name or code hash"
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
          <div
            role="group"
            aria-label="Role"
            className="border-base-border mb-6 flex flex-wrap gap-7 border-b"
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
                className={`-mb-px flex h-11 items-center gap-2 border-b-2 text-[15px] font-medium ${role === value ? 'text-text-bright border-[#2edba3]' : 'text-text-dim border-transparent'}`}
              >
                {label}{' '}
                {count !== undefined && (
                  <span className="text-text-dim text-xs font-normal">
                    {count.toLocaleString('en-US')}
                  </span>
                )}
              </button>
            ))}
          </div>
          {data?.classSummary && data.roleCounts && (
            <ScriptCodeClassSummary
              classes={data.classSummary}
              lockScriptCount={data.roleCounts.locks}
            />
          )}
          <section aria-label="Script list" className="min-w-0">
            <div className="text-text-dim mb-3 flex flex-wrap items-center gap-2 text-xs">
              <label htmlFor="scripts-sort">sorted by</label>
              <select
                id="scripts-sort"
                aria-label="Sort scripts"
                value={sortKey}
                onChange={(event) => {
                  const next = event.target.value as SortKey;
                  sortBy(next, next === 'name' ? 'asc' : 'desc');
                }}
                className="border-base-border bg-base-surface text-text rounded border px-2 py-1 text-xs"
              >
                <option value="capacity">CKB in their cells</option>
                <option value="used">Occupied CKB</option>
                <option value="liveCells">Live cells</option>
                <option value="cells">Cells ever</option>
                <option value="name">Name</option>
              </select>
              <button
                type="button"
                aria-label="Reverse order"
                onClick={() => sortBy(sortKey, sortDirection === 'asc' ? 'desc' : 'asc')}
                className="hover:text-text-bright"
              >
                {sortDirection === 'asc' ? '↑' : '↓'}
              </button>
              {data?.total !== undefined && data.total !== null && (
                <span className="ml-auto">{formatCount(data.total, 'script')}</span>
              )}
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
                      {s.scriptKind ? s.scriptKind.replace('+', ' + ') : 'unregistered'}
                    </div>
                    <ScriptCodeClass script={s} />
                    <div className="flex flex-wrap justify-between gap-2">
                      <span>
                        {countText(s.liveCellsCount)} live cells · of {countText(s.cellsCount)} ever
                      </span>
                      <span>{capacityText(s.ownedCapacitySum!)}</span>
                    </div>
                    <div className="text-text-dim">Occupied {occupiedShare(s).share}</div>
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
          </section>
        </DetailLayout>
      </main>
    </div>
  );
}
