import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { SyncBanner } from '@/components/stats-cards';
import type { NetworkStats } from '@/lib/api';

// The sync banner the home page shows over its figures (components/stats-cards.tsx). Lane E3
// removed the six-card StatsCards grid that no page rendered, and its tests; the banner's tests,
// which reached it through that grid, render it directly.

function createNetworkResponse(syncStatus?: Record<string, unknown>) {
  return {
    latestBlock: 1000000,
    avgBlockTime: '10.5s',
    hashRate: '1.23 EH/s',
    difficulty: '2.34 P',
    epoch: '500 (45%)',
    tps: '1.23',
    syncStatus,
  } as unknown as NetworkStats;
}

function createBulkSyncStatus(overrides?: Record<string, unknown>) {
  return {
    isSyncing: true,
    syncedBlock: 500000,
    tipBlock: 1000000,
    progress: 50.0,
    estimatedTime: '2h 30m',
    chartDataMayBeIncomplete: false,
    blocksPerSecond: 1500.5,
    emaBlocksPerSecond: 1200.0,
    syncMode: 'bulk',
    ...overrides,
  };
}

describe('SyncBanner', () => {
  // lane exactness, part B3: the progress and the rates are cut, never rounded: 99.96 % read
  // "100.0%" while the sync still ran, and 1,299.9 blocks a second "1.3k"
  it('cuts its progress and its rates', () => {
    render(
      <SyncBanner
        stats={createNetworkResponse(
          createBulkSyncStatus({
            progress: 99.96,
            emaBlocksPerSecond: 1299.9,
            emaTxsPerSecond: 999.99,
          })
        )}
      />
    );
    const banner = screen.getByTestId('sync-banner');
    expect(banner).toHaveTextContent('99.9% (');
    expect(banner).not.toHaveTextContent('100.0%');
    expect(screen.getByText('1.2k')).toBeInTheDocument();
    expect(screen.getByText('999')).toBeInTheDocument();
  });

  it('shows bulk sync progress details when available', () => {
    render(
      <SyncBanner
        stats={createNetworkResponse(
          createBulkSyncStatus({
            elapsedTime: '1h 10m',
            txsPerSecond: 10500.0,
            emaTxsPerSecond: 9800.0,
          })
        )}
      />
    );

    expect(screen.getByText('BULK SYNCING…')).toBeInTheDocument();
    expect(screen.getByText('1.2k')).toBeInTheDocument();
    expect(screen.getByText('blocks/s')).toBeInTheDocument();
    expect(screen.getByText('Elapsed: 1h 10m')).toBeInTheDocument();
    expect(screen.getByText('9.8k')).toBeInTheDocument();
    expect(screen.getByText('txns/s')).toBeInTheDocument();
    expect(screen.getByText('ETA: 2h 30m')).toBeInTheDocument();

    // Rubbing × Song print, phase 2: a plain well with the celadon live mark, no glowing card
    const banner = screen.getByTestId('sync-banner');
    expect(banner).toHaveClass('well');
    expect(banner.className).not.toMatch(/terminal-card|terminal-border-glow/);
    expect(screen.getByTestId('sync-banner-live')).toHaveClass('bg-jade');
    expect(screen.getByTestId('sync-banner-live')).toHaveAttribute('aria-hidden', 'true');
  });

  it('does not show sync banner during non-bulk syncing', () => {
    const { container } = render(
      <SyncBanner
        stats={createNetworkResponse(
          createBulkSyncStatus({
            syncedBlock: 999100,
            progress: 99.9,
            estimatedTime: '3m',
            elapsedTime: '15m',
            blocksPerSecond: 20,
            emaBlocksPerSecond: 18,
            syncMode: 'normal',
          })
        )}
      />
    );

    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByText('BULK SYNCING…')).not.toBeInTheDocument();
    expect(screen.queryByText(/Elapsed:/)).not.toBeInTheDocument();
  });
});
