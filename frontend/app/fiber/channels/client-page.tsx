'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import {
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
import { cn, formatUtc8Date } from '@/lib/utils';
import { BOARD_INKS, inkStyle } from '@/lib/design-tokens';
import { FIBER_STATE_WORDS, fiberDuration } from '@/lib/fiber-format';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import { WIDEST_FIBER_LIFETIME, WIDEST_TX_VALUE_SUMMARY, shortChars } from '@/lib/list-bounds';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
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
 * The 6 px between the two parties of a pair on its line (the flex gap, set from this constant),
 * so the Parties column budgets the pair whole.
 */
const PARTY_GAP_PX = 6;

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
  // a link inside the row's line: the channel id is the row's key (T5)
  return party.address ? (
    <Link href={`/address/${party.address}`} look="inline" title={party.address}>
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
  // a link inside the row's line, as the table writes it: the channel id is the row's key
  return party.address ? (
    <Link href={`/address/${party.address}`} look="inline" title={party.address}>
      {text}
    </Link>
  ) : (
    <span title={party.lockScriptHash}>{text}</span>
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
 * Line 1 of a channel on a phone (the phone-rows board's head()): the channel id at its tight size
 * below sm, at the key's 14 px (11 characters, 92.4; no stone, as the table's), the 12 px between the
 * parts and the channel's capacity, the list's sort, in the summary form at its widest, a capacity
 * under a billion CKB cut to whole CKB ("999,999,999", WIDEST_TX_VALUE_SUMMARY, 11 characters at
 * 13 px, 85.8) and " CKB" at the unit's 12 px (28.8): 219 of a 360 px phone's 328 px line.
 */
const CHANNEL_PHONE_HEAD_PX = phoneHead(
  'Fiber channels',
  textPx(shortChars(SHORT_FORMS.tight.hash), fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(WIDEST_TX_VALUE_SUMMARY.length, fontPx('small')) +
    textPx(' CKB'.length, fontPx('meta'))
);

/**
 * A channel on a phone (the phone-rows board; PB-17): the fields the table states and no others —
 * the channel and its capacity, the list's sort; its parties, joined as the table joins them; quiet,
 * the day it opened and how long it has been open or lived, each named, and its state where the
 * list is of every state (`showState`, as the table writes it under the id there). The cards from
 * 560 px take the same three parts.
 */
function ChannelPhoneRow({
  channel,
  showState,
  lived,
}: {
  channel: FiberChannel;
  showState: boolean;
  /** How long it is open or lived, in the table's head's words ("open for", "lifetime"). */
  lived: string;
}) {
  // the channel id at the size of its parties (R9): tight below sm, the row size from it
  const size = useTightBelowSm();
  return (
    <PhoneRow
      testId="fiber-channel-row-mobile"
      head={CHANNEL_PHONE_HEAD_PX}
      lines={[
        {
          key: 'channel',
          tone: 'head',
          left: (
            <Link
              href={`/fiber/channels/${channel.channelId}`}
              look="key"
              title={channel.channelId}
            >
              <ShortId value={channel.channelId} kind="hash" size={size} />
            </Link>
          ),
          right: (
            <span className="text-text-bright">
              <CkbAmount
                form="summary"
                shannons={BigInt(channel.capacity)}
                signed={false}
                tone="plain"
              />
            </span>
          ),
        },
        {
          key: 'parties',
          tone: 'body',
          left: (
            <span className="flex min-w-0 flex-wrap gap-x-1.5">
              {channel.observedParties.map((party, index) => (
                <span key={party.lockScriptHash}>
                  {index > 0 && <span className="text-text-dim">↔ </span>}
                  <PartySuffix party={party} />
                </span>
              ))}
            </span>
          ),
        },
        {
          key: 'time',
          tone: 'quiet',
          left: (
            <Dotted
              parts={[
                <Labelled key="opened" label="opened">
                  <Utc8Date timestamp={channel.openTimestamp} />
                </Labelled>,
                <Labelled key="lived" label={lived}>
                  {lifetime(channel)}
                </Labelled>,
                showState && <ChannelState key="state" state={channel.state} />,
              ]}
            />
          ),
        },
      ]}
    />
  );
}

export default function FiberChannelsPage() {
  const [stateFilter, setStateFilter] = useState<StateFilter>('open');
  const pageSize = usePageSize(DEFAULT_PAGE_SIZE);
  // a change of page size at md starts the walk again (lane c2-finish, 1d), and so does a change of the list it pages (lane cd-fixes-controls): the state
  const pagination = useCursorPagination(pageSize, { list: [stateFilter] });
  const network = useQuery({ queryKey: ['fiber-network'], queryFn: api.getFiberNetwork });
  const pin = useFiberGraphPin();
  const channels = useQuery({
    queryKey: ['fiber-channels', stateFilter, pagination.cursor, pageSize],
    queryFn: pagination.ask((cursor) =>
      api.getFiberChannels({
        limit: pageSize,
        cursor,
        state: stateFilter === 'all' ? undefined : stateFilter,
        sort: 'capacity',
      })
    ),
  });
  const summary = network.data?.summary;
  // where the ranked list stands (lib/pagination.ts): the rows shown of the response's total at
  // its page size, the page the response was asked as (the list keeps no previous page while the
  // next loads, so the two never part); nothing for a list on one page, an empty one too
  const channelsAt = channels.data
    ? rankedPageAt('Fiber channels', channels.data, pagination)
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
                {/* a way onward to a place on this page: the link writes its "↓" */}
                <Link href="#evidence" look="onward">
                  How it works
                </Link>
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
              onChange={setStateFilter}
              counted
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
                      // Table-rows T5: the channel id is the row's one bright thing, its key.
                      render: (channel) => (
                        <>
                          <Link
                            href={`/fiber/channels/${channel.channelId}`}
                            look="key"
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
                      // The pair on one line (lane P4; review fiber-channels: some 380 px stood
                      // empty between the parties and their capacity, the flexible column's): two
                      // lock hashes at the row size, the wider form (17 characters each; an
                      // address is 13), "↔ " before the second, 36 characters at 14 px (302.4),
                      // the 6 px between them and the gap: 324.4. It was one party (158.8) and
                      // the rest of the container.
                      minWidth: columnPx(
                        body(2 * shortChars(SHORT_FORMS.row.hash) + '↔ '.length) +
                          PARTY_GAP_PX +
                          CELL_GAP_PX
                      ),
                      render: (channel) => (
                        <span
                          className="flex min-w-0 flex-wrap"
                          style={{ columnGap: PARTY_GAP_PX }}
                        >
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
                      // A list column's figure (readability R5; lane P4, review fiber-channels:
                      // every amount wrote ".00000000 CKB"): the summary form, as the phone row's
                      // line 1 writes it, its exact value in its title. A capacity under a billion
                      // CKB cut to whole CKB, "999,999,999" (11 characters, 92.4), its unit " CKB"
                      // at 12 px (28.8) and the gap: 137.2. It was every decimal, 212.8.
                      minWidth: columnPx(
                        body(WIDEST_TX_VALUE_SUMMARY.length) + meta(' CKB'.length) + CELL_GAP_PX
                      ),
                      numeric: true,
                      render: (channel) => (
                        <CkbAmount
                          form="summary"
                          shannons={BigInt(channel.capacity)}
                          signed={false}
                          tone="plain"
                        />
                      ),
                    },
                    {
                      key: 'opened',
                      label: 'Opened',
                      // the date (10 characters, 84) and the gap; it was 120. A date is a value,
                      // in the reading ink (R3; it was the quiet ink).
                      minWidth: columnPx(body(formatUtc8Date(0).length) + CELL_GAP_PX),
                      className: ROW,
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
                      // px); the last column, no gap. It was 110. The flexible column (lane P4):
                      // read from its left, so the room the container gives beyond the columns'
                      // sum (843 px) stands at the row's end, after the last figure.
                      minWidth: columnPx(body(WIDEST_FIBER_LIFETIME.length)),
                      width: 'auto',
                      render: (channel) => <span>{lifetime(channel)}</span>,
                    },
                  ])}
                  mobileRow={(channel) => (
                    <ChannelPhoneRow
                      channel={channel}
                      showState={stateFilter === 'all'}
                      lived={lastColumn.toLowerCase()}
                    />
                  )}
                />
                {/* The controls board's pager for a ranked list (the channels by capacity). */}
                {channelsAt && <Pager mode="pages" label="channels" {...channelsAt} />}
              </>
            )}
          </section>
        </div>
        <div id="evidence" className="scroll-mt-4">
          {/* One column: how to read the graph is its key in tier 1 now (the plan's P4). */}
          <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0">
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
          </DetailTier2>
        </div>
      </main>
    </div>
  );
}
