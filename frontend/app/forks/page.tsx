'use client';

import { useState, type ReactNode } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { Prose } from '@/components/ui/prose';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { TopicHero, type TopicDisplayFigure, type TopicFigure } from '@/components/ui/topic-hero';
import { Pager } from '@/components/ui/pager';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { ClockText, useClockTime } from '@/components/ui/utc8-date';
import { ForkDaysPlot, ForkDaysReading, useForkDaysPin } from '@/components/forks/fork-days-chart';
import { ForkGlyph } from '@/components/forks/fork-glyph';
import { MinerLink } from '@/components/forks/miner-link';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type ForkNodeRecord, type ReorgEvent } from '@/lib/api';
import { DEFAULT_PAGE_SIZE, pageCount } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { WIDEST_BLOCK_NUMBER, shortChars } from '@/lib/list-bounds';
import { FORK_COLORS } from '@/lib/design-tokens';
import {
  UNCLE_RATE_TARGET,
  forkEventHref,
  missingAddressWords,
  orphanFateWords,
  uncleDistanceWords,
  uncleRateFigure,
} from '@/lib/fork-format';
import { formatExactDecimal } from '@/lib/exact-integer';
import { formatTimeAgo, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { unitNode, unitText } from '@/components/ui/units';

type Overview = Awaited<ReturnType<typeof api.getForkOverview>>;

/** Each column head holds its words in a span, so its text box is its words (R16), not the cell. */
function spanHeads<T>(columns: ListColumn<T>[]): ListColumn<T>[] {
  return columns.map((column) => ({
    ...column,
    header: column.header ?? <span>{column.label}</span>,
  }));
}

/**
 * The branch switches' columns, each budgeted from the widest value it holds (readability F3-e) at
 * the row's body step (14 px), a meta line at 12 and the gap after every column but the last.
 */
const body = (chars: number) => textPx(chars, fontPx('body'));
/** The fork glyph's width (components/forks/fork-glyph.tsx draws it on a 210 px viewBox). */
const FORK_GLYPH_PX = 210;
/** A time on the UTC+8 clock to the second, "2026-10-22 22:43:55": 19 characters at every time. */
const SEEN_CHARS = formatUtc8DateTime(0).slice(0, 19).length;
/**
 * A sibling found apart from its rival: six digits of milliseconds, "999,999 ms" (under 1,000 s;
 * a sibling is found while its parent is the tip, blocks some ten seconds apart).
 */
const WIDEST_APART = '999,999 ms';

/** R15: a topic page shows a list's first rows; the rest expand in place (there is no list page). */
const PREVIEW_ROWS = 5;

/** R15, toward 32–34 px rows: rows at body (as the DAO page's depositor rows), 6 px above and
 * below the 44 px fork glyph. */
const ROW = 'text-body py-1.5';

const PREVIEW_STATUS_WORDS: Record<ReorgEvent['previewStatus'], string> = {
  available: 'available',
  no_old_branch: 'no old branch recorded',
  old_branch_unavailable: 'old branch no longer on the node',
  node_reader_unavailable: 'node reader unavailable',
  not_loaded: 'fate not loaded',
};

/** The page's one display figure (hierarchy v3, R11, R13): the window's uncle rate, bright. */
function uncleRate(overview: Overview): TopicDisplayFigure | undefined {
  const rate = uncleRateFigure(overview);
  if (!rate) return undefined;
  const window = `${formatCount(overview.days.length, 'day')}${overview.throughDate ? ` through ${overview.throughDate}` : ''}`;
  return {
    label: `uncle rate · ${window}`,
    value: rate.percent,
    unit: '%',
    companion: `${formatCount(rate.uncles, 'uncle')} in ${formatCount(rate.blocks, 'block')} · target ${UNCLE_RATE_TARGET}`,
  };
}

/** This node's own record, one quiet row under the days' reading (R13). */
function nodeFigures(overview: Overview): TopicFigure[] {
  return [
    {
      label: 'This node switched branch',
      value: formatCount(overview.nodeSwitchesAllTime, 'time'),
      sub: switchedSub(overview.nodeRecord),
    },
    ...orphanFigures(overview.nodeRecord),
  ];
}

/** "since 2026-09-25, 1 of them more than one block deep" — the node's own record (R2b-b). */
function switchedSub(record: ForkNodeRecord): string {
  const parts = [
    record.observedSince ? `since ${formatUtc8Date(record.observedSince)}` : 'not live yet',
  ];
  if (record.switchesDeeperThanOneBlock > 0)
    parts.push(
      `${record.switchesDeeperThanOneBlock.toLocaleString('en-US')} of them more than one block deep`
    );
  if (record.deepDetections > 0)
    parts.push(`${formatCount(record.deepDetections, 'deep detection')}`);
  return parts.join(', ');
}

function orphanFigures(record: ForkNodeRecord): TopicFigure[] {
  const fates = record.orphanFates;
  // Nothing dropped yet: no orphan figures at all, rather than "0 uncles" and "0 of 0".
  if (!fates || fates.blocks.total === 0) return [];
  const { blocks, userTransactions: txs } = fates;
  const rest = [
    blocks.canonicalNow > 0 &&
      `${blocks.canonicalNow.toLocaleString('en-US')} back on the main chain`,
    blocks.notIncludedAsUncle > 0 &&
      `${blocks.notIncludedAsUncle.toLocaleString('en-US')} never taken as an uncle`,
    blocks.uncleFatePending > 0 &&
      `${blocks.uncleFatePending.toLocaleString('en-US')} can still be taken`,
  ].filter(Boolean);
  return [
    {
      label: 'Its orphans',
      value: formatCount(blocks.includedAsUncle, 'uncle'),
      sub: [uncleDistanceWords(blocks.uncleDistances), ...rest].filter(Boolean).join(' · '),
    },
    {
      label: 'Orphaned transactions',
      value: `${txs.committedLater.toLocaleString('en-US')} of ${txs.total.toLocaleString('en-US')}`,
      sub:
        txs.total === 0 ? 'the dropped blocks held only cellbases' : 'committed in a later block',
    },
  ];
}

/** "79 of 266 were one miner against itself · median gap 78 ms" (Forks V67 list header). */
function competingLine(record: ForkNodeRecord): string | null {
  const fates = record.orphanFates;
  if (!fates) return 'what became of the dropped blocks needs the node reader';
  const { compared, sameMiner, gapMedianMs } = fates.competing;
  if (compared === 0) return null;
  return [
    `${sameMiner.toLocaleString('en-US')} of ${compared.toLocaleString('en-US')} were one miner against itself`,
    gapMedianMs !== null && `median gap ${formatExactDecimal(gapMedianMs, 'fork median gap')} ms`,
  ]
    .filter(Boolean)
    .join(' · ');
}

/** The branch switches' total: the API serves it on every page, and a page without it is refused. */
function switchesTotal(total: number | undefined): number {
  if (total === undefined) throw new Error('Forks list response has no total');
  return total;
}

const seenWhen = (event: ReorgEvent) => formatUtc8DateTime(event.detectedAt).slice(0, 19);

/**
 * When this node saw the switch, in body ink: the fork point is the row's one bright thing
 * (table-rows T5). `compact` (the table row, T1): the time alone, its age and any missing-address
 * words in the row's title (`switchTitle`); the phone row keeps them as lines.
 */
function Seen({ event, compact = false }: { event: ReorgEvent; compact?: boolean }) {
  const href = forkEventHref(event);
  const when = seenWhen(event);
  return (
    <span>
      {href ? (
        <Link href={href} className="text-text hover:text-jade">
          {unitText(when)}
        </Link>
      ) : (
        <span className="text-text">{unitText(when)}</span>
      )}
      {!compact && (
        <span className="text-text-dim text-meta block w-fit">
          <ClockText text={(now) => formatTimeAgo(event.detectedAt, now)} />
        </span>
      )}
      {!compact && !href && (
        <span className="text-text-dim text-meta block w-fit">
          {missingAddressWords(event.firstOrphanStatus)}
        </span>
      )}
    </span>
  );
}

/**
 * A switch row's title: what left its one-line "Seen" cell (T1), then its key. Its age is the
 * clock's, at `now` (lane E4).
 */
function switchTitle(event: ReorgEvent, now: number): string {
  return [
    `seen ${seenWhen(event)} UTC+8, ${formatTimeAgo(event.detectedAt, now)}`,
    ...(forkEventHref(event) ? [] : [missingAddressWords(event.firstOrphanStatus)]),
    `fork point ${formatBlockNumber(event.forkPointNumber)}, depth ${event.depth}`,
  ].join(' · ');
}

/** The row's key (table-rows T5): the fork point, bright at the row's body size. */
function ForkPoint({ event }: { event: ReorgEvent }) {
  return (
    <span>
      <Link href={`/blocks/${event.forkPointNumber}`} className="text-text-bright hover:text-jade">
        {formatBlockNumber(event.forkPointNumber)}
      </Link>
      <span className="text-text-dim text-meta block w-fit">depth {event.depth}</span>
    </span>
  );
}

function Shape({ event }: { event: ReorgEvent }) {
  if (event.eventType === 'deep' || !event.preview) return null;
  return (
    <ForkGlyph
      forkPoint={event.forkPointNumber}
      depth={event.depth}
      uncleIn={event.preview.uncleInBlock}
      fate={event.preview.fate as Parameters<typeof orphanFateWords>[0]}
    />
  );
}

/** The two miners, at the size of the form the row is drawn in (U-09). */
function Miners({ event, size }: { event: ReorgEvent; size: ShortSize }): ReactNode {
  if (event.eventType === 'deep') return <span className="text-text-dim">—</span>;
  if (!event.preview)
    return <span className="text-text-dim">{PREVIEW_STATUS_WORDS[event.previewStatus]}</span>;
  const {
    oldMinerLockHash: dropped,
    oldMinerAddress,
    oldMinerPool,
    competingMinerLockHash: winner,
    competingMinerAddress,
    competingMinerPool,
    fate,
  } = event.preview;
  const side = (
    hash: string | null,
    address: string | null,
    pool: string | null,
    fallback: string
  ) => {
    if ((hash === null) !== (address === null))
      throw new Error(
        `Fork ${formatBlockNumber(event.forkPointNumber)}: miner lock ${hash} and address ${address} disagree`
      );
    return address ? (
      // T5: the fork point is the row's one bright thing; the miners read in body ink.
      <MinerLink address={address} pool={pool} size={size} className="text-text" />
    ) : (
      <span className="text-text-dim">{fallback}</span>
    );
  };
  return (
    <span>
      {side(dropped, oldMinerAddress, oldMinerPool, 'witness unavailable')}{' '}
      <span className="text-text-dim">vs</span>{' '}
      {side(
        winner,
        competingMinerAddress,
        competingMinerPool,
        fate === 'canonical_now' ? 'the same block now' : 'witness unavailable'
      )}
      {dropped && dropped === winner && (
        <span className="text-text-dim text-meta ml-1.5">same miner</span>
      )}
    </span>
  );
}

function FoundApart({ event }: { event: ReorgEvent }) {
  const gap = event.preview?.timestampGapMs;
  if (gap == null) return <span className="text-text-dim">—</span>;
  // T5: a number at 12 px, tabular.
  return (
    <span className="text-text text-small tabular-nums">
      {Math.abs(gap).toLocaleString('en-US')} ms
    </span>
  );
}

/**
 * What became of the dropped block: the one cell kept on two lines (table-rows T1's exception, it
 * is the row's outcome), in body ink since the fork point is the row's one bright thing (T5).
 */
function Fate({ event }: { event: ReorgEvent }) {
  if (event.eventType === 'deep')
    return (
      <span>
        <span className="text-text">No rollback at detection</span>
        <span className="text-text-dim text-meta block w-fit">
          indexed {formatBlockNumber(event.oldTipNumber)} → node{' '}
          {formatBlockNumber(event.newTipNumber)}
        </span>
      </span>
    );
  if (!event.preview)
    return <span className="text-text-dim">{PREVIEW_STATUS_WORDS[event.previewStatus]}</span>;
  const { fate, uncleInBlock, noncellbaseTransactions, committedLaterTransactions } = event.preview;
  const words = orphanFateWords(fate as Parameters<typeof orphanFateWords>[0]);
  return (
    <span>
      {uncleInBlock !== null ? (
        // R11: neutral words; the glyph's arrow keeps the jade
        <Link href={`/blocks/${uncleInBlock}`} className="text-text hover:underline">
          {`${words} in ${formatBlockNumber(uncleInBlock)} (+${uncleInBlock - event.forkPointNumber - 1})`}
        </Link>
      ) : (
        <span className="text-text">{words}</span>
      )}
      <span className="text-text-dim text-meta block w-fit">
        {noncellbaseTransactions === 0
          ? 'cellbase only'
          : `${committedLaterTransactions.toLocaleString('en-US')} of ${noncellbaseTransactions.toLocaleString('en-US')} tx committed later`}
      </span>
    </span>
  );
}

/** The list's key to its glyphs (Forks V67): drawn swatches, neutral words (R11). */
function ShapeKey() {
  return (
    <ul aria-label="Shape key" className="text-meta text-text-dim flex flex-wrap gap-x-4 gap-y-1">
      <li className="inline-flex items-center gap-1.5">
        <svg width="14" height="10" aria-hidden="true">
          <rect
            x="1"
            y="1"
            width="12"
            height="8"
            fill="none"
            stroke={FORK_COLORS.dropped}
            strokeDasharray="2 2"
          />
        </svg>
        dashed: the block it dropped
      </li>
      <li className="inline-flex items-center gap-1.5">
        <svg width="16" height="10" aria-hidden="true">
          <line x1="0" y1="5" x2="15" y2="5" stroke={FORK_COLORS.uncleLink} strokeWidth="1.5" />
        </svg>
        → the block that took it as an uncle
      </li>
    </ul>
  );
}

export default function ForksPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = usePageSize(DEFAULT_PAGE_SIZE);
  const pin = useForkDaysPin();
  const [expanded, setExpanded] = useState(false);
  const overview = useQuery({
    queryKey: ['forks-overview', 30],
    queryFn: () => api.getForkOverview(30),
  });
  const events = useQuery({
    queryKey: ['forks', cursor, limit],
    queryFn: () => api.getForks({ cursor, limit }),
    placeholderData: keepPreviousData,
  });
  const data = overview.data;
  const switches = data?.days.reduce((sum, day) => sum + day.nodeSwitches, 0);
  if (data && switches !== data.nodeSwitchesInWindow)
    throw new Error(
      `Fork overview: days hold ${switches} switches but the window says ${data.nodeSwitchesInWindow}`
    );
  const competing = data ? competingLine(data.nodeRecord) : null;
  // Collapsed to the first rows until "all … ↓" (R15); a later page is always shown whole.
  const collapsed =
    !expanded &&
    !hasPrevious &&
    events.data !== undefined &&
    events.data.data.length > PREVIEW_ROWS;
  const shown = events.data
    ? collapsed
      ? events.data.data.slice(0, PREVIEW_ROWS)
      : events.data.data
    : [];
  // The rows' titles hold their ages (T1): the page reads the one clock through them, so it renders
  // again only when one of the titles shown changes (lane E4).
  const now = useClockTime((t) => shown.map((e) => switchTitle(e, t)).join('\n'));
  const total = events.data?.total;
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 max-w-[1280px] pb-[72px] pt-7">
        {/* Hierarchy v3 (R13): the uncle rate and the days under it are the hero; the concept,
            the chart's long caption and the list's record note are tier 2. */}
        <div data-tier="1" className="min-w-0">
          <TopicHero
            layout="figure"
            title="Forks"
            lede={
              <>
                Two blocks, one height: one stays, one is an uncle.{' '}
                <a href="#evidence" className="hover:text-text-bright hover:underline">
                  How it works<span className="whitespace-nowrap"> ↓</span>
                </a>
              </>
            }
            figure={data ? uncleRate(data) : undefined}
            picture={
              isAwaited(overview) ? (
                <p role="status" className="text-text-dim text-small">
                  Loading fork history…
                </p>
              ) : overview.error ? (
                <p role="alert" className="text-rouge text-small max-w-measure">
                  Fork history unavailable: {(overview.error as Error).message}
                </p>
              ) : data && data.days.length ? (
                <ForkDaysPlot days={data.days} pin={pin} />
              ) : (
                <p className="text-text-dim text-small">No completed fork days are indexed.</p>
              )
            }
            reading={
              data &&
              data.days.length > 0 && (
                <div data-role="reading">
                  <ForkDaysReading days={data.days} pin={pin} />
                  <p className="text-text text-body max-w-measure mt-3.5">
                    Uncles are the complete record; this node sees only forks it had to switch.
                  </p>
                </div>
              )
            }
            figures={data ? nodeFigures(data) : undefined}
          />
          <section aria-labelledby="fork-switches-heading" className="mt-16 min-w-0">
            <div className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
              {/* Phase 5 (B8): the list as a command run on its count, once the list has counted
                  it, and on whose view it is; the shape key stays the row's control. */}
              <PromptHeading
                id="fork-switches-heading"
                command="branch switches"
                args={
                  total === undefined ? undefined : `${total.toLocaleString('en-US')} · this node`
                }
              />
              <ShapeKey />
            </div>
            {isAwaited(events) && <p role="status">Loading branch switches…</p>}
            {events.error && (
              <p role="alert" className="text-rouge text-small max-w-measure">
                Branch switches unavailable: {(events.error as Error).message}
              </p>
            )}
            {events.data && (
              <>
                <ListTable
                  label="Branch switches on this node"
                  rows={shown}
                  rowKey={(event) => String(event.id)}
                  rowTitle={(event) => switchTitle(event, now)}
                  empty="No branch switches observed by this node. Chain uncles above can still exist."
                  columns={spanHeads<ReorgEvent>([
                    {
                      key: 'seen',
                      className: ROW,
                      label: 'Seen (UTC+8)',
                      // the time to the second (19 characters at 14 px, 159.6) and the gap; it
                      // was 160, the gap uncounted
                      minWidth: columnPx(body(SEEN_CHARS) + CELL_GAP_PX),
                      render: (e) => <Seen event={e} compact />,
                    },
                    {
                      key: 'fork',
                      className: ROW,
                      label: 'Fork point',
                      // the fork point, "#99,999,999" (92.4; its depth a meta line under it), and
                      // the gap; it was 120
                      minWidth: columnPx(body(WIDEST_BLOCK_NUMBER.length) + CELL_GAP_PX),
                      render: (e) => <ForkPoint event={e} />,
                    },
                    {
                      key: 'shape',
                      className: ROW,
                      label: 'Shape',
                      // the 210 px glyph and the gap
                      minWidth: columnPx(FORK_GLYPH_PX + CELL_GAP_PX),
                      render: (e) => <Shape event={e} />,
                    },
                    {
                      key: 'miners',
                      className: ROW,
                      label: 'Dropped block’s miner vs winner’s',
                      // The flexible column (readability F3-e: a pool name has no bound): two
                      // unnamed miners at the row size, 13 + " vs " + 13 characters at the row's
                      // 14 px (252), and the gap; it takes what the container gives beyond the
                      // sum. It was 250, budgeted at 13 px.
                      minWidth: columnPx(
                        body(2 * shortChars(SHORT_FORMS.row.address) + ' vs '.length) + CELL_GAP_PX
                      ),
                      width: 'auto',
                      render: (e) => <Miners event={e} size="row" />,
                    },
                    {
                      key: 'apart',
                      className: ROW,
                      label: 'Found apart',
                      // "999,999 ms" at 13 px (78), wider than the head's words, and the gap; it
                      // was 100
                      minWidth: columnPx(
                        textPx(WIDEST_APART.length, fontPx('small')) + CELL_GAP_PX
                      ),
                      numeric: true,
                      render: (e) => <FoundApart event={e} />,
                    },
                    {
                      key: 'fate',
                      className: ROW,
                      label: 'What became of it',
                      // The widest of its two lines: the fate "uncle inclusion still possible"
                      // (252 at 14 px), or a deep switch's "indexed #99,999,999 → node
                      // #99,999,999" on its meta line (38 characters at 12 px, 273.6, the wider);
                      // the last column, no gap. It was 210, which broke both lines.
                      minWidth: columnPx(
                        Math.max(
                          body('uncle inclusion still possible'.length),
                          textPx(
                            `indexed ${WIDEST_BLOCK_NUMBER} → node ${WIDEST_BLOCK_NUMBER}`.length,
                            fontPx('meta')
                          )
                        )
                      ),
                      render: (e) => <Fate event={e} />,
                    },
                  ])}
                  mobileRow={(event) => (
                    <div className="text-small min-w-0 space-y-2">
                      <div className="flex flex-wrap justify-between gap-2">
                        <Seen event={event} />
                        <ForkPoint event={event} />
                      </div>
                      <Shape event={event} />
                      <div>
                        <Miners event={event} size="tight" /> · <FoundApart event={event} />
                      </div>
                      <Fate event={event} />
                    </div>
                  )}
                />
                {/* The controls board's pager: the short list's own form while the first five
                    stand for the list ("all 18 branch switches ↓"), then a cursor in time, newest
                    first, the page of the pages the served total makes at the served page size;
                    nothing under an empty list. */}
                {events.data.data.length === 0 ? null : collapsed ? (
                  <div className="mt-3">
                    <Pager
                      mode="more"
                      all={{
                        count: switchesTotal(events.data.total),
                        singular: 'branch switch',
                        plural: 'branch switches',
                        onShow: () => setExpanded(true),
                      }}
                    />
                  </div>
                ) : (
                  <div className="mt-5">
                    <Pager
                      mode="time"
                      label="branch switches"
                      order="newest first"
                      page={page}
                      pages={pageCount(switchesTotal(events.data.total), events.data.limit)}
                      onNewer={hasPrevious ? goToPrevious : undefined}
                      onOlder={
                        events.data.hasMore ? () => goToNext(events.data!.nextCursor) : undefined
                      }
                    />
                  </div>
                )}
              </>
            )}
          </section>
        </div>
        <div id="evidence" className="scroll-mt-4">
          <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-2">
            <div className="min-w-0">
              <DetailSection title="How forks are counted">
                <Prose>
                  <p>
                    Two miners sometimes find a block at the same height, a fraction of a second
                    apart. The chain keeps one. The other is not lost: a later block includes it as
                    an uncle, and CKB counts uncles to measure how often this happens and set the
                    difficulty, aiming at one in 40 blocks.
                  </p>
                  {data && data.days.length > 0 && (
                    <p>
                      The chain recorded {unitNode(formatCount(data.unclesInWindow, 'uncle'))} in
                      these {unitNode(formatCount(data.days.length, 'day'))}; this node switched
                      branches {unitNode(formatCount(data.nodeSwitchesInWindow, 'time'))} in the
                      same days. It only notices a fork when the losing block reached it first and
                      it had to switch. The uncles are the complete record, and they are on chain
                      for anyone to recount.
                    </p>
                  )}
                </Prose>
              </DetailSection>
            </div>
            <div className="min-w-0 space-y-8">
              <DetailSection title="How the chart is drawn">
                <Prose>
                  <p>
                    In the chart each grey bar is a day&apos;s uncles on chain and the jade inside
                    it this node&apos;s branch switches that day; point at a day, or focus the chart
                    and use ← → to step through the days.
                  </p>
                  {data && <p>Days: {data.scope}.</p>}
                </Prose>
              </DetailSection>
              {competing && (
                <DetailSection title="This node’s branch switches, compared">
                  <Prose>
                    <p>{competing}</p>
                  </Prose>
                </DetailSection>
              )}
            </div>
          </DetailTier2>
        </div>
      </main>
    </div>
  );
}
