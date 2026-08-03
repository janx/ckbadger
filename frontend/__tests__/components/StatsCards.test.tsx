import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StatsCards } from '@/components/stats-cards';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';

const API_BASE = '/api/:network/v1';

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

function createNetworkResponse(syncStatus?: Record<string, unknown>) {
  return {
    latestBlock: 1000000,
    avgBlockTime: '10.5s',
    hashRate: '1.23 EH/s',
    difficulty: '2.34 P',
    epoch: '500 (45%)',
    tps: '1.23',
    syncStatus,
  };
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

describe('StatsCards', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders stat cards with network data', async () => {
    render(<StatsCards />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText('LATEST BLOCK')).toBeInTheDocument();
    });

    expect(screen.getByText('AVG BLOCK TIME')).toBeInTheDocument();
    expect(screen.getByText('HASH RATE')).toBeInTheDocument();
    expect(screen.getByText('DIFFICULTY')).toBeInTheDocument();
    expect(screen.getByText('CURRENT EPOCH')).toBeInTheDocument();
    expect(screen.getByText('TPS (24H)')).toBeInTheDocument();
  });

  it('shows bulk sync progress details when available', async () => {
    server.use(
      http.get(`${API_BASE}/statistics/network`, () => {
        return HttpResponse.json(
          createNetworkResponse(
            createBulkSyncStatus({
              elapsedTime: '1h 10m',
              txsPerSecond: 10500.0,
              emaTxsPerSecond: 9800.0,
            })
          )
        );
      })
    );

    render(<StatsCards />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText('BULK SYNCING...')).toBeInTheDocument();
    });

    expect(screen.getByText('1.2K')).toBeInTheDocument();
    expect(screen.getByText('blocks/s')).toBeInTheDocument();
    expect(screen.getByText('Elapsed: 1h 10m')).toBeInTheDocument();
    expect(screen.getByText('9.8K')).toBeInTheDocument();
    expect(screen.getByText('txns/s')).toBeInTheDocument();
    expect(screen.getByText('ETA: 2h 30m')).toBeInTheDocument();
  });

  it('does not show sync banner during non-bulk syncing', async () => {
    server.use(
      http.get(`${API_BASE}/statistics/network`, () => {
        return HttpResponse.json(
          createNetworkResponse(
            createBulkSyncStatus({
              syncedBlock: 999100,
              progress: 99.9,
              estimatedTime: '3m',
              elapsedTime: '15m',
              blocksPerSecond: 20,
              emaBlocksPerSecond: 18,
              syncMode: 'normal',
            })
          )
        );
      })
    );

    render(<StatsCards />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText('LATEST BLOCK')).toBeInTheDocument();
    });

    expect(screen.queryByText('BULK SYNCING...')).not.toBeInTheDocument();
    expect(screen.queryByText(/Elapsed:/)).not.toBeInTheDocument();
  });
});
