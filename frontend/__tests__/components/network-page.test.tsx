import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { DEFAULT_API_BASE } from '@/lib/runtime-config';
import { server } from '../msw/server';
import { NetworkClientPage } from '@/app/network/client-page';

const API_BASE = DEFAULT_API_BASE;

// Isolate the page from the global chrome (mirrors chart-page.test.tsx).
vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header" />,
}));

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

describe('NetworkClientPage', () => {
  it('shows the onboarding empty state when the crawler is off / has no data', async () => {
    // Default MSW handler serves { enabled:false, hasData:false, lastRound:null }.
    render(<NetworkClientPage />, { wrapper: createWrapper() });

    // Explains what the peer crawler is.
    expect(await screen.findByText('The CKB peer crawler')).toBeInTheDocument();
    // Honest reachability caveat is present.
    expect(screen.getByText(/discoverable \/ reachable nodes only/i)).toBeInTheDocument();
    // How to enable it.
    expect(screen.getByText('How to enable')).toBeInTheDocument();
    expect(screen.getByText(/enabled = true/)).toBeInTheDocument();
  });

  it('shows a waiting message when enabled but the first round has not finished', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({ enabled: true, hasData: false, lastRound: null })
      )
    );

    render(<NetworkClientPage />, { wrapper: createWrapper() });

    expect(await screen.findByText(/waiting for the first round/i)).toBeInTheDocument();
  });

  it('shows the dashboard summary cards when crawl data exists', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({
          enabled: true,
          hasData: true,
          lastRound: {
            totalKnown: 2,
            reachable: 1,
            unreachable: 1,
            frontierDrained: true,
            roundId: 5,
            started: 0,
            finished: 0,
            dialed: 2,
            foreignDropped: 0,
            newNodes: 0,
          },
        })
      )
    );

    render(<NetworkClientPage />, { wrapper: createWrapper() });

    // Honest summary-card labels (never "total network nodes").
    expect(await screen.findByText('Discovered Reachable')).toBeInTheDocument();
    expect(screen.getByText('Unreachable')).toBeInTheDocument();
    expect(screen.getByText('Total Known')).toBeInTheDocument();
    // frontierDrained:true ⇒ no partial-round badge.
    expect(screen.queryByText(/partial round/i)).not.toBeInTheDocument();
    // Reachability caveat still visible on the dashboard.
    expect(screen.getByText(/discoverable \/ reachable nodes only/i)).toBeInTheDocument();
  });

  it('flags a partial round when the frontier was not drained', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({
          enabled: true,
          hasData: true,
          lastRound: {
            totalKnown: 3,
            reachable: 2,
            unreachable: 1,
            frontierDrained: false,
            roundId: 6,
            started: 0,
            finished: 0,
            dialed: 3,
            foreignDropped: 0,
            newNodes: 0,
          },
        })
      )
    );

    render(<NetworkClientPage />, { wrapper: createWrapper() });

    expect(await screen.findByText(/partial round/i)).toBeInTheDocument();
  });
});
