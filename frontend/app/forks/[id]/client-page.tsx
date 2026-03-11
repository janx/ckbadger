'use client';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { notFound, useParams } from '@/src/navigation';
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
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8 text-center">
          <h1 className="text-negative text-2xl font-bold">Error loading fork event</h1>
          <p className="text-text-dim mt-2">{(error as Error).message}</p>
        </main>
      </div>
    );
  }
  if (isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="bg-base-elevated mb-6 h-10 w-64 animate-pulse rounded" />
          <div className="grid gap-6 md:grid-cols-2">
            <div className="border-base-border bg-base-surface/50 h-64 animate-pulse rounded border" />
            <div className="border-base-border bg-base-surface/50 h-64 animate-pulse rounded border" />
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
    <div className="bg-base-bg min-h-screen">
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
                    <span className="text-text-dim">({formatTimeAgo(event.detectedAt)})</span>
                  </span>
                </DataField>
                <DataField label="Depth">
                  <span className="text-warning font-mono">{event.depth} blocks</span>
                </DataField>
                <DataField label="Fork Point">
                  <div className="space-y-1">
                    <Link
                      href={`/blocks/${event.forkPointNumber}`}
                      className="text-emphasis font-mono hover:underline"
                    >
                      #{event.forkPointNumber.toLocaleString()}
                    </Link>
                    <HexDisplay value={event.forkPointHash} size="sm" />
                  </div>
                </DataField>
                {event.resolvedAt && (
                  <DataField label="Resolved">
                    <div>
                      {new Date(event.resolvedAt).toLocaleString()}
                      <div className="text-positive mt-1 text-xs">
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
                <div className="border-negative/30 bg-negative/10 rounded border p-4">
                  <div className="text-negative mb-2 text-xs font-medium uppercase tracking-wider">
                    Old Tip (Orphaned)
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-text font-mono">
                      Height: {event.oldTipNumber.toLocaleString()}
                    </span>
                  </div>
                  <div className="mt-2">
                    <HexDisplay value={event.oldTipHash} size="sm" />
                  </div>
                </div>
                <div className="flex justify-center">
                  <div className="text-warning text-2xl">↓</div>
                </div>
                <div className="border-positive/30 bg-positive/10 rounded border p-4">
                  <div className="text-positive mb-2 text-xs font-medium uppercase tracking-wider">
                    New Tip (Canonical)
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-text font-mono">
                      Height: {event.newTipNumber.toLocaleString()}
                    </span>
                  </div>
                  <div className="mt-2">
                    <HexDisplay value={event.newTipHash} size="sm" />
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
                <div className="border-base-border bg-base-surface/50 text-text-dim flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                  <div className="w-32">Number</div>
                  <div className="flex-1">Hash</div>
                  <div className="w-24 text-center">Tx Count</div>
                  <div className="w-28 text-right">Time</div>
                </div>
                {orphanedBlocks.length === 0 ? (
                  <div className="text-text-dim px-4 py-8 text-center">
                    No orphaned blocks data available
                  </div>
                ) : (
                  orphanedBlocks.map((block) => (
                    <TerminalRow key={block.hash}>
                      <div className="flex items-center">
                        <div className="w-32">
                          <Link
                            href={`/blocks/${block.number}`}
                            className="text-emphasis font-mono hover:underline"
                          >
                            #{block.number.toLocaleString()}
                          </Link>
                        </div>
                        <div className="flex-1">
                          <HexDisplay value={block.hash} size="sm" />
                        </div>
                        <div className="text-text w-24 text-center font-mono">
                          {block.transactionsCount}
                        </div>
                        <div className="text-text-dim w-28 text-right">
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
                <div className="border-base-border bg-base-surface/50 text-text-dim flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                  <div className="flex-1">Transaction</div>
                  <div className="w-36">Fee</div>
                  <div className="w-24 text-right">I/O</div>
                </div>
                {orphanedTransactions.length === 0 ? (
                  <div className="text-text-dim px-4 py-8 text-center">
                    No orphaned transactions data available
                  </div>
                ) : (
                  orphanedTransactions.map((tx) => (
                    <TerminalRow key={tx.hash}>
                      <div className="flex items-center">
                        <div className="flex-1">
                          <Link href={`/tx/${tx.hash}`} className="hover:underline">
                            <HexDisplay value={tx.hash} size="sm" />
                          </Link>
                          <Link
                            href={`/blocks/${tx.blockNumber}`}
                            className="text-text-dim hover:text-text block font-mono text-xs"
                          >
                            #{tx.blockNumber.toLocaleString()}
                          </Link>
                        </div>
                        <div className="text-text w-36 font-mono">
                          {tx.totalCapacity ? parseInt(tx.totalCapacity).toLocaleString() : '-'}{' '}
                          <span className="text-text-dim">shannons</span>
                        </div>
                        <div className="text-text-dim w-24 text-right font-mono">
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
