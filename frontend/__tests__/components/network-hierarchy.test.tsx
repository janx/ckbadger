import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { NetworkClientPage } from '@/app/network/client-page';
import { NetworkMapNotes, NetworkMapView } from '@/app/network/network-map';
import { HostingBar } from '@/app/network/distributions';
import { NetworkTrends } from '@/app/network/trends';
import { PeersTable } from '@/app/network/nodes-table';
import { AdvertisementGraph } from '@/app/network/advertisement-graph';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
import type { NetworkMap } from '@/lib/api';

// Hierarchy v3 on /network (plan 2026-11-02 lane N, rulings R11–R16): the DAO page's anatomy —
// title row, one short lede, one display figure with its companion, the picture right under it,
// one reading row, the other figures in one quiet row; sections of evidence → reading → caption;
// everything else in tier 2.

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
  ...(localSessionObserved && {
    localSession: {
      connectedDurationMs: 3_725_000,
      lastPingDurationMs: 42,
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
    point('12200fce0b01aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2d1b5', 'US', true, true),
    point('1220aaaa', 'US', true, false),
    point('1220bbbb', 'DE', false, false),
    point('1220cccc', 'DE', true, false),
  ],
};

const distributions = {
  verifiedRetained: 66,
  sameNetworkReachable: 62,
  verifiedUnavailable: 4,
  versions: [
    { label: '0.204.0', count: 60 },
    { label: '0.203.0', count: 6 },
  ],
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
  protocols: [{ label: '/ckb/2', count: 66 }],
};

function serveRound(activeRound: unknown = null) {
  server.use(
    http.get(`${API_BASE}/network/summary`, () =>
      HttpResponse.json({ enabled: true, hasData: true, lastRound, activeRound })
    ),
    http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)),
    http.get(`${API_BASE}/network/distributions`, () => HttpResponse.json(distributions))
  );
}

/** The page with its map and its tier 2 loaded. */
async function renderPage(activeRound: unknown = null) {
  serveRound(activeRound);
  const view = render(wrap(<NetworkClientPage />));
  const hero = await screen.findByRole('region', { name: 'Where the nodes are' });
  await within(hero).findByRole('img', { name: /Approximate GeoIP locations/ });
  const tier1 = view.container.querySelector('[data-tier="1"]') as HTMLElement;
  const tier2 = view.container.querySelector('[data-tier="2"]') as HTMLElement;
  return { ...view, hero, tier1, tier2 };
}

const follows = (a: Node, b: Node) =>
  Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

describe('Network page hero (hierarchy v3 R11, R13; lane N, N1)', () => {
  beforeEach(() => stubChartWidth(1248));
  afterEach(() => vi.restoreAllMocks());

  it('opens with the title row and one short lede; the old paragraph moves to tier 2', async () => {
    const { hero, tier1, tier2 } = await renderPage();
    expect(tier1.contains(hero)).toBe(true);
    // Tier 2 is the page's second tier, not a block inside tier 1.
    expect(tier1.querySelector('[data-tier="2"]')).toBeNull();
    expect(tier1.firstElementChild!.contains(hero)).toBe(true);
    const h1 = within(hero).getByRole('heading', { level: 1, name: 'Where the nodes are' });
    expect(h1).toHaveClass('text-index', 'text-text-bright');
    // The round and its age sit on the title row.
    const round = within(hero).getByText(/^Round #97 · updated /);
    expect(round).toHaveClass('text-small', 'text-text-dim');
    expect(h1.parentElement!.contains(round)).toBe(true);
    const lede = within(hero).getByText((_, el) => el?.getAttribute('data-role') === 'lede');
    expect(lede.textContent).toMatch(/^Nodes find each other by word of mouth/);
    expect(lede.textContent!.length).toBeLessThanOrEqual(90);
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    expect(within(lede).getByRole('link', { name: 'How it works ↓' })).toHaveAttribute(
      'href',
      '#evidence'
    );
    expect(tier2.closest('#evidence')).not.toBeNull();
    // The four-line paragraph is not in tier 1; it opens tier 2's explanation.
    expect(within(tier1).queryByText(/each node tells its peers about other nodes/)).toBeNull();
    const how = within(tier2).getByRole('heading', { name: 'How the crawler sees the network' });
    expect(how.closest('section')).toHaveTextContent(
      'CKB nodes find each other by word of mouth: each node tells its peers about other nodes it knows. This crawler asks every node it can reach, then tries to connect to each one it hears about. A node it cannot reach may still be running, behind NAT or a firewall.'
    );
  });

  it('shows reachable now as the one jade display figure with its companion on the same row', async () => {
    const { hero } = await renderPage();
    const figure = hero.querySelector('[data-role="reachable"]') as HTMLElement;
    expect(figure).toHaveTextContent(/^62$/);
    // R4: display from 640 up, index below; R11: the page's class colour.
    expect(figure).toHaveClass('text-jade', 'text-index', 'sm:text-display');
    // One display-size line box holds the figure and its scale.
    expect(figure.parentElement).toHaveClass('text-index', 'sm:text-display', 'whitespace-nowrap');
    const row = figure.parentElement!.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline');
    expect(row).toHaveTextContent(/^62 of 66answered as CKB mainnet nodes$/);
    // Its scale, the verified nodes, beside it at hero size in the dim ink (as the DAO figure's
    // fraction): the figure's second display-size run.
    const scale = row.querySelector('[data-role="reachable-of"]')!;
    expect(scale).toHaveTextContent(/^of 66$/);
    expect(scale).toHaveClass('text-hero', 'text-text-dim');
    expect(row.querySelector('.text-jade:not([data-role="reachable"])')).toBeNull();
    // Its label over it, quiet; no tracked capitals on this page (CH-53).
    expect(row.previousElementSibling).toHaveTextContent(/^Reachable now$/);
    expect(row.previousElementSibling).toHaveClass('text-meta', 'text-text-dim');
    expect(hero.querySelector('.uppercase')).toBeNull();
  });

  it('draws the map right under the figure, inside the hero, with no heading of its own', async () => {
    const { hero, container } = await renderPage();
    const figure = hero.querySelector('[data-role="reachable"]')!;
    const mapSvg = within(hero).getByRole('img', { name: /Approximate GeoIP locations/ });
    expect(follows(figure, mapSvg)).toBe(true);
    expect(within(hero).queryAllByRole('heading', { level: 2 })).toHaveLength(0);
    expect(screen.queryByRole('heading', { name: 'Verified peers across the world' })).toBeNull();
    // The picture is at least 280 px tall at every width (the map's own floor is 220 + its band).
    expect(Number(mapSvg.getAttribute('height'))).toBeGreaterThanOrEqual(220);
    expect(container.querySelector('[data-tier="1"] h2')).toHaveTextContent(
      /^Where they run$|^Which software they run$/
    );
  });

  it('collapses the other three figures into one quiet row under the map', async () => {
    const { hero } = await renderPage();
    const row = screen.getByRole('group', { name: 'Latest crawler round' });
    expect(hero.contains(row)).toBe(false);
    expect(follows(within(hero).getByRole('img', { name: /Approximate GeoIP/ }), row)).toBe(true);
    const terms = within(row).getAllByRole('term');
    expect(terms.map((t) => t.textContent)).toEqual([
      'Verified before, not now',
      'Heard of',
      'Latest round',
    ]);
    for (const t of terms) expect(t).toHaveClass('text-meta', 'text-text-dim');
    const values = within(row).getAllByRole('definition');
    expect(values).toHaveLength(3);
    for (const d of values) expect(d).toHaveClass('text-section', 'text-text-bright');
    expect(values[0]).toHaveTextContent(/^4$/);
    expect(values[1]).toHaveTextContent(/^181 · 115 never verified$/);
    expect(values[2]).toHaveTextContent(
      /^#97 · \d\d-\d\d \d\d:\d\d UTC\+8 · 14 min · 2,345 dials$/
    );
    // The sub-notes are inline and dim, not a second line of figures.
    for (const note of Array.from(row.querySelectorAll('dd > span')))
      expect(note).toHaveClass('text-small', 'text-text-dim');
    expect(row).not.toHaveTextContent('Reachable now');
    expect(row.querySelector('.uppercase, [class*="rouge"], [class*="negative"]')).toBeNull();
  });

  it('keeps jade to the figure alone in tier 1; an active round reads as neutral words', async () => {
    const { tier1, hero } = await renderPage({
      roundId: 98,
      startedAt: 2,
      lastCheckpointAt: 3,
      candidatePeers: 181,
      completedPeers: 40,
      addressAttempts: 90,
      blockedReason: null,
    });
    await within(tier1).findByRole('heading', { name: 'Which software they run' });
    const figure = hero.querySelector('[data-role="reachable"]')!;
    const coloured = Array.from(tier1.querySelectorAll('[class]')).filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) =>
          /^text-(jade|positive|negative|rouge|gold|warning|info|aqua|interactive|emphasis)(-dim|-bright)?(\/\d+)?$/.test(
            c
          )
        )
    );
    expect(coloured.length).toBeGreaterThan(0);
    for (const el of coloured) expect(figure.contains(el)).toBe(true);
    const active = within(hero).getByText(/round #98 crawling 40\/181/);
    expect(active).toHaveClass('text-text-dim');
  });
});

describe('Network map, under the picture (hierarchy v3 R12–R14; lane N, N2)', () => {
  afterEach(() => vi.restoreAllMocks());

  const renderMap = async (width = 908, neverVerified = 115) => {
    stubChartWidth(width);
    server.use(http.get(`${API_BASE}/network/map`, () => HttpResponse.json(map)));
    const view = render(
      wrap(
        <>
          <NetworkMapView neverVerified={neverVerified} />
          <div data-testid="notes">
            <NetworkMapNotes />
          </div>
        </>
      )
    );
    const svg = await screen.findByRole('img', { name: /Approximate GeoIP locations/ });
    return { ...view, svg };
  };

  it('names the map inside its plot, above the clusters, and keeps it at least 280 px tall', async () => {
    for (const width of [908, 358]) {
      const { svg, unmount } = await renderMap(width);
      const name = svg.querySelector('[data-role="chart-name"]')!;
      expect(name).toHaveTextContent(/^verified nodes · approximate GeoIP place$/);
      expect(name).toHaveAttribute('font-size', '11');
      // The clusters are drawn below the name's band.
      const band = Number(name.getAttribute('y'));
      for (const dot of Array.from(svg.querySelectorAll('[data-dot]')))
        expect(Number(dot.getAttribute('cy'))).toBeGreaterThan(band + 4);
      expect(Number(svg.getAttribute('height'))).toBeGreaterThanOrEqual(280);
      unmount();
      vi.restoreAllMocks();
    }
  });

  it('keys the map in a drawn legend beside it, with no hint and no prose in the panel', async () => {
    await renderMap();
    const panel = screen.getByRole('complementary', { name: 'Selected peer evidence' });
    const key = within(panel).getByRole('img', { name: /^Key:/ });
    expect(key.tagName.toLowerCase()).toBe('svg');
    expect(key).toHaveAccessibleName(
      "Key: reachable now 3, verified before, not now 1, this node's 1 session peers, heard of, never verified 115"
    );
    for (const words of [
      'reachable now · 3',
      'verified before, not now · 1',
      "this node's 1 session peers",
      'heard of, never verified · 115',
    ])
      expect(within(key).getByText(words)).toBeInTheDocument();
    expect(panel).not.toHaveTextContent(/tap or hover/i);
    expect(panel.querySelectorAll('p')).toHaveLength(0);
  });

  it('reads the map in one row, keeps the never-verified strip, and ends with one caption', async () => {
    const { container } = await renderMap();
    const view = container.firstElementChild as HTMLElement;
    const readout = screen.getByRole('status', { name: 'Node readout' });
    // The idle reading is the map's own count; the hint is the row's, once (R14).
    expect(readout).toHaveTextContent(/^4 verified nodes in 2 countriestap or hover$/);
    expect(view.querySelectorAll('[data-hint="tap-or-hover"]')).toHaveLength(1);
    const strip = screen.getByRole('img', { name: '115 peers only heard of' });
    const caption = view.querySelector('[data-role="caption"]') as HTMLElement;
    expect(caption.textContent).toBe(
      'Every node this crawler verified, at its approximate GeoIP place; no links are drawn, because no one can see them from outside.'
    );
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    expect(caption).toHaveClass('text-body', 'text-text', 'max-w-[640px]');
    expect(follows(readout, strip)).toBe(true);
    expect(follows(strip, caption)).toBe(true);
    // Nothing else under the map: no line above it, no coverage or displacement notes.
    expect(view).not.toHaveTextContent(/ringed: the/);
    expect(view).not.toHaveTextContent(/approximate GeoIP coordinates/);
    expect(view).not.toHaveTextContent(/displaced/);
    expect(view.querySelector('figcaption')).toBeNull();
  });

  it('keeps every sentence that left the map in the developer notes', async () => {
    await renderMap();
    const notes = screen.getByTestId('notes');
    await waitFor(() =>
      expect(notes).toHaveTextContent(
        '4 verified nodes in 2 countries · ringed: the 1 this node had a session with in the latest round.'
      )
    );
    expect(notes).toHaveTextContent(
      '4 of 4 retained verified peers have approximate GeoIP coordinates; 0 locations are unavailable. Dots with close coordinates are displaced to keep them selectable. There is no centre: every node keeps a handful of connections of its own, and that mesh cannot be observed from outside, so no links are drawn.'
    );
    expect(notes).toHaveTextContent(
      "Each node keeps its own handful of connections to others. Those links cannot be seen from outside, so none are drawn: not even this node's, which would make it look like a centre."
    );
  });

  it('puts the map notes in tier 2 of the page', async () => {
    stubChartWidth(1248);
    const { tier1, tier2 } = await renderPage();
    await waitFor(() => expect(tier2).toHaveTextContent(/approximate GeoIP coordinates/));
    expect(tier1).not.toHaveTextContent(/approximate GeoIP coordinates/);
    expect(tier1).not.toHaveTextContent(/ringed: the/);
  });
});

describe('Where they run and which software (hierarchy v3 R12, R14, R15; lane N, N3)', () => {
  afterEach(() => vi.restoreAllMocks());

  const renderHosting = async (width: number) => {
    stubChartWidth(width);
    server.use(
      http.get(`${API_BASE}/network/distributions`, () => HttpResponse.json(distributions))
    );
    render(wrap(<HostingBar />));
    return screen.findByRole('region', { name: 'Where they run' });
  };

  it('heads its own section and draws a 28 px bar with names and shares inside the segments', async () => {
    const section = await renderHosting(1200);
    const h2 = within(section).getByRole('heading', { level: 2, name: 'Where they run' });
    expect(h2).toHaveClass('text-section', 'text-text-bright');
    expect(h2.parentElement).toHaveClass('mb-[18px]');
    const bar = within(section).getByRole('img', { name: 'Verified nodes by hosting network' });
    expect(bar).toHaveClass('h-[28px]');
    const words = Array.from(bar.querySelectorAll('[data-provider]')).map((s) => s.textContent);
    expect(words).toEqual(['Amazon.com 54.55%', 'Hetzner Online GmbH 30.30%', 'Unknown 15.15%']);
    // The words are their own box inside the coloured segment, as the DAO's concentration bar.
    for (const segment of Array.from(bar.querySelectorAll('[data-provider]'))) {
      expect(segment.childNodes).toHaveLength(1);
      expect(segment.firstElementChild!.tagName).toBe('SPAN');
    }
  });

  it('writes a share alone, or nothing, where a segment cannot hold its words', async () => {
    const section = await renderHosting(358);
    const bar = within(section).getByRole('img', { name: 'Verified nodes by hosting network' });
    const words = Array.from(bar.querySelectorAll('[data-provider]')).map((s) => s.textContent);
    expect(words).toEqual(['Amazon.com 54.55%', '30.30%', '']);
  });

  it('gathers the exact counts in one quiet line and closes with one caption (R12, R15)', async () => {
    const section = await renderHosting(1200);
    const exact = section.querySelector('[data-role="exact-amounts"]') as HTMLElement;
    expect(exact).toHaveTextContent(/^Amazon\.com 36 · Hetzner Online GmbH 20 · Unknown 10$/);
    expect(exact).toHaveClass('text-small', 'text-text-dim');
    for (const value of Array.from(exact.querySelectorAll('[data-role="count"]')))
      expect(value).toHaveClass('text-text');
    const caption = section.lastElementChild as HTMLElement;
    expect(caption).toHaveAttribute('data-role', 'caption');
    expect(caption).toHaveTextContent(
      /^36 of the 66 verified nodes run on Amazon\.com, across 2 countries\.$/
    );
    expect(caption).toHaveClass('text-body', 'text-text', 'max-w-[640px]');
    // heading, bar, exact line, caption: no legend row of its own.
    expect(section.children).toHaveLength(4);
  });

  it('heads the software list alone; its scope note is tier 2', async () => {
    stubChartWidth(1248);
    const { tier1, tier2 } = await renderPage();
    const h2 = await within(tier1).findByRole('heading', { name: 'Which software they run' });
    expect(h2).toHaveClass('text-section');
    expect(h2.parentElement).toHaveClass('mb-[18px]');
    expect(tier1).not.toHaveTextContent('reachable now or verified before');
    expect(tier2).toHaveTextContent(
      '“Which software they run” counts every verified node, reachable now or verified before.'
    );
    // The list itself is unchanged: one row per version, its count beside it.
    expect(tier1.querySelectorAll('[data-version-bar]')).toHaveLength(2);
  });
});

describe('Network page rhythm, Over time and tier 2 (hierarchy v3 R6, R13, R14, R16; lane N, N4)', () => {
  afterEach(() => vi.restoreAllMocks());

  it('spaces the sections as the v3 rhythm: 64 px after the hero, 72 px apart, h2 → evidence 18', async () => {
    stubChartWidth(1248);
    const { tier1 } = await renderPage();
    await within(tier1).findByRole('region', { name: 'Where they run' });
    await within(tier1).findByRole('region', { name: 'Which software they run' });
    const blocks = Array.from(tier1.children) as HTMLElement[];
    expect(blocks[0]).toContainElement(screen.getByRole('region', { name: 'Where the nodes are' }));
    const holder = (name: string) =>
      blocks.find((b) => b.contains(screen.getByRole('region', { name })))!;
    expect(holder('Where they run')).toHaveClass('mt-16');
    expect(holder('Which software they run')).toHaveClass('mt-[72px]');
    expect(holder('Over time')).toHaveClass('mt-[72px]');
    expect(tier1).not.toHaveClass('space-y-8');
    for (const h2 of Array.from(tier1.querySelectorAll('h2'))) {
      expect(h2).toHaveClass('text-section', 'text-text-bright');
      // The heading row of the DAO page's sections: a flex row, 18 px above the evidence.
      expect(h2.parentElement).toHaveClass('mb-[18px]', 'flex', 'items-baseline');
    }
  });

  it('draws Over time with its name and values inside the plot, one reading row and one caption', async () => {
    stubChartWidth(900);
    render(wrap(<NetworkTrends />));
    const section = await screen.findByRole('region', { name: 'Over time' });
    const svg = await within(section).findByRole('img', {
      name: 'Verified and reachable peers, peers',
    });
    expect(section.querySelector('figcaption')).toBeNull();
    expect(svg.querySelector('[data-role="chart-name"]')).toHaveTextContent(
      'verified and reachable peers per completed UTC+8 day'
    );
    expect(svg.querySelectorAll('[data-role="y-tick"]').length).toBeGreaterThan(0);
    // R14: the first and the last day on the axis.
    expect([...svg.querySelectorAll('[data-x-tick]')].map((n) => n.textContent)).toEqual([
      '07-01',
      '07-03',
    ]);
    const reading = within(section).getByRole('status', { name: 'Peer reading' });
    expect(reading).toHaveTextContent(
      /^2025-07-03 · Same-network reachable 80 peers · Verified \(retained\) 120 peerstap or hover$/
    );
    const caption = section.lastElementChild as HTMLElement;
    expect(caption).toHaveAttribute('data-role', 'caption');
    expect(caption).toHaveClass('text-body', 'text-text', 'max-w-[640px]');
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    // The paragraph and the panel heading leave tier 1.
    expect(section).not.toHaveTextContent(/Daily history of retained verified peers/);
    expect(section).not.toHaveTextContent(/Retained verification state/);
    expect(section).not.toHaveTextContent(/Rounds per day are unavailable/);
  });

  it('keeps the Over time notes in tier 2, word for word', async () => {
    stubChartWidth(1248);
    const { tier2 } = await renderPage();
    await waitFor(() =>
      expect(tier2).toHaveTextContent(/Coverage: 2025-07-01 through 2025-07-03\./)
    );
    expect(tier2).toHaveTextContent(
      "“Over time” draws the retained verification state (daily): verified and reachable peers per completed UTC+8 day. Daily history of retained verified peers, by UTC+8 day. The current (incomplete) day is excluded. Round counts were not recorded. Days older than the crawler's hourly retention show as gaps, never zero."
    );
    expect(tier2).toHaveTextContent(
      'Shaded days have no retained round observation. Rounds per day are unavailable because the crawler did not record counts.'
    );
  });

  it('keeps tier 1 to one coloured text, one hint per section and short prose (R11, R14, §4)', async () => {
    stubChartWidth(1248);
    const { container, tier1, hero } = await renderPage();
    await within(tier1).findByRole('img', { name: 'Verified and reachable peers, peers' });
    await within(tier1).findByRole('region', { name: 'Where they run' });
    const figure = hero.querySelector('[data-role="reachable"]')!;
    expect(tier1.querySelector('figcaption')).toBeNull();
    // No inline ink outside the graphics either (a chart key's coloured series names).
    for (const el of Array.from(tier1.querySelectorAll('[style*="color"]')))
      if (!el.closest('svg') && !(el as HTMLElement).style.backgroundColor)
        expect(figure.contains(el)).toBe(true);
    for (const section of Array.from(tier1.querySelectorAll('section')))
      expect(section.querySelectorAll('[data-hint="tap-or-hover"]').length).toBeLessThanOrEqual(1);
    const prose = Array.from(tier1.querySelectorAll('[data-role="caption"], [data-role="lede"]'));
    expect(prose).toHaveLength(4);
    for (const p of prose) {
      expect(p.className).toMatch(/(^|\s)max-w-\[(640|560)px\](\s|$)/);
      expect(p.textContent!.length).toBeLessThanOrEqual(160);
    }
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
  });

  it('orders tier 2: how the crawler sees it, the round, protocols, daily counts, who named whom, peers', async () => {
    stubChartWidth(1248);
    const { tier2 } = await renderPage();
    await within(tier2).findByRole('heading', { name: 'Peer evidence' });
    expect(
      within(tier2)
        .getAllByRole('heading', { level: 2 })
        .map((h) => h.textContent)
    ).toEqual([
      'How the crawler sees the network',
      'Crawler round evidence',
      'Protocols they speak',
      'Daily version and country counts',
      'Who named whom',
      'Peer evidence',
    ]);
  });
});

describe('Network tier 2 lists, five rows first (R5, R8; lane N, N4)', () => {
  afterEach(() => vi.restoreAllMocks());

  it('lists the first five peers and loads fifty more at a time', async () => {
    const limits: (string | null)[] = [];
    const peer = (i: number) => ({
      peerId: `1220${String(i).padStart(60, '0')}`,
      crawlerDialState: 'reachable',
      participation: {
        discoveryAdvertised: false,
        directSessionObserved: false,
        crawlerIdentified: true,
      },
      sessionInitiators: [],
      primaryAddr: `/ip4/10.0.0.${i}/tcp/8115`,
      version: '0.204.0',
      country: 'US',
      asn: 'AS1 Example',
      lastAdvertisedAt: null,
      lastDialObservedAt: 1751500800,
      latestPositiveObservedAt: 1751500800,
      lastReachableAt: 1751500800,
      rttMs: 10,
    });
    server.use(
      http.get(`${API_BASE}/network/peers`, ({ request }) => {
        const url = new URL(request.url);
        limits.push(url.searchParams.get('limit'));
        const from = url.searchParams.get('cursor') ? 5 : 0;
        const n = Number(url.searchParams.get('limit'));
        const items = Array.from({ length: Math.min(n, 7 - from) }, (_, k) => peer(from + k));
        return HttpResponse.json({ items, nextCursor: from + n < 7 ? items.at(-1)!.peerId : null });
      })
    );
    render(wrap(<PeersTable />));
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(5));
    expect(limits).toEqual(['5']);
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(7));
    expect(limits).toEqual(['5', '50']);
  });

  it('shows who named whom in five rows of two, then every observation on demand', async () => {
    stubChartWidth(1248);
    server.use(
      http.get(`${API_BASE}/network/advertisements`, () =>
        HttpResponse.json({
          roundId: 5,
          finishedAt: 1751500800,
          coverage: 'retainedLatestObservationOnly',
          distinctPeerPairs: 12,
          edges: Array.from({ length: 12 }, (_, i) => ({
            advertiserPeerId: 'aaaaaaaa'.repeat(8),
            targetPeerId: `${i}`.padStart(64, 'b'),
            alias: `alias-${i}`,
            observedAt: 1751500800,
          })),
        })
      )
    );
    render(wrap(<AdvertisementGraph roundId={5} />));
    await waitFor(() => expect(screen.getAllByRole('listitem')).toHaveLength(10));
    fireEvent.click(screen.getByRole('button', { name: 'Show all 12 observations' }));
    expect(screen.getAllByRole('listitem')).toHaveLength(12);
    expect(screen.getByRole('button', { name: 'Show first 10 observations' })).toBeInTheDocument();
  });
});
