'use client';

import { useQuery } from '@tanstack/react-query';
import { api, NetworkStats } from '@/lib/api';
import { TerminalNumber } from '@/components/ui/terminal-number';

function formatSyncSpeed(blocksPerSecond: number | null): string | null {
  if (blocksPerSecond === null || blocksPerSecond <= 0) {
    return null;
  }
  if (blocksPerSecond >= 1000) {
    return `${(blocksPerSecond / 1000).toFixed(1)}K`;
  }
  return blocksPerSecond.toFixed(0);
}

export function SyncBanner({ stats }: { stats: NetworkStats }) {
  const { syncStatus } = stats;

  if (!syncStatus.isSyncing) {
    return null;
  }

  const syncSpeed = formatSyncSpeed(syncStatus.emaBlocksPerSecond);
  const isBulkSync = syncStatus.syncMode === 'bulk';
  const hasExtraInfo = syncSpeed || syncStatus.estimatedTime || syncStatus.elapsedTime;

  return (
    <div className="terminal-card border-terminal-dark p-3">
      <div className="relative z-10 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <div className="bg-terminal-green h-2 w-2 animate-pulse rounded-full" />
          <span className="text-terminal-dim font-mono text-sm font-medium">
            {isBulkSync ? 'BULK SYNCING...' : 'SYNCING BLOCKCHAIN DATA...'}
          </span>
        </div>
        <span className="text-terminal-dark font-mono text-sm">
          <TerminalNumber value={syncStatus.progress.toFixed(1)} glowIntensity="subtle" />% (
          <TerminalNumber
            value={syncStatus.syncedBlock.toLocaleString()}
            glowIntensity="none"
          /> / <TerminalNumber value={syncStatus.tipBlock.toLocaleString()} glowIntensity="none" />)
        </span>
      </div>
      {hasExtraInfo && (
        <div className="text-terminal-dark relative z-10 mt-1 flex items-center gap-3 font-mono text-xs">
          {syncSpeed && (
            <span>
              <TerminalNumber value={syncSpeed} glowIntensity="subtle" /> blocks/s
            </span>
          )}
          {syncStatus.elapsedTime && <span>Elapsed: {syncStatus.elapsedTime}</span>}
          {syncStatus.estimatedTime && <span>ETA: {syncStatus.estimatedTime}</span>}
        </div>
      )}
      <div className="bg-terminal-bg relative z-10 mt-2 h-1.5 w-full overflow-hidden rounded-full">
        <div
          className="from-terminal-dark via-terminal-dim to-terminal-green h-full rounded-full bg-gradient-to-r transition-all duration-500"
          style={{ width: `${syncStatus.progress}%` }}
        />
      </div>
    </div>
  );
}

export function StatsCards() {
  const { data: stats, isLoading } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    refetchInterval: 10000,
  });

  const cards = [
    {
      label: 'LATEST BLOCK',
      value: stats?.latestBlock?.toLocaleString() ?? '-',
      glow: 'strong' as const,
    },
    { label: 'AVG BLOCK TIME', value: stats?.avgBlockTime ?? '-', glow: 'subtle' as const },
    { label: 'HASH RATE', value: stats?.hashRate ?? '-', glow: 'subtle' as const },
    { label: 'DIFFICULTY', value: stats?.difficulty ?? '-', glow: 'subtle' as const },
    { label: 'CURRENT EPOCH', value: stats?.epoch ?? '-', glow: 'subtle' as const },
    { label: 'TPS (24H)', value: stats?.tps ?? '-', glow: 'subtle' as const },
  ];

  return (
    <div>
      {stats && <SyncBanner stats={stats} />}
      <div className="mt-4 grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
        {cards.map((card) => (
          <div key={card.label} className="terminal-card terminal-border-glow p-4">
            <div className="text-terminal-dark relative z-10 font-mono text-xs uppercase tracking-wider">
              {card.label}
            </div>
            <div
              className={`relative z-10 mt-1 text-lg font-semibold ${isLoading ? 'animate-terminal-flicker' : ''}`}
            >
              <TerminalNumber value={card.value} glowIntensity={card.glow} animate={!isLoading} />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
