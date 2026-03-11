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
import { Badge, PageHeader } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { formatTimeAgo } from '@/lib/utils';

export default function BlocksPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = 25;

  const { data, isLoading } = useQuery({
    queryKey: ['blocks', cursor, limit],
    queryFn: () => api.getBlocks({ cursor, limit }),
    staleTime: 30_000,
    placeholderData: keepPreviousData,
  });

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader title="Blocks" subtitle="Browse all blocks on the CKB network" />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Block List</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="border-base-border bg-base-surface/50 text-text-muted hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider sm:flex">
              <div className="w-24 shrink-0">Block</div>
              <div className="min-w-0 flex-1">Hash</div>
              <div className="w-16 shrink-0 text-center">Txs</div>
              <div className="w-24 shrink-0 text-right">Time</div>
            </div>

            {isLoading
              ? Array.from({ length: 10 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="hidden animate-pulse items-center sm:flex">
                      <div className="w-24 shrink-0">
                        <div className="bg-base-elevated h-4 w-20 rounded" />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="bg-base-elevated h-4 w-48 rounded" />
                      </div>
                      <div className="w-16 shrink-0 text-center">
                        <div className="bg-base-elevated mx-auto h-4 w-8 rounded" />
                      </div>
                      <div className="w-24 shrink-0 text-right">
                        <div className="bg-base-elevated ml-auto h-4 w-20 rounded" />
                      </div>
                    </div>
                    <div className="animate-pulse space-y-1 sm:hidden">
                      <div className="flex items-center justify-between">
                        <div className="bg-base-elevated h-4 w-20 rounded" />
                        <div className="bg-base-elevated h-4 w-16 rounded" />
                      </div>
                      <div className="bg-base-elevated h-4 w-40 rounded" />
                      <div className="bg-base-elevated h-4 w-12 rounded" />
                    </div>
                  </TerminalRow>
                ))
              : data?.data?.map((block) => (
                  <TerminalRow key={block.number}>
                    {/* Table layout (sm+) */}
                    <div className="hidden items-center sm:flex">
                      <div className="w-24 shrink-0">
                        <Link
                          href={`/blocks/${block.number}`}
                          className="text-emphasis font-mono hover:underline"
                        >
                          #{block.number.toLocaleString()}
                        </Link>
                        {block.hardforkActivation && (
                          <div className="mt-1">
                            <Badge variant="amber" className="text-[10px]">
                              HF · {block.hardforkActivation.shortName.toUpperCase()}
                            </Badge>
                          </div>
                        )}
                      </div>
                      <div className="min-w-0 flex-1">
                        <Link href={`/blocks/${block.hash}`} className="hover:underline">
                          <HexDisplay value={block.hash} startChars={12} endChars={8} />
                        </Link>
                      </div>
                      <div className="text-warning w-16 shrink-0 text-center font-mono">
                        {block.transactionsCount}
                      </div>
                      <div className="text-text-muted w-24 shrink-0 text-right">
                        {formatTimeAgo(block.timestamp)}
                      </div>
                    </div>
                    {/* Card layout (<sm) */}
                    <div className="space-y-1 sm:hidden">
                      <div className="flex items-center justify-between">
                        <Link
                          href={`/blocks/${block.number}`}
                          className="text-emphasis font-mono hover:underline"
                        >
                          #{block.number.toLocaleString()}
                        </Link>
                        <span className="text-text-muted text-xs">
                          {formatTimeAgo(block.timestamp)}
                        </span>
                      </div>
                      <div>
                        <Link href={`/blocks/${block.hash}`} className="hover:underline">
                          <HexDisplay value={block.hash} startChars={10} endChars={6} />
                        </Link>
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="text-warning font-mono">
                          {block.transactionsCount} txs
                        </span>
                        {block.hardforkActivation && (
                          <Badge variant="amber" className="text-[10px]">
                            HF · {block.hardforkActivation.shortName.toUpperCase()}
                          </Badge>
                        )}
                      </div>
                    </div>
                  </TerminalRow>
                ))}
          </TerminalPanelContent>

          {data && (
            <TerminalPanelFooter>
              <CursorPagination
                total={data.total}
                totalLabel="blocks"
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
