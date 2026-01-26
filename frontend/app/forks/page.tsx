'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
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
  const { cursor, hasPrevious, goToNext, goToPrevious } = useCursorPagination();
  const limit = 25;

  const { data, isLoading } = useQuery({
    queryKey: ['forks', cursor, limit],
    queryFn: () => api.getForks({ cursor, limit }),
    staleTime: 30_000,
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
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Fork Events"
          subtitle="Monitor blockchain reorganizations and fork events"
        />

        <TerminalPanel>
          <TerminalPanelHeader indicator="warning">Fork Event Log</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
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
                        <div className="h-5 w-16 rounded bg-slate-800" />
                      </div>
                      <div className="w-16 text-center">
                        <div className="mx-auto h-4 w-8 rounded bg-slate-800" />
                      </div>
                      <div className="w-28">
                        <div className="h-4 w-20 rounded bg-slate-800" />
                      </div>
                      <div className="flex-1">
                        <div className="h-8 w-32 rounded bg-slate-800" />
                      </div>
                      <div className="flex-1">
                        <div className="h-8 w-32 rounded bg-slate-800" />
                      </div>
                      <div className="w-24 text-center">
                        <div className="mx-auto h-8 w-16 rounded bg-slate-800" />
                      </div>
                      <div className="w-28 text-right">
                        <div className="ml-auto h-4 w-20 rounded bg-slate-800" />
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
                      <div className="text-amber w-16 text-center font-mono">{event.depth}</div>
                      <div className="w-28">
                        <Link
                          href={`/blocks/${event.forkPointNumber}`}
                          className="text-terminal-green font-mono hover:underline"
                        >
                          #{event.forkPointNumber.toLocaleString()}
                        </Link>
                      </div>
                      <div className="flex-1">
                        <div className="font-mono text-slate-300">
                          #{event.oldTipNumber.toLocaleString()}
                        </div>
                        <HexDisplay
                          value={event.oldTipHash}
                          color="white"
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                      <div className="flex-1">
                        <div className="font-mono text-slate-300">
                          #{event.newTipNumber.toLocaleString()}
                        </div>
                        <HexDisplay
                          value={event.newTipHash}
                          color="white"
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                      <div className="w-24 text-center">
                        <div className="font-mono text-sm text-red-400">
                          {event.orphanedBlocksCount} blocks
                        </div>
                        <div className="font-mono text-xs text-slate-500">
                          {event.orphanedTxsCount} txs
                        </div>
                      </div>
                      <div className="w-28 text-right text-slate-500">
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
