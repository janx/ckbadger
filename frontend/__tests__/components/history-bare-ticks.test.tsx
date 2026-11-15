import { afterEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { plotLeftOf, yTickLabels } from '@/__tests__/utils/y-ticks';
import { AssetHistory } from '@/components/asset/asset-history';
import { CollectionHolders } from '@/components/asset/collection-holders';
import { NetworkTrends } from '@/app/network/trends';

// Lane A5 follow-up (UI review 2026-11-14, U-19): on a phone the history gutter was 104–110 px
// because every tick repeated its unit ("1.5k transactions"); a tick is the bare value, the unit
// is said once in the plot's name, and the gutter is at most 56 px. A count axis ticks in whole
// numbers ("0.5 transfers" was drawn for a week of one transfer).

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const BARE = /^−?[\d.,]+[kMB]?$/;
const DAY = 86_400_000;
const date = (i: number) => new Date(Date.UTC(2024, 0, 1) + i * DAY).toISOString().slice(0, 10);
const daily = Array.from({ length: 29 }, (_, i) => ({
  date: date(i),
  capacity: String((1000 + i * 10) * 1e8),
  occupiedCapacity: String(600 * 1e8),
  supply: String((19_000_000 + i * 50_000) * 1e6),
}));

function usdc(weekly: { weekStart: string; count: number }[]) {
  return (
    <AssetHistory
      assetClass="tokens"
      daily={daily}
      weekly={weekly}
      weeklyNoun="transactions"
      completeThrough={date(28)}
      token={{ symbol: 'USDC', decimals: 6 }}
    />
  );
}

describe('a token history at 358 px', () => {
  it('writes bare values in a gutter of at most 56 px, each plot naming its unit once', () => {
    stubChartWidth(358);
    render(
      usdc([0, 7, 14, 21].map((d, k) => ({ weekStart: date(d), count: [420, 1510, 380, 90][k] })))
    );
    const plots = screen.getAllByRole('img');
    // Re-pinned with the plot-name fit (lane A5, Chrome at 360): the supply plot's name is the
    // fixed 'supply in tokens', the symbol having no length bound.
    const units = ['CKB', 'tokens', 'transactions'];
    plots.forEach((plot, i) => {
      for (const t of yTickLabels(plot)) expect(t.textContent, units[i]).toMatch(BARE);
      expect(plot.querySelector('[data-role="chart-name"]')?.textContent).toContain(units[i]);
    });
    const left = new Set(plots.map(plotLeftOf));
    expect(left.size).toBe(1);
    expect([...left][0]).toBeLessThanOrEqual(56);
  });

  it('ticks a week of one transaction 0 and 1, never 0.5', () => {
    stubChartWidth(358);
    render(usdc([{ weekStart: date(7), count: 1 }]));
    const weekly = screen.getByRole('img', { name: /per week/ });
    expect(yTickLabels(weekly).map((t) => t.textContent)).toEqual(['0', '1']);
  });
});

describe('"over time" at 358 px', () => {
  it('writes its peers as bare whole numbers in a gutter of at most 56 px', async () => {
    stubChartWidth(358);
    const days = [1751299200, 1751385600, 1751472000];
    server.use(
      http.get('/api/:network/v1/network/history', ({ request }) => {
        const metric = new URL(request.url).searchParams.get('metric') ?? '';
        return HttpResponse.json({
          coverageFrom: days[0],
          coverageTo: days[2],
          dailyCoverage: days.map((ts) => ({
            ts,
            hasRetainedObservation: true,
            roundsPerDay: null,
          })),
          granularity: 'day',
          metric,
          points: days.map((ts) => ({
            ts,
            scalar: metric === 'reachablePeers' ? 1 : 2,
            buckets: [],
          })),
          roundCountsAvailable: false,
        });
      })
    );
    render(<NetworkTrends />);
    const svg = await screen.findByRole('img', { name: 'Verified and reachable peers, peers' });
    const labels = yTickLabels(svg).map((t) => t.textContent!);
    expect(labels.length).toBeGreaterThan(1);
    for (const l of labels) expect(Number.isInteger(Number(l)), l).toBe(true);
    expect(plotLeftOf(svg)).toBeLessThanOrEqual(56);
    expect(svg.querySelector('[data-role="chart-name"]')?.textContent).toContain('peers');
  });
});

describe('holder counts', () => {
  it('draws a page whose largest holder has 7 items on a whole-number scale', async () => {
    stubChartWidth(1248);
    const load = vi.fn().mockResolvedValue({
      data: [7, 2].map((itemCount, i) => ({
        lockScriptHash: `0x${String(i + 1).repeat(64)}`,
        address: null,
        itemCount,
        lockCodeHash: null,
        lockKind: null,
      })),
      limit: 10,
      total: 2,
      hasMore: false,
      nextCursor: null,
    });
    render(<CollectionHolders load={load} queryKey="seven" assetClass="objects" />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    expect(table.querySelector('[data-scale-ticks]')).toHaveTextContent(/^010$/);
  });
});
