'use client';

import type { ReactNode } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { DetailSection } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import { TopicHero, type TopicFigure } from '@/components/ui/topic-hero';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { ForkDaysChart } from '@/components/forks/fork-days-chart';
import { ForkGlyph } from '@/components/forks/fork-glyph';
import { MinerLink } from '@/components/forks/miner-link';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type ForkNodeRecord, type ReorgEvent } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
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

const PREVIEW_STATUS_WORDS: Record<ReorgEvent['previewStatus'], string> = {
  available: 'available',
  no_old_branch: 'no old branch recorded',
  old_branch_unavailable: 'old branch no longer on the node',
  node_reader_unavailable: 'node reader unavailable',
  not_loaded: 'fate not loaded',
};

function heroFigures(overview: Overview): TopicFigure[] {
  const rate = uncleRateFigure(overview);
  const window = `${formatCount(overview.days.length, 'day')}${overview.throughDate ? ` through ${overview.throughDate}` : ''}`;
  return [
    ...(rate
      ? [
          {
            label: `Uncle rate · ${window}`,
            value: rate.rate,
            sub: `${formatCount(rate.uncles, 'uncle')} in ${formatCount(rate.blocks, 'block')} · target ${UNCLE_RATE_TARGET}`,
          },
        ]
      : []),
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
      tone: 'jade',
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
      <span className="text-text-dim block text-[11px]">{formatTimeAgo(event.detectedAt)}</span>
      {!href && (
        <span className="text-text-dim block text-[11px]">
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
      <span className="text-text-dim block text-[11px]">depth {event.depth}</span>
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
    competingMinerLockHash: winner,
    competingMinerAddress,
    fate,
  } = event.preview;
  const side = (hash: string | null, address: string | null, fallback: string) => {
    if ((hash === null) !== (address === null))
      throw new Error(
        `Fork ${formatBlockNumber(event.forkPointNumber)}: miner lock ${hash} and address ${address} disagree`
      );
    return address ? (
      <MinerLink address={address} />
    ) : (
      <span className="text-text-dim">{fallback}</span>
    );
  };
  return (
    <span>
      {side(dropped, oldMinerAddress, 'witness unavailable')}{' '}
      <span className="text-[#4b5266]">vs</span>{' '}
      {side(
        winner,
        competingMinerAddress,
        fate === 'canonical_now' ? 'the same block now' : 'witness unavailable'
      )}
      {dropped && dropped === winner && (
        <span className="text-text-dim ml-1.5 text-[11px]">same miner</span>
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
        <span className="text-text-dim block text-[11px]">
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
        <Link href={`/blocks/${uncleInBlock}`} className="text-jade hover:underline">
          {`${words} in ${formatBlockNumber(uncleInBlock)} (+${uncleInBlock - event.forkPointNumber - 1})`}
        </Link>
      ) : (
        <span className="text-text-bright">{words}</span>
      )}
      <span className="text-text-dim block text-[11px]">
        {noncellbaseTransactions === 0
          ? 'cellbase only'
          : `${committedLaterTransactions.toLocaleString('en-US')} of ${noncellbaseTransactions.toLocaleString('en-US')} tx committed later`}
      </span>
    </span>
  );
}

export default function ForksPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = DEFAULT_PAGE_SIZE;
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
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto flex min-w-0 max-w-[1280px] flex-col gap-11 px-4 pb-[72px] pt-7">
        <TopicHero
          title="Forks"
          intro="Two miners sometimes find a block at the same height, a fraction of a second apart. The chain keeps one. The other is not lost: a later block includes it as an uncle, and CKB counts uncles to measure how often this happens and set the difficulty, aiming at one in 40 blocks."
          figures={data ? heroFigures(data) : undefined}
        />
        <DetailSection
          title="The chain’s record and this node’s view"
          actions={
            <span className="text-text-dim flex flex-wrap gap-x-[18px] gap-y-1 text-[11.5px]">
              <span className="inline-flex items-center gap-1.5">
                <span
                  className="h-2.5 w-2.5 rounded-sm opacity-50"
                  style={{ background: FORK_COLORS.uncles }}
                />
                uncles on chain, per day
              </span>
              <span className="inline-flex items-center gap-1.5">
                <span
                  className="h-2.5 w-2.5 rounded-sm"
                  style={{ background: FORK_COLORS.nodeSwitches }}
                />
                this node’s branch switches
              </span>
            </span>
          }
        >
          {overview.isLoading && <p role="status">Loading fork history…</p>}
          {overview.error && (
            <p role="alert" className="text-rouge text-[12px]">
              Fork history unavailable: {(overview.error as Error).message}
            </p>
          )}
          {data &&
            (data.days.length ? (
              <>
                <ForkDaysChart days={data.days} />
                <p className="text-text mt-3 max-w-[940px] text-[12.5px] leading-relaxed">
                  The chain recorded {formatCount(data.unclesInWindow, 'uncle')} in these{' '}
                  {formatCount(data.days.length, 'day')}; this node switched branches{' '}
                  {formatCount(data.nodeSwitchesInWindow, 'time')} in the same days. It only notices
                  a fork when the losing block reached it first and it had to switch. The uncles are
                  the complete record, and they are on chain for anyone to recount.
                </p>
                <p className="text-text-dim mt-1 text-[11px]">{data.scope}</p>
              </>
            ) : (
              <p className="text-text-dim text-[12px]">No completed fork days are indexed.</p>
            ))}
        </DetailSection>
        <DetailSection
          title="Branch switches on this node"
          actions={
            <span className="text-text-dim flex flex-wrap gap-x-4 gap-y-1 text-[11.5px]">
              <span>dashed: the block it dropped</span>
              <span className="text-jade">→ the block that took it as an uncle</span>
              {data && competingLine(data.nodeRecord) && (
                <span>{competingLine(data.nodeRecord)}</span>
              )}
            </span>
          }
        >
          {events.isLoading && <p role="status">Loading branch switches…</p>}
          {events.error && (
            <p role="alert" className="text-rouge text-[12px]">
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
                columns={[
                  {
                    key: 'seen',
                    label: 'Seen (UTC+8)',
                    minWidth: 160,
                    render: (e) => <Seen event={e} />,
                  },
                  {
                    key: 'fork',
                    label: 'Fork point',
                    minWidth: 120,
                    render: (e) => <ForkPoint event={e} />,
                  },
                  {
                    key: 'shape',
                    label: 'Shape',
                    minWidth: 226,
                    render: (e) => <Shape event={e} />,
                  },
                  {
                    key: 'miners',
                    label: 'Dropped block’s miner vs winner’s',
                    minWidth: 260,
                    render: (e) => <Miners event={e} />,
                  },
                  {
                    key: 'apart',
                    label: 'Found apart',
                    minWidth: 100,
                    numeric: true,
                    render: (e) => <FoundApart event={e} />,
                  },
                  {
                    key: 'fate',
                    label: 'What became of it',
                    minWidth: 210,
                    render: (e) => <Fate event={e} />,
                  },
                ]}
                mobileRow={(event) => (
                  <div className="min-w-0 space-y-2 text-xs">
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
        </DetailSection>
      </main>
    </div>
  );
}
