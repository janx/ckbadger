'use client';

import { useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams, notFound } from 'next/navigation';
import { api } from '@/lib/api';
import { Header } from '@/components/layout/header';
import { formatTimeAgo } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';

export default function ForkDetailPage() {
  const params = useParams();
  const id = params.id as string;
  const forkId = parseInt(id, 10);

  const { data, isLoading, error } = useQuery({
    queryKey: ['fork', forkId],
    queryFn: () => api.getForkDetail(forkId),
  });

  if (isNaN(forkId)) {
    notFound();
  }

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

  if (error) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8 text-center">
          <h1 className="text-2xl font-bold text-red-400">Error loading fork event</h1>
          <p className="mt-2 text-slate-400">{(error as Error).message}</p>
        </main>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="mb-6 h-10 w-64 animate-pulse rounded bg-slate-800" />
          <div className="grid gap-6 md:grid-cols-2">
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
          </div>
        </main>
      </div>
    );
  }

  if (!data) {
    notFound();
  }

  const { event, orphanedBlocks, orphanedTransactions } = data;

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title={`Fork Event #${event.id}`}
          badge={
            <Badge variant={getBadgeVariant(event.eventType)}>
              {event.eventType.toUpperCase()}
            </Badge>
          }
        />

        <div className="mb-8 grid gap-6 md:grid-cols-2">
          <TerminalPanel>
            <TerminalPanelHeader indicator="warning">Event Details</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid>
                <DataField label="Detected">
                  <span>
                    {new Date(event.detectedAt).toLocaleString()}{' '}
                    <span className="text-slate-500">({formatTimeAgo(event.detectedAt)})</span>
                  </span>
                </DataField>
                <DataField label="Depth">
                  <span className="text-amber font-mono">{event.depth} blocks</span>
                </DataField>
                <DataField label="Fork Point">
                  <div className="space-y-1">
                    <Link
                      href={`/blocks/${event.forkPointNumber}`}
                      className="text-terminal-green font-mono hover:underline"
                    >
                      #{event.forkPointNumber.toLocaleString()}
                    </Link>
                    <HexDisplay value={event.forkPointHash} color="accent" size="sm" />
                  </div>
                </DataField>
                {event.resolvedAt && (
                  <DataField label="Resolved">
                    <div>
                      {new Date(event.resolvedAt).toLocaleString()}
                      <div className="mt-1 text-xs text-green-400">
                        Action: {event.resolutionAction}
                      </div>
                    </div>
                  </DataField>
                )}
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="warning">Chain Split</TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="space-y-4">
                <div className="rounded border border-red-900/50 bg-red-950/20 p-4">
                  <div className="mb-2 text-xs font-medium uppercase tracking-wider text-red-400">
                    Old Tip (Orphaned)
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="font-mono text-slate-300">
                      Height: {event.oldTipNumber.toLocaleString()}
                    </span>
                  </div>
                  <div className="mt-2">
                    <HexDisplay value={event.oldTipHash} color="accent" size="sm" />
                  </div>
                </div>

                <div className="flex justify-center">
                  <div className="text-amber text-2xl">↓</div>
                </div>

                <div className="rounded border border-green-900/50 bg-green-950/20 p-4">
                  <div className="mb-2 text-xs font-medium uppercase tracking-wider text-green-400">
                    New Tip (Canonical)
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="font-mono text-slate-300">
                      Height: {event.newTipNumber.toLocaleString()}
                    </span>
                  </div>
                  <div className="mt-2">
                    <HexDisplay value={event.newTipHash} color="accent" size="sm" />
                  </div>
                </div>
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        </div>

        <Tabs defaultValue="blocks">
          <TabsList>
            <TabsTrigger value="blocks">Orphaned Blocks ({orphanedBlocks.length})</TabsTrigger>
            <TabsTrigger value="transactions">
              Orphaned Transactions ({orphanedTransactions.length})
            </TabsTrigger>
          </TabsList>

          <TerminalPanel className="mt-4">
            <TabsContent value="blocks" className="m-0">
              <TerminalPanelContent padding="none">
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  <div className="w-32">Number</div>
                  <div className="flex-1">Hash</div>
                  <div className="w-24 text-center">Tx Count</div>
                  <div className="w-28 text-right">Time</div>
                </div>

                {orphanedBlocks.length === 0 ? (
                  <div className="px-4 py-8 text-center text-slate-500">
                    No orphaned blocks data available
                  </div>
                ) : (
                  orphanedBlocks.map((block) => (
                    <TerminalRow key={block.hash}>
                      <div className="flex items-center">
                        <div className="w-32">
                          <Link
                            href={`/blocks/${block.number}`}
                            className="text-terminal-green font-mono hover:underline"
                          >
                            #{block.number.toLocaleString()}
                          </Link>
                        </div>
                        <div className="flex-1">
                          <HexDisplay value={block.hash} color="accent" size="sm" />
                        </div>
                        <div className="w-24 text-center font-mono text-slate-300">
                          {block.transactionsCount}
                        </div>
                        <div className="w-28 text-right text-slate-500">
                          {formatTimeAgo(block.timestamp)}
                        </div>
                      </div>
                    </TerminalRow>
                  ))
                )}
              </TerminalPanelContent>
            </TabsContent>

            <TabsContent value="transactions" className="m-0">
              <TerminalPanelContent padding="none">
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  <div className="flex-1">Transaction</div>
                  <div className="w-36">Fee</div>
                  <div className="w-24 text-right">I/O</div>
                </div>

                {orphanedTransactions.length === 0 ? (
                  <div className="px-4 py-8 text-center text-slate-500">
                    No orphaned transactions data available
                  </div>
                ) : (
                  orphanedTransactions.map((tx) => (
                    <TerminalRow key={tx.hash}>
                      <div className="flex items-center">
                        <div className="flex-1">
                          <Link href={`/tx/${tx.hash}`} className="hover:underline">
                            <HexDisplay value={tx.hash} color="accent" size="sm" />
                          </Link>
                          <Link
                            href={`/blocks/${tx.blockNumber}`}
                            className="block font-mono text-xs text-slate-500 hover:text-slate-300"
                          >
                            #{tx.blockNumber.toLocaleString()}
                          </Link>
                        </div>
                        <div className="w-36 font-mono text-slate-300">
                          {tx.totalCapacity ? parseInt(tx.totalCapacity).toLocaleString() : '-'}{' '}
                          <span className="text-slate-500">shannons</span>
                        </div>
                        <div className="w-24 text-right font-mono text-slate-400">
                          {tx.inputsCount ?? '-'} / {tx.outputsCount ?? '-'}
                        </div>
                      </div>
                    </TerminalRow>
                  ))
                )}
              </TerminalPanelContent>
            </TabsContent>
          </TerminalPanel>
        </Tabs>
      </main>
    </div>
  );
}
