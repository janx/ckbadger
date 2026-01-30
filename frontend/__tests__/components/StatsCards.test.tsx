import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StatsCards, IndexRebuildBanner } from '@/components/stats-cards';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';

const API_BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001/api/v1';

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

  it('shows index rebuild banner when task is running', async () => {
    server.use(
      http.get(`${API_BASE}/tasks/active`, () => {
        return HttpResponse.json({
          indexRebuild: {
            isRebuilding: true,
            total: 26,
            completed: 13,
            currentIndex: 'idx_blocks_timestamp',
            failed: [],
            progress: 50,
            startedAt: '2024-01-15T10:30:00Z',
          },
        });
      })
    );

    render(<StatsCards />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText('REBUILDING INDEXES...')).toBeInTheDocument();
    });

    expect(screen.getByText('50.0')).toBeInTheDocument();
    expect(screen.getByText('Current: idx_blocks_timestamp')).toBeInTheDocument();
  });

  it('does not show index rebuild banner when no task running', async () => {
    server.use(
      http.get(`${API_BASE}/tasks/active`, () => {
        return HttpResponse.json({
          indexRebuild: null,
        });
      })
    );

    render(<StatsCards />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText('LATEST BLOCK')).toBeInTheDocument();
    });

    expect(screen.queryByText('REBUILDING INDEXES...')).not.toBeInTheDocument();
  });

  it('shows sync banner when syncing', async () => {
    server.use(
      http.get(`${API_BASE}/statistics/network`, () => {
        return HttpResponse.json({
          latestBlock: 1000000,
          avgBlockTime: '10.5s',
          hashRate: '1.23 EH/s',
          difficulty: '2.34 P',
          epoch: '500 (45%)',
          tps: '1.23',
          syncStatus: {
            isSyncing: true,
            syncedBlock: 500000,
            tipBlock: 1000000,
            progress: 50.0,
            estimatedTime: '2h 30m',
            chartDataMayBeIncomplete: false,
            blocksPerSecond: 1500.5,
            emaBlocksPerSecond: 1200.0,
          },
        });
      })
    );

    render(<StatsCards />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText('SYNCING BLOCKCHAIN DATA...')).toBeInTheDocument();
    });
  });

  it('shows sync speed when available', async () => {
    server.use(
      http.get(`${API_BASE}/statistics/network`, () => {
        return HttpResponse.json({
          latestBlock: 1000000,
          avgBlockTime: '10.5s',
          hashRate: '1.23 EH/s',
          difficulty: '2.34 P',
          epoch: '500 (45%)',
          tps: '1.23',
          syncStatus: {
            isSyncing: true,
            syncedBlock: 500000,
            tipBlock: 1000000,
            progress: 50.0,
            estimatedTime: '2h 30m',
            chartDataMayBeIncomplete: false,
            blocksPerSecond: 1500.5,
            emaBlocksPerSecond: 1200.0,
          },
        });
      })
    );

    render(<StatsCards />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText('SYNCING BLOCKCHAIN DATA...')).toBeInTheDocument();
    });

    // Should show EMA blocks per second formatted as "1.2K blocks/s"
    expect(screen.getByText('1.2K')).toBeInTheDocument();
    expect(screen.getByText('blocks/s')).toBeInTheDocument();
  });
});
