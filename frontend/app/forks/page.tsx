'use client';

import Link from '@/components/ui/link';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { formatTimeAgo } from '@/lib/utils';

export default function ForksPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = 25;

  const { data, isLoading } = useQuery({
    queryKey: ['forks', cursor, limit],
    queryFn: () => api.getForks({ cursor, limit }),
    staleTime: 30_000,
    placeholderData: keepPreviousData,
  });

  const getBadgeVariant = (type: string): 'red' | 'blue' | 'green' | 'gray' => {
    switch (type.toLowerCase()) {
      case 'deep_fork':
      case 'deep':
        return 'red';
      case 'reorg':
        return 'blue';
      case 'resolved':
        return 'green';
      default:
        return 'gray';
    }
  };

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Fork Events"
          subtitle="Monitor blockchain reorganizations and fork events"
        />

        <TerminalPanel>
          <TerminalPanelHeader indicator="warning">Fork Event Log</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="border-base-border bg-base-surface/50 text-text-muted flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
              <div className="w-24">Event</div>
              <div className="w-16 text-center">Depth</div>
              <div className="w-28">Fork Point</div>
              <div className="flex-1">Old Tip</div>
              <div className="flex-1">New Tip</div>
              <div className="w-24 text-center">Orphaned</div>
              <div className="w-28 text-right">Time</div>
            </div>

            {isLoading
              ? Array.from({ length: 10 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="flex animate-pulse items-center">
                      <div className="w-24">
                        <div className="bg-base-elevated h-5 w-16 rounded" />
                      </div>
                      <div className="w-16 text-center">
                        <div className="bg-base-elevated mx-auto h-4 w-8 rounded" />
                      </div>
                      <div className="w-28">
                        <div className="bg-base-elevated h-4 w-20 rounded" />
                      </div>
                      <div className="flex-1">
                        <div className="bg-base-elevated h-8 w-32 rounded" />
                      </div>
                      <div className="flex-1">
                        <div className="bg-base-elevated h-8 w-32 rounded" />
                      </div>
                      <div className="w-24 text-center">
                        <div className="bg-base-elevated mx-auto h-8 w-16 rounded" />
                      </div>
                      <div className="w-28 text-right">
                        <div className="bg-base-elevated ml-auto h-4 w-20 rounded" />
                      </div>
                    </div>
                  </TerminalRow>
                ))
              : data?.data?.map((event) => (
                  <TerminalRow key={event.id}>
                    <div className="flex items-center">
                      <div className="w-24">
                        <Link href={`/forks/${event.id}`}>
                          <Badge variant={getBadgeVariant(event.eventType)}>
                            {event.eventType.toUpperCase()}
                          </Badge>
                        </Link>
                      </div>
                      <div className="text-warning w-16 text-center font-mono">{event.depth}</div>
                      <div className="w-28">
                        <Link
                          href={`/blocks/${event.forkPointNumber}`}
                          className="text-emphasis font-mono hover:underline"
                        >
                          #{event.forkPointNumber.toLocaleString()}
                        </Link>
                      </div>
                      <div className="flex-1">
                        <div className="text-text-secondary font-mono">
                          #{event.oldTipNumber.toLocaleString()}
                        </div>
                        <HexDisplay
                          value={event.oldTipHash}
                          color="accent"
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                      <div className="flex-1">
                        <div className="text-text-secondary font-mono">
                          #{event.newTipNumber.toLocaleString()}
                        </div>
                        <HexDisplay
                          value={event.newTipHash}
                          color="accent"
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                      <div className="w-24 text-center">
                        <div className="font-mono text-sm text-red-400">
                          {event.orphanedBlocksCount} blocks
                        </div>
                        <div className="text-text-muted font-mono text-xs">
                          {event.orphanedTxsCount} txs
                        </div>
                      </div>
                      <div className="text-text-muted w-28 text-right">
                        {formatTimeAgo(event.detectedAt)}
                      </div>
                    </div>
                  </TerminalRow>
                ))}
          </TerminalPanelContent>

          {data && (
            <TerminalPanelFooter>
              <CursorPagination
                total={data.total}
                totalLabel="events"
                pageSize={limit}
                page={page}
                hasMore={data.hasMore}
                hasPrevious={hasPrevious}
                onNext={() => goToNext(data.nextCursor)}
                onPrevious={goToPrevious}
              />
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
