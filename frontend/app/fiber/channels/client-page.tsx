'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { FiberNetworkGraph } from '@/components/fiber/fiber-network-graph';
import { observedDuration } from '@/components/fiber/fiber-channel-board';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type FiberChannelState } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatTimeAgo, formatCkbAmount } from '@/lib/utils';

type StateFilter = 'all' | FiberChannelState;
const filters: { value: StateFilter; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'open', label: 'Open' },
  { value: 'cooperativelyClosed', label: 'Cooperative' },
  { value: 'forceClosed', label: 'Force-close waiting' },
  { value: 'settled', label: 'Settled' },
];
const stateLabel: Record<FiberChannelState, string> = {
  open: 'Open',
  cooperativelyClosed: 'Cooperative close',
  forceClosed: 'Force-close waiting',
  settled: 'Settled',
};
const short = (hash: string) => `${hash.slice(0, 10)}…${hash.slice(-8)}`;

export default function FiberChannelsPage() {
  const [stateFilter, setStateFilter] = useState<StateFilter>('all');
  const pagination = useCursorPagination();
  const network = useQuery({ queryKey: ['fiber-network'], queryFn: api.getFiberNetwork });
  const channels = useQuery({
    queryKey: ['fiber-channels', stateFilter, pagination.cursor],
    queryFn: () =>
      api.getFiberChannels({
        limit: DEFAULT_PAGE_SIZE,
        cursor: pagination.cursor,
        state: stateFilter === 'all' ? undefined : stateFilter,
      }),
  });
  const stats = network.data?.summary;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        <div className="text-jade font-mono text-[11px] uppercase tracking-[0.16em]">
          Payment channels · on-chain view
        </div>
        <h1 className="text-text-bright mt-1 text-[26px] font-semibold">Fiber channels</h1>
        <p className="text-text-dim mt-2 max-w-3xl text-[15px] leading-relaxed">
          Fiber parties lock CKB or tokens in a shared cell and can pay off chain. This page shows
          only openings and closes observed on CKB; it cannot reveal off-chain payments, balances,
          or routing.
        </p>
        {network.isError && (
          <p role="alert" className="text-rouge mt-5 text-[13px]">
            Fiber network evidence could not be loaded: {String(network.error)}
          </p>
        )}
        {network.isLoading && (
          <p className="text-text-dim py-8 text-[13px]">Loading indexed Fiber network…</p>
        )}
        {stats && (
          <div className="border-base-border mt-7 grid min-h-[72px] gap-4 border-y py-3 sm:grid-cols-2 lg:grid-cols-5">
            <div>
              <div className="text-text-dim text-[11px]">Channels ever</div>
              <div className="text-text-bright mt-1 font-mono text-[18px]">
                {stats.totalChannels.toLocaleString()}
              </div>
              <div className="text-text-dim text-[11px]">
                {stats.firstOpenTimestamp
                  ? `since ${new Date(Number(stats.firstOpenTimestamp)).toLocaleDateString()}`
                  : 'none yet'}
              </div>
            </div>
            <div>
              <div className="text-text-dim text-[11px]">Open now</div>
              <div className="text-jade mt-1 font-mono text-[18px]">
                {stats.openChannels.toLocaleString()}
              </div>
              <div className="text-text-dim text-[11px]">
                {formatCkbAmount(stats.totalCapacityLocked).full} CKB locked
              </div>
            </div>
            <div>
              <div className="text-text-dim text-[11px]">Observed parties</div>
              <div className="text-text-bright mt-1 font-mono text-[18px]">
                {stats.observedParties.toLocaleString()}
              </div>
              <div className="text-text-dim text-[11px]">
                in {stats.observedPairs.toLocaleString()} on-chain pairs
              </div>
            </div>
            <div>
              <div className="text-text-dim text-[11px]">Force-close waiting</div>
              <div className="text-text-bright mt-1 font-mono text-[18px]">
                {stats.forceClosedWaitingChannels.toLocaleString()}
              </div>
              <div className="text-text-dim text-[11px]">
                {stats.forceClosedSettledChannels} settled · {stats.cooperativelyClosedChannels}{' '}
                cooperative
              </div>
            </div>
            <div>
              <div className="text-text-dim text-[11px]">Observed closed lifetime</div>
              <div className="text-text-bright mt-1 font-mono text-[18px]">
                {observedDuration(stats.typicalClosedLifetimeMs)}
              </div>
              <div className="text-text-dim text-[11px]">
                lower median · {stats.closedWithinHour} under 1 h
              </div>
            </div>
          </div>
        )}
        {network.data && <FiberNetworkGraph network={network.data} />}
        <section className="border-base-border border-t pt-5" aria-label="Fiber channels">
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div>
              <h2 className="text-text-bright text-[18px] font-medium">Channels on CKB</h2>
              <p className="text-text-dim mt-1 text-[12px]">
                Each row is an indexed funding cell and its on-chain lifecycle.
              </p>
            </div>
            <div
              className="flex flex-wrap gap-1"
              role="group"
              aria-label="Filter channels by state"
            >
              {filters.map((filter) => (
                <button
                  key={filter.value}
                  type="button"
                  aria-pressed={stateFilter === filter.value}
                  onClick={() => {
                    setStateFilter(filter.value);
                    pagination.reset();
                  }}
                  className={`border px-2.5 py-1 font-mono text-[11px] ${stateFilter === filter.value ? 'border-jade text-jade' : 'border-base-border text-text-dim hover:text-text'}`}
                >
                  {filter.label}
                </button>
              ))}
            </div>
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
              <div className="border-base-border text-text-dim mt-4 hidden grid-cols-[minmax(0,1.5fr)_minmax(0,1.7fr)_150px_145px_110px] gap-4 border-y py-2 font-mono text-[11px] uppercase tracking-wider md:grid">
                <span>Channel</span>
                <span>Observed locks</span>
                <span>State</span>
                <span className="text-right">Funding capacity</span>
                <span className="text-right">Opened</span>
              </div>
              {channels.data.data.length === 0 && (
                <p className="text-text-dim py-10 text-[13px]">
                  No {stateFilter === 'all' ? '' : `${stateFilter} `}channels observed.
                </p>
              )}
              {channels.data.data.map((channel) => (
                <div
                  key={channel.channelId}
                  className="border-base-border grid gap-2 border-b py-3 text-[12px] md:grid-cols-[minmax(0,1.5fr)_minmax(0,1.7fr)_150px_145px_110px] md:items-center md:gap-4"
                >
                  <Link
                    href={`/fiber/channels/${channel.channelId}`}
                    className="text-jade min-w-0 break-all font-mono hover:underline"
                    title={channel.channelId}
                  >
                    {short(channel.channelId)}
                  </Link>
                  <div className="text-text-dim flex min-w-0 flex-wrap gap-x-2 gap-y-1 font-mono text-[11px]">
                    {channel.observedParties.map((party) =>
                      party.address ? (
                        <Link
                          key={party.lockScriptHash}
                          href={`/address/${party.address}`}
                          className="hover:text-jade"
                          title={party.address}
                        >
                          {short(party.address)}
                        </Link>
                      ) : (
                        <span key={party.lockScriptHash} title={party.lockScriptHash}>
                          Unknown address · {short(party.lockScriptHash)}
                        </span>
                      )
                    )}
                  </div>
                  <span
                    className={
                      channel.state === 'open'
                        ? 'text-jade'
                        : channel.state === 'forceClosed'
                          ? 'text-text-bright'
                          : 'text-text-dim'
                    }
                  >
                    {stateLabel[channel.state]}
                  </span>
                  <span className="text-text-bright font-mono md:text-right">
                    {formatCkbAmount(channel.capacity).full} CKB
                  </span>
                  <span className="text-text-dim md:text-right">
                    {formatTimeAgo(channel.openTimestamp)}
                  </span>
                </div>
              ))}
              {(channels.data.hasMore || pagination.hasPrevious) && (
                <div className="mt-4 flex justify-center">
                  <CursorPagination
                    total={channels.data.total}
                    totalLabel="channels"
                    pageSize={DEFAULT_PAGE_SIZE}
                    hasMore={channels.data.hasMore}
                    hasPrevious={pagination.hasPrevious}
                    page={pagination.page}
                    onNext={() => pagination.goToNext(channels.data?.nextCursor)}
                    onPrevious={pagination.goToPrevious}
                  />
                </div>
              )}
            </>
          )}
        </section>
      </main>
    </div>
  );
}
