import { afterEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import evidence from '@/__tests__/fixtures/dao-evidence.json';
import producer from '@/__tests__/fixtures/network-api-producer/hardforks-deployments.json';
import { AssetHistory } from '@/components/asset/asset-history';
import { ScriptHistoryFigure } from '@/components/scripts/script-family-board';
import { DaoHistoryFigure, historySeries } from '@/components/dao/dao-history';
import { DaoGrows } from '@/components/dao/dao-grows';
import { VmLaneTimeline } from '@/components/hardforks/vm-lane-timeline';
import { NetworkTrends } from '@/app/network/trends';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { api, type ChartResponse, type HardforkHashTypeLaneF2 } from '@/lib/api';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return {
    ...actual,
    api: { ...actual.api, getDaoArHistory: vi.fn(), calculateDaoCompensation: vi.fn() },
  };
});

// Lane A5 (UI review 2026-11-14), Chrome at 360: the script page's history plot was named "CKB in
// Default Lock cells · occupied and free", 297 px of SVG text in a plot with 262 px of room, so it
// ran past the viewport. An SVG text does not wrap: a plot's name must fit its plot at every width
// for every value it can carry. Every history plot of the lane is checked here at the content
// width of a 360 px phone (328 px) and of a 390 px one (358 px): each line of its name, at the
// measured advance of a JetBrains Mono glyph (0.6 em: 6.6 px at 11 px), must end inside the plot.

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const ADVANCE_EM = 0.6;
const WIDTHS = [328, 358] as const;

/** The lines of a plot's name: a tspan with its own x starts a new line, any other continues one. */
function nameLines(name: Element): { x: number; text: string }[] {
  const lines = [{ x: Number(name.getAttribute('x')), text: '' }];
  for (const node of Array.from(name.childNodes)) {
    const el = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : null;
    if (el?.hasAttribute('x')) lines.push({ x: Number(el.getAttribute('x')), text: '' });
    lines[lines.length - 1].text += node.textContent ?? '';
  }
  return lines.filter((l) => l.text.length > 0);
}

/** Every line of the name ends at or before `right`; returns the widest line's end. */
function expectNameFits(svg: Element, right: number, what: string): string {
  const name = svg.querySelector('[data-role="chart-name"]');
  expect(name, `${what} has a name`).not.toBeNull();
  const px = Number(name!.getAttribute('font-size') ?? '11') * ADVANCE_EM;
  for (const line of nameLines(name!)) {
    const end = line.x + line.text.length * px;
    expect(
      end,
      `${what}: "${line.text}" (${line.text.length} × ${px} px from ${line.x})`
    ).toBeLessThanOrEqual(right);
  }
  return name!.textContent ?? '';
}

/** A Chart's plot ends 10 px short of its svg. */
const chartRight = (svg: Element) => Number(svg.getAttribute('width')) - 10;

const DAY = 86_400_000;
const date = (i: number) => new Date(Date.UTC(2024, 0, 1) + i * DAY).toISOString().slice(0, 10);

describe.each(WIDTHS)('history plot names at %i px', (width) => {
  it('fits the script page history (a script name of any length)', () => {
    stubChartWidth(width);
    for (const name of ['JoyID', 'Default Lock', 'Secp256k1 / multisig (legacy, deprecated)']) {
      const { unmount } = render(
        <ScriptHistoryFigure
          family={{ ...fixture.family, name } as never}
          chart={fixture.chart as never}
          lineage={null}
          numbers={new Map()}
          active={null}
          onActive={() => {}}
        />
      );
      for (const svg of screen.getAllByRole('img'))
        expectNameFits(svg, chartRight(svg), `${name} history`);
      unmount();
    }
  });

  it('fits the token and collection history (every unit and noun)', () => {
    stubChartWidth(width);
    const daily = Array.from({ length: 29 }, (_, i) => ({
      date: date(i),
      capacity: String((1000 + i * 10) * 1e8),
      occupiedCapacity: String(600 * 1e8),
      supply: String((19_000_000 + i * 50_000) * 1e6),
    }));
    const weekly = [0, 7, 14, 21].map((d) => ({ weekStart: date(d), count: 1510 }));
    const tokens = [
      { symbol: 'USDC', decimals: 6 },
      { symbol: 'A_VERY_LONG_TOKEN_SYMBOL_OF_FORTY_CHARS!', decimals: 6 },
      { symbol: null, decimals: 6 },
      { symbol: 'RAW', decimals: null },
    ];
    for (const token of [...tokens, undefined])
      for (const weeklyNoun of ['transfers', 'transactions'] as const) {
        const { unmount } = render(
          <AssetHistory
            assetClass={token ? 'tokens' : 'objects'}
            daily={daily}
            weekly={weekly}
            weeklyNoun={weeklyNoun}
            completeThrough={date(28)}
            token={token}
          />
        );
        for (const svg of screen.getAllByRole('img'))
          expectNameFits(svg, chartRight(svg), `${token?.symbol ?? 'collection'} history`);
        unmount();
      }
  });

  it('fits the network "over time" plot', async () => {
    stubChartWidth(width);
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
            scalar: metric === 'reachablePeers' ? 57 : 78,
            buckets: [],
          })),
          roundCountsAvailable: false,
        });
      })
    );
    render(<NetworkTrends />);
    const svg = await screen.findByRole('img', { name: 'Verified and reachable peers, peers' });
    expectNameFits(svg, chartRight(svg), 'over time');
  });

  it('fits the DAO deposited, share and weekly panels', () => {
    stubChartWidth(width);
    const values = Array.from({ length: 160 }, (_, i) => 10_000_000_000 + i * 1_000_000);
    const deposited: ChartResponse = {
      data: values.map((v, i) => ({ date: date(i), value: String(v), value2: '100' })),
      title: 'Total Deposit',
      yAxisLabel: 'CKB',
    } as ChartResponse;
    const ratio: ChartResponse = {
      data: values.map((_, i) => ({ date: date(i), value: '35.1234' })),
      title: 'Ratio',
      yAxisLabel: '%',
    } as ChartResponse;
    function Picture() {
      const pin = usePinnedReadout<number>();
      return <DaoHistoryFigure series={historySeries(deposited, ratio, 'All')} pin={pin} />;
    }
    const { container } = render(<Picture />);
    for (const panel of ['deposited', 'share', 'weekly']) {
      const svg = container.querySelector(`[data-panel="${panel}"]`)!;
      // these panels draw to their svg's right edge
      expectNameFits(svg, Number(svg.getAttribute('width')), `DAO ${panel}`);
    }
  });

  it('fits the DAO AR plot', async () => {
    stubChartWidth(width);
    vi.mocked(api.getDaoArHistory).mockResolvedValue({
      anchor: { blockNumber: 4, blockHash: '0x' + '4'.repeat(64) },
      days: [1, 2, 3].map((n, i) => ({
        date: date(i),
        blockNumber: n,
        ar: String(10000000000000000 + i * 50000000000000),
        arIndex: (1 + i * 0.005).toFixed(16),
      })),
    });
    vi.mocked(api.calculateDaoCompensation).mockReturnValue(new Promise(() => {}));
    render(<DaoGrows evidence={evidence as never} />);
    const svg = await screen.findByRole('img', { name: /^AR by date/ });
    expectNameFits(svg, chartRight(svg), 'AR');
  });

  it('fits the hardforks VM lanes', () => {
    stubChartWidth(width);
    const lanes: HardforkHashTypeLaneF2[] = [
      { hashType: 'type', knownVersions: 60, liveReferences: '1765012' },
      { hashType: 'data1', knownVersions: 19, liveReferences: '97771' },
      { hashType: 'data2', knownVersions: 0, liveReferences: '0' },
      { hashType: 'data', knownVersions: 1, liveReferences: '472' },
    ];
    render(
      <VmLaneTimeline
        editions={[
          { shortName: 'Mirana', activationEpoch: 5414, activationDate: '2022-05-10' },
          { shortName: 'Meepo', activationEpoch: 12293, activationDate: '2025-07-01' },
        ]}
        lanes={lanes}
        deployments={producer.scriptDeployments.map((d) => ({ ...d, hashTypes: [] }))}
        genesisMs={Date.parse('2019-11-15T21:49:50.812Z')}
        tipMs={Date.parse('2026-10-30T00:00:00.000Z')}
        tipEpoch={15292}
      />
    );
    const svg = screen.getAllByRole('img')[0];
    expectNameFits(svg, Number(svg.getAttribute('width')), 'VM lanes');
  });
});
