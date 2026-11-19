'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import {
  FiberGraphNotes,
  FiberPartiesNote,
  FiberNetworkPicture,
  FiberNetworkReading,
  useFiberGraphPin,
} from '@/components/fiber/fiber-network-graph';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { Prose } from '@/components/ui/prose';
import { TopicHero, type TopicDisplayFigure, type TopicFigure } from '@/components/ui/topic-hero';
import { FilterChips } from '@/components/ui/filter-chips';
import { Pager } from '@/components/ui/pager';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type FiberChannel, type FiberChannelState, type FiberNetwork } from '@/lib/api';
import { DEFAULT_PAGE_SIZE, rankedPageAt } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { formatCount, countNoun } from '@/lib/format-count';
import { cn, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { BOARD_INKS, inkStyle } from '@/lib/design-tokens';
import { FIBER_STATE_WORDS, fiberDuration } from '@/lib/fiber-format';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { WIDEST_TX_VALUE, shortChars } from '@/lib/list-bounds';
import { PhoneRow } from '@/components/ui/phone-row';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { ShortId } from '@/components/ui/short-id';
import { SHORT_FORMS, shortenHash } from '@/lib/shorten';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';
import { Utc8Date } from '@/components/ui/utc8-date';

type StateFilter = 'all' | FiberChannelState;

/** Each column head holds its words in a span, so its text box is its words (R16), not the cell. */
function spanHeads<T>(columns: ListColumn<T>[]): ListColumn<T>[] {
  return columns.map((column) => ({
    ...column,
    header: column.header ?? <span>{column.label}</span>,
  }));
}

/** The board's chip order and words (Fiber V65); counts are the summary's own fields. */
const filters: {
  value: StateFilter;
  label: string;
  count: (summary: FiberNetwork['summary']) => number;
}[] = [
  { value: 'open', label: 'Open', count: (s) => s.openChannels },
  {
    value: 'cooperativelyClosed',
    label: 'Closed together',
    count: (s) => s.cooperativelyClosedChannels,
  },
  { value: 'settled', label: 'Force-closed, settled', count: (s) => s.forceClosedSettledChannels },
  {
    value: 'forceClosed',
    label: 'Force-closed, not settled',
    count: (s) => s.forceClosedWaitingChannels,
  },
  { value: 'all', label: 'All', count: (s) => s.totalChannels },
];
/** R15: table rows 32–34 px — rows at body (as the DAO page's depositor rows), 6 px above and below. */
const ROW = 'text-body py-1.5';

/**
 * The channel list's columns, each budgeted from the widest value it holds (readability F3-e) at
 * the row's body step (14 px), a meta line or a unit at 12 and the gap after every column but the
 * last.
 */
const body = (chars: number) => textPx(chars, fontPx('body'));
const meta = (chars: number) => textPx(chars, fontPx('meta'));
/**
 * The longest a channel lives on its duration form: under 10,000 days (some 27 years), "9999d 23h".
 */
const WIDEST_LIFETIME = fiberDuration(String((9999 * 86_400 + 23 * 3600) * 1000), 'bound');

/**
 * A party in the table, by its address or lock hash at the row size (U-09; it was the last six
 * characters alone, a form of its own); its whole address or lock hash is its title and link.
 */
function Party({ party }: { party: FiberChannel['observedParties'][number] }) {
  const text = party.address ? (
    <ShortId value={party.address} kind="address" size="row" />
  ) : (
    <ShortId value={party.lockScriptHash} kind="hash" size="row" />
  );
  return party.address ? (
    <Link
      href={`/address/${party.address}`}
      className="text-text hover:text-jade"
      title={party.address}
    >
      {text}
    </Link>
  ) : (
    <span className="text-text" title={party.lockScriptHash}>
      {text}
    </span>
  );
}

/** The page's one display figure (hierarchy v3, R11, R13): open now, in Fiber's jade. */
function openNowFigure(summary: FiberNetwork['summary']): TopicDisplayFigure {
  return {
    label: 'open now',
    value: summary.openChannels.toLocaleString('en-US'),
    unit: countNoun(summary.openChannels, 'channel', 'channels'),
    companion: (
      <>
        locking{' '}
        <CkbAmount
          form="summary"
          shannons={BigInt(summary.totalCapacityLocked)}
          signed={false}
          tone="plain"
        />
      </>
    ),
    tone: 'jade',
  };
}

/** The other three figures, one quiet row under the picture's reading (R13). */
function quietFigures(summary: FiberNetwork['summary']): TopicFigure[] {
  const closed = summary.totalChannels - summary.openChannels;
  if (closed < 0 || summary.closedWithinHour > closed)
    throw new Error(
      `Fiber summary: ${summary.closedWithinHour} closed within an hour of ${summary.totalChannels} channels, ${summary.openChannels} open`
    );
  return [
    {
      label: 'Channels ever',
      value: summary.totalChannels.toLocaleString('en-US'),
      sub: [
        summary.firstOpenTimestamp
          ? `since ${formatUtc8Date(summary.firstOpenTimestamp)}`
          : 'none yet',
        ...summary.channelsByToken.map(
          (token) =>
            `${token.channels.toLocaleString('en-US')} in ${token.symbol ?? shortenHash(token.udtTypeHash, 'row')}`
        ),
      ].join(' · '),
    },
    {
      label: 'Parties',
      value: summary.observedParties.toLocaleString('en-US'),
      sub: `in ${formatCount(summary.observedPairs, 'pair')}`,
    },
    {
      label: 'Typical lifetime',
      value:
        summary.typicalClosedLifetimeMs === null
          ? 'none closed yet'
          : fiberDuration(summary.typicalClosedLifetimeMs, 'typical closed lifetime'),
      sub: `${summary.closedWithinHour.toLocaleString('en-US')} of ${closed.toLocaleString('en-US')} closed within an hour`,
    },
  ];
}

/**
 * A party in the phone row: its address or lock hash at the tight size below sm; from sm (the
 * rows are drawn under the 820 px table, at 640 and 768 too) the row size the network reading
 * above the list writes the same party in (R9).
 */
function PartySuffix({ party }: { party: FiberChannel['observedParties'][number] }) {
  const size = useTightBelowSm();
  const text = party.address ? (
    <ShortId value={party.address} kind="address" size={size} />
  ) : (
    <ShortId value={party.lockScriptHash} kind="hash" size={size} />
  );
  return party.address ? (
    <Link
      href={`/address/${party.address}`}
      className="text-text-bright hover:text-jade"
      title={party.address}
    >
      {text}
    </Link>
  ) : (
    <span className="text-text-bright" title={party.lockScriptHash}>
      {text}
    </span>
  );
}

const STATE_DOT: Record<FiberChannelState, string> = {
  open: 'bg-jade',
  cooperativelyClosed: 'bg-text-ghost',
  settled: 'bg-text-ghost',
  forceClosed: 'bg-gold',
};

function ChannelState({ state }: { state: FiberChannelState }) {
  return (
    <span
      // R11: tier 1's only coloured text is the display figure; the dot carries the state colour.
      className={cn(
        'text-meta inline-flex items-center gap-1.5 whitespace-nowrap',
        state === 'open' ? 'text-text' : 'text-text-dim'
      )}
    >
      <span aria-hidden="true" className={cn('h-1.5 w-1.5 rounded-full', STATE_DOT[state])} />
      {FIBER_STATE_WORDS[state]}
    </span>
  );
}

function lifetime(channel: FiberChannel): string {
  if (channel.observedLifetimeMs === null)
    throw new Error(`Fiber channel ${channel.channelId} has no observed lifetime`);
  return fiberDuration(channel.observedLifetimeMs, `${channel.channelId} lifetime`);
}

/**
 * A channel on a phone (board L8): id and state; the pair, their locks and the capacity; when it
 * opened and how long it lived — three labelled lines. In a wide container (640) two lines.
 */
function ChannelPhoneRow({ channel, wide }: { channel: FiberChannel; wide: boolean }) {
  // the channel id at the size of its parties (R9): tight below sm, the row size from it
  const size = useTightBelowSm();
  const id = (
    <Link
      href={`/fiber/channels/${channel.channelId}`}
      className="text-[color:var(--ink)] hover:underline"
      style={inkStyle(BOARD_INKS.link)}
      title={channel.channelId}
    >
      <ShortId value={channel.channelId} kind="hash" size={size} />
    </Link>
  );
  const pair = (
    <span>
      {channel.observedParties.map((party, index) => (
        <span key={party.lockScriptHash}>
          {index > 0 && <span className="text-text-dim"> ⇄ </span>}
          <PartySuffix party={party} />
        </span>
      ))}{' '}
      <span className="text-text-dim">
        {channel.observedParties.map((party) => party.lockScriptName ?? 'unnamed lock').join(' · ')}
      </span>
    </span>
  );
  const capacity = (
    <span className="text-text-bright">
      <CkbAmount form="summary" shannons={BigInt(channel.capacity)} signed={false} tone="plain" />
    </span>
  );
  const when = `opened ${formatUtc8DateTime(channel.openTimestamp).slice(0, 16)} · ${
    channel.state === 'open' ? 'open for' : 'lifetime'
  } ${lifetime(channel)}`;
  return (
    <PhoneRow
      testId="fiber-channel-row-mobile"
      lines={
        wide
          ? [
              {
                key: 'channel',
                tone: 'head',
                left: (
                  <>
                    {id} <span className="text-text-dim text-small">· </span>
                    <span className="text-small">{pair}</span>
                  </>
                ),
                right: capacity,
              },
              {
                key: 'time',
                tone: 'quiet',
                left: when,
                right: <ChannelState state={channel.state} />,
              },
            ]
          : [
              {
                key: 'channel',
                tone: 'head',
                left: id,
                right: <ChannelState state={channel.state} />,
              },
              { key: 'pair', tone: 'body', left: pair, right: capacity },
              { key: 'time', tone: 'quiet', left: when },
            ]
      }
    />
  );
}

export default function FiberChannelsPage() {
  const [stateFilter, setStateFilter] = useState<StateFilter>('open');
  const pageSize = usePageSize(DEFAULT_PAGE_SIZE);
  // a change of page size at md starts the walk again (lane c2-finish, 1d)
  const pagination = useCursorPagination(pageSize);
  const network = useQuery({ queryKey: ['fiber-network'], queryFn: api.getFiberNetwork });
  const pin = useFiberGraphPin();
  const channels = useQuery({
    queryKey: ['fiber-channels', stateFilter, pagination.cursor, pageSize],
    queryFn: () =>
      api.getFiberChannels({
        limit: pageSize,
        cursor: pagination.cursor,
        state: stateFilter === 'all' ? undefined : stateFilter,
        sort: 'capacity',
      }),
  });
  const summary = network.data?.summary;
  // where the ranked list stands (lib/pagination.ts): the rows shown of the response's total at
  // its page size, the page the response was asked as (the list keeps no previous page while the
  // next loads, so the two never part); nothing for a list on one page, an empty one too
  const channelsAt = channels.data
    ? rankedPageAt('Fiber channels', pagination.page, channels.data)
    : null;
  const chosen = filters.find((filter) => filter.value === stateFilter)!;
  const lastColumn = stateFilter === 'open' ? 'Open for' : 'Lifetime';
  return (
    <div className="min-h-screen">
      <main className="container mx-auto max-w-[1280px] pb-16 pt-8">
        {/* Hierarchy v3 (R13): the hero is open now and the network picture; the concept and the
            graph's explainer are tier 2. Hero → first section 64 px (R6). */}
        <div data-tier="1" className="min-w-0">
          <TopicHero
            layout="figure"
            title="Fiber channels"
            lede={
              <>
                Channels touch the chain only to open and to close.{' '}
                <a href="#evidence" className="hover:text-text-bright hover:underline">
                  How it works<span className="whitespace-nowrap"> ↓</span>
                </a>
              </>
            }
            figure={summary ? openNowFigure(summary) : undefined}
            picture={
              network.data ? (
                <FiberNetworkPicture network={network.data} pin={pin} />
              ) : network.isError ? (
                <p role="alert" className="text-rouge text-body max-w-measure">
                  Fiber network evidence could not be loaded: {String(network.error)}
                </p>
              ) : (
                <p className="text-text-dim text-body">Loading indexed Fiber network…</p>
              )
            }
            reading={network.data && <FiberNetworkReading network={network.data} pin={pin} />}
            figures={summary ? quietFigures(summary) : undefined}
          />
          <section
            aria-labelledby="fiber-channels-heading"
            className="mt-16 flex min-w-0 flex-col gap-2.5"
          >
            {/* Phase 5 (B7): the list as a command run on its count in the chosen state, once the
                list has counted it; the sort is the note. */}
            <PromptHeading
              id="fiber-channels-heading"
              command="Channels"
              args={
                channels.data?.total === undefined
                  ? undefined
                  : `${channels.data.total.toLocaleString('en-US')} ${chosen.label.toLowerCase()}`
              }
              note="sorted by capacity"
            />
            {/* The state is the controls board's filter (one of five, its words lowercase, each
                with the summary's count once it is in; on a phone a select after "state" where
                the chips do not fit its line): a state starts the list again from its first page. */}
            <FilterChips
              label="state"
              values={filters.map((filter) => ({
                key: filter.value,
                label: filter.label.toLowerCase(),
                count: summary ? filter.count(summary) : undefined,
              }))}
              value={stateFilter}
              onChange={(next) => {
                setStateFilter(next);
                pagination.reset();
              }}
            />
            {channels.isError && (
              <p role="alert" className="text-rouge text-body max-w-measure py-7">
                Channel list could not be loaded: {String(channels.error)}
              </p>
            )}
            {isAwaited(channels) && (
              <p className="text-text-dim text-body py-7">Loading channels…</p>
            )}
            {channels.data && (
              <>
                <ListTable
                  label={`${chosen.label} channels`}
                  rows={channels.data.data}
                  rowKey={(channel) => channel.channelId}
                  empty={`No ${chosen.label.toLowerCase()} channels observed.`}
                  columns={spanHeads<FiberChannel>([
                    {
                      key: 'channel',
                      className: ROW,
                      label: 'Channel',
                      // The channel id at the row size (17 characters, 142.8), or the widest
                      // state "All" writes under it, "force-closed · not settled" (26 at 12 px,
                      // 187.2, the wider), and the gap. It was 200.
                      minWidth: columnPx(
                        Math.max(
                          body(shortChars(SHORT_FORMS.row.hash)),
                          meta(Math.max(...Object.values(FIBER_STATE_WORDS).map((w) => w.length)))
                        ) + CELL_GAP_PX
                      ),
                      // Table-rows T5: the channel id is the row's one bright thing.
                      render: (channel) => (
                        <>
                          <Link
                            href={`/fiber/channels/${channel.channelId}`}
                            className="text-text-bright hover:underline"
                            title={channel.channelId}
                          >
                            <ShortId value={channel.channelId} kind="hash" size="row" />
                          </Link>
                          {stateFilter === 'all' && (
                            <span className="text-text-dim text-meta block">
                              {FIBER_STATE_WORDS[channel.state]}
                            </span>
                          )}
                        </>
                      ),
                    },
                    {
                      key: 'parties',
                      className: ROW,
                      label: 'Parties',
                      // The flexible column (readability F3-e), as the grid's 1fr was: one party
                      // whole at its minimum, its lock hash at the row size (17 characters, 142.8;
                      // an address is 13), and the gap; the pair joins on one line where the
                      // container gives more. It was 180.
                      minWidth: columnPx(body(shortChars(SHORT_FORMS.row.hash)) + CELL_GAP_PX),
                      width: 'auto',
                      render: (channel) => (
                        <span className="flex min-w-0 flex-wrap gap-x-1.5">
                          {channel.observedParties.map((party, index) => (
                            <span key={party.lockScriptHash}>
                              {index > 0 && (
                                <span
                                  className="text-[color:var(--ink)]"
                                  style={inkStyle(BOARD_INKS.quiet)}
                                >
                                  ↔{' '}
                                </span>
                              )}
                              <Party party={party} />
                            </span>
                          ))}
                        </span>
                      ),
                    },
                    {
                      key: 'capacity',
                      className: ROW,
                      label: 'Capacity',
                      // A capacity under a billion CKB, every decimal (20 characters, 168), its
                      // unit " CKB" at 12 px (28.8) and the gap. It was 210.
                      minWidth: columnPx(
                        body(WIDEST_TX_VALUE.length) + meta(' CKB'.length) + CELL_GAP_PX
                      ),
                      numeric: true,
                      render: (channel) => (
                        <CkbAmount
                          form="column"
                          shannons={BigInt(channel.capacity)}
                          signed={false}
                          tone="plain"
                        />
                      ),
                    },
                    {
                      key: 'opened',
                      label: 'Opened',
                      // the date (10 characters, 84) and the gap; it was 120
                      minWidth: columnPx(body(formatUtc8Date(0).length) + CELL_GAP_PX),
                      className: cn(ROW, 'text-text-dim'),
                      render: (channel) => (
                        <span>
                          <Utc8Date timestamp={channel.openTimestamp} />
                        </span>
                      ),
                    },
                    {
                      key: 'lifetime',
                      className: ROW,
                      label: lastColumn,
                      // "9999d 23h" (9 characters, 75.6), wider than the head ("Lifetime" at 12
                      // px); the last column, no gap. It was 110.
                      minWidth: columnPx(body(WIDEST_LIFETIME.length)),
                      numeric: true,
                      render: (channel) => <span>{lifetime(channel)}</span>,
                    },
                  ])}
                  mobileRow={(channel, wide) => <ChannelPhoneRow channel={channel} wide={wide} />}
                />
                {/* The controls board's pager for a ranked list (the channels by capacity). */}
                {channelsAt && (
                  <Pager
                    mode="pages"
                    label="channels"
                    {...channelsAt}
                    onPrevious={pagination.goToPrevious}
                    onNext={() => pagination.goToNext(channels.data?.nextCursor)}
                  />
                )}
              </>
            )}
          </section>
        </div>
        <div id="evidence" className="scroll-mt-4">
          <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-2">
            <DetailSection title="What the chain sees of Fiber">
              <Prose>
                <p>
                  Fiber is a payment-channel network on CKB. Two parties lock CKB, or a token, in
                  one shared cell and pay each other off chain as often as they like. Only opening
                  and closing a channel touch the chain, so that is what CKBadger sees.
                </p>
                {network.data && <FiberPartiesNote network={network.data} />}
              </Prose>
            </DetailSection>
            <DetailSection title="How to read the graph">
              <FiberGraphNotes />
            </DetailSection>
          </DetailTier2>
        </div>
      </main>
    </div>
  );
}
