import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { AssetHistory } from '@/components/asset/asset-history';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { expectTicksInGutter, plotLeftOf } from '@/__tests__/utils/y-ticks';

// U-19 / PB-27 / DY-05 (UI review 2026-11-14): the history plots of the token, cluster, class and
// identity-collection pages wrote their round values inside the plot in the ghost ink, under the
// line ("20M USDC"), behind the bars ("500 transactions") and, by day, on the filled celadon area
// at 1.1–1.3:1. One treatment for every such chart: the values in a gutter left of the plot, in the
// dim ink, the three plots of one history sharing it.

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

// USDC's history, in its shape: about 20M in supply, a few hundred transactions a week.
const DAY = 86_400_000;
const date = (i: number) => new Date(Date.UTC(2024, 0, 1) + i * DAY).toISOString().slice(0, 10);
const daily = Array.from({ length: 29 }, (_, i) => ({
  date: date(i),
  capacity: String((1000 + i * 10) * 1e8),
  occupiedCapacity: String(600 * 1e8),
  supply: String((19_000_000 + i * 50_000) * 1e6),
}));
const weekly = [0, 7, 14, 21].map((d, k) => ({
  weekStart: date(d),
  count: [420, 510, 380, 90][k],
}));

describe.each([1248, 326])('a token history at %i px (U-19, DY-05)', (width) => {
  it('writes every value of its three plots in one dim gutter left of the plots', () => {
    stubChartWidth(width);
    render(
      <AssetHistory
        assetClass="tokens"
        daily={daily}
        weekly={weekly}
        weeklyNoun="transactions"
        completeThrough={date(28)}
        token={{ symbol: 'USDC', decimals: 6 }}
      />
    );
    const plots = screen.getAllByRole('img');
    expect(plots).toHaveLength(3);
    const units = ['CKB', 'USDC', 'transactions'];
    plots.forEach((plot, i) => {
      const labels = expectTicksInGutter(plot, units[i]);
      for (const l of labels) expect(l).toMatch(new RegExp(` ${units[i]}$`));
    });
    // one time axis, one gutter: the three plots start at one x
    expect(new Set(plots.map(plotLeftOf)).size).toBe(1);
  });

  it('keeps the main plot’s name inside the plot beside the gutter (SVG text cannot wrap)', () => {
    stubChartWidth(width);
    render(
      <AssetHistory
        assetClass="tokens"
        daily={daily}
        weekly={weekly}
        weeklyNoun="transactions"
        completeThrough={date(28)}
        token={{ symbol: 'USDC', decimals: 6 }}
      />
    );
    const main = screen.getByRole('img', { name: 'CKB held in its cells, CKB' });
    const name = main.querySelector('[data-role="chart-name"]')!;
    // the plot ends 10 px short of the svg; an 11 px glyph is 6.6 px
    expect(Number(name.getAttribute('x')) + name.textContent!.length * 6.6).toBeLessThanOrEqual(
      width - 10
    );
  });
});
