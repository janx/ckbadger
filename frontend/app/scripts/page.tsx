'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import { AppLink } from '@/components/ui/app-link';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { Prose } from '@/components/ui/prose';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { WIDEST_CKB_HELD, WIDEST_ROW_COUNT } from '@/lib/list-bounds';
import { PhoneRow } from '@/components/ui/phone-row';
import { AlignedBar, BAR_FIGURE_GAP, ScaleHeader, trackPx } from '@/components/ui/aligned-bar';
import { RowTag } from '@/components/ui/row-tag';
import {
  ScriptCodeClassBar,
  ScriptCodeClassLegend,
  ScriptCodeClassSummary,
  codeClassTotals,
  DEPRECATED_TITLE,
  immutableHow,
  isImmutableClass,
  scriptClassLabel,
} from '@/components/scripts/script-code-evidence';
import { HeroFigure } from '@/components/scripts/script-hero';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type KnownScript, type ScriptsListApi } from '@/lib/api';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { getScriptRefBadgeLabel, normalizeScriptRefHashType } from '@/lib/script-ref';
import { formatCkbAmount, formatUtc8Date } from '@/lib/utils';
import { shortenHash } from '@/lib/shorten';
import { formatCount } from '@/lib/format-count';
import { useDocumentTitle } from '@/lib/use-document-title';
import { percentOf, withUnit } from '@/lib/format-unit';
import { unitNode } from '@/components/ui/units';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';

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
        ...immutableHow(immutable.map((row) => row.class)),
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
        fill={replaceable ? 'var(--c-base-bg)' : 'var(--c-text)'}
        fillOpacity={replaceable ? 1 : 0.55}
        stroke={replaceable ? 'var(--c-text-bright)' : 'var(--c-text)'}
      />
    </svg>
  );
}

/**
 * The Code column on one line (table-rows L1-1, board Scripts): "upgradable · N versions" in dim
 * ink with its pip; immutable code — the rarer, stronger fact — is the cell's one accent, a jade
 * "immutable" row tag (lane F follow-up). By whom, since when and how it is pinned are its title
 * (and the row's).
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
      {code && !code.replaceable ? (
        <RowTag variant="accent">immutable</RowTag>
      ) : (
        <span className="truncate">
          {code ? code.head.toLowerCase() : 'code evidence unavailable'}
        </span>
      )}
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
          {/* Lane F follow-up: immutable is the accent tag; the name stays the one bright thing. */}
          {code.replaceable ? (
            <span>{code.head}</span>
          ) : (
            <RowTag variant="accent">immutable</RowTag>
          )}
          <span className="text-text-dim">· {code.sub}</span>
        </>
      ) : (
        <span className="text-text-dim">code evidence unavailable</span>
      )}
    </span>
  );
  // The counts as two facts, each whole: from 560 px one line beside the description (50
  // characters at most, 390 px); below it two lines by construction, live of ever over the occupied
  // share (the coordinator's sweep, F3-e: one line of 45 glyphs, 351 px, stood past a 328 px row),
  // the widest "99,999,999 live · of 99,999,999" (31 characters at 13 px, 241.8).
  const liveOf = `${countText(s.liveCellsCount)} live · of ${countText(s.cellsCount)}`;
  const occupied = `${occupiedShare(s).share} occupied`;
  const cells = wide ? (
    <span className="whitespace-nowrap">
      {liveOf} · {occupied}
    </span>
  ) : (
    <span className="inline-flex flex-col items-end">
      <span className="whitespace-nowrap">{liveOf}</span>
      <span className="whitespace-nowrap">{occupied}</span>
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
      {s.deprecated && (
        <RowTag variant="warn" title={DEPRECATED_TITLE} className="ml-2">
          deprecated
        </RowTag>
      )}
    </>
  );
  const description = hasName(s)
    ? s.description
    : `${getScriptRefBadgeLabel(s.hashType)} · ${shortenHash(s.codeHash, 'tight')}`;
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
  return { share: total === BigInt(0) ? '0.00%' : percentOf(occupied, total) };
}

function capacityText(value: string) {
  return <CkbAmount form="summary" shannons={BigInt(value)} signed={false} tone="plain" />;
}

function countText(value: number | undefined): string {
  return value === undefined ? 'unavailable' : value.toLocaleString('en-US');
}

/**
 * A row's live and ever cells for its hover: "1,234 live of 5,678 cells ever", the noun by the one
 * plural rule (U-15), or a plain sentence when the API states no count.
 */
function cellsEverText(script: KnownScript): string {
  if (script.liveCellsCount === undefined || script.cellsCount === undefined)
    return 'cell counts unavailable';
  return `${script.liveCellsCount.toLocaleString('en-US')} live of ${formatCount(script.cellsCount, 'cell')} ever`;
}

function roleText(script: KnownScript): string {
  return script.scriptKind ? script.scriptKind.replace('+', ' + ') : 'unregistered';
}

/** An unlabeled family's identity in the table: its reference at the row size (U-09). */
function refText(script: KnownScript): string {
  return `${getScriptRefBadgeLabel(script.hashType)} · ${shortenHash(script.codeHash, 'row')}`;
}

/**
 * The Script column on one line (T5): the name at 13 px bright, its role and a warn "deprecated"
 * (lane F follow-up 2) as row tags. An unlabeled family keeps its short reference beside the tag (it is its identity);
 * a description is the row's title.
 */
function ScriptName({ script }: { script: KnownScript }) {
  const name = hasName(script) ? script.name!.trim() : 'Unlabeled';
  return (
    <span className="flex min-w-0 items-center gap-2">
      <AppLink
        href={scriptHref(script)}
        title={!hasName(script) ? `${script.hashType} ref:${script.codeHash}` : undefined}
        className="text-body text-text-bright hover:text-emphasis leading-row min-w-0 truncate hover:underline"
      >
        {name}
      </AppLink>
      <RowTag>{roleText(script)}</RowTag>
      {script.deprecated && (
        <RowTag variant="warn" title={DEPRECATED_TITLE}>
          deprecated
        </RowTag>
      )}
      {!hasName(script) && (
        <span className="text-text-dim text-meta truncate">{refText(script)}</span>
      )}
    </span>
  );
}

/** A column head at 11 px (T5). */
function Head({ label }: { label: string }) {
  return <span className="text-meta">{label}</span>;
}

/** The occupied share's bar runs on parts per million of the CKB in the cells. */
const SHARE_SCALE = 1_000_000;

/**
 * The list's one bar, the occupied share (readability B5: the CKB bar left and with it the "bar:"
 * switch of ruling D2). Its track: its head's ticks, "0", "50" and "100%", whole over it a glyph
 * apart at 12 px (64.8; readability F3-e: it was 80). Its figure: "100.00%", 7 characters at 13 px
 * (54.6).
 */
const SHARE_TRACK = trackPx(['0', shareTick(SHARE_SCALE / 2), shareTick(SHARE_SCALE)]);
export const SHARE_FIGURE = columnPx(textPx('100.00%'.length, fontPx('small')));

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

/** "0 · 50 · 100%": the occupied-share scale's ticks, the sign on the last (CH-14). */
function shareTick(ppm: number): string {
  const percent = (ppm / SHARE_SCALE) * 100;
  return ppm === SHARE_SCALE ? withUnit(String(percent), '%') : String(percent);
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
    cellsEverText(script),
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
        companion={[
          `locked by ${formatCount(counts.locks, 'lock script')}`,
          `${formatCount(counts.all, 'script')} in all`,
          formatCount(counts.types, 'type'),
        ]}
      />
      <p className="text-text-dim text-meta mb-1.5 mt-[18px]">who can change the code</p>
      <ScriptCodeClassBar classes={summary.classSummary} />
    </>
  );
}

const SCRIPTS_TITLE = ['Scripts'] as const;

export default function ScriptsPage() {
  useDocumentTitle(SCRIPTS_TITLE);
  const pagination = useCursorPagination();
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>();
  const [role, setRole] = useState<'lock' | 'type' | undefined>();
  const [sortKey, setSortKey] = useState<SortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');
  const pageSize = usePageSize(DEFAULT_PAGE_SIZE);
  // The table's page (declared first: its request goes first).
  const read = useQuery({
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
  const { data, isError, error } = read;
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
  const columns: ListColumn<KnownScript>[] = [
    {
      key: 'script',
      label: 'Script',
      header: <Head label="Script" />,
      // The flexible column (readability F3-e): at its minimum the default lock's name whole,
      // "Secp256k1 Blake160" (18 characters at 14 px, 151.2), its 8 px gap and the widest role tag,
      // "unregistered" (86.4 and the tag's 12 px of sides and border), and the gap: 273.6; it takes
      // what the container gives beyond the sum, a longer name ending in the one ellipsis (the
      // row's title holds it). It was 220.
      minWidth: columnPx(
        textPx('Secp256k1 Blake160'.length, fontPx('body')) +
          8 +
          textPx('unregistered'.length, fontPx('meta')) +
          2 * 5 +
          2 * 1 +
          CELL_GAP_PX
      ),
      width: 'auto',
      render: (s) => <ScriptName script={s} />,
    },
    {
      key: 'code',
      label: 'Code',
      header: <Head label="Code" />,
      // The pip (10 px), its 8 px gap and "upgradable · 99 versions" (24 characters at 13 px,
      // 187.2), wider than "code evidence unavailable" (195, no pip) and the "immutable" tag, and
      // the gap: 221.2. It was 220.
      minWidth: columnPx(
        Math.max(
          10 + 8 + textPx('upgradable · 99 versions'.length, fontPx('small')),
          textPx('code evidence unavailable'.length, fontPx('small'))
        ) + CELL_GAP_PX
      ),
      render: (s) => <ScriptCodeClass script={s} />,
    },
    {
      key: 'cells',
      label: 'Live cells',
      header: <Head label="Live cells" />,
      // Eight digits of live cells, or "unavailable" (11 characters at 13 px, 85.8, the wider),
      // and the gap. It was 110.
      minWidth: columnPx(
        textPx(Math.max(WIDEST_ROW_COUNT.length, 'unavailable'.length), fontPx('small')) +
          CELL_GAP_PX
      ),
      numeric: true,
      render: (s) => <span title={cellsEverText(s)}>{countText(s.liveCellsCount)}</span>,
    },
    {
      key: 'capacity',
      label: 'CKB in its cells',
      // B5 (plan §7.6): the amount alone, right aligned under its head; its linear bar, scaled to
      // the page's largest (50B), was a 1 px stub on most rows. The printed amount, whole CKB under
      // 100 billion in the summary form ("99,999,999,999", 14 characters at 13 px, 109.2), and the
      // gap (readability F3-e: it was 144 for a two-decimal form the list no longer writes).
      header: <Head label="CKB in its cells" />,
      minWidth: columnPx(textPx(WIDEST_CKB_HELD.length, fontPx('small')) + CELL_GAP_PX),
      numeric: true,
      render: (s) => (
        <CkbAmount
          form="summary"
          shannons={heldCapacity(s).total}
          signed={false}
          tone="inherit"
          unit={false}
        />
      ),
    },
    {
      key: 'occupied',
      label: 'Occupied',
      // The occupied share beside its bar on 0 · 50 · 100 %, the name over the share (B5): the
      // track, the bar's 10 px gap and the share; the last column, no gap after it (it counted 16).
      minWidth: columnPx(SHARE_TRACK + BAR_FIGURE_GAP + SHARE_FIGURE),
      header: (
        <ScaleHeader
          label="Occupied"
          scale={SHARE_SCALE}
          trackWidth={SHARE_TRACK}
          midpoint
          format={shareTick}
          figureWidth={SHARE_FIGURE}
        />
      ),
      render: (s) => (
        <AlignedBar
          label="Occupied share of CKB in its cells"
          scale={SHARE_SCALE}
          trackWidth={SHARE_TRACK}
          minFill={2}
          segments={[{ key: 'occupied', label: 'occupied', value: occupiedPpm(s), tone: 'data' }]}
          display={<span className="text-text-dim">{occupiedShare(s).share}</span>}
          displayWidth={SHARE_FIGURE}
        />
      ),
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
        placeholder="search by name or code hash"
        aria-label="Search scripts"
        className="border-base-border bg-base-surface text-text-bright text-small h-7 w-48 border px-3 align-middle sm:w-64"
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
        className="border-base-border text-text-bright text-small ml-2 inline-flex h-7 items-center border px-3 align-middle"
      >
        Search
      </button>
    </form>
  );

  return (
    <div className="min-h-screen">
      <main className="container mx-auto py-8">
        <div className="text-body min-w-0 font-mono">
          {/* Hierarchy v3 (R13, R6): hero → the list 64 px; tier 2 after the 96 px boundary. */}
          <div data-tier="1" className="min-w-0">
            <section aria-label="Scripts" className="min-w-0">
              <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
                <h1 className="text-text-bright text-index font-medium">Scripts</h1>
                {searchForm}
              </div>
              <p data-role="lede" className="text-text-dim text-body max-w-measure mt-1.5">
                Locks decide who can spend a cell; types, what it is.{' '}
                <a href="#evidence" className="hover:text-text-bright hover:underline">
                  How it works<span className="whitespace-nowrap"> ↓</span>
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
                <p data-role="caption" className="text-text text-body max-w-measure mt-3.5">
                  Code pinned by its data hash, or held in a code cell no one can spend, can never
                  change; whoever can spend any other code cell can replace its code.
                </p>
              </>
            )}
            <section aria-label="Script list" className="mt-16 min-w-0">
              <div
                role="group"
                aria-label="Role"
                className="border-base-border mb-[18px] flex flex-wrap gap-7 border-b max-md:gap-x-4"
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
                    className={`text-section max-md:text-body -mb-px flex h-11 items-center gap-2 border-b-2 font-medium max-md:gap-1.5 ${role === value ? 'text-text-bright border-jade' : 'text-text-dim border-transparent'}`}
                  >
                    {label}{' '}
                    {count !== undefined && (
                      <span className="text-text-dim text-small max-md:text-meta font-normal">
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
                  className="border-base-border bg-base-surface text-text text-small border px-2 py-1"
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
                  <span className="ml-auto">{unitNode(formatCount(data.total, 'script'))}</span>
                )}
              </div>
              {isAwaited(read) ? (
                <div className="text-text-dim text-small py-8">Loading scripts…</div>
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
                  empty="No scripts."
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
                    currentCount={data.data.length}
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
                <Prose>
                  <p>{LEAD}</p>
                </Prose>
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
        <TopicCharts topic="/scripts" />
      </main>
    </div>
  );
}
