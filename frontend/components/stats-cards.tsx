'use client';

import type { NetworkStats } from '@/lib/api';
import { TerminalNumber } from '@/components/ui/terminal-number';
import { cutFloat } from '@/lib/exact-integer';

/**
 * A rate the indexer reports, cut: thousands at one place from 1,000, else whole (lane exactness,
 * part B3: it was rounded, 1,299.9 blocks a second read 1.3k).
 */
function formatRate(value: number | null | undefined): string | null {
  if (value === null || value === undefined || value <= 0) {
    return null;
  }
  if (value >= 1000) return `${cutFloat(value, 1, 'sync rate', BigInt(1000))}k`;
  return cutFloat(value, 0, 'sync rate');
}

export function SyncBanner({ stats }: { stats: NetworkStats }) {
  const { syncStatus } = stats;

  if (!syncStatus.isSyncing || syncStatus.syncMode !== 'bulk') {
    return null;
  }

  const syncSpeed = formatRate(syncStatus.emaBlocksPerSecond);
  const txnsSpeed = formatRate(syncStatus.emaTxsPerSecond ?? syncStatus.txsPerSecond ?? null);
  const hasExtraInfo = syncSpeed || txnsSpeed || syncStatus.estimatedTime || syncStatus.elapsedTime;

  // A plain well (rubbing × Song print, phase 2) with the celadon live mark: no glowing card.
  return (
    <div data-testid="sync-banner" className="well p-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span
            aria-hidden="true"
            data-testid="sync-banner-live"
            className="bg-jade h-2 w-2 animate-pulse rounded-full"
          />
          <span className="text-gold-dim text-body font-mono font-medium">BULK SYNCING…</span>
        </div>
        <span className="text-gold-dim text-body font-mono">
          {/* cut, never rounded: 99.96 % read 100.0 while the sync ran (lane exactness, part B3) */}
          <TerminalNumber
            value={cutFloat(syncStatus.progress, 1, 'sync progress')}
            glowIntensity="subtle"
          />
          % (
          <TerminalNumber
            value={syncStatus.syncedBlock.toLocaleString()}
            glowIntensity="none"
          /> / <TerminalNumber value={syncStatus.tipBlock.toLocaleString()} glowIntensity="none" />)
        </span>
      </div>
      {hasExtraInfo && (
        <div className="text-gold-dim text-small mt-1 flex items-center gap-3 font-mono">
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
      <div className="bg-base-bg mt-2 h-1.5 w-full overflow-hidden">
        <div
          className="from-gold-dim via-gold-dim to-gold h-full bg-gradient-to-r transition-all duration-500"
          style={{ width: `${syncStatus.progress}%` }}
        />
      </div>
    </div>
  );
}
