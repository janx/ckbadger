'use client';

import { useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { PageHeader } from '@/components/ui/page-header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { StatBlock, StatGrid } from '@/components/ui/stat-block';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type FiberChannel, type FiberChannelState } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatTimeAgo, formatCkbAmount, formatCkbCompact } from '@/lib/utils';
import { Badge } from '@/components/ui/page-header';

type StateFilter = 'all' | FiberChannelState;

const STATE_FILTERS: { value: StateFilter; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'open', label: 'Open' },
  { value: 'cooperativelyClosed', label: 'Closed' },
  { value: 'forceClosed', label: 'Force Closed' },
  { value: 'settled', label: 'Settled' },
];

function getStateBadge(state: FiberChannelState) {
  switch (state) {
    case 'open':
      return <Badge variant="green">Open</Badge>;
    case 'cooperativelyClosed':
      return <Badge variant="gray">Closed</Badge>;
    case 'forceClosed':
      return <Badge variant="red">Force Closed</Badge>;
    case 'settled':
      return <Badge variant="neutral">Settled</Badge>;
    default:
      return <Badge variant="gray">{state}</Badge>;
  }
}

function truncateAddress(addr: string): string {
  if (addr.length <= 20) return addr;
  return `${addr.slice(0, 10)}...${addr.slice(-8)}`;
}

export default function FiberChannelsPage() {
  const [stateFilter, setStateFilter] = useState<StateFilter>('all');
  const pagination = useCursorPagination();

  const { data: stats } = useQuery({
    queryKey: ['fiber-stats'],
    queryFn: () => api.getFiberStats(),
  });

  const { data: channels, isLoading } = useQuery({
    queryKey: ['fiber-channels', stateFilter, pagination.cursor],
    queryFn: () =>
      api.getFiberChannels({
        limit: DEFAULT_PAGE_SIZE,
        cursor: pagination.cursor,
        state: stateFilter === 'all' ? undefined : stateFilter,
      }),
    placeholderData: keepPreviousData,
  });

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <PageHeader title="Fiber Network" subtitle="Payment Channel Explorer" />

        {stats && (
          <TerminalPanel className="mb-8">
            <TerminalPanelContent>
              <StatGrid columns={4}>
                <StatBlock label="Total Channels" value={stats.totalChannels} color="default" />
                <StatBlock label="Open Channels" value={stats.openChannels} color="jade" />
                <StatBlock label="Closed Channels" value={stats.closedChannels} color="default" />
                <StatBlock
                  label="Locked Capacity"
                  value={formatCkbCompact(stats.totalCapacityLocked).value}
                  suffix=" CKB"
                  color="gold"
                  subtext={formatCkbAmount(stats.totalCapacityLocked).full}
                />
              </StatGrid>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        <TerminalPanel>
          <TerminalPanelHeader>Channels</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="border-base-border flex items-center gap-1.5 border-b px-4 py-2">
              {STATE_FILTERS.map((f) => (
                <button
                  key={f.value}
                  onClick={() => {
                    setStateFilter(f.value);
                    pagination.reset();
                  }}
                  className={`rounded px-2 py-0.5 font-mono text-xs transition-colors ${
                    stateFilter === f.value
                      ? 'bg-emphasis/15 text-emphasis'
                      : 'text-text-dim hover:text-text'
                  }`}
                >
                  {f.label}
                </button>
              ))}
            </div>

            {isLoading ? (
              <div className="text-text-dim py-12 text-center">Loading channels...</div>
            ) : channels?.data && channels.data.length > 0 ? (
              <>
                <div
                  className="border-base-border bg-base-surface/50 text-text-dim hidden items-center gap-x-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider md:grid"
                  style={{ gridTemplateColumns: '12rem 6rem 1fr 8rem 5.5rem' }}
                >
                  <div>Channel ID</div>
                  <div>Status</div>
                  <div>Participants</div>
                  <div className="text-right">Capacity</div>
                  <div className="text-right">Opened</div>
                </div>
                {channels.data.map((channel: FiberChannel) => (
                  <TerminalRow key={channel.channelId}>
                    <div
                      className="hidden w-full items-center gap-x-4 md:grid"
                      style={{ gridTemplateColumns: '12rem 6rem 1fr 8rem 5.5rem' }}
                    >
                      <div>
                        <Link href={`/fiber/channels/${channel.channelId}`}>
                          <HexDisplay
                            value={channel.channelId}
                            truncate
                            startChars={8}
                            endChars={6}
                          />
                        </Link>
                      </div>
                      <div>{getStateBadge(channel.state)}</div>
                      <div className="flex flex-wrap gap-1">
                        {channel.participants.map((addr) => (
                          <Link
                            key={addr}
                            href={`/address/${addr}`}
                            className="text-text-dim hover:text-text font-mono text-xs transition-colors"
                          >
                            {truncateAddress(addr)}
                          </Link>
                        ))}
                      </div>
                      <div className="text-text-bright text-right font-mono text-sm">
                        {formatCkbAmount(channel.capacity).full} CKB
                      </div>
                      <div className="text-text-dim text-right text-sm">
                        {formatTimeAgo(channel.openTimestamp)}
                      </div>
                    </div>

                    {/* Mobile layout */}
                    <div className="space-y-1.5 md:hidden">
                      <div className="flex items-center justify-between gap-2">
                        <Link href={`/fiber/channels/${channel.channelId}`}>
                          <HexDisplay
                            value={channel.channelId}
                            truncate
                            startChars={8}
                            endChars={6}
                          />
                        </Link>
                        {getStateBadge(channel.state)}
                      </div>
                      <div className="flex items-center justify-between gap-2 text-sm">
                        <span className="text-text-bright font-mono">
                          {formatCkbAmount(channel.capacity).full} CKB
                        </span>
                        <span className="text-text-dim text-xs">
                          {formatTimeAgo(channel.openTimestamp)}
                        </span>
                      </div>
                      <div className="text-text-dim flex flex-wrap gap-1 font-mono text-xs">
                        {channel.participants.map((addr) => (
                          <Link
                            key={addr}
                            href={`/address/${addr}`}
                            className="hover:text-text transition-colors"
                          >
                            {truncateAddress(addr)}
                          </Link>
                        ))}
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </>
            ) : (
              <div className="text-text-dim py-12 text-center">
                {stateFilter === 'all' ? 'No channels found' : `No ${stateFilter} channels found`}
              </div>
            )}
          </TerminalPanelContent>
          {(channels?.hasMore || pagination.hasPrevious) && (
            <TerminalPanelFooter className="flex justify-center">
              <CursorPagination
                total={channels?.total}
                totalLabel="channels"
                pageSize={DEFAULT_PAGE_SIZE}
                hasMore={channels?.hasMore ?? false}
                hasPrevious={pagination.hasPrevious}
                page={pagination.page}
                onNext={() => pagination.goToNext(channels?.nextCursor)}
                onPrevious={pagination.goToPrevious}
              />
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
