import { describe, it, expect, onTestFinished, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen, waitFor, within } from '../utils/test-utils';
import { server } from '../msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { HostingBar, NetworkDistributions, NetworkProtocols } from '@/app/network/distributions';
import {
  mergePeerCounts,
  NetworkShareHistory,
  NetworkTrendNotes,
  NetworkTrends,
} from '@/app/network/trends';
import producerGaps from '@/__tests__/fixtures/network-api-producer/network-history-gaps.json';

const API_BASE = '/api/:network/v1';

describe('NetworkDistributions', () => {
  // F2 charts (CH-52, CH-54): versions in tier 1; ASNs as hosting organisations; protocols tier 2.
  it('renders verified-peer versions, hosting organisations and protocols', async () => {
    server.use(
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          verifiedRetained: 7,
          sameNetworkReachable: 5,
          verifiedUnavailable: 2,
          versions: [
            { label: '0.114.0', count: 4 },
            { label: '0.113.0', count: 1 },
          ],
          countries: [
            { label: 'United States', count: 3 },
            { label: 'Germany', count: 2 },
          ],
          asns: [
            { label: 'AS24940 Hetzner', count: 2 },
            { label: 'Unknown', count: 5 },
          ],
          protocols: [{ label: '/ckb/2', count: 5 }],
        })
      )
    );

    render(
      <>
        <NetworkDistributions />
        <HostingBar />
        <NetworkProtocols />
      </>
    );

    // Version bar labels are rendered as real DOM text (not SVG geometry).
    expect(await screen.findByText('0.114.0')).toBeInTheDocument();
    // The ASN's organisation, with the one-sentence reading over the verified peers.
    expect(
      screen.getByText('2 of the 7 verified nodes run on Hetzner, across 2 countries.')
    ).toBeInTheDocument();
    expect(screen.getByText('/ckb/2')).toBeInTheDocument();
  });

  it('renders an empty-state note when a distribution has no data', async () => {
    server.use(
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          verifiedRetained: 0,
          sameNetworkReachable: 0,
          verifiedUnavailable: 0,
          versions: [],
          countries: [],
          asns: [],
          protocols: [],
        })
      )
    );

    render(<NetworkDistributions />);

    // Empty distributions render an honest "no data" note rather than a broken chart.
    expect((await screen.findAllByText(/no data/i)).length).toBeGreaterThan(0);
  });
});

describe('NetworkTrends', () => {
  it('renders missing retained days as a visual gap with unavailable round counts', async () => {
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        if (metric === 'verifiedPeers') return HttpResponse.json(producerGaps);
        return HttpResponse.json({
          ...producerGaps,
          metric,
          points: producerGaps.points.map((point) => ({
            ...point,
            scalar: metric === 'reachablePeers' ? 5 : 0,
          })),
        });
      })
    );
    stubChartWidth(900);
    // Hierarchy v3 (N4): how to read the chart and its coverage are tier 2's notes.
    const { container } = render(
      <>
        <NetworkTrends />
        <NetworkTrendNotes />
      </>
    );
    // CH-50: the shared chart, a shaded gap the series never bridge, and the gap note.
    expect(
      await screen.findByRole('img', { name: 'Verified and reachable peers, peers' })
    ).toBeInTheDocument();
    expect(container.querySelector('[data-chart-gap="no rounds 01-12"]')).not.toBeNull();
    // D2: the API's days are UTC+8 calendar days, and the page says so.
    expect(
      screen.getByText('verified and reachable peers per completed UTC+8 day')
    ).toBeInTheDocument();
    expect(screen.queryByText(/UTC day/)).toBeNull();
    expect(screen.getByText(/Coverage: 1970-01-11 through 1970-01-13\./)).toBeInTheDocument();
    expect(
      screen.getByText(/Days older than the crawler's hourly retention show as gaps, never zero\./)
    ).toBeInTheDocument();
    const verifiedLine = container.querySelectorAll('path[fill="none"]')[1].getAttribute('d')!;
    expect(verifiedLine.match(/M/g)).toHaveLength(2);
    expect(screen.getByText(/Shaded days have no retained round observation/)).toBeInTheDocument();
    expect(screen.getByText(/Rounds per day are unavailable/i)).toBeInTheDocument();
  });

  it('refuses a daily point that is not a UTC+8 midnight (D2)', async () => {
    const utcMidnight = 1751328000; // 2025-07-01 00:00 UTC, i.e. 08:00 UTC+8
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        return HttpResponse.json({
          ...observedVerified([utcMidnight]),
          metric,
          points: [{ ts: utcMidnight, scalar: metric === 'reachablePeers' ? 5 : 8, buckets: [] }],
        });
      })
    );
    render(<NetworkTrends />);
    // Said where the chart would be (and where the coverage line would be), never drawn.
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Invalid peer history: network history day 1751328000 is not a UTC+8 midnight'
    );
    expect(screen.queryByRole('img', { name: 'Verified and reachable peers, peers' })).toBeNull();
  });

  it('draws gapless history on the shared chart without the shaded-day note (CH-50)', async () => {
    const days = [1751299200, 1751385600];
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        return HttpResponse.json({
          metric,
          granularity: 'day',
          dailyCoverage: days.map((ts) => ({
            ts,
            hasRetainedObservation: true,
            roundsPerDay: null,
          })),
          coverageFrom: days[0],
          coverageTo: days[1],
          roundCountAvailability: 'notRecorded',
          points: days.map((ts) => ({
            ts,
            scalar: metric === 'reachablePeers' ? 63 : 80,
            buckets: [],
          })),
        });
      })
    );
    stubChartWidth(900);
    const { container } = render(<NetworkTrends />);
    expect(
      await screen.findByRole('img', { name: 'Verified and reachable peers, peers' })
    ).toBeInTheDocument();
    expect(container.querySelector('[data-chart-gap]')).toBeNull();
    expect(screen.queryByText(/Shaded days/)).not.toBeInTheDocument();
    expect(screen.queryByRole('list', { name: /missing days/i })).not.toBeInTheDocument();
  });

  const observedVerified = (ts: number[]) => ({
    metric: 'verifiedPeers',
    granularity: 'day',
    dailyCoverage: ts.map((t) => ({ ts: t, hasRetainedObservation: true, roundsPerDay: null })),
    coverageFrom: ts[0],
    coverageTo: ts.at(-1),
    roundCountAvailability: 'notRecorded',
    points: ts.map((t) => ({ ts: t, scalar: 80, buckets: [] })),
  });

  it('keeps the page up while the reachable history is still loading (#2)', async () => {
    server.use(
      http.get(`${API_BASE}/network/history`, async ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        if (metric === 'verifiedPeers') return HttpResponse.json(observedVerified([1751299200]));
        if (metric === 'reachablePeers') return new Promise<never>(() => {});
        return HttpResponse.json({ metric, granularity: 'day', points: [] });
      })
    );
    render(<NetworkTrends />);
    expect(await screen.findByRole('heading', { name: 'Over time' })).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText('Loading verified peer history…')).toBeInTheDocument()
    );
  });

  it('reports a failed reachable history instead of crashing the page (#2)', async () => {
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        if (metric === 'verifiedPeers') return HttpResponse.json(observedVerified([1751299200]));
        if (metric === 'reachablePeers')
          return HttpResponse.json({ error: 'unavailable' }, { status: 500 });
        return HttpResponse.json({ metric, granularity: 'day', points: [] });
      })
    );
    render(<NetworkTrends />);
    expect(await screen.findByText('Failed to load verified peer history.')).toBeInTheDocument();
  });

  it('pairs verified and reachable histories day by day on one time axis', () => {
    const merged = mergePeerCounts(
      {
        metric: 'verifiedPeers',
        granularity: 'day',
        dailyCoverage: [{ ts: 1751299200, hasRetainedObservation: true, roundsPerDay: null }],
        coverageFrom: 1751299200,
        coverageTo: 1751299200,
        roundCountAvailability: 'notRecorded',
        points: [{ ts: 1751299200, scalar: 100, buckets: [] }],
      },
      {
        metric: 'reachablePeers',
        granularity: 'day',
        dailyCoverage: [{ ts: 1751299200, hasRetainedObservation: true, roundsPerDay: null }],
        coverageFrom: 1751299200,
        coverageTo: 1751299200,
        roundCountAvailability: 'notRecorded',
        points: [{ ts: 1751299200, scalar: 60, buckets: [] }],
      }
    );

    // CH-50: the design draws the verified total as a line over the reachable area, on one
    // shared time axis, so the merged point carries both totals and its day.
    expect(merged.error).toBeNull();
    expect(merged.data[0]).toEqual({
      label: '2025-07-01',
      x: 1751299200,
      values: { reachable: '60', verified: '100' },
    });
  });

  it('renders daily peer state and retained version/country counts without invented shares', async () => {
    // Points already exclude the current day (mirrors the server dropping the current-day bucket).
    const days = [1751299200, 1751385600, 1751472000];
    const coverage = {
      dailyCoverage: days.map((ts) => ({ ts, hasRetainedObservation: true, roundsPerDay: null })),
      coverageFrom: days[0],
      coverageTo: days[2],
      roundCountAvailability: 'notRecorded',
    };
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        if (metric === 'verifiedPeers') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            ...coverage,
            points: days.map((ts, i) => ({ ts, scalar: 100 + i * 10, buckets: [] })),
          });
        }
        if (metric === 'reachablePeers') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            ...coverage,
            points: days.map((ts, i) => ({ ts, scalar: 60 + i * 5, buckets: [] })),
          });
        }
        if (metric === 'versionShare') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            ...coverage,
            points: days.map((ts) => ({
              ts,
              scalar: 0,
              buckets: [
                { label: '0.114.0', count: 4 },
                { label: '0.113.0', count: 2 },
              ],
            })),
          });
        }
        // countryShare
        return HttpResponse.json({
          metric,
          granularity: 'day',
          ...coverage,
          points: days.map((ts) => ({
            ts,
            scalar: 0,
            buckets: [
              { label: 'United States', count: 3 },
              { label: 'Germany', count: 2 },
            ],
          })),
        });
      })
    );

    // The daily count tables now sit in tier 2 (CH-54), rendered by their own component.
    render(
      <>
        <NetworkTrends />
        <NetworkShareHistory />
      </>
    );

    expect(await screen.findByRole('heading', { name: 'Over time' })).toBeInTheDocument();
    // The series are named in the reading row (no coloured key above the plot, hierarchy v3).
    expect(await screen.findByRole('status', { name: 'Peer reading' })).toHaveTextContent(
      'Same-network reachable 70 peers · Verified (retained) 120 peers'
    );
    // Historical totals were not persisted, so these are exact retained counts rather than shares.
    // Table-rows T8: every day is one line; its whole label list is the row's title.
    expect(screen.getAllByTitle(/0.114.0 4 · 0.113.0 2/)).toHaveLength(3);
    expect(screen.getAllByTitle(/United States 3 · Germany 2/)).toHaveLength(3);
    expect(screen.getAllByText(/percentages are unavailable/).length).toBeGreaterThan(0);
  });

  it('passes to=now on every daily query so the API drops the current-day bucket', async () => {
    const seenTo: Record<string, string | null> = {};
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const url = new URL(request.url);
        const metric = url.searchParams.get('metric') ?? '';
        seenTo[metric] = url.searchParams.get('to');
        return HttpResponse.json({ metric, granularity: 'day', points: [] });
      })
    );

    render(
      <>
        <NetworkTrends />
        <NetworkShareHistory />
      </>
    );

    // Wait until all four daily queries have fired.
    await waitFor(() => {
      expect(seenTo.verifiedPeers).not.toBeUndefined();
      expect(seenTo.reachablePeers).not.toBeUndefined();
      expect(seenTo.versionShare).not.toBeUndefined();
      expect(seenTo.countryShare).not.toBeUndefined();
    });

    // Daily trends must pass `to = now` so the API drops the current-day bucket.
    expect(seenTo.verifiedPeers).not.toBeNull();
    expect(seenTo.reachablePeers).not.toBeNull();
    expect(seenTo.versionShare).not.toBeNull();
    expect(seenTo.countryShare).not.toBeNull();
  });

  it('surfaces a verified/reachable history invariant violation', async () => {
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        if (metric === 'verifiedPeers') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            points: [{ ts: 1751299200, scalar: 4, buckets: [] }],
          });
        }
        if (metric === 'reachablePeers') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            points: [{ ts: 1751299200, scalar: 5, buckets: [] }],
          });
        }
        return HttpResponse.json({ metric, granularity: 'day', points: [] });
      })
    );

    render(<NetworkTrends />);

    expect(await screen.findByText(/reachablePeers 5 exceeds verifiedPeers 4/)).toBeInTheDocument();
  });

  // Table-rows T8: a day is one line — the day (the row's key), a count bar of its retained
  // labels with the most common one in jade, how many labels, and the most common with its count;
  // every label and count stays in the row's title, a day without a round says so on its line.
  it('draws each day as one line with a count bar; every label and count in its title (T8)', async () => {
    const days = [1751299200, 1751385600, 1751472000, 1751558400];
    const observed = days.filter((ts) => ts !== 1751472000);
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        return HttpResponse.json({
          metric,
          granularity: 'day',
          dailyCoverage: days.map((ts) => ({
            ts,
            hasRetainedObservation: observed.includes(ts),
            roundsPerDay: null,
          })),
          coverageFrom: days[0],
          coverageTo: days[3],
          roundCountAvailability: 'notRecorded',
          points: observed.map((ts, i) => ({
            ts,
            scalar: 0,
            buckets:
              i === 2
                ? []
                : [
                    { label: '0.210.0 (2592ddf 2026-09-17)', count: 35 + i },
                    { label: '0.209.0 (d166e28 2026-07-29)', count: 8 },
                    { label: '0.5.3', count: 1 },
                  ],
          })),
        });
      })
    );
    render(<NetworkShareHistory />);
    const table = await screen.findByRole('table', {
      name: 'Client Version Share daily observations',
    });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(4);
    expect(within(table).getByText('Retained count').closest('th')).not.toBeNull();
    // Every head holds its words in an inline element, so its text box is its words (R16).
    for (const head of within(table).getAllByRole('columnheader'))
      expect(head.firstElementChild?.tagName).toBe('SPAN');
    // The first day: the key, the bar (35 of 44 in jade), 3 labels, the most common.
    const [first, , missing, empty] = rows;
    expect(first).toHaveAttribute(
      'title',
      '2025-07-01 · 0.210.0 (2592ddf 2026-09-17) 35 · 0.209.0 (d166e28 2026-07-29) 8 · 0.5.3 1'
    );
    expect(within(first).getByText('2025-07-01')).toHaveClass(
      'text-body',
      'text-text-bright',
      'leading-[18px]'
    );
    expect(
      within(first).getByRole('img', {
        name: 'Retained count: 44, 35 0.210.0 (2592ddf 2026-09-17), 9 other labels',
      })
    ).toBeInTheDocument();
    expect(first.querySelector('[data-segment="leader"]')).toHaveStyle({
      backgroundColor: 'rgb(46, 219, 163)',
    });
    expect(first.querySelector('[data-track]')).toHaveStyle({ height: '8px' });
    const cells = within(first).getAllByRole('cell');
    expect(cells.map((cell) => cell.textContent)).toEqual([
      '2025-07-01',
      '44',
      '3',
      '0.210.0 (2592ddf 2026-09-17) 35',
    ]);
    // One line per day: no cell holds a second block.
    for (const cell of cells) expect(cell.querySelector('.block, p, div')).toBeNull();
    expect(missing).toHaveAttribute('title', '2025-07-03 · No retained round observation');
    expect(missing).toHaveTextContent('No retained round observation');
    expect(missing.querySelector('[data-aligned-bar]')).toBeNull();
    expect(empty).toHaveAttribute('title', '2025-07-04 · No retained labels');
    expect(empty).toHaveTextContent('No retained labels');
  });

  // A title is no hover on a touch screen: the phone row also says every label, quietly, last.
  it('keeps every column of a day on its phone row, and every label in a quiet last line', async () => {
    const days = [1751299200, 1751385600];
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) =>
        HttpResponse.json({
          metric: new URL(request.url).searchParams.get('metric'),
          granularity: 'day',
          dailyCoverage: days.map((ts, i) => ({
            ts,
            hasRetainedObservation: i === 0,
            roundsPerDay: null,
          })),
          coverageFrom: days[0],
          coverageTo: days[1],
          roundCountAvailability: 'notRecorded',
          points: [
            {
              ts: days[0],
              scalar: 0,
              buckets: [
                { label: 'US', count: 12 },
                { label: 'DE', count: 10 },
              ],
            },
          ],
        })
      )
    );
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    render(<NetworkShareHistory />);
    const rows = await screen.findAllByTestId('share-day-row-mobile');
    expect(rows).toHaveLength(4);
    expect(phoneLines(rows[0]).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['day', 'head', '2025-07-01', '22'],
      ['leader', 'body', 'US 12', '2 labels'],
      ['labels', 'quiet', 'US 12 · DE 10', null],
    ]);
    expect(phoneLines(rows[1]).map((l) => [l.key, l.left, l.right])).toEqual([
      ['day', '2025-07-02', null],
      ['leader', 'No retained round observation', null],
    ]);
    expect(rows[0].closest('[role="listitem"]')).toHaveAttribute(
      'title',
      '2025-07-01 · US 12 · DE 10'
    );
  });

  it('shows a version-history request failure instead of an empty share', async () => {
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric');
        if (metric === 'versionShare')
          return HttpResponse.json({ error: 'unavailable' }, { status: 503 });
        return HttpResponse.json({
          metric,
          granularity: 'day',
          points: [],
          dailyCoverage: [],
          coverageFrom: null,
          coverageTo: null,
          roundCountAvailability: 'notRecorded',
        });
      })
    );
    render(<NetworkShareHistory />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to load client version share.'
    );
  });
});
