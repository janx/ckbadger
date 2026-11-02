'use client';

import type { ReactNode } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { TopicHero, type TopicDisplayFigure, type TopicFigure } from '@/components/ui/topic-hero';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { ForkDaysPlot, ForkDaysReading, useForkDaysPin } from '@/components/forks/fork-days-chart';
import { ForkGlyph } from '@/components/forks/fork-glyph';
import { MinerLink } from '@/components/forks/miner-link';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type ForkNodeRecord, type ReorgEvent } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
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

type Overview = Awaited<ReturnType<typeof api.getForkOverview>>;

/** Each column head holds its words in a span, so its text box is its words (R16), not the cell. */
function spanHeads<T>(columns: ListColumn<T>[]): ListColumn<T>[] {
  return columns.map((column) => ({
    ...column,
    header: column.header ?? <span>{column.label}</span>,
  }));
}

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

function Seen({ event }: { event: ReorgEvent }) {
  const href = forkEventHref(event);
  const when = formatUtc8DateTime(event.detectedAt).slice(0, 19);
  return (
    <span>
      {href ? (
        <Link href={href} className="text-text-bright hover:text-jade">
          {when}
        </Link>
      ) : (
        <span className="text-text-bright">{when}</span>
      )}
      <span className="text-text-dim text-meta block w-fit">{formatTimeAgo(event.detectedAt)}</span>
      {!href && (
        <span className="text-text-dim text-meta block w-fit">
          {missingAddressWords(event.firstOrphanStatus)}
        </span>
      )}
    </span>
  );
}

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

function Miners({ event }: { event: ReorgEvent }): ReactNode {
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
      <MinerLink address={address} pool={pool} />
    ) : (
      <span className="text-text-dim">{fallback}</span>
    );
  };
  return (
    <span>
      {side(dropped, oldMinerAddress, oldMinerPool, 'witness unavailable')}{' '}
      <span className="text-[#4b5266]">vs</span>{' '}
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
  return <span className="text-text">{Math.abs(gap).toLocaleString('en-US')} ms</span>;
}

function Fate({ event }: { event: ReorgEvent }) {
  if (event.eventType === 'deep')
    return (
      <span>
        <span className="text-text-bright">No rollback at detection</span>
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
        // R11: the words in bright ink; the glyph's arrow keeps the jade
        <Link href={`/blocks/${uncleInBlock}`} className="text-text-bright hover:underline">
          {`${words} in ${formatBlockNumber(uncleInBlock)} (+${uncleInBlock - event.forkPointNumber - 1})`}
        </Link>
      ) : (
        <span className="text-text-bright">{words}</span>
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
            rx="2"
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
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 max-w-[1280px] px-4 pb-[72px] pt-7">
        {/* Hierarchy v3 (R13): the uncle rate and the days under it are the hero; the concept,
            the chart's long caption and the list's record note are tier 2. */}
        <div data-tier="1" className="min-w-0">
          <TopicHero
            layout="figure"
            title="Forks"
            lede={
              <>
                Two blocks, one height: one stays, one is an uncle.{' '}
                <a
                  href="#evidence"
                  className="hover:text-text-bright whitespace-nowrap hover:underline"
                >
                  How it works ↓
                </a>
              </>
            }
            figure={data ? uncleRate(data) : undefined}
            picture={
              overview.isLoading ? (
                <p role="status" className="text-text-dim text-small">
                  Loading fork history…
                </p>
              ) : overview.error ? (
                <p role="alert" className="text-rouge text-small">
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
                  <p className="text-text text-body mt-3.5 max-w-[640px]">
                    Uncles are the complete record; this node sees only the forks it had to switch.
                  </p>
                </div>
              )
            }
            figures={data ? nodeFigures(data) : undefined}
          />
          <section aria-labelledby="fork-switches-heading" className="mt-16 min-w-0">
            <div className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
              <h2 id="fork-switches-heading" className="text-text-bright text-section font-medium">
                Branch switches on this node
              </h2>
              <ShapeKey />
            </div>
            {events.isLoading && <p role="status">Loading branch switches…</p>}
            {events.error && (
              <p role="alert" className="text-rouge text-small">
                Branch switches unavailable: {(events.error as Error).message}
              </p>
            )}
            {events.data && (
              <>
                <ListTable
                  label="Branch switches on this node"
                  rows={events.data.data}
                  rowKey={(event) => String(event.id)}
                  empty="No branch switches observed by this node. Chain uncles above can still exist."
                  columns={spanHeads<ReorgEvent>([
                    {
                      key: 'seen',
                      className: ROW,
                      label: 'Seen (UTC+8)',
                      minWidth: 160,
                      render: (e) => <Seen event={e} />,
                    },
                    {
                      key: 'fork',
                      className: ROW,
                      label: 'Fork point',
                      minWidth: 120,
                      render: (e) => <ForkPoint event={e} />,
                    },
                    {
                      key: 'shape',
                      className: ROW,
                      label: 'Shape',
                      minWidth: 226,
                      render: (e) => <Shape event={e} />,
                    },
                    {
                      key: 'miners',
                      className: ROW,
                      label: 'Dropped block’s miner vs winner’s',
                      minWidth: 260,
                      render: (e) => <Miners event={e} />,
                    },
                    {
                      key: 'apart',
                      className: ROW,
                      label: 'Found apart',
                      minWidth: 100,
                      numeric: true,
                      render: (e) => <FoundApart event={e} />,
                    },
                    {
                      key: 'fate',
                      className: ROW,
                      label: 'What became of it',
                      minWidth: 210,
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
                        <Miners event={event} /> · <FoundApart event={event} />
                      </div>
                      <Fate event={event} />
                    </div>
                  )}
                />
                <div className="mt-5">
                  <CursorPagination
                    total={events.data.total}
                    totalLabel="switches"
                    pageSize={limit}
                    page={page}
                    hasMore={events.data.hasMore}
                    hasPrevious={hasPrevious}
                    onNext={() => goToNext(events.data!.nextCursor)}
                    onPrevious={goToPrevious}
                  />
                </div>
              </>
            )}
          </section>
        </div>
        <div id="evidence" className="scroll-mt-4">
          <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-2">
            <div className="min-w-0">
              <DetailSection title="How forks are counted">
                <div className="flex flex-col gap-3">
                  <p className="text-text-dim text-body">
                    Two miners sometimes find a block at the same height, a fraction of a second
                    apart. The chain keeps one. The other is not lost: a later block includes it as
                    an uncle, and CKB counts uncles to measure how often this happens and set the
                    difficulty, aiming at one in 40 blocks.
                  </p>
                  {data && data.days.length > 0 && (
                    <p className="text-text-dim text-body">
                      The chain recorded {formatCount(data.unclesInWindow, 'uncle')} in these{' '}
                      {formatCount(data.days.length, 'day')}; this node switched branches{' '}
                      {formatCount(data.nodeSwitchesInWindow, 'time')} in the same days. It only
                      notices a fork when the losing block reached it first and it had to switch.
                      The uncles are the complete record, and they are on chain for anyone to
                      recount.
                    </p>
                  )}
                </div>
              </DetailSection>
            </div>
            <div className="min-w-0 space-y-8">
              <DetailSection title="How the chart is drawn">
                <div className="flex flex-col gap-3">
                  <p className="text-text-dim text-body">
                    In the chart each grey bar is a day&apos;s uncles on chain and the jade inside
                    it this node&apos;s branch switches that day; point at a day, or focus the chart
                    and use ← → to step through the days.
                  </p>
                  {data && <p className="text-text-dim text-body">Days: {data.scope}.</p>}
                </div>
              </DetailSection>
              {competing && (
                <DetailSection title="This node’s branch switches, compared">
                  <p className="text-text-dim text-body">{competing}</p>
                </DetailSection>
              )}
            </div>
          </DetailTier2>
        </div>
      </main>
    </div>
  );
}
