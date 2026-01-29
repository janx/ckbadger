'use client';

import { useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { ProgressBar } from '@/components/ui/progress-bar';
import { StatBlock, StatGrid } from '@/components/ui/stat-block';

export default function StatusPage() {
  const { data: status, isLoading } = useQuery({
    queryKey: ['systemStatus'],
    queryFn: () => api.getSystemStatus(),
    refetchInterval: 5000,
  });

  if (isLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="mb-6 h-10 w-48 animate-pulse rounded bg-slate-800" />
          <div className="grid gap-6 md:grid-cols-2">
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
          </div>
        </main>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="System Status"
          subtitle="Monitor indexer sync, integrity checks, and label imports"
        />

        <div className="grid items-start gap-6 md:grid-cols-2">
          <div className="flex flex-col gap-6">
            <TerminalPanel>
              <TerminalPanelHeader
                indicator={status?.sync.isSyncing ? 'warning' : 'active'}
                actions={
                  <Badge variant={status?.sync.isSyncing ? 'amber' : 'green'}>
                    {status?.sync.isSyncing ? 'Syncing' : 'Synced'}
                  </Badge>
                }
              >
                Blockchain Sync
              </TerminalPanelHeader>
              <TerminalPanelContent>
                <div className="space-y-4">
                  <div>
                    <div className="mb-2 flex justify-between font-mono text-sm">
                      <span className="text-slate-500">Progress</span>
                      <span className="text-terminal-green">
                        {status?.sync.progress.toFixed(2)}%
                      </span>
                    </div>
                    <ProgressBar
                      value={status?.sync.progress || 0}
                      max={100}
                      showLabel={false}
                      color="green"
                      size="md"
                    />
                  </div>

                  <StatGrid columns={2}>
                    <StatBlock
                      label="Synced Block"
                      value={status?.sync.syncedBlock.toLocaleString() ?? '-'}
                      size="sm"
                    />
                    <StatBlock
                      label="Tip Block"
                      value={status?.sync.tipBlock.toLocaleString() ?? '-'}
                      size="sm"
                    />
                    <StatBlock
                      label="Behind"
                      value={(
                        (status?.sync.tipBlock ?? 0) - (status?.sync.syncedBlock ?? 0)
                      ).toLocaleString()}
                      suffix=" blocks"
                      size="sm"
                      color={status?.sync.isSyncing ? 'amber' : 'green'}
                    />
                    <StatBlock label="ETA" value={status?.sync.estimatedTime || '-'} size="sm" />
                  </StatGrid>

                  {status?.sync.lastSyncedAt && (
                    <div className="border-t border-slate-800 pt-4 font-mono text-xs text-slate-500">
                      Last synced: {new Date(status.sync.lastSyncedAt).toLocaleString()}
                    </div>
                  )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>

            {(status?.indexRebuild?.isRebuilding || (status?.indexRebuild?.progress ?? 0) > 0) && (
              <TerminalPanel>
                <TerminalPanelHeader
                  indicator={status?.indexRebuild.isRebuilding ? 'warning' : 'active'}
                  actions={
                    <Badge variant={status?.indexRebuild.isRebuilding ? 'amber' : 'green'}>
                      {status?.indexRebuild.isRebuilding ? 'Rebuilding' : 'Complete'}
                    </Badge>
                  }
                >
                  Index Rebuild
                </TerminalPanelHeader>
                <TerminalPanelContent>
                  <div className="space-y-4">
                    <div>
                      <div className="mb-2 flex justify-between font-mono text-sm">
                        <span className="text-slate-500">Progress</span>
                        <span className="text-terminal-green">
                          {status?.indexRebuild.progress.toFixed(2)}%
                        </span>
                      </div>
                      <ProgressBar
                        value={status?.indexRebuild.progress || 0}
                        max={100}
                        showLabel={false}
                        color={status?.indexRebuild.isRebuilding ? 'amber' : 'green'}
                        size="md"
                      />
                    </div>

                    <StatGrid columns={2}>
                      <StatBlock
                        label="Completed"
                        value={`${status?.indexRebuild.completed ?? 0} / ${status?.indexRebuild.total ?? 0}`}
                        size="sm"
                      />
                      <StatBlock
                        label="Current"
                        value={status?.indexRebuild.currentIndex ?? '-'}
                        size="sm"
                      />
                      <StatBlock
                        label="Failed"
                        value={status?.indexRebuild.failed?.length ?? 0}
                        size="sm"
                        color={status?.indexRebuild.failed?.length ? 'amber' : 'green'}
                      />
                      <StatBlock
                        label="Status"
                        value={status?.indexRebuild.isRebuilding ? 'Building...' : 'Done'}
                        size="sm"
                        color={status?.indexRebuild.isRebuilding ? 'amber' : 'green'}
                      />
                    </StatGrid>

                    {status?.indexRebuild.startedAt && (
                      <div className="border-t border-slate-800 pt-4 font-mono text-xs text-slate-500">
                        Started: {new Date(status.indexRebuild.startedAt).toLocaleString()}
                      </div>
                    )}
                  </div>
                </TerminalPanelContent>
              </TerminalPanel>
            )}

            <TerminalPanel>
              <TerminalPanelHeader
                indicator={status?.labelImport.isRunning ? 'warning' : 'active'}
                actions={
                  <Badge variant={status?.labelImport.isRunning ? 'purple' : 'green'}>
                    {status?.labelImport.isRunning ? 'Running' : 'Idle'}
                  </Badge>
                }
              >
                Label Import
              </TerminalPanelHeader>
              <TerminalPanelContent>
                <div className="space-y-4">
                  <div className="grid gap-3">
                    <div className="rounded border border-slate-800 bg-slate-900/50 p-4">
                      <div className="mb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                        Token Labels
                      </div>
                      <div className="flex items-baseline justify-between">
                        <span className="text-terminal-green text-xl font-semibold tabular-nums">
                          {status?.labelImport.tokenImportedCount.toLocaleString()}
                        </span>
                        <span className="font-mono text-sm text-slate-400">
                          / {status?.labelImport.tokenTotalCount.toLocaleString()}
                        </span>
                      </div>
                      <ProgressBar
                        value={status?.labelImport.tokenImportedCount || 0}
                        max={status?.labelImport.tokenTotalCount || 1}
                        showLabel={false}
                        color="green"
                        size="sm"
                        className="mt-2"
                      />
                    </div>

                    <div className="rounded border border-slate-800 bg-slate-900/50 p-4">
                      <div className="mb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                        Script Labels
                      </div>
                      <div className="flex items-baseline justify-between">
                        <span className="text-terminal-green text-xl font-semibold tabular-nums">
                          {status?.labelImport.scriptImportedCount.toLocaleString()}
                        </span>
                        <span className="font-mono text-sm text-slate-400">
                          / {status?.labelImport.scriptTotalCount.toLocaleString()}
                        </span>
                      </div>
                      <ProgressBar
                        value={status?.labelImport.scriptImportedCount || 0}
                        max={status?.labelImport.scriptTotalCount || 1}
                        showLabel={false}
                        color="amber"
                        size="sm"
                        className="mt-2"
                      />
                    </div>
                  </div>

                  {status?.labelImport.lastCheckAt && (
                    <div className="border-t border-slate-800 pt-4 font-mono text-xs text-slate-500">
                      Last check: {new Date(status.labelImport.lastCheckAt).toLocaleString()}
                    </div>
                  )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
          </div>

          <TerminalPanel>
            <TerminalPanelHeader
              indicator={status?.integrity.isRunning ? 'warning' : 'active'}
              actions={
                <Badge variant={status?.integrity.isRunning ? 'blue' : 'green'}>
                  {status?.integrity.isRunning ? 'Running' : 'Idle'}
                </Badge>
              }
            >
              Transaction Cycles Fill
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="space-y-4">
                <div>
                  <div className="mb-2 flex justify-between font-mono text-sm">
                    <span className="text-slate-500">Progress</span>
                    <span className="text-terminal-green">
                      {status?.integrity.progress.toFixed(2)}%
                    </span>
                  </div>
                  <ProgressBar
                    value={status?.integrity.progress || 0}
                    max={100}
                    showLabel={false}
                    color="blue"
                    size="md"
                  />
                </div>

                <StatGrid columns={2}>
                  <StatBlock
                    label="Status"
                    value={status?.integrity.isRunning ? 'Running' : 'Idle'}
                    size="sm"
                    color={status?.integrity.isRunning ? 'amber' : 'green'}
                  />
                  <StatBlock label="ETA" value={status?.integrity.estimatedTime || '-'} size="sm" />
                  <StatBlock
                    label="Processed"
                    value={status?.integrity.processedCount.toLocaleString() ?? '-'}
                    size="sm"
                  />
                  <StatBlock
                    label="Cycles Missing"
                    value={status?.integrity.missingCyclesCount.toLocaleString() ?? '-'}
                    size="sm"
                  />
                </StatGrid>

                {status?.integrity.recentFixes && status.integrity.recentFixes.length > 0 && (
                  <div>
                    <div className="mb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                      Recent Fixes
                    </div>
                    <div className="max-h-48 overflow-y-auto rounded border border-slate-800 bg-slate-900/50">
                      {status.integrity.recentFixes.map((fix) => (
                        <TerminalRow key={fix.txHash} className="py-2">
                          <div className="flex items-center justify-between">
                            <Link href={`/tx/${fix.txHash}`} className="hover:underline">
                              <HexDisplay
                                value={fix.txHash}
                                color="green"
                                size="sm"
                                startChars={8}
                                endChars={6}
                              />
                            </Link>
                            <span className="font-mono text-xs text-slate-400">
                              {fix.cycles.toLocaleString()} cycles
                            </span>
                          </div>
                        </TerminalRow>
                      ))}
                    </div>
                  </div>
                )}

                {status?.integrity.lastCheckAt && (
                  <div className="border-t border-slate-800 pt-4 font-mono text-xs text-slate-500">
                    Last check: {new Date(status.integrity.lastCheckAt).toLocaleString()}
                  </div>
                )}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        </div>
      </main>
    </div>
  );
}
