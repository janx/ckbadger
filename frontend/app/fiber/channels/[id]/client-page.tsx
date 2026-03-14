'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { PageHeader, Badge } from '@/components/ui/page-header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { StatBlock, StatGrid } from '@/components/ui/stat-block';
import { HexDisplay } from '@/components/ui/hex-display';
import { api, type FiberChannelState, type FiberTimelineEvent } from '@/lib/api';
import { useParams } from '@/src/navigation';
import { formatTimeAgo, formatCkbAmount } from '@/lib/utils';

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

const EVENT_LABELS: Record<string, string> = {
  channel_open: 'Channel Opened',
  channel_close: 'Channel Closed',
  force_close: 'Force Closed',
  settlement: 'Settlement',
};

function getEventLabel(event: string): string {
  return EVENT_LABELS[event] ?? event.replace(/_/g, ' ');
}

function getEventColor(event: string): string {
  switch (event) {
    case 'channel_open':
      return 'text-jade';
    case 'channel_close':
      return 'text-text';
    case 'force_close':
      return 'text-negative';
    case 'settlement':
      return 'text-gold';
    default:
      return 'text-text-dim';
  }
}

export default function FiberChannelDetailPage() {
  const params = useParams();
  const channelId = params.id as string;

  const {
    data: channel,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['fiber-channel', channelId],
    queryFn: () => api.getFiberChannel(channelId),
  });

  if (isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <div className="animate-pulse">
            <div className="bg-base-surface mb-8 h-12 w-64 rounded" />
            <div className="mb-8 grid gap-4 md:grid-cols-3">
              <div className="bg-base-surface h-32 rounded" />
              <div className="bg-base-surface h-32 rounded" />
              <div className="bg-base-surface h-32 rounded" />
            </div>
          </div>
        </main>
      </div>
    );
  }

  if (error || !channel) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">Channel not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <PageHeader
          title="Fiber Channel"
          hash={channel.channelId}
          badge={getStateBadge(channel.state)}
        />

        <TerminalPanel className="mb-8">
          <TerminalPanelContent>
            <StatGrid columns={3}>
              <StatBlock
                label="Capacity"
                value={formatCkbAmount(channel.capacity).full}
                suffix=" CKB"
                color="gold"
              />
              <StatBlock label="State" value={channel.state} color="default" />
              <StatBlock
                label="Opened"
                value={formatTimeAgo(channel.openTimestamp)}
                color="default"
              />
            </StatGrid>
          </TerminalPanelContent>
        </TerminalPanel>

        <div className="mb-8 grid gap-8 md:grid-cols-2">
          <TerminalPanel>
            <TerminalPanelHeader>Participants</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              {channel.participants.map((addr, i) => (
                <TerminalRow key={addr}>
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-text-dim font-mono text-xs">Participant {i + 1}</span>
                    <Link
                      href={`/address/${addr}`}
                      className="text-text hover:text-aqua font-mono text-xs transition-colors"
                    >
                      <HexDisplay value={addr} truncate startChars={12} endChars={8} />
                    </Link>
                  </div>
                </TerminalRow>
              ))}
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader>Details</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <TerminalRow>
                <div className="flex items-center justify-between gap-2">
                  <span className="text-text-dim font-mono text-xs">Funding TX</span>
                  <Link
                    href={`/tx/${channel.fundingTxHash}`}
                    className="text-text hover:text-aqua font-mono text-xs transition-colors"
                  >
                    <HexDisplay
                      value={channel.fundingTxHash}
                      truncate
                      startChars={8}
                      endChars={6}
                    />
                  </Link>
                </div>
              </TerminalRow>
              <TerminalRow>
                <div className="flex items-center justify-between gap-2">
                  <span className="text-text-dim font-mono text-xs">Output Index</span>
                  <span className="text-text font-mono text-xs">{channel.fundingOutputIndex}</span>
                </div>
              </TerminalRow>
              <TerminalRow>
                <div className="flex items-center justify-between gap-2">
                  <span className="text-text-dim font-mono text-xs">Opened At Block</span>
                  <Link
                    href={`/blocks/${channel.openBlock}`}
                    className="text-text hover:text-aqua font-mono text-xs transition-colors"
                  >
                    #{channel.openBlock.toLocaleString()}
                  </Link>
                </div>
              </TerminalRow>
              {channel.closeBlock !== null && (
                <TerminalRow>
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-text-dim font-mono text-xs">Closed At Block</span>
                    <Link
                      href={`/blocks/${channel.closeBlock}`}
                      className="text-text hover:text-aqua font-mono text-xs transition-colors"
                    >
                      #{channel.closeBlock.toLocaleString()}
                    </Link>
                  </div>
                </TerminalRow>
              )}
              {channel.closeTxHash && (
                <TerminalRow>
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-text-dim font-mono text-xs">Close TX</span>
                    <Link
                      href={`/tx/${channel.closeTxHash}`}
                      className="text-text hover:text-aqua font-mono text-xs transition-colors"
                    >
                      <HexDisplay
                        value={channel.closeTxHash}
                        truncate
                        startChars={8}
                        endChars={6}
                      />
                    </Link>
                  </div>
                </TerminalRow>
              )}
              {channel.settlementBlock !== null && (
                <TerminalRow>
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-text-dim font-mono text-xs">Settlement Block</span>
                    <Link
                      href={`/blocks/${channel.settlementBlock}`}
                      className="text-text hover:text-aqua font-mono text-xs transition-colors"
                    >
                      #{channel.settlementBlock.toLocaleString()}
                    </Link>
                  </div>
                </TerminalRow>
              )}
              {channel.settlementTxHash && (
                <TerminalRow>
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-text-dim font-mono text-xs">Settlement TX</span>
                    <Link
                      href={`/tx/${channel.settlementTxHash}`}
                      className="text-text hover:text-aqua font-mono text-xs transition-colors"
                    >
                      <HexDisplay
                        value={channel.settlementTxHash}
                        truncate
                        startChars={8}
                        endChars={6}
                      />
                    </Link>
                  </div>
                </TerminalRow>
              )}
            </TerminalPanelContent>
          </TerminalPanel>
        </div>

        {channel.timeline && channel.timeline.length > 0 && (
          <TerminalPanel>
            <TerminalPanelHeader>Lifecycle Timeline</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <div className="relative">
                {channel.timeline.map((evt: FiberTimelineEvent, i: number) => (
                  <TerminalRow key={`${evt.event}-${evt.blockNumber}`}>
                    <div className="flex items-center gap-4">
                      <div className="flex items-center gap-2">
                        <div
                          className={`h-2.5 w-2.5 rounded-full ${
                            evt.event === 'channel_open'
                              ? 'bg-jade'
                              : evt.event === 'force_close'
                                ? 'bg-negative'
                                : evt.event === 'settlement'
                                  ? 'bg-gold'
                                  : 'bg-text-dim'
                          }`}
                        />
                        {i < channel.timeline.length - 1 && (
                          <div className="bg-base-border absolute bottom-0 left-[1.15rem] top-0 w-px" />
                        )}
                      </div>
                      <div className="flex flex-1 items-center justify-between gap-2">
                        <div>
                          <span className={`font-mono text-sm ${getEventColor(evt.event)}`}>
                            {getEventLabel(evt.event)}
                          </span>
                          <div className="text-text-dim flex items-center gap-2 font-mono text-xs">
                            <Link
                              href={`/blocks/${evt.blockNumber}`}
                              className="hover:text-text transition-colors"
                            >
                              #{evt.blockNumber.toLocaleString()}
                            </Link>
                            <Link
                              href={`/tx/${evt.txHash}`}
                              className="hover:text-text transition-colors"
                            >
                              <HexDisplay
                                value={evt.txHash}
                                truncate
                                startChars={6}
                                endChars={4}
                                size="sm"
                              />
                            </Link>
                          </div>
                        </div>
                        <span className="text-text-dim shrink-0 font-mono text-xs">
                          {formatTimeAgo(evt.timestamp)}
                        </span>
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}
      </main>
    </div>
  );
}
