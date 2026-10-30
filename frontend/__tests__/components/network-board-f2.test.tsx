import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { NetworkClientPage } from '@/app/network/client-page';
import { NetworkMapView } from '@/app/network/network-map';
import { HostingBar, NetworkDistributions } from '@/app/network/distributions';
import { NetworkTrends } from '@/app/network/trends';
import type { NetworkMap } from '@/lib/api';

// F2 charts group: the Network board (V70, "no edges, not centred on this node").

const API_BASE = '/api/:network/v1';
vi.mock('@/components/layout/header', () => ({ Header: () => <div data-testid="header" /> }));

function wrap(children: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

const lastRound = {
  candidatePeers: 181,
  verifiedRetainedPeers: 66,
  reachablePeers: 62,
  verifiedUnavailablePeers: 4,
  exhaustedCandidates: 119,
  roundId: 97,
  startedAt: 1761782400,
  finishedAt: 1761783240,
  addressAttempts: 2345,
  nonSuccessfulAddressAttempts: 2000,
  foreignPeers: 0,
  malformedAddresses: 0,
  newVerifiedPeers: 1,
  peerOutcomes: {
    sameNetworkIdentified: 62,
    exhausted: { withRetainedVerification: 4, withoutRetainedVerification: 115 },
    foreignNetwork: { withRetainedVerification: 0, withoutRetainedVerification: 0 },
  },
  addressObservations: {
    dialRequestFailed: 1,
    noAuthenticatedSessionBeforeDeadline: 0,
    authenticatedSessionWithoutIdentifyBeforeDeadline: 0,
    malformedIdentify: 0,
    foreignNetwork: 0,
    sameNetworkIdentified: 62,
  },
  discovery: {
    validNodesMessages: 1,
    validResponseMessages: 1,
    validAnnounceMessages: 0,
    malformedMessages: 0,
    unexpectedMessages: 0,
    normalizedAdvertisedAddresses: 2,
    rejectedAdvertisedAddresses: 0,
  },
  localObserver: null,
  directSessionObservations: { observerInitiated: 0, peerInitiated: 0 },
};

const point = (
  peerId: string,
  country: string,
  reachable: boolean,
  localSessionObserved: boolean
): NetworkMap['points'][number] => ({
  peerId,
  country,
  lat: country === 'US' ? 40 : 50,
  lon: country === 'US' ? -74 : 8,
  reachable,
  clientVersion: 'ckb/0.204.0',
  asn: 'AS16509 Amazon.com',
  firstSeen: 1751328000,
  lastSeen: 1751500800,
  advertiserCount: 3,
  localSessionObserved,
});
const map: NetworkMap = {
  roundId: 97,
  verifiedRetained: 4,
  located: 4,
  locationUnavailable: 0,
  points: [
    point('12200fce0b01aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2d1b5', 'US', true, true),
    point('1220aaaa', 'US', true, false),
    point('1220bbbb', 'DE', false, false),
    point('1220cccc', 'DE', true, false),
  ],
};

describe('Network board (F2 charts)', () => {
  beforeEach(() => stubChartWidth(880));
  afterEach(() => vi.restoreAllMocks());

  it('heads the page in plain words with four quiet figures, no red and no tracked capitals (CH-53)', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({ enabled: true, hasData: true, lastRound, activeRound: null })
      )
    );
    const { container } = render(wrap(<NetworkClientPage />));
    expect(
      await screen.findByText(/CKB nodes find each other by word of mouth/)
    ).toBeInTheDocument();
    const figures = screen.getByRole('group', { name: 'Latest crawler round' });
    expect(figures).toHaveTextContent(/Reachable now\s*62\s*answered as CKB mainnet nodes/);
    expect(figures).toHaveTextContent(/Verified before, not now\s*4/);
    expect(figures).toHaveTextContent(/Heard of\s*181\s*115 never verified/);
    expect(figures).toHaveTextContent(/Latest round\s*#97\s*.*UTC\+8 · 14 min · 2,345 dials/);
    expect(figures.querySelector('.uppercase, [class*="rouge"], [class*="negative"]')).toBeNull();
    // The old tier-1 evidence box and per-round counters moved to the developer tier.
    const tier2 = container.querySelector('[data-tier="2"]')!;
    expect(within(tier2 as HTMLElement).getByText(/2,345 address attempts/)).toBeInTheDocument();
  });

  it('draws reachable dots solid, verified-before hollow, this node’s session peers ringed (CH-51)', async () => {
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    render(wrap(<NetworkMapView neverVerified={115} />));
    const svg = await screen.findByRole('img', { name: /Approximate GeoIP locations/ });
    const dots = [...svg.querySelectorAll('[data-peer]')];
    const by = (id: string) => dots.find((dot) => dot.getAttribute('data-peer') === id)!;
    expect(by('1220aaaa').querySelector('[data-dot="reachable"]')).not.toBeNull();
    expect(by('1220bbbb').querySelector('[data-dot="verified-before"]')).toHaveAttribute(
      'fill',
      '#0c0e15'
    );
    expect(by(map.points[0].peerId).querySelector('[data-ring]')).not.toBeNull();
    expect(svg.querySelectorAll('[data-ring]')).toHaveLength(1);
    // No line of any kind joins two peers.
    expect(svg.querySelectorAll('line[data-link], path[data-link]')).toHaveLength(0);
    // Country counts sit at their clusters, on the map.
    const labels = [...svg.querySelectorAll('[data-country-label]')].map((n) => n.textContent);
    expect(labels.sort()).toEqual(['DE 2', 'US 2']);
  });

  it('starts with a legend instead of a selected peer, and names a hovered peer briefly (CH-51, CH-57)', async () => {
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    render(wrap(<NetworkMapView neverVerified={115} />));
    const panel = await screen.findByRole('complementary', { name: 'Selected peer evidence' });
    expect(panel).toHaveTextContent('Hover a node');
    expect(panel).toHaveTextContent('reachable now · 3');
    expect(panel).toHaveTextContent('verified before, not now · 1');
    expect(panel).toHaveTextContent("this node's 1 session peers");
    expect(panel).toHaveTextContent('heard of, never verified · 115');
    expect(panel).toHaveTextContent(/none are drawn/);
    fireEvent.focus(screen.getByLabelText(/^US peer 12200fce0b…/));
    expect(panel).toHaveTextContent('peer 12200fce0b…a2d1b5');
    expect(panel).not.toHaveTextContent(map.points[0].peerId);
    expect(panel).toHaveTextContent('to this nodea session in the latest round');
    expect(panel).toHaveTextContent('in address booksof 3 peers in round #97');
  });

  it('shows the never-verified peers as an unlocated strip under the map (CH-51)', async () => {
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    render(wrap(<NetworkMapView neverVerified={115} />));
    const strip = await screen.findByRole('img', { name: '115 peers only heard of' });
    expect(strip.querySelectorAll('[data-unlocated]')).toHaveLength(115);
    expect(
      screen.getByText(
        '115 more peers were only heard of: never verified, so their place is unknown'
      )
    ).toBeInTheDocument();
  });

  it('draws dots at a readable size at phone width (CH-57)', async () => {
    stubChartWidth(358);
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    render(wrap(<NetworkMapView neverVerified={0} />));
    const svg = await screen.findByRole('img', { name: /Approximate GeoIP locations/ });
    // Drawn 1:1 (viewBox as wide as the box), so a 3.6 px dot stays 3.6 px on a phone.
    expect(svg.getAttribute('viewBox')).toMatch(/^0 0 35\d /);
    expect(svg.querySelector('[data-dot="reachable"]')).toHaveAttribute('r', '3.6');
  });

  it('merges one provider’s ASNs into a hosting bar with one sentence (CH-52)', async () => {
    server.use(
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          verifiedRetained: 66,
          sameNetworkReachable: 62,
          verifiedUnavailable: 4,
          versions: [{ label: '0.204.0', count: 66 }],
          countries: [
            { label: 'US', count: 40 },
            { label: 'DE', count: 20 },
            { label: 'Unknown', count: 6 },
          ],
          asns: [
            { label: 'AS16509 Amazon.com', count: 31 },
            { label: 'AS24940 Hetzner Online GmbH', count: 20 },
            { label: 'AS14618 Amazon.com', count: 5 },
            { label: 'Unknown', count: 10 },
          ],
          protocols: [],
        })
      )
    );
    render(wrap(<HostingBar />));
    const bar = await screen.findByRole('img', { name: 'Verified nodes by hosting network' });
    expect(
      [...bar.querySelectorAll('[data-provider]')].map((n) => n.getAttribute('data-provider'))
    ).toEqual(['Amazon.com', 'Hetzner Online GmbH', 'Unknown']);
    expect(
      screen.getByText('36 of the 66 verified nodes run on Amazon.com, across 2 countries.')
    ).toBeInTheDocument();
    expect(screen.queryByText('AS16509 Amazon.com')).toBeNull();
  });

  it('keeps only the version bars in tier 1, in the data hue (CH-54)', async () => {
    server.use(
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          verifiedRetained: 7,
          sameNetworkReachable: 5,
          verifiedUnavailable: 2,
          versions: [
            { label: '0.204.0', count: 5 },
            { label: '0.203.0', count: 2 },
          ],
          countries: [{ label: 'US', count: 7 }],
          asns: [{ label: 'AS1 Example', count: 7 }],
          protocols: [{ label: '/ckb/2', count: 7 }],
        })
      )
    );
    render(wrap(<NetworkDistributions />));
    expect(
      await screen.findByRole('heading', { name: 'Which software they run' })
    ).toBeInTheDocument();
    expect(screen.getByText('0.204.0')).toBeInTheDocument();
    expect(screen.queryByText('Countries')).toBeNull();
    expect(screen.queryByText('Networks (ASN)')).toBeNull();
    expect(screen.queryByText('/ckb/2')).toBeNull();
    const fills = [...document.querySelectorAll('[data-version-bar]')];
    expect(fills).toHaveLength(2);
    expect(fills[0]).toHaveStyle({ backgroundColor: '#2edba3' });
  });

  it('draws verified as a line over the reachable area, with dated x labels (CH-58)', async () => {
    render(wrap(<NetworkTrends />));
    const svg = await screen.findByRole('img', { name: 'Verified and reachable peers, peers' });
    expect(svg.querySelectorAll('path[fill="#dee2ec"]')).toHaveLength(0);
    expect(svg.querySelector('path[fill="#2edba3"]')).not.toBeNull();
    const ticks = [...svg.querySelectorAll('[data-x-tick]')].map((n) => n.textContent);
    expect(ticks).toEqual(['07-01', '07-02', '07-03']);
    // The daily share tables are developer evidence, not part of "Over time".
    expect(screen.queryByText('Client Version Share')).toBeNull();
  });

  it('titles the peer table once, without tracked capitals (CH-56)', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({ enabled: true, hasData: true, lastRound, activeRound: null })
      )
    );
    const { container } = render(wrap(<NetworkClientPage />));
    await screen.findByText('Reachable now');
    expect(await screen.findAllByRole('heading', { name: 'Peer evidence' })).toHaveLength(1);
    expect(screen.queryByText('Observed network peers')).toBeNull();
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2.querySelector('.uppercase')).toBeNull();
  });
});
