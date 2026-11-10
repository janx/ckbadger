import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { NetworkClientPage } from '@/app/network/client-page';
import { layoutCountryClusters, NetworkMapView } from '@/app/network/network-map';
import { PeersTable } from '@/app/network/nodes-table';
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
  // Round 3 (R-13): this node's session is present exactly when it was observed.
  ...(localSessionObserved && {
    localSession: {
      connectedDurationMs: 3_725_000,
      lastPingDurationMs: 42,
      // 2025-07-03 00:00:00 UTC = 08:00:00 UTC+8, the crawler clock (seconds, like lastSeen).
      observedAt: 1751500800,
      initiator: 'observerInitiated' as const,
    },
  }),
});
const map: NetworkMap = {
  roundId: 97,
  verifiedRetained: 4,
  located: 4,
  locationUnavailable: 0,
  points: [
    {
      ...point('12200fce0b01aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2d1b5', 'US', true, true),
      alias: { address: '/ip4/203.0.113.7/tcp/8115', verifiedAt: 1751500700 },
    },
    point('1220aaaa', 'US', true, false),
    point('1220bbbb', 'DE', false, false),
    point('1220cccc', 'DE', true, false),
  ],
};

describe('Network board (F2 charts)', () => {
  beforeEach(() => stubChartWidth(880));
  afterEach(() => vi.restoreAllMocks());

  it('heads the page in plain words with one figure and three quiet ones, no red and no tracked capitals (CH-53)', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () =>
        HttpResponse.json({ enabled: true, hasData: true, lastRound, activeRound: null })
      )
    );
    const { container } = render(wrap(<NetworkClientPage />));
    expect(await screen.findByText(/^Nodes find each other by word of mouth/)).toBeInTheDocument();
    // Hierarchy v3 (R13): reachable now is the hero's figure; the other three share one quiet row.
    const hero = screen.getByRole('region', { name: 'Where the nodes are' });
    expect(hero).toHaveTextContent(/Reachable now\s*62 of 66\s*answered as CKB mainnet nodes/);
    // Consistency plan C1: the dashboard names the tab, as the page's other states do.
    expect(document.title).toBe('Where the nodes are · CKBadger');
    const figures = screen.getByRole('group', { name: 'Latest crawler round' });
    expect(figures).toHaveTextContent(/Verified before, not now\s*4/);
    expect(figures).toHaveTextContent(/Heard of\s*181 · 115 never verified/);
    expect(figures).toHaveTextContent(/Latest round\s*#97 · .*UTC\+8 · 14 min · 2,345 dials/);
    for (const el of [hero, figures])
      expect(el.querySelector('.uppercase, [class*="rouge"], [class*="negative"]')).toBeNull();
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
      'var(--c-base-bg)'
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
    // Hierarchy v3: the key is drawn; the hint is the reading row's, the prose tier 2's.
    expect(within(panel).getByRole('img', { name: /^Key:/ })).toBeInTheDocument();
    expect(panel).toHaveTextContent('reachable now · 3');
    expect(panel).toHaveTextContent('verified before, not now · 1');
    expect(panel).toHaveTextContent("this node's 1 session peers");
    expect(panel).toHaveTextContent('heard of, never verified · 115');
    expect(panel).not.toHaveTextContent(/tap or hover|none are drawn/i);
    fireEvent.focus(screen.getByLabelText(/^US peer 12200fce0b…/));
    // R-13 (CH-57): titled by the alias the crawler last verified, the peer id beneath.
    expect(panel.querySelector('p')).toHaveTextContent('/ip4/203.0.113.7/tcp/8115');
    expect(panel).toHaveTextContent('peer12200fce0b…a2d1b5');
    expect(panel).not.toHaveTextContent(map.points[0].peerId);
    expect(panel).toHaveTextContent(
      'to this nodeconnected for 1 h 2 m · ping 42 ms · this node dialled it (as of 08:00:00 UTC+8)'
    );
    expect(panel).toHaveTextContent('in address booksof 3 peers in round #97');
  });

  it('a tap pins a node’s identity in the line under the map; a second tap clears it (R6)', async () => {
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    render(wrap(<NetworkMapView neverVerified={0} />));
    const svg = await screen.findByRole('img', { name: /Approximate GeoIP locations/ });
    const dot = screen.getByRole('button', { name: /^US peer 12200fce0b…/ });
    const readout = screen.getByRole('status', { name: 'Node readout' });
    const panel = screen.getByRole('complementary', { name: 'Selected peer evidence' });
    // the line opens the lines under the map (hierarchy v3: the picture, then its reading)
    expect(readout.parentElement!.firstElementChild).toBe(readout);
    expect(readout.parentElement!.previousElementSibling!.contains(svg)).toBe(true);
    expect(readout).toHaveTextContent(/^4 verified nodes in 2 countriestap or hover$/);
    // a tap: the emulated hover and focus, then the click; the pin outlasts the pointer
    fireEvent.mouseEnter(dot);
    fireEvent.focus(dot);
    fireEvent.click(dot);
    fireEvent.mouseLeave(svg);
    expect(readout).toHaveTextContent(
      /^selected \/ip4\/203\.0\.113\.7\/tcp\/8115 · peer 12200fce0b…a2d1b5 · US · reachable now · ckb\/0\.204\.0/
    );
    expect(dot).toHaveAttribute('aria-pressed', 'true');
    expect(panel.querySelector('p')).toHaveTextContent('/ip4/203.0.113.7/tcp/8115');
    // a second tap on it lets go, though the emulated hover is still on it
    fireEvent.mouseEnter(dot);
    fireEvent.click(dot);
    expect(readout).toHaveTextContent(/^4 verified nodes in 2 countriestap or hover$/);
    expect(within(panel).getByRole('img', { name: /^Key:/ })).toBeInTheDocument();
  });

  it('lets a pin go when a refetched map no longer has its node (R6)', async () => {
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
      </QueryClientProvider>
    );
    fireEvent.click(await screen.findByRole('button', { name: /^US peer 12200fce0b…/ }));
    const readout = screen.getByRole('status', { name: 'Node readout' });
    expect(readout).toHaveTextContent(/^selected /);
    const gone = { ...map, points: map.points.slice(1) };
    act(() => {
      client.setQueryData(['network', 'map'], gone);
    });
    await waitFor(() => expect(readout).toHaveTextContent(/^4 verified nodes in 2 countries/));
  });

  it('says nothing of an alias or a session the API does not give (R-13)', async () => {
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    render(wrap(<NetworkMapView neverVerified={0} />));
    const panel = await screen.findByRole('complementary', { name: 'Selected peer evidence' });
    fireEvent.focus(screen.getByLabelText(/^DE peer 1220bbbb/));
    expect(panel.querySelector('p')).toHaveTextContent('peer 1220bbbb');
    expect(panel).toHaveTextContent('to this nodeno session in the latest round');
    expect(panel).not.toHaveTextContent(/connected for|ping/);
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
    expect(fills[0]).toHaveStyle({ backgroundColor: 'var(--c-jade)' });
  });

  it('draws verified as a line over the reachable area, with its first and last day on the axis (CH-58, R14)', async () => {
    render(wrap(<NetworkTrends />));
    const svg = await screen.findByRole('img', { name: 'Verified and reachable peers, peers' });
    expect(svg.querySelectorAll('path[fill="var(--c-text-bright)"]')).toHaveLength(0);
    // visual finish V13: the reachable area is a jade gradient (light, not paint); verified,
    // the bare line, has none
    const areas = [...svg.querySelectorAll('path[fill^="url(#"]')];
    expect(areas).toHaveLength(1);
    const gradient = svg.querySelector(
      `linearGradient[id="${areas[0].getAttribute('fill')!.slice(5, -1)}"]`
    )!;
    expect(gradient.querySelector('stop')).toHaveAttribute('stop-color', 'var(--c-jade)');
    const ticks = [...svg.querySelectorAll('[data-x-tick]')].map((n) => n.textContent);
    expect(ticks).toEqual(['07-01', '07-03']);
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
    // The tier's own eyebrow (R6) is its one line of tracked capitals (`.eyebrow` since material
    // pass v2, move 3); the table title has none.
    expect([...tier2.querySelectorAll('.uppercase, .eyebrow')].map((el) => el.textContent)).toEqual(
      ['Evidence · for developers']
    );
  });

  it('keeps country clusters and their labels apart, as the board relaxes them (visual pass)', () => {
    const at = (country: string, lat: number, lon: number, n: number) =>
      Array.from({ length: n }, (_, i) => ({
        ...point(`${country}${i}`, country, i % 2 === 0, i === 0),
        lat,
        lon,
      }));
    const points = [
      ...at('DE', 51, 10, 10),
      ...at('FR', 46.5, 2, 10),
      ...at('GB', 54, -2, 3),
      ...at('PL', 52, 19, 1),
      ...at('NL', 52.3, 4.9, 4),
      ...at('US', 39, -98, 12),
    ];
    const { dots, labels } = layoutCountryClusters(points, 880, 430);
    expect(dots).toHaveLength(points.length);
    for (let i = 0; i < dots.length; i++)
      for (let j = i + 1; j < dots.length; j++)
        expect(Math.hypot(dots[i].x - dots[j].x, dots[i].y - dots[j].y)).toBeGreaterThanOrEqual(7);
    const box = (l: (typeof labels)[number]) => {
      const w = `${l.country} ${l.count}`.length * 6.2;
      return { x0: l.x - w / 2, x1: l.x + w / 2, y0: l.y - 10, y1: l.y + 2 };
    };
    for (let i = 0; i < labels.length; i++)
      for (let j = i + 1; j < labels.length; j++) {
        const a = box(labels[i]);
        const b = box(labels[j]);
        const overlap = a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
        expect(overlap, `${labels[i].country} / ${labels[j].country}`).toBe(false);
      }
    // Every cluster stays on the map.
    for (const d of dots) {
      expect(d.x).toBeGreaterThanOrEqual(0);
      expect(d.x).toBeLessThanOrEqual(880);
      expect(d.y).toBeGreaterThanOrEqual(0);
      expect(d.y).toBeLessThanOrEqual(430);
    }
  });

  it('labels only clusters of two or more on a phone map and lists the rest (responsive A#12)', () => {
    // The 2026-11-01 mainnet crawl: country, verified nodes, mean GeoIP position.
    const crawl: [string, number, number, number][] = [
      ['AE', 2, 25.1, 55.3],
      ['AU', 2, -35.8, 148.1],
      ['BR', 1, -23.5, -46.6],
      ['CA', 4, 44.1, -77.9],
      ['CL', 1, -33.5, -70.7],
      ['CN', 1, 34.8, 113.7],
      ['DE', 10, 50.4, 9.2],
      ['FR', 3, 48.9, 4.3],
      ['GB', 1, 51.5, -0.1],
      ['HK', 10, 22.3, 114.2],
      ['ID', 1, -6.2, 106.8],
      ['IN', 1, 19.1, 72.9],
      ['JP', 1, 35.7, 139.7],
      ['KR', 6, 37.5, 127.0],
      ['MO', 1, 22.2, 113.6],
      ['PL', 1, 52.2, 21.0],
      ['SG', 8, 1.3, 103.8],
      ['TW', 1, 25.1, 121.5],
      ['US', 12, 38.9, -94.8],
      ['ZA', 1, -33.9, 18.4],
    ];
    const points = crawl.flatMap(([country, n, lat, lon]) =>
      Array.from({ length: n }, (_, i) => ({
        ...point(`${country}${i}`, country, true, false),
        lat,
        lon,
      }))
    );
    const singles = crawl.filter(([, n]) => n === 1).map(([country]) => country);
    // 358 px: the phone column. Its map is 220 px tall.
    const phone = layoutCountryClusters(points, 358, 220);
    expect(phone.labels.map((l) => l.country)).toEqual(
      crawl.filter(([, n]) => n >= 2).map(([country]) => country)
    );
    expect(phone.listed.map((l) => l.country)).toEqual(singles);
    expect(phone.dots).toHaveLength(points.length);
    const box = (l: { country: string; count: number; x: number; y: number }) => {
      const w = `${l.country} ${l.count}`.length * 6.2;
      return { x0: l.x - w / 2, x1: l.x + w / 2, y0: l.y - 10, y1: l.y + 2 };
    };
    for (let i = 0; i < phone.labels.length; i++)
      for (let j = i + 1; j < phone.labels.length; j++) {
        const a = box(phone.labels[i]);
        const b = box(phone.labels[j]);
        const overlap = a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
        expect(overlap, `${phone.labels[i].country} / ${phone.labels[j].country}`).toBe(false);
      }
    // A desktop map labels every cluster and lists none.
    const desktop = layoutCountryClusters(points, 880, 430);
    expect(desktop.labels).toHaveLength(crawl.length);
    expect(desktop.listed).toEqual([]);
  });

  it('names every unlabelled single-node country under the phone map, as text', async () => {
    stubChartWidth(358);
    const solo = (peerId: string, country: string, lat: number, lon: number) => ({
      ...point(peerId, country, true, false),
      lat,
      lon,
    });
    server.use(
      http.get(`${API_BASE}/network/map`, () =>
        HttpResponse.json({
          roundId: 9,
          verifiedRetained: 4,
          located: 4,
          locationUnavailable: 0,
          points: [
            solo('us1', 'US', 39, -95),
            solo('us2', 'US', 39, -95),
            solo('tw1', 'TW', 25, 121),
            solo('mo1', 'MO', 22, 113),
          ],
        })
      )
    );
    render(wrap(<NetworkMapView neverVerified={0} />));
    const list = await screen.findByLabelText('Countries with one verified node');
    expect(list).toHaveTextContent('MO 1');
    expect(list).toHaveTextContent('TW 1');
    expect(
      [...document.querySelectorAll('[data-country-label]')].map((n) => n.textContent)
    ).toEqual(['US 2']);
  });

  it('lets the peer filters shrink to a phone width (visual pass: 390 overflowed to 448)', async () => {
    render(wrap(<PeersTable />));
    const select = await screen.findByLabelText('Filter by address observation');
    expect(select).toHaveClass('max-w-full');
    expect(screen.getByLabelText('Filter by crawler dial state')).toHaveClass('max-w-full');
  });
});
