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
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader title="Blocks" subtitle="Browse all blocks on the CKB network" />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Block List</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
              <div className="w-32">Block</div>
              <div className="flex-1">Hash</div>
              <div className="w-20 text-center">Txs</div>
              <div className="w-32 text-right">Time</div>
            </div>

            {isLoading
              ? Array.from({ length: 10 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="flex animate-pulse items-center">
                      <div className="w-32">
                        <div className="h-4 w-20 rounded bg-slate-800" />
                      </div>
                      <div className="flex-1">
                        <div className="h-4 w-48 rounded bg-slate-800" />
                      </div>
                      <div className="w-20 text-center">
                        <div className="mx-auto h-4 w-8 rounded bg-slate-800" />
                      </div>
                      <div className="w-32 text-right">
                        <div className="ml-auto h-4 w-20 rounded bg-slate-800" />
                      </div>
                    </div>
                  </TerminalRow>
                ))
              : data?.data?.map((block) => (
                  <TerminalRow key={block.number}>
                    <div className="flex items-center">
                      <div className="w-32">
                        <Link
                          href={`/blocks/${block.number}`}
                          className="text-terminal-green font-mono hover:underline"
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
                      <div className="flex-1">
                        <Link href={`/blocks/${block.hash}`} className="hover:underline">
                          <HexDisplay
                            value={block.hash}
                            color="white"
                            startChars={12}
                            endChars={8}
                          />
                        </Link>
                      </div>
                      <div className="text-amber w-20 text-center font-mono">
                        {block.transactionsCount}
                      </div>
                      <div className="w-32 text-right text-slate-500">
                        {formatTimeAgo(block.timestamp)}
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
