import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { NetworkClientPage } from '@/app/network/client-page';

const API_BASE = '/api/:network/v1';

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
    // Default MSW handler serves no completed or active round.
    render(<NetworkClientPage />, { wrapper: createWrapper() });

    // Explains what the peer crawler is.
    expect(await screen.findByText('The CKB peer crawler')).toBeInTheDocument();
    // Honest reachability caveat is present.
    expect(screen.getByText(/discoverable \/ reachable nodes only/i)).toBeInTheDocument();
    // How to enable it.
    expect(screen.getByText('How to enable')).toBeInTheDocument();
    expect(screen.getByText(/enabled = true/)).toBeInTheDocument();
    expect(screen.getByText(/this network's config\.toml/i)).toBeInTheDocument();
    expect(screen.queryByText(/ckbadger\.toml/i)).not.toBeInTheDocument();
  });

  it('shows a waiting message when enabled but the first round has not finished', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({ enabled: true, hasData: false, lastRound: null, activeRound: null })
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
            candidatePeers: 2,
            attemptedPeers: 2,
            reachablePeers: 1,
            unreachablePeers: 1,
            roundId: 5,
            startedAt: 0,
            finishedAt: 0,
            addressAttempts: 2,
            failedAddressAttempts: 1,
            foreignPeers: 0,
            malformedAddresses: 0,
            newNodes: 0,
          },
          activeRound: null,
        })
      )
    );

    render(<NetworkClientPage />, { wrapper: createWrapper() });

    // Honest summary-card labels (never "total network nodes").
    expect(await screen.findByText('Discovered Reachable')).toBeInTheDocument();
    expect(screen.getByText('Failed Peer Candidates')).toBeInTheDocument();
    expect(screen.getByText('Total Known')).toBeInTheDocument();
    expect(screen.queryByText(/crawling/i)).not.toBeInTheDocument();
    // Reachability caveat still visible on the dashboard.
    expect(screen.getByText(/discoverable \/ reachable nodes only/i)).toBeInTheDocument();
  });

  it('shows active progress separately from the last completed round', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({
          enabled: true,
          hasData: true,
          lastRound: {
            totalKnown: 3,
            candidatePeers: 3,
            attemptedPeers: 3,
            reachablePeers: 2,
            unreachablePeers: 1,
            roundId: 5,
            startedAt: 0,
            finishedAt: 1,
            addressAttempts: 3,
            failedAddressAttempts: 1,
            foreignPeers: 0,
            malformedAddresses: 0,
            newNodes: 0,
          },
          activeRound: {
            roundId: 6,
            startedAt: 2,
            lastCheckpointAt: 3,
            candidatePeers: 3,
            completedPeers: 2,
            addressAttempts: 2,
            blockedReason: null,
          },
        })
      )
    );

    render(<NetworkClientPage />, { wrapper: createWrapper() });

    expect(await screen.findByText(/round #6 crawling 2\/3/i)).toBeInTheDocument();
  });

  it('surfaces an actionable blocked reason before the first round publishes', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({
          enabled: true,
          hasData: false,
          lastRound: null,
          activeRound: {
            roundId: 1,
            startedAt: 2,
            lastCheckpointAt: 3,
            candidatePeers: 3,
            completedPeers: 1,
            addressAttempts: 1,
            blockedReason: 'frontier capacity exceeded: limit=3',
          },
        })
      )
    );

    render(<NetworkClientPage />, { wrapper: createWrapper() });

    expect(await screen.findByText(/round #1 blocked/i)).toHaveTextContent(
      'frontier capacity exceeded: limit=3'
    );
  });
});
