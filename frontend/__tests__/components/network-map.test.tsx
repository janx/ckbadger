import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { onlineManager, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import {
  countryName,
  layoutCountryClusters,
  mapCounts,
  NetworkMapNotes,
  rankCountries,
} from '@/app/network/network-map';
import { NetworkMapView } from '@/__tests__/utils/network-map-view';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { SVG_TEXT } from '@/lib/chart-type';
import { tightLinePx } from '@/lib/type-metrics';
import type { NetworkMap } from '@/lib/api';
import producerMap from '@/__tests__/fixtures/network-api-producer/network-map.json';

const first: NetworkMap['points'][number] = {
  peerId: 'a',
  country: 'US',
  lat: 40,
  lon: -70,
  reachable: true,
  clientVersion: 'ckb/0.114',
  asn: 'AS1 Example',
  firstSeen: 100,
  lastSeen: 200,
  advertiserCount: 2,
  localSessionObserved: true,
  // Round 3 (R-13): present exactly when localSessionObserved is.
  localSession: {
    connectedDurationMs: 60_000,
    observedAt: 200,
    initiator: 'peerInitiated',
  },
};
const second = { ...first, peerId: 'b', reachable: false, clientVersion: 'ckb/0.113' };

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('NetworkMapView', () => {
  it('renders the API producer map payload with retained scope and unknown location count', async () => {
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(producerMap)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
        <NetworkMapNotes />
      </QueryClientProvider>
    );
    // F2 charts (CH-51): the panel starts as the legend; a peer's readout follows focus.
    fireEvent.focus(await screen.findByLabelText('US peer 7065657241, reachable now'));
    const detail = screen.getByRole('complementary', { name: 'Selected peer evidence' });
    expect(detail).toHaveTextContent('0.119.0');
    expect(detail).toHaveTextContent('US · AS65000 Ex');
    // Hierarchy v3 (N2): the coverage note is tier 2's, in the map notes.
    expect(screen.getByText(/1 of 2 retained verified peers/)).toHaveTextContent(
      '1 locations are unavailable'
    );
  });

  // Review fix: a read paused between retries (a hidden tab, the browser offline) is pending
  // without fetching, so isLoading is false: the map and its notes said "Crawler locations
  // unavailable: null" while they waited.
  it('waits while its read is paused offline, then draws the map once back online', async () => {
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(producerMap)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    onlineManager.setOnline(false);
    try {
      render(
        <QueryClientProvider client={client}>
          <NetworkMapView neverVerified={0} />
          <NetworkMapNotes />
        </QueryClientProvider>
      );
      expect(await screen.findByText('Loading crawler locations…')).toBeInTheDocument();
      expect(screen.queryByRole('alert')).toBeNull();
      act(() => onlineManager.setOnline(true));
      expect(await screen.findByLabelText('US peer 7065657241, reachable now')).toBeInTheDocument();
      expect(screen.queryByRole('alert')).toBeNull();
    } finally {
      onlineManager.setOnline(true);
    }
  });

  // Midnight, continued (review 2026-11-03, item 5, move 3): the map is an instrument, in the one
  // frame; the peer evidence rail beside it stays flat.
  it('draws the map inside the instrument frame, measured within it (move 3)', async () => {
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(producerMap)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
      </QueryClientProvider>
    );
    const map = await screen.findByRole('img', {
      name: 'Approximate GeoIP locations of retained verified CKB peers',
    });
    const frame = map.closest('[data-instrument-frame]') as HTMLElement;
    expect(frame).not.toBeNull();
    expect(frame).toHaveAttribute('aria-label', 'Map of verified peers');
    // a flat tile since rubbing × Song print: no lit top edge
    expect(frame).toHaveClass('border', 'border-base-border');
    expect(frame).not.toHaveClass('neon-edge-top');
    expect(frame).not.toContainElement(
      screen.getByRole('complementary', { name: 'Selected peer evidence' })
    );
  });

  it('displaces coincident dots deterministically without changing source coordinates', () => {
    // the dots of the default 720 × 353 map (720 × 0.49, rounded)
    const layout = layoutCountryClusters([second, first], 720, 353).dots;
    expect(layout.map(({ point }) => point.peerId)).toEqual(['a', 'b']);
    expect([layout[0].x, layout[0].y]).not.toEqual([layout[1].x, layout[1].y]);
    expect(Math.hypot(layout[0].x - layout[1].x, layout[0].y - layout[1].y)).toBeGreaterThanOrEqual(
      8
    );
    expect(layout[0].point.lat).toBe(40);
    expect(layout[1].point.lon).toBe(-70);
  });

  it('shows focused peer evidence and country count', async () => {
    server.use(
      http.get('/api/:network/v1/network/map', () =>
        HttpResponse.json({
          roundId: 5,
          verifiedRetained: 2,
          located: 2,
          locationUnavailable: 0,
          points: [first, second],
        })
      )
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
      </QueryClientProvider>
    );
    const peer = await screen.findByLabelText(
      'US peer b, verified before, not reachable now, a session with this node in the latest round'
    );
    fireEvent.focus(peer);
    const detail = screen.getByRole('complementary', { name: 'Selected peer evidence' });
    expect(detail).toHaveTextContent('ckb/0.113');
    expect(detail).toHaveTextContent('verified before, not reachable now');
    expect(detail).toHaveTextContent('of 2 peers in round #5');
    // The country count sits at its cluster on the map (CH-51), not in a list under it.
    expect(
      [...document.querySelectorAll('[data-country-label]')].map((node) => node.textContent)
    ).toEqual(['US 2']);
  });
});

// Readability plan 2026-11-18, P4 (the review's "network" page, cause 1): a country stood on the
// map only as its code and its count under its cluster, a phone map labelled only the countries of
// two or more, and ranking them meant scanning every label. Beside the map, the panel's resting
// content is the key and every country ranked by its verified nodes — the counts the map's labels
// state, from one grouping of the map's points — with its English name beside its code.
describe('NetworkMap: the countries ranked beside the map (readability P4)', () => {
  const at = (peerId: string, country: string, lat: number, lon: number) => ({
    ...first,
    peerId,
    country,
    lat,
    lon,
    localSessionObserved: false,
    localSession: undefined,
  });
  const points = [
    at('hk1', 'HK', 22.3, 114.2),
    at('hk2', 'HK', 22.3, 114.2),
    at('hk3', 'HK', 22.3, 114.2),
    at('us1', 'US', 39, -95),
    at('us2', 'US', 39, -95),
    at('de1', 'DE', 50, 9),
    at('de2', 'DE', 50, 9),
    at('mo1', 'MO', 22.2, 113.6),
    at('xx1', 'XX', -10, 20),
  ];
  const mapOf = (list: typeof points) => ({
    roundId: 7,
    verifiedRetained: list.length,
    located: list.length,
    locationUnavailable: 0,
    points: list,
  });
  const renderMap = () => {
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(mapOf(points))));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
      </QueryClientProvider>
    );
  };

  // lane exactness, part A2 (the pages review's NOTE 3): a located node whose GeoIP record names no country is
  // "Unknown" from both routes; it is in no country, so it is not counted as one, and the line says
  // how many there are (the map counted "Unknown", or "", as a country)
  it('counts no country for a located node without one, and says how many there are', async () => {
    const withNone = [...points, at('un1', 'Unknown', 1, 1), at('un2', 'Unknown', 1, 1)];
    expect(mapCounts(mapOf(withNone))).toMatchObject({ countries: 5, noCountry: 2 });
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(mapOf(withNone))));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
      </QueryClientProvider>
    );
    const readout = await screen.findByRole('status', { name: 'Node readout' });
    expect(readout).toHaveTextContent(/^11 verified nodes in 5 countries, 2 with no country known/);
  });

  it('ranks the countries by their nodes, then by their code', () => {
    expect(rankCountries(points)).toEqual([
      { country: 'HK', count: 3 },
      { country: 'DE', count: 2 },
      { country: 'US', count: 2 },
      { country: 'MO', count: 1 },
      { country: 'XX', count: 1 },
    ]);
  });

  it('names a country by its English name; a code the runtime cannot name has none', () => {
    expect(countryName('HK')).toBe('Hong Kong SAR China');
    expect(countryName('US')).toBe('United States');
    expect(countryName('XX')).toBeNull();
    expect(countryName('Unknown')).toBeNull();
  });

  it.each([880, 358])(
    'stands the ranked list in the resting panel at %i px, its counts the map labels’ own',
    async (width) => {
      stubChartWidth(width);
      renderMap();
      const panel = await screen.findByRole('complementary', { name: 'Selected peer evidence' });
      // the key first, then the countries
      expect(within(panel).getByRole('img', { name: /^Key:/ })).toBeInTheDocument();
      const list = within(panel).getByRole('list', { name: 'Countries by verified nodes' });
      expect(list.tagName).toBe('OL');
      const rows = [...list.querySelectorAll('li')].map((li) =>
        [...li.children].map((cell) => cell.textContent)
      );
      expect(rows).toEqual([
        ['HK', 'Hong Kong SAR China', '3'],
        ['DE', 'Germany', '2'],
        ['US', 'United States', '2'],
        ['MO', 'Macao SAR China', '1'],
        ['XX', '', '1'],
      ]);
      // a count is a value, in the reading ink; the code is the row's key, as on the map
      for (const li of list.querySelectorAll('li')) {
        expect(li.children[0]).toHaveClass('text-text-bright');
        expect(li.children[1]).toHaveClass('text-text');
        expect(li.children[2]).toHaveClass('text-text', 'tabular-nums');
      }
      // every country the map labels says the count the list says
      const counts = new Map(rows.map(([code, , count]) => [code, count]));
      const labels = [...document.querySelectorAll('[data-country-label]')].map(
        (node) => node.textContent!
      );
      expect(labels.length).toBeGreaterThan(0);
      for (const label of labels) {
        const [code, count] = label.split(' ');
        expect(counts.get(code), label).toBe(count);
      }
      // said once: no line under a phone map names the countries it leaves unlabelled
      expect(screen.queryByLabelText('Countries with one verified node')).toBeNull();
    }
  );

  it('gives the panel to a named node, and the key and the countries back when it is let go', async () => {
    stubChartWidth(880);
    renderMap();
    const panel = await screen.findByRole('complementary', { name: 'Selected peer evidence' });
    const dot = screen.getByRole('button', { name: /^HK peer hk1/ });
    fireEvent.click(dot);
    expect(within(panel).queryByRole('list', { name: 'Countries by verified nodes' })).toBeNull();
    fireEvent.click(dot);
    expect(
      within(panel).getByRole('list', { name: 'Countries by verified nodes' })
    ).toBeInTheDocument();
  });

  // Tests NOTE-2 (the review of the fixes, 2026-11-19): the map's place labels had no test of their
  // own. Each stands where the layout puts it, under the map's name band, centred on its cluster,
  // and says its code and its count in the one grouping of the map's points.
  it('writes each place label where the layout puts it, under the name band', async () => {
    stubChartWidth(880);
    renderMap();
    await screen.findByRole('img', { name: /Approximate GeoIP locations/ });
    // the map's own size for an 880 px box (network-map.tsx): 880 × 0.49, rounded
    const height = Math.max(256, Math.round(880 * 0.49));
    const { labels } = layoutCountryClusters(points, 880, height);
    // the name band of a one-line name: 2 px, its tight line and 7 px
    const band = 2 + tightLinePx(SVG_TEXT.label) + 7;
    const drawn = [...document.querySelectorAll('[data-country-label]')];
    expect(drawn.map((node) => node.textContent)).toEqual(
      labels.map((l) => `${l.country} ${l.count}`)
    );
    drawn.forEach((node, i) => {
      expect(Number(node.getAttribute('x'))).toBeCloseTo(labels[i].x, 6);
      expect(Number(node.getAttribute('y'))).toBeCloseTo(labels[i].y + band, 6);
      expect(node).toHaveAttribute('text-anchor', 'middle');
      expect(node).toHaveAttribute('font-size', String(SVG_TEXT.tick));
    });
  });
});
