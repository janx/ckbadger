import { afterEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { expectTicksInGutter, yTickLabels } from '@/__tests__/utils/y-ticks';
import { NetworkTrends } from '@/app/network/trends';

// PB-01 (UI review 2026-11-14): at 390 the axis value "50 peers" sat under the first gap band
// ("50 pe"), and on a phone the bands carried no words, so nothing said what a dark band is.
// DY-04: the caption named the night colours of the series ("the white line"), which by day is dark.

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const DAY = 86_400;
/** UTC+8 midnights from 2025-07-01: observed on days 0, 3, 4 and 7, none on 1, 2, 5, 6. */
const days = Array.from({ length: 8 }, (_, i) => 1751299200 + i * DAY);
const observed = new Set([0, 3, 4, 7]);

function serve() {
  server.use(
    http.get('/api/:network/v1/network/history', ({ request }) => {
      const metric = new URL(request.url).searchParams.get('metric') ?? '';
      return HttpResponse.json({
        coverageFrom: days[0],
        coverageTo: days[7],
        dailyCoverage: days.map((ts, i) => ({
          ts,
          hasRetainedObservation: observed.has(i),
          roundsPerDay: null,
        })),
        granularity: 'day',
        metric,
        points: days
          .filter((_, i) => observed.has(i))
          .map((ts) => ({ ts, scalar: metric === 'reachablePeers' ? 57 : 78, buckets: [] })),
        roundCountsAvailable: false,
      });
    })
  );
}

describe.each([358, 1232])('"over time" at %i px (PB-01, DY-04)', (width) => {
  it('writes its values in the dim gutter, clear of every gap band', async () => {
    stubChartWidth(width);
    serve();
    render(<NetworkTrends />);
    const svg = await screen.findByRole('img', { name: 'Verified and reachable peers, peers' });
    expectTicksInGutter(svg, 'peers');
    const bands = Array.from(svg.querySelectorAll('[data-chart-gap] rect'));
    expect(bands.length).toBe(2);
    const firstBand = Math.min(...bands.map((b) => Number(b.getAttribute('x'))));
    for (const label of yTickLabels(svg))
      expect(Number(label.getAttribute('x')), label.textContent!).toBeLessThan(firstBand);
  });

  it('says once, under the plot, what a shaded band is', async () => {
    stubChartWidth(width);
    serve();
    render(<NetworkTrends />);
    const section = await screen.findByRole('region', { name: 'Over time' });
    await within(section).findByRole('img', { name: 'Verified and reachable peers, peers' });
    const legend = section.querySelector('[data-role="gap-legend"]')!;
    expect(legend).toHaveTextContent('no rounds: no retained crawl round that day');
    // its swatch is the band's own fill
    expect(legend.querySelector('[aria-hidden="true"]')).toHaveClass('bg-base-surface');
  });

  it('names the series in its caption, never a colour of one theme', async () => {
    stubChartWidth(width);
    serve();
    render(<NetworkTrends />);
    const section = await screen.findByRole('region', { name: 'Over time' });
    await within(section).findByRole('img', { name: 'Verified and reachable peers, peers' });
    const caption = section.querySelector('[data-role="caption"]')!;
    expect(caption.textContent).not.toMatch(/white|black|jade|celadon|green/i);
    expect(caption).toHaveTextContent(/line .*verified node/);
    expect(caption).toHaveTextContent(/area .*reachable/);
  });
});
