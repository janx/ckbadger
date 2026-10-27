import { describe, it, expect } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen, waitFor } from '../utils/test-utils';
import { server } from '../msw/server';
import { NetworkDistributions } from '@/app/network/distributions';
import { mergePeerCounts, NetworkTrends } from '@/app/network/trends';
import producerGaps from '@/__tests__/fixtures/network-api-producer/network-history-gaps.json';

const API_BASE = '/api/:network/v1';

describe('NetworkDistributions', () => {
  it('renders verified-peer distributions and the exact retained reachability split', async () => {
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
          asns: [{ label: 'AS24940 Hetzner', count: 2 }],
          protocols: [{ label: '/ckb/2', count: 5 }],
        })
      )
    );

    render(<NetworkDistributions />);

    // Version + country bar labels are rendered as real DOM text (not SVG geometry).
    expect(await screen.findByText('0.114.0')).toBeInTheDocument();
    expect(screen.getByText('United States')).toBeInTheDocument();
    // ASN + protocol bars too.
    expect(screen.getByText('AS24940 Hetzner')).toBeInTheDocument();
    expect(screen.getByText('/ckb/2')).toBeInTheDocument();
    expect(
      screen.getByText('5 same-network reachable · 2 verified unavailable')
    ).toBeInTheDocument();
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

    expect(
      await screen.findByText('0 same-network reachable · 0 verified unavailable')
    ).toBeInTheDocument();
    // Empty distributions render an honest "no data" note rather than a broken chart.
    expect(screen.getAllByText(/no data/i).length).toBeGreaterThan(0);
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
    render(<NetworkTrends />);
    expect(
      await screen.findByRole('list', {
        name: /Daily verified peer observations and missing days/i,
      })
    ).toBeInTheDocument();
    expect(
      screen.getByRole('listitem', { name: /no retained round observation/i })
    ).toBeInTheDocument();
    expect(screen.getByText(/Rounds per day are unavailable/i)).toBeInTheDocument();
  });

  it('derives a disjoint reachable/unavailable stack from paired verified histories', () => {
    const merged = mergePeerCounts(
      {
        metric: 'verifiedPeers',
        granularity: 'day',
        dailyCoverage: [{ ts: 1751328000, hasRetainedObservation: true, roundsPerDay: null }],
        coverageFrom: 1751328000,
        coverageTo: 1751328000,
        roundCountAvailability: 'notRecorded',
        points: [{ ts: 1751328000, scalar: 100, buckets: [] }],
      },
      {
        metric: 'reachablePeers',
        granularity: 'day',
        dailyCoverage: [{ ts: 1751328000, hasRetainedObservation: true, roundsPerDay: null }],
        coverageFrom: 1751328000,
        coverageTo: 1751328000,
        roundCountAvailability: 'notRecorded',
        points: [{ ts: 1751328000, scalar: 60, buckets: [] }],
      }
    );

    expect(merged.error).toBeNull();
    expect(merged.data[0].values).toEqual({
      sameNetworkReachable: '60',
      verifiedUnavailable: '40',
    });
  });

  it('renders daily peer state and retained version/country counts without invented shares', async () => {
    // Points already exclude the current day (mirrors the server dropping the current-day bucket).
    const days = [1751328000, 1751414400, 1751500800];
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

    render(<NetworkTrends />);

    expect(await screen.findByText('Retained verification state (daily)')).toBeInTheDocument();
    expect((await screen.findAllByText('Same-network reachable')).length).toBeGreaterThan(0);
    expect(screen.getAllByText('Verified unavailable').length).toBeGreaterThan(0);
    // Historical totals were not persisted, so these are exact retained counts rather than shares.
    expect(screen.getAllByText(/0.114.0 4 · 0.113.0 2/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/United States 3 · Germany 2/).length).toBeGreaterThan(0);
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

    render(<NetworkTrends />);

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
            points: [{ ts: 1751328000, scalar: 4, buckets: [] }],
          });
        }
        if (metric === 'reachablePeers') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            points: [{ ts: 1751328000, scalar: 5, buckets: [] }],
          });
        }
        return HttpResponse.json({ metric, granularity: 'day', points: [] });
      })
    );

    render(<NetworkTrends />);

    expect(await screen.findByText(/reachablePeers 5 exceeds verifiedPeers 4/)).toBeInTheDocument();
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
    render(<NetworkTrends />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to load client version share.'
    );
  });
});
