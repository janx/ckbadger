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
    expect(screen.getByText(/an advertised candidate is not verified/i)).toBeInTheDocument();
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
            candidatePeers: 2,
            verifiedRetainedPeers: 2,
            reachablePeers: 1,
            verifiedUnavailablePeers: 1,
            exhaustedCandidates: 1,
            roundId: 5,
            startedAt: 0,
            finishedAt: 0,
            addressAttempts: 2,
            nonSuccessfulAddressAttempts: 1,
            foreignPeers: 0,
            malformedAddresses: 0,
            newVerifiedPeers: 0,
            peerOutcomes: {
              sameNetworkIdentified: 1,
              exhausted: { withRetainedVerification: 1, withoutRetainedVerification: 0 },
              foreignNetwork: { withRetainedVerification: 0, withoutRetainedVerification: 0 },
            },
            addressObservations: {
              dialRequestFailed: 1,
              noAuthenticatedSessionBeforeDeadline: 0,
              authenticatedSessionWithoutIdentifyBeforeDeadline: 0,
              malformedIdentify: 0,
              foreignNetwork: 0,
              sameNetworkIdentified: 1,
            },
            discovery: {
              validNodesMessages: 1,
              malformedMessages: 0,
              unexpectedMessages: 0,
              normalizedAdvertisedAddresses: 2,
              rejectedAdvertisedAddresses: 0,
            },
          },
          activeRound: null,
        })
      )
    );

    render(<NetworkClientPage />, { wrapper: createWrapper() });

    expect(await screen.findByText('Advertised candidates')).toBeInTheDocument();
    expect(screen.getByText('Same-network reachable')).toBeInTheDocument();
    expect(screen.getByText('Verified retained')).toBeInTheDocument();
    expect(screen.getAllByText('Verified unavailable').length).toBeGreaterThan(0);
    expect(screen.queryByText('Total Known')).not.toBeInTheDocument();
    expect(screen.queryByText('Failed Peer Candidates')).not.toBeInTheDocument();
    expect(screen.queryByText(/crawling/i)).not.toBeInTheDocument();
    expect(screen.getByText(/an advertised candidate is not verified/i)).toBeInTheDocument();
  });

  it('shows active progress separately from the last completed round', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({
          enabled: true,
          hasData: true,
          lastRound: {
            candidatePeers: 3,
            verifiedRetainedPeers: 3,
            reachablePeers: 2,
            verifiedUnavailablePeers: 1,
            exhaustedCandidates: 1,
            roundId: 5,
            startedAt: 0,
            finishedAt: 1,
            addressAttempts: 3,
            nonSuccessfulAddressAttempts: 1,
            foreignPeers: 0,
            malformedAddresses: 0,
            newVerifiedPeers: 0,
            peerOutcomes: {
              sameNetworkIdentified: 2,
              exhausted: { withRetainedVerification: 1, withoutRetainedVerification: 0 },
              foreignNetwork: { withRetainedVerification: 0, withoutRetainedVerification: 0 },
            },
            addressObservations: {
              dialRequestFailed: 1,
              noAuthenticatedSessionBeforeDeadline: 0,
              authenticatedSessionWithoutIdentifyBeforeDeadline: 0,
              malformedIdentify: 0,
              foreignNetwork: 0,
              sameNetworkIdentified: 2,
            },
            discovery: {
              validNodesMessages: 2,
              malformedMessages: 0,
              unexpectedMessages: 0,
              normalizedAdvertisedAddresses: 3,
              rejectedAdvertisedAddresses: 0,
            },
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
