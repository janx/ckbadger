'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { FiberNetworkGraph } from '@/components/fiber/fiber-network-graph';
import { TopicHero, type TopicFigure } from '@/components/ui/topic-hero';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type FiberChannel, type FiberChannelState, type FiberNetwork } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatCount } from '@/lib/format-count';
import { cn, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { FIBER_STATE_WORDS, fiberDuration } from '@/lib/fiber-format';
import { ExactCkb } from '@/components/activity-event-row';
import { ListTable } from '@/components/ui/list-table';
import { PhoneRow } from '@/components/ui/phone-row';

type StateFilter = 'all' | FiberChannelState;
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
const shortId = (hash: string) => `${hash.slice(0, 10)}…${hash.slice(-6)}`;

function Party({ party }: { party: FiberChannel['observedParties'][number] }) {
  const text = party.address
    ? `${party.address.slice(0, 10)}…${party.address.slice(-6)}`
    : `${party.lockScriptHash.slice(0, 10)}…${party.lockScriptHash.slice(-6)}`;
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

function heroFigures(summary: FiberNetwork['summary']): TopicFigure[] {
  const closed = summary.totalChannels - summary.openChannels;
  if (closed < 0 || summary.closedWithinHour > closed)
    throw new Error(
      `Fiber summary: ${summary.closedWithinHour} closed within an hour of ${summary.totalChannels} channels, ${summary.openChannels} open`
    );
  return [
    {
      label: 'Open now',
      value: summary.openChannels.toLocaleString('en-US'),
      sub: (
        <>
          locking{' '}
          <ExactCkb shannons={BigInt(summary.totalCapacityLocked)} signed={false} tone="plain" />
        </>
      ),
    },
    {
      label: 'Channels ever',
      value: summary.totalChannels.toLocaleString('en-US'),
      sub: [
        summary.firstOpenTimestamp
          ? `since ${formatUtc8Date(summary.firstOpenTimestamp)}`
          : 'none yet',
        ...summary.channelsByToken.map(
          (token) =>
            `${token.channels.toLocaleString('en-US')} in ${token.symbol ?? `${token.udtTypeHash.slice(0, 10)}…`}`
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

/** A party as the fiber graph labels it (board L8): the last six characters of its address. */
function PartySuffix({ party }: { party: FiberChannel['observedParties'][number] }) {
  const id = party.address ?? party.lockScriptHash;
  const text = `…${id.slice(-6)}`;
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
      className={cn(
        'inline-flex items-center gap-1.5 whitespace-nowrap text-[11px]',
        state === 'open' ? 'text-jade' : 'text-text-dim'
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
  const id = (
    <Link
      href={`/fiber/channels/${channel.channelId}`}
      className="text-[#6aa9c4] hover:underline"
      title={channel.channelId}
    >
      {shortId(channel.channelId)}
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
      <ExactCkb shannons={BigInt(channel.capacity)} signed={false} tone="plain" />
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
                    {id} <span className="text-text-dim text-xs">· </span>
                    <span className="text-xs">{pair}</span>
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
  const pagination = useCursorPagination();
  const network = useQuery({ queryKey: ['fiber-network'], queryFn: api.getFiberNetwork });
  const channels = useQuery({
    queryKey: ['fiber-channels', stateFilter, pagination.cursor],
    queryFn: () =>
      api.getFiberChannels({
        limit: DEFAULT_PAGE_SIZE,
        cursor: pagination.cursor,
        state: stateFilter === 'all' ? undefined : stateFilter,
        sort: 'capacity',
      }),
  });
  const summary = network.data?.summary;
  const chosen = filters.find((filter) => filter.value === stateFilter)!;
  const lastColumn = stateFilter === 'open' ? 'Open for' : 'Lifetime';
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto flex max-w-[1280px] flex-col gap-11 px-4 pb-[72px] pt-7">
        <TopicHero
          title="Fiber channels"
          intro="Fiber is a payment-channel network on CKB. Two parties lock CKB, or a token, in one shared cell and pay each other off chain as often as they like. Only opening and closing a channel touch the chain, so that is what CKBadger sees."
          figures={summary ? heroFigures(summary) : undefined}
        />
        {network.isError && (
          <p role="alert" className="text-rouge text-[13px]">
            Fiber network evidence could not be loaded: {String(network.error)}
          </p>
        )}
        {network.isLoading && (
          <p className="text-text-dim text-[13px]">Loading indexed Fiber network…</p>
        )}
        {network.data && <FiberNetworkGraph network={network.data} />}
        <section aria-labelledby="fiber-channels-heading" className="flex min-w-0 flex-col gap-2.5">
          <div className="flex flex-wrap items-baseline justify-between gap-4">
            <h2 id="fiber-channels-heading" className="text-text-bright text-[15px] font-medium">
              Channels
            </h2>
            <span className="text-text-dim text-[12px]">sorted by capacity</span>
          </div>
          <div className="flex flex-wrap gap-1.5" role="group" aria-label="State">
            {filters.map((filter) => {
              const on = stateFilter === filter.value;
              return (
                <button
                  key={filter.value}
                  type="button"
                  aria-pressed={on}
                  onClick={() => {
                    setStateFilter(filter.value);
                    pagination.reset();
                  }}
                  className={cn(
                    'h-[30px] rounded-md border px-[11px] text-[12px]',
                    on
                      ? 'bg-base-elevated text-text-bright border-[#2c3348]'
                      : 'border-base-border-subtle text-text hover:text-text-bright'
                  )}
                >
                  {filter.label}
                  {summary && (
                    <span className="text-text-dim">
                      {' '}
                      {filter.count(summary).toLocaleString('en-US')}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
          {channels.isError && (
            <p role="alert" className="text-rouge py-7 text-[13px]">
              Channel list could not be loaded: {String(channels.error)}
            </p>
          )}
          {channels.isLoading && (
            <p className="text-text-dim py-7 text-[13px]">Loading channels…</p>
          )}
          {channels.data && (
            <>
              <ListTable
                label={`${chosen.label} channels`}
                rows={channels.data.data}
                rowKey={(channel) => channel.channelId}
                empty={`No ${chosen.label.toLowerCase()} channels observed.`}
                columns={[
                  {
                    key: 'channel',
                    label: 'Channel',
                    minWidth: 200,
                    render: (channel) => (
                      <>
                        <Link
                          href={`/fiber/channels/${channel.channelId}`}
                          className="text-[#6aa9c4] hover:underline"
                          title={channel.channelId}
                        >
                          {shortId(channel.channelId)}
                        </Link>
                        {stateFilter === 'all' && (
                          <span className="text-text-dim block text-[11px]">
                            {FIBER_STATE_WORDS[channel.state]}
                          </span>
                        )}
                      </>
                    ),
                  },
                  {
                    key: 'parties',
                    label: 'Parties',
                    minWidth: 180,
                    render: (channel) => (
                      <span className="flex min-w-0 flex-wrap gap-x-1.5">
                        {channel.observedParties.map((party, index) => (
                          <span key={party.lockScriptHash}>
                            {index > 0 && <span className="text-[#4b5266]">↔ </span>}
                            <Party party={party} />
                          </span>
                        ))}
                      </span>
                    ),
                  },
                  {
                    key: 'capacity',
                    label: 'Capacity',
                    minWidth: 210,
                    numeric: true,
                    render: (channel) => (
                      <ExactCkb shannons={BigInt(channel.capacity)} signed={false} tone="plain" />
                    ),
                  },
                  {
                    key: 'opened',
                    label: 'Opened',
                    minWidth: 120,
                    className: 'text-text-dim',
                    render: (channel) => formatUtc8Date(channel.openTimestamp),
                  },
                  {
                    key: 'lifetime',
                    label: lastColumn,
                    minWidth: 110,
                    numeric: true,
                    render: lifetime,
                  },
                ]}
                mobileRow={(channel, wide) => <ChannelPhoneRow channel={channel} wide={wide} />}
              />
              {(channels.data.hasMore || pagination.hasPrevious) && (
                <CursorPagination
                  className="pt-1"
                  total={channels.data.total}
                  totalLabel="channels"
                  pageSize={DEFAULT_PAGE_SIZE}
                  page={pagination.page}
                  currentCount={channels.data.data.length}
                  hasMore={channels.data.hasMore}
                  hasPrevious={pagination.hasPrevious}
                  onNext={() => pagination.goToNext(channels.data?.nextCursor)}
                  onPrevious={pagination.goToPrevious}
                />
              )}
            </>
          )}
        </section>
      </main>
    </div>
  );
}
