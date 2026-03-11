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
            <div className="border-base-border bg-base-surface/50 text-text-dim hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider lg:flex">
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
                    {/* Table skeleton (lg+) */}
                    <div className="hidden animate-pulse items-center lg:flex">
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
                    {/* Card skeleton (<lg) */}
                    <div className="animate-pulse space-y-1.5 lg:hidden">
                      <div className="flex items-center justify-between gap-2">
                        <div className="bg-base-elevated h-5 w-16 rounded" />
                        <div className="bg-base-elevated h-4 w-20 rounded" />
                      </div>
                      <div className="flex items-center gap-4">
                        <div className="bg-base-elevated h-4 w-16 rounded" />
                        <div className="bg-base-elevated h-4 w-24 rounded" />
                      </div>
                      <div className="flex items-center gap-4">
                        <div className="bg-base-elevated h-4 w-20 rounded" />
                        <div className="bg-base-elevated h-4 w-20 rounded" />
                      </div>
                      <div className="bg-base-elevated h-4 w-32 rounded" />
                    </div>
                  </TerminalRow>
                ))
              : data?.data?.map((event) => (
                  <TerminalRow key={event.id}>
                    {/* Table row (lg+) */}
                    <div className="hidden items-center lg:flex">
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
                        <div className="text-text font-mono">
                          #{event.oldTipNumber.toLocaleString()}
                        </div>
                        <HexDisplay
                          value={event.oldTipHash}
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                      <div className="flex-1">
                        <div className="text-text font-mono">
                          #{event.newTipNumber.toLocaleString()}
                        </div>
                        <HexDisplay
                          value={event.newTipHash}
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                      <div className="w-24 text-center">
                        <div className="text-negative font-mono text-sm">
                          {event.orphanedBlocksCount} blocks
                        </div>
                        <div className="text-text-dim font-mono text-xs">
                          {event.orphanedTxsCount} txs
                        </div>
                      </div>
                      <div className="text-text-dim w-28 text-right">
                        {formatTimeAgo(event.detectedAt)}
                      </div>
                    </div>
                    {/* Card row (<lg) */}
                    <div className="space-y-1.5 lg:hidden">
                      <div className="flex items-center justify-between gap-2">
                        <Link href={`/forks/${event.id}`}>
                          <Badge variant={getBadgeVariant(event.eventType)}>
                            {event.eventType.toUpperCase()}
                          </Badge>
                        </Link>
                        <span className="text-text-dim text-xs">
                          {formatTimeAgo(event.detectedAt)}
                        </span>
                      </div>
                      <div className="flex items-center gap-4 text-xs">
                        <span className="text-text">
                          Depth: <span className="text-warning font-mono">{event.depth}</span>
                        </span>
                        <Link
                          href={`/blocks/${event.forkPointNumber}`}
                          className="text-emphasis font-mono hover:underline"
                        >
                          Fork #{event.forkPointNumber.toLocaleString()}
                        </Link>
                      </div>
                      <div className="flex items-center gap-4 text-xs">
                        <span className="text-text-dim">
                          Old:{' '}
                          <span className="text-text font-mono">
                            #{event.oldTipNumber.toLocaleString()}
                          </span>
                        </span>
                        <span className="text-text-dim">
                          New:{' '}
                          <span className="text-text font-mono">
                            #{event.newTipNumber.toLocaleString()}
                          </span>
                        </span>
                      </div>
                      <div className="text-negative text-xs">
                        {event.orphanedBlocksCount} blocks, {event.orphanedTxsCount} txs orphaned
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
