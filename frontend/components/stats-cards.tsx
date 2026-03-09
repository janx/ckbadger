'use client';

import { useQuery } from '@tanstack/react-query';
import { api, NetworkStats } from '@/lib/api';
import { TerminalNumber } from '@/components/ui/terminal-number';

function formatRate(value: number | null | undefined): string | null {
  if (value === null || value === undefined || value <= 0) {
    return null;
  }
  if (value >= 1000) {
    return `${(value / 1000).toFixed(1)}K`;
  }
  return value.toFixed(0);
}

export function SyncBanner({ stats }: { stats: NetworkStats }) {
  const { syncStatus } = stats;

  if (!syncStatus.isSyncing || syncStatus.syncMode !== 'bulk') {
    return null;
  }

  const syncSpeed = formatRate(syncStatus.emaBlocksPerSecond);
  const txnsSpeed = formatRate(syncStatus.emaTxsPerSecond ?? syncStatus.txsPerSecond ?? null);
  const hasExtraInfo = syncSpeed || txnsSpeed || syncStatus.estimatedTime || syncStatus.elapsedTime;

  return (
    <div className="terminal-card border-emphasis-dim p-3">
      <div className="relative z-10 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <div className="bg-emphasis h-2 w-2 animate-pulse rounded-full" />
          <span className="text-emphasis-dim font-mono text-sm font-medium">BULK SYNCING...</span>
        </div>
        <span className="text-emphasis-dim font-mono text-sm">
          <TerminalNumber value={syncStatus.progress.toFixed(1)} glowIntensity="subtle" />% (
          <TerminalNumber
            value={syncStatus.syncedBlock.toLocaleString()}
            glowIntensity="none"
          /> / <TerminalNumber value={syncStatus.tipBlock.toLocaleString()} glowIntensity="none" />)
        </span>
      </div>
      {hasExtraInfo && (
        <div className="text-emphasis-dim relative z-10 mt-1 flex items-center gap-3 font-mono text-xs">
          {syncSpeed && (
            <span>
              <TerminalNumber value={syncSpeed} glowIntensity="subtle" /> blocks/s
            </span>
          )}
          {txnsSpeed && (
            <span>
              <TerminalNumber value={txnsSpeed} glowIntensity="subtle" /> txns/s
            </span>
          )}
          {syncStatus.elapsedTime && <span>Elapsed: {syncStatus.elapsedTime}</span>}
          {syncStatus.estimatedTime && <span>ETA: {syncStatus.estimatedTime}</span>}
        </div>
      )}
      <div className="bg-base-bg relative z-10 mt-2 h-1.5 w-full overflow-hidden rounded-full">
        <div
          className="from-emphasis-dim via-emphasis-dim to-emphasis h-full rounded-full bg-gradient-to-r transition-all duration-500"
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
            <div className="text-emphasis-dim relative z-10 font-mono text-xs uppercase tracking-wider">
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
