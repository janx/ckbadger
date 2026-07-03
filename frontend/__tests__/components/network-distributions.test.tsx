import { describe, it, expect } from 'vitest';
import { http, HttpResponse } from 'msw';
import { DEFAULT_API_BASE } from '@/lib/runtime-config';
import { render, screen, waitFor } from '../utils/test-utils';
import { server } from '../msw/server';
import { NetworkDistributions } from '@/app/network/distributions';
import { NetworkTrends } from '@/app/network/trends';

const API_BASE = DEFAULT_API_BASE;

describe('NetworkDistributions', () => {
  it('renders version / country / asn / protocol bars and a reachable-vs-unreachable stat', async () => {
    server.use(
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          totalKnown: 7,
          reachable: 5,
          unreachable: 2,
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
    // Reachable-vs-unreachable stat.
    expect(screen.getByText('5 reachable · 2 unreachable')).toBeInTheDocument();
  });

  it('renders an empty-state note when a distribution has no data', async () => {
    server.use(
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          totalKnown: 0,
          reachable: 0,
          unreachable: 0,
          versions: [],
          countries: [],
          asns: [],
          protocols: [],
        })
      )
    );

    render(<NetworkDistributions />);

    // Reachability stat still renders with zeros.
    expect(await screen.findByText('0 reachable · 0 unreachable')).toBeInTheDocument();
    // Empty distributions render an honest "no data" note rather than a broken chart.
    expect(screen.getAllByText(/no data/i).length).toBeGreaterThan(0);
  });
});

describe('NetworkTrends', () => {
  it('renders the daily node-count line series and version/country share areas from the MSW points', async () => {
    // Points already exclude the current day (mirrors the server dropping the current-day bucket).
    const days = [1751328000, 1751414400, 1751500800];
    server.use(
      http.get(`${API_BASE}/network/history`, ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        if (metric === 'totalNodes') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            points: days.map((ts, i) => ({ ts, scalar: 100 + i * 10, buckets: [] })),
          });
        }
        if (metric === 'reachableNodes') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
            points: days.map((ts, i) => ({ ts, scalar: 60 + i * 5, buckets: [] })),
          });
        }
        if (metric === 'versionShare') {
          return HttpResponse.json({
            metric,
            granularity: 'day',
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

    // Node-count line series: MultiSeriesLineChart only renders its legend controls when it
    // received non-empty data, so their presence proves the merged points flowed in.
    expect(await screen.findByRole('button', { name: 'Total Nodes' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Reachable Nodes' })).toBeInTheDocument();
    // Version-share stacked area legend (series derived from the per-point buckets).
    expect(screen.getByText('0.114.0')).toBeInTheDocument();
    // Country-share stacked area legend.
    expect(screen.getByText('United States')).toBeInTheDocument();
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
      expect(seenTo.totalNodes).not.toBeUndefined();
      expect(seenTo.reachableNodes).not.toBeUndefined();
      expect(seenTo.versionShare).not.toBeUndefined();
      expect(seenTo.countryShare).not.toBeUndefined();
    });

    // Daily trends must pass `to = now` so the API drops the current-day bucket.
    expect(seenTo.totalNodes).not.toBeNull();
    expect(seenTo.reachableNodes).not.toBeNull();
    expect(seenTo.versionShare).not.toBeNull();
    expect(seenTo.countryShare).not.toBeNull();
  });
});
