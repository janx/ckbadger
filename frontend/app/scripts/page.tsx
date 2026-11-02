'use client';

import { useEffect, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useInRouterContext } from 'react-router-dom';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { AppLink } from '@/components/ui/app-link';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { PhoneRow } from '@/components/ui/phone-row';
import { AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { RowTag } from '@/components/ui/row-tag';
import {
  ScriptCodeClassBar,
  ScriptCodeClassLegend,
  ScriptCodeClassSummary,
  codeClassTotals,
  isImmutableClass,
  scriptClassLabel,
  scriptShare,
} from '@/components/scripts/script-code-evidence';
import { HeroFigure } from '@/components/scripts/script-hero';
import { ExactCkb } from '@/components/activity-event-row';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type KnownScript, type ScriptsListApi } from '@/lib/api';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { getScriptRefBadgeLabel, normalizeScriptRefHashType } from '@/lib/script-ref';
import { formatCkbAmount, formatUtc8Date, truncateHash } from '@/lib/utils';
import { formatCount } from '@/lib/format-count';
import { formatTick, niceCeil } from '@/lib/chart-ticks';
import { useDocumentTitle } from '@/lib/use-document-title';

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

/** The board's Code column as words: "Immutable" or "Upgradable · N versions", then by whom. */
function scriptCodeClass(
  script: KnownScript
): { head: string; sub: string; replaceable: boolean } | null {
  const classes = script.codeClasses ?? [];
  if (!classes.length) return null;
  const replaceable = classes.filter((row) => !isImmutableClass(row.class));
  const immutable = classes.filter((row) => isImmutableClass(row.class));
  if (script.firstDeployedAt === undefined)
    throw new Error(`Script family ${script.name} row has no firstDeployedAt`);
  const first = script.firstDeployedAt;
  if (!replaceable.length)
    return {
      head: 'Immutable',
      sub: [
        ...(first ? [first.blockNumber === 0 ? 'genesis' : formatUtc8Date(first.timestampMs)] : []),
        ...[...immutable]
          .sort((a, b) => Number(b.class === 'unspendable') - Number(a.class === 'unspendable'))
          .map((row) => (row.class === 'fixed' ? 'pinned by data hash' : 'code cell unspendable')),
      ].join(' · '),
      replaceable: false,
    };
  if (script.versionsCount === undefined)
    throw new Error(`Script family ${script.name} has replaceable code but no versionsCount`);
  return {
    head: `Upgradable · ${formatCount(script.versionsCount, 'version')}`,
    sub: [
      `by ${replaceable.map((row) => GOVERNOR[row.class] ?? scriptClassLabel(row.class).toLowerCase()).join(' or ')}`,
      ...(first ? [`since ${formatUtc8Date(first.timestampMs)}`] : []),
      ...(immutable.length ? ['some references pinned'] : []),
    ].join(' · '),
    replaceable: true,
  };
}

/** The pip of the Code column: a hollow ring for upgradable code, a filled dot for immutable. */
function CodeClassPip({ replaceable }: { replaceable: boolean }) {
  return (
    <svg width="10" height="10" aria-hidden="true" className="shrink-0">
      <circle
        cx="5"
        cy="5"
        r="3.8"
        fill={replaceable ? '#0c0e15' : '#a0a8be'}
        fillOpacity={replaceable ? 1 : 0.55}
        stroke={replaceable ? '#dee2ec' : '#a0a8be'}
      />
    </svg>
  );
}

/**
 * The Code column on one line (table-rows L1-1, board Scripts): "immutable" or "upgradable · N
 * versions" in dim ink with its pip; by whom and since when are its title (and the row's).
 */
function ScriptCodeClass({ script }: { script: KnownScript }) {
  const code = scriptCodeClass(script);
  return (
    <span
      data-testid="script-code-class"
      title={codeStatusText(script)}
      className="text-text-dim flex min-w-0 items-center gap-2 whitespace-nowrap"
    >
      {code && <CodeClassPip replaceable={code.replaceable} />}
      <span className="truncate">
        {code ? code.head.toLowerCase() : 'code evidence unavailable'}
      </span>
    </span>
  );
}

/** "upgradable · 5 versions · by a multisig · since 2023-10-07": the code status in full. */
function codeStatusText(script: KnownScript): string {
  const code = scriptCodeClass(script);
  return code ? `${code.head.toLowerCase()} · ${code.sub}` : 'code evidence unavailable';
}

/**
 * A script family on a phone (board L6): name and the CKB its cells hold; role, code class and
 * cell counts with the occupied share; the description in full, never clamped. In a wide
 * container (640) two lines: role and code join the name, the counts share the description's line.
 */
function ScriptPhoneRow({ script: s, wide }: { script: KnownScript; wide: boolean }) {
  const code = scriptCodeClass(s);
  const role = s.scriptKind ?? 'unregistered';
  const kind = (
    <span className="inline-flex flex-wrap items-center gap-x-1.5">
      {role}
      <span className="text-text-dim">·</span>
      {code ? (
        <>
          <CodeClassPip replaceable={code.replaceable} />
          <span className={code.replaceable ? 'text-text-bright' : undefined}>{code.head}</span>
          <span className="text-text-dim">· {code.sub}</span>
        </>
      ) : (
        <span className="text-text-dim">code evidence unavailable</span>
      )}
    </span>
  );
  const cells = (
    <span className="whitespace-nowrap">
      {countText(s.liveCellsCount)} live · of {countText(s.cellsCount)} · {occupiedShare(s).share}{' '}
      occupied
    </span>
  );
  const name = (
    <>
      <AppLink
        href={scriptHref(s)}
        title={!hasName(s) ? `${s.hashType} ref:${s.codeHash}` : undefined}
        className="text-text-bright hover:text-emphasis font-medium hover:underline"
      >
        {hasName(s) ? s.name!.trim() : 'Unlabeled'}
      </AppLink>
      {s.deprecated && <span className="text-text-dim text-meta ml-2">deprecated</span>}
    </>
  );
  const description = hasName(s)
    ? s.description
    : `${getScriptRefBadgeLabel(s.hashType)} · ${truncateHash(s.codeHash, 10, 8)}`;
  return (
    <PhoneRow
      testId="script-row-mobile"
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: wide ? (
            <>
              {name}
              <span className="text-text-dim text-small"> · </span>
              <span className="text-text-dim text-small">{kind}</span>
            </>
          ) : (
            name
          ),
          right: <span className="text-text-bright">{capacityText(s.ownedCapacitySum!)}</span>,
        },
        !wide && { key: 'code', tone: 'body', left: kind, right: cells },
        {
          key: 'description',
          tone: 'quiet',
          left: description,
          right: wide ? cells : undefined,
        },
      ]}
    />
  );
}

/** The exact CKB in a family's live cells and the occupied part of it, checked. */
function heldCapacity(script: KnownScript): { total: bigint; occupied: bigint } {
  if (script.ownedCapacitySum === undefined || script.ownedKnowledgeSum === undefined)
    throw new Error(`Script ${script.codeHash} row has no owned capacity`);
  const total = BigInt(script.ownedCapacitySum);
  const occupied = BigInt(script.ownedKnowledgeSum);
  if (occupied < BigInt(0) || occupied > total)
    throw new Error(`Script ${script.codeHash} has invalid occupied capacity`);
  return { total, occupied };
}

function occupiedShare(script: KnownScript): { share: string } {
  const { total, occupied } = heldCapacity(script);
  return { share: total === BigInt(0) ? '0%' : scriptShare(occupied, total) };
}

function capacityText(value: string) {
  return <ExactCkb shannons={BigInt(value)} signed={false} tone="plain" />;
}

function countText(value: number | undefined): string {
  return value === undefined ? 'unavailable' : value.toLocaleString('en-US');
}

function roleText(script: KnownScript): string {
  return script.scriptKind ? script.scriptKind.replace('+', ' + ') : 'unregistered';
}

/** An unlabeled family's identity: its reference, short. */
function refText(script: KnownScript): string {
  return `${getScriptRefBadgeLabel(script.hashType)} · ${truncateHash(script.codeHash, 10, 8)}`;
}

/**
 * The Script column on one line (T5): the name at 13 px bright, its role and a ghost "deprecated"
 * as row tags. An unlabeled family keeps its short reference beside the tag (it is its identity);
 * a description is the row's title.
 */
function ScriptName({ script }: { script: KnownScript }) {
  const name = hasName(script) ? script.name!.trim() : 'Unlabeled';
  return (
    <span className="flex min-w-0 items-center gap-2">
      <AppLink
        href={scriptHref(script)}
        title={!hasName(script) ? `${script.hashType} ref:${script.codeHash}` : undefined}
        className="text-body text-text-bright hover:text-emphasis min-w-0 truncate leading-[18px] hover:underline"
      >
        {name}
      </AppLink>
      <RowTag>{roleText(script)}</RowTag>
      {script.deprecated && <RowTag variant="ghost">deprecated</RowTag>}
      {!hasName(script) && (
        <span className="text-text-dim text-meta truncate">{refText(script)}</span>
      )}
    </span>
  );
}

/** "454,352,694.86": an exact CKB amount cut (never rounded up) to 2 decimals; the exact one is the row's title. */
function CkbTwoDp({ shannons }: { shannons: bigint }) {
  const { integer, decimal } = formatCkbAmount(shannons);
  return (
    <span data-ckb-2dp className="whitespace-nowrap">
      {integer}
      <span className="text-text-dim text-meta">.{decimal.slice(0, 2)}</span>
    </span>
  );
}

/** A column head at 11 px (T5). */
function Head({ label }: { label: string }) {
  return <span className="text-meta">{label}</span>;
}

/** The bar column's track (px): every row and the header ticks share it. */
const CKB_TRACK = 240;
/**
 * The printed amount right of the track: the board's 118 px plus the room "46,743,011,404.23" takes
 * at 12 px with its 11 px decimals, so the largest amount on the page never wraps.
 */
const CKB_DISPLAY = 128;
const SHANNONS_PER_CKB = BigInt(100000000);

/** Whole CKB, for the bar's geometry only (the printed amount and the title stay exact). */
function wholeCkb(shannons: bigint): number {
  return Number(shannons / SHANNONS_PER_CKB);
}

/**
 * The bar column's scale: the page's largest whole-CKB amount rounded up to a round 1 / 1.5 / 2 /
 * 2.5 / 3 / 4 / 5 / 6 / 8 × 10^k (board: 46.7B → "0 · 25B · 50B"). A page whose scripts hold
 * nothing draws empty tracks on a scale of 1.
 */
function ckbBarScale(values: readonly number[]): number {
  const max = values.length ? Math.max(...values) : 0;
  if (max === 0) return 1;
  const scale = Math.round(niceCeil(max));
  if (!Number.isSafeInteger(scale) || scale < max)
    throw new Error(`Scripts bar scale ${scale} does not cover ${max} CKB`);
  return scale;
}

/** What the list's bar shows (ruling D2): the sort column's CKB, or the occupied share of it. */
type BarField = 'ckb' | 'occupied';
const BAR_FIELDS: readonly { value: BarField; label: string }[] = [
  { value: 'ckb', label: 'CKB' },
  { value: 'occupied', label: 'occupied' },
];

function barFromUrl(raw: string | null): BarField {
  return raw === 'occupied' ? 'occupied' : 'ckb';
}

/** The occupied share's bar runs on parts per million of the CKB in the cells. */
const SHARE_SCALE = 1_000_000;

/**
 * Occupied ÷ CKB in the cells, in ppm, for the bar's geometry: rounded up, so a nonzero share is
 * never drawn empty (the printed share and the title stay exact).
 */
function occupiedPpm(script: KnownScript): number {
  const { total, occupied } = heldCapacity(script);
  if (total === BigInt(0)) return 0;
  const scale = BigInt(SHARE_SCALE);
  return Number((occupied * scale + total - BigInt(1)) / total);
}

/** "0 · 50 · 100 %": the occupied-share scale's ticks. */
function shareTick(ppm: number): string {
  const percent = (ppm / SHARE_SCALE) * 100;
  return ppm === SHARE_SCALE ? `${percent} %` : String(percent);
}

/**
 * The row's hover (D3): every field that left the row — role, description, the code status with
 * holder and date, cells ever, the exact amount and the occupied CKB with its share.
 */
function scriptRowTitle(script: KnownScript): string {
  const { total, occupied } = heldCapacity(script);
  return [
    hasName(script)
      ? script.name!.trim()
      : `Unlabeled ${script.hashType ?? ''} ref ${script.codeHash}`,
    roleText(script),
    ...(script.deprecated ? ['deprecated'] : []),
    ...(hasName(script) && script.description?.trim() ? [script.description.trim()] : []),
    codeStatusText(script),
    `${countText(script.liveCellsCount)} live of ${countText(script.cellsCount)} cells ever`,
    `${formatCkbAmount(total).full} CKB`,
    `${formatCkbAmount(occupied).full} occupied (${occupiedShare(script).share})`,
  ].join(' · ');
}

/** The board's lead sentence: what a lock and a type script are (tier 2 since hierarchy v3). */
const LEAD =
  'Every cell runs one lock script, which decides who can spend it, and may run one type script, which decides what the cell is and how it may change.';

/**
 * The hero's figure and picture (hierarchy v3, C0 / C1): the CKB under these scripts' locks with
 * the script counts on its baseline row, then "who can change the code" as a 28 px bar, its key in
 * one quiet row and one caption. From the role-less summary, so the table's tabs never move it.
 */
function ScriptsHeroFigure({ summary }: { summary: ScriptsListApi }) {
  // As before v3: without the class summary or the role counts the page states no total.
  if (!summary.classSummary || !summary.roleCounts) return null;
  const { total } = codeClassTotals(summary.classSummary);
  const counts = summary.roleCounts;
  return (
    <>
      <HeroFigure
        label="CKB under these scripts"
        shannons={total}
        companion={`locked by ${formatCount(counts.locks, 'lock script')} · ${formatCount(counts.all, 'script')} in all · ${formatCount(counts.types, 'type')}`}
      />
      <p className="text-text-dim text-meta mb-1.5 mt-[18px]">who can change the code</p>
      <ScriptCodeClassBar classes={summary.classSummary} />
    </>
  );
}

const SCRIPTS_TITLE = ['Scripts'] as const;

export default function ScriptsPage() {
  useDocumentTitle(SCRIPTS_TITLE);
  const searchParams = useSearchParams();
  const pathname = usePathname();
  const router = useRouter();
  const inRouter = useInRouterContext();
  const urlBar = barFromUrl(searchParams.get('bar'));
  const [bar, setBar] = useState<BarField>(urlBar);
  // D2: the bar follows its URL, also when a link or history entry changes it under this page.
  useEffect(() => setBar(urlBar), [urlBar]);
  const chooseBar = (value: BarField) => {
    setBar(value);
    // Replace: a view choice is not a step of history. Outside a router there is no address to
    // keep in step, and leaving the page would reload it.
    if (inRouter) {
      const query = new URLSearchParams(searchParams);
      if (value === 'ckb') query.delete('bar');
      else query.set('bar', value);
      router.replace(query.size ? `${pathname}?${query}` : pathname, { scroll: false });
    }
  };
  const pagination = useCursorPagination();
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>();
  const [role, setRole] = useState<'lock' | 'type' | undefined>();
  const [sortKey, setSortKey] = useState<SortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');
  const pageSize = usePageSize(DEFAULT_PAGE_SIZE);
  // The table's page (declared first: its request goes first).
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['scripts', pagination.cursor, pageSize, search, role, sortKey, sortDirection],
    queryFn: () =>
      api.getScripts({
        limit: pageSize,
        cursor: pagination.cursor,
        decoderType: undefined,
        search,
        role,
        sortKey,
        sortDirection,
      }),
    placeholderData: keepPreviousData,
  });
  // The hero's figure and bar (hierarchy v3): every script the search matches, never the role tab
  // under it (the API's class summary follows the role filter), so one row is enough.
  const summary = useQuery({
    queryKey: ['scripts-summary', search],
    queryFn: () => api.getScripts({ limit: 1, search }),
    placeholderData: keepPreviousData,
  });

  function sortBy(next: SortKey, direction: SortDirection) {
    setSortKey(next);
    setSortDirection(direction);
    pagination.reset();
  }

  const rows = data?.data ?? [];
  const ckbScale = ckbBarScale(rows.map((s) => wholeCkb(heldCapacity(s).total)));
  const barScale = bar === 'occupied' ? SHARE_SCALE : ckbScale;
  const columns: ListColumn<KnownScript>[] = [
    {
      key: 'script',
      label: 'Script',
      header: <Head label="Script" />,
      minWidth: 220,
      width: 'auto',
      render: (s) => <ScriptName script={s} />,
    },
    {
      key: 'code',
      label: 'Code',
      header: <Head label="Code" />,
      minWidth: 220,
      render: (s) => <ScriptCodeClass script={s} />,
    },
    {
      key: 'cells',
      label: 'Live cells',
      header: <Head label="Live cells" />,
      minWidth: 110,
      numeric: true,
      render: (s) => (
        <span
          title={`${countText(s.liveCellsCount)} live of ${countText(s.cellsCount)} cells ever`}
        >
          {countText(s.liveCellsCount)}
        </span>
      ),
    },
    {
      key: 'capacity',
      label: 'CKB in its cells',
      // track + 10 px + the printed amount + the cell's 16 px gutter
      minWidth: CKB_TRACK + 10 + CKB_DISPLAY + 16,
      header: (
        <ScaleHeader
          label={bar === 'occupied' ? 'CKB in its cells · bar: occupied share' : 'CKB in its cells'}
          scale={barScale}
          trackWidth={CKB_TRACK}
          midpoint
          format={bar === 'occupied' ? shareTick : (n) => formatTick(n, ckbScale / 2)}
        />
      ),
      render: (s) => {
        const { total } = heldCapacity(s);
        return (
          <AlignedBar
            label={bar === 'occupied' ? 'Occupied share of CKB in its cells' : 'CKB in its cells'}
            scale={barScale}
            trackWidth={CKB_TRACK}
            minFill={2}
            segments={[
              bar === 'occupied'
                ? { key: 'occupied', label: 'occupied', value: occupiedPpm(s), tone: 'data' }
                : { key: 'ckb', label: 'CKB', value: wholeCkb(total), tone: 'data' },
            ]}
            display={<CkbTwoDp shannons={total} />}
            displayWidth={CKB_DISPLAY}
          />
        );
      },
    },
    {
      key: 'occupied',
      label: 'Occupied',
      header: <Head label="Occupied" />,
      minWidth: 90,
      numeric: true,
      render: (s) => <span className="text-text-dim">{occupiedShare(s).share}</span>,
    },
  ];

  const searchForm = (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        setSearch(searchInput.trim() || undefined);
        pagination.reset();
      }}
      // One inline group (as the DAO page's range buttons): one line on the title row.
      className="min-w-0 whitespace-nowrap"
    >
      <input
        value={searchInput}
        onChange={(event) => setSearchInput(event.target.value)}
        placeholder="Search by name or code hash"
        aria-label="Search scripts"
        className="border-base-border bg-base-surface text-text-bright text-small h-7 w-48 rounded border px-3 align-middle sm:w-64"
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
          className="text-text-dim ml-2 inline-flex h-7 items-center align-middle"
        >
          ×
        </button>
      )}
      <button
        type="submit"
        className="border-base-border text-text-bright text-small ml-2 inline-flex h-7 items-center rounded border px-3 align-middle"
      >
        Search
      </button>
    </form>
  );

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="text-body min-w-0 font-mono">
          {/* Hierarchy v3 (R13, R6): hero → the list 64 px; tier 2 after the 96 px boundary. */}
          <div data-tier="1" className="min-w-0">
            <section aria-label="Scripts" className="min-w-0">
              <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
                <h1 className="text-text-bright text-index font-medium">Scripts</h1>
                {searchForm}
              </div>
              <p data-role="lede" className="text-text-dim text-body mt-1.5 max-w-[560px]">
                Locks decide who can spend a cell; types, what it is.{' '}
                <a
                  href="#evidence"
                  className="hover:text-text-bright whitespace-nowrap hover:underline"
                >
                  How it works ↓
                </a>
              </p>
              {summary.isError ? (
                <div role="alert" className="text-negative text-small mt-9">
                  <strong>Failed to load the script summary</strong>:{' '}
                  {summary.error instanceof Error ? summary.error.message : 'Unknown error'}
                </div>
              ) : summary.data ? (
                <ScriptsHeroFigure summary={summary.data} />
              ) : (
                <div className="text-text-dim text-small mt-9">Loading the script summary…</div>
              )}
            </section>
            {summary.data?.classSummary && (
              <>
                <div className="mt-2.5">
                  <ScriptCodeClassLegend classes={summary.data.classSummary} />
                </div>
                <p data-role="caption" className="text-text text-body mt-3.5 max-w-[640px]">
                  Code pinned by its data hash, or held in a code cell no one can spend, can never
                  change; whoever can spend any other code cell can replace its code.
                </p>
              </>
            )}
            <section aria-label="Script list" className="mt-16 min-w-0">
              <div
                role="group"
                aria-label="Role"
                className="border-base-border mb-[18px] flex flex-wrap gap-7 border-b"
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
                    className={`text-section -mb-px flex h-11 items-center gap-2 border-b-2 font-medium ${role === value ? 'text-text-bright border-[#2edba3]' : 'text-text-dim border-transparent'}`}
                  >
                    {label}{' '}
                    {count !== undefined && (
                      <span className="text-text-dim text-small font-normal">
                        {count.toLocaleString('en-US')}
                      </span>
                    )}
                  </button>
                ))}
              </div>
              <div className="text-text-dim text-small mb-3 flex flex-wrap items-center gap-2">
                <label htmlFor="scripts-sort">sorted by</label>
                <select
                  id="scripts-sort"
                  aria-label="Sort scripts"
                  value={sortKey}
                  onChange={(event) => {
                    const next = event.target.value as SortKey;
                    sortBy(next, next === 'name' ? 'asc' : 'desc');
                  }}
                  className="border-base-border bg-base-surface text-text text-small rounded border px-2 py-1"
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
                {/* D2: what the bar shows, beside the sort it pictures; written to the URL. */}
                <span
                  role="group"
                  aria-label="Bar shows"
                  className="ml-3 inline-flex items-center whitespace-nowrap"
                >
                  <span className="mr-2">bar:</span>
                  {BAR_FIELDS.map((field, i) => (
                    <button
                      key={field.value}
                      type="button"
                      aria-pressed={bar === field.value}
                      onClick={() => chooseBar(field.value)}
                      className={`text-small inline-flex h-6 items-center border px-2 ${i === 0 ? 'rounded-l-[4px]' : '-ml-px rounded-r-[4px]'} ${bar === field.value ? 'border-base-border bg-base-border-subtle text-text-bright' : 'border-base-border-subtle text-text hover:text-text-bright'}`}
                    >
                      {field.label}
                    </button>
                  ))}
                </span>
                {data?.total !== undefined && data.total !== null && (
                  <span className="ml-auto">{formatCount(data.total, 'script')}</span>
                )}
              </div>
              {isLoading ? (
                <div className="text-text-dim text-small py-8">Loading scripts...</div>
              ) : isError ? (
                <div role="alert" className="text-negative text-small py-8">
                  <strong>Failed to load scripts</strong>:{' '}
                  {error instanceof Error ? error.message : 'Unknown error'}
                </div>
              ) : (
                <ListTable
                  label="Scripts"
                  columns={columns}
                  rows={rows}
                  rowKey={(s) => s.codeHash}
                  empty="No scripts found"
                  density="row"
                  rowTitle={scriptRowTitle}
                  // T5: body ink for the row, the name alone bright.
                  rowClassName={() => 'text-text'}
                  mobileRow={(s, wide) => <ScriptPhoneRow script={s} wide={wide} />}
                />
              )}
              {data && data.data.length > 0 && (
                <div className="border-base-border mt-4 border-t pt-4">
                  <CursorPagination
                    total={data.total ?? undefined}
                    totalLabel="scripts"
                    pageSize={pageSize}
                    page={pagination.page}
                    hasMore={data.hasMore}
                    hasPrevious={pagination.hasPrevious}
                    onNext={() => pagination.goToNext(data.nextCursor)}
                    onPrevious={pagination.goToPrevious}
                  />
                </div>
              )}
            </section>
          </div>
          <div id="evidence" className="scroll-mt-4">
            <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-2">
              <DetailSection title="How scripts work">
                <p className="text-body max-w-[640px]">{LEAD}</p>
              </DetailSection>
              {summary.data?.classSummary && summary.data.roleCounts && (
                <ScriptCodeClassSummary
                  title="Who can change the code, class by class"
                  classes={summary.data.classSummary}
                  lockScriptCount={summary.data.roleCounts.locks}
                />
              )}
            </DetailTier2>
          </div>
        </div>
      </main>
    </div>
  );
}
