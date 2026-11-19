import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { placeXTicks, type PlacedXTick } from '@/lib/chart-ticks';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-12: Nominal APC's x axis skipped "year 10" (on a phone "year 0, year 5,
// year 12.5, year 17.5"); Epoch durations printed evenly spaced ticks "2.05, 3.45, 3.73 … 4.90,
// 50.50" with no unit; Block intervals "0.0, 7.1, 14.3 … 50.0"; Difficulty "6k P" beside the figure
// "5,229.34 P"; Cell size "100k-1m CKB", "≥1m CKB" beside "1M cells".

vi.mock('@/lib/api', () => ({
  api: {
    getNominalApcChart: vi.fn(),
    getEpochTimeDistributionChart: vi.fn(),
    getBlockTimeDistributionChart: vi.fn(),
    getDifficultyChart: vi.fn(),
    getCellSizeDistributionChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));
vi.mock('@/components/charts/blocks-of-day', () => ({ BlocksOfDay: () => null }));
const chartProps = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => {
    chartProps.push(props);
    return <div role="img" aria-label={props.title} />;
  },
}));
type Tick = { index: number; label: string };
const ticksOf = (title: string) =>
  chartProps.filter((props) => props.title === title).at(-1)!.xTicks as Tick[];

/** The ticks a plot `width` px wide keeps, for a line (`slot` false) or slot bars. */
function kept(ticks: Tick[], points: number, width: number, slot: boolean): PlacedXTick[] {
  const left = 56;
  const right = width - 10;
  const at = (index: number) =>
    left + (slot ? (index + 0.5) / points : index / (points - 1)) * (right - left);
  return placeXTicks(
    ticks.map((tick) => ({ ...tick, at: at(tick.index) })),
    { left, right, width, categorical: false }
  ).ticks;
}
/** Numbers of round labels, which must step evenly: no value of the sequence left out. */
function assertRegular(labels: string[]): void {
  const values = labels.map((label) => Number(label.replace(/^year /, '').replace(/ \w+$/, '')));
  expect(values.every(Number.isFinite)).toBe(true);
  const steps = values.slice(1).map((value, i) => +(value - values[i]).toFixed(9));
  expect(new Set(steps).size).toBeLessThanOrEqual(1);
}

describe('axis ticks (CH-12)', () => {
  beforeEach(() => {
    chartProps.length = 0;
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
  });

  it('keeps year 10 on the model axis, a regular sequence at every width', async () => {
    const data = Array.from({ length: 81 }, (_, i) => ({
      date: String(i / 4),
      value: String(4.2 - i / 40),
      value2: i < 28 ? 'past' : i === 28 ? 'current' : 'future',
    }));
    vi.mocked(api.getNominalApcChart).mockResolvedValue({
      title: 'Nominal APC',
      yAxisLabel: 'Nominal APC (%)',
      y2AxisLabel: 'nominal-apc-timeline',
      data,
    });
    render(<ChartDetail slug="nominal-apc" />);
    await screen.findByRole('img', { name: 'Nominal APC' });
    const ticks = ticksOf('Nominal APC');
    expect(ticks.map((tick) => tick.label)).toContain('year 10');
    assertRegular(ticks.map((tick) => tick.label));
    for (const width of [1232, 608, 358, 326])
      assertRegular(kept(ticks, 81, width, false).map((tick) => tick.label));
  });

  it('labels Epoch durations at round hours, the unit once, its outliers open-ended', async () => {
    const minutes = [123, ...Array.from({ length: 100 }, (_, i) => 207 + i), 3030];
    vi.mocked(api.getEpochTimeDistributionChart).mockResolvedValue({
      title: 'Epoch Time Distribution',
      yAxisLabel: 'Epochs',
      data: minutes.map((m) => ({ date: (m / 60).toFixed(2), value: '3' })),
    });
    render(<ChartDetail slug="epoch-time-distribution" />);
    await screen.findByRole('img', { name: 'Epoch durations' });
    const labels = ticksOf('Epoch durations').map((tick) => tick.label);
    expect(labels[0]).toBe('≤ 2.05');
    expect(labels.at(-1)).toBe('≥ 50.50 h');
    expect(labels.filter((label) => label.includes('h'))).toHaveLength(1);
    const round = labels.slice(1, -1);
    expect(round.length).toBeGreaterThan(2);
    for (const label of round) expect(Number(label) % 0.25).toBe(0);
    assertRegular(round);
  });

  // Readability review 2026-11-18 (lane A): on mainnet the axis read "≤ 2.05 / 3.5 / 4.5 / 5.5",
  // 3.5 → 4.5 h over 574 px and 4.5 → 5.5 h over 325, with no tick at the 4 h peak. The indexer
  // counts epochs by the minute (one bucket a minute, even); the store and the API keep only the
  // minutes some epoch took, and the axis gave each of those one slot, so a minute no epoch took
  // had none and the hours ran unevenly; the labels were then thinned every k-th from "≤ 2.05",
  // dropping 4.0 and the last label, which carries the unit. Mainnet's 120 buckets of 2026-11-18,
  // as minute:epochs:
  const MAINNET_EPOCH_MINUTES: [number, number][] = [
    '123:1 163:1 177:1 187:1 192:3 193:1 194:3 195:3 196:2 198:1 199:4 201:1 202:3 203:1 ',
    '204:2 205:10 206:5 207:8 208:9 209:10 210:20 211:10 212:11 213:28 214:26 215:26 216:42 ',
    '217:40 218:49 219:62 220:72 221:95 222:113 223:132 224:164 225:187 226:231 227:240 ',
    '228:266 229:327 230:339 231:383 232:414 233:450 234:523 235:538 236:584 237:596 238:557 ',
    '239:623 240:608 241:628 242:613 243:609 244:536 245:550 246:486 247:479 248:435 249:403 ',
    '250:407 251:299 252:303 253:208 254:233 255:176 256:153 257:158 258:121 259:103 260:88 ',
    '261:83 262:60 263:53 264:40 265:25 266:30 267:27 268:26 269:16 270:14 271:20 272:12 ',
    '273:12 274:4 275:8 276:1 277:7 278:7 279:3 280:3 281:3 282:5 283:2 285:3 286:3 287:3 ',
    '288:2 289:3 290:2 291:2 293:1 294:1 296:2 299:1 300:1 302:1 303:1 306:1 307:1 308:2 ',
    '310:2 313:1 319:1 340:1 460:1 505:1 523:1 1033:1 3030:1',
  ]
    .join('')
    .split(' ')
    .map((pair) => pair.split(':').map(Number) as [number, number]);
  const mountMainnetEpochs = async () => {
    vi.mocked(api.getEpochTimeDistributionChart).mockResolvedValue({
      title: 'Epoch Time Distribution',
      yAxisLabel: 'Epochs',
      // the API's label: the minute in hours, two decimals, half up (no minute ties at .xx5)
      data: MAINNET_EPOCH_MINUTES.map(([m, n]) => ({
        date: (m / 60).toFixed(2),
        value: String(n),
      })),
    });
    render(<ChartDetail slug="epoch-time-distribution" />);
    await screen.findByRole('img', { name: 'Epoch durations' });
    const props = chartProps.filter((entry) => entry.title === 'Epoch durations').at(-1)!;
    return { data: props.data as { label: string }[], ticks: props.xTicks as Tick[] };
  };
  const minute = (label: string) => Math.round(Number(label) * 60);
  const isEdge = (label: string) => /^[≤≥] /.test(label);

  it('gives every minute of Epoch durations its slot between the outliers marked open-ended', async () => {
    const { data, ticks } = await mountMainnetEpochs();
    const low = ticks.find((tick) => tick.label.startsWith('≤ '))!;
    const high = ticks.find((tick) => tick.label.startsWith('≥ '))!;
    expect([low.label, high.label]).toEqual(['≤ 2.05', '≥ 7.67 h']);
    const run = data.slice(low.index + 1, high.index).map((point) => minute(point.label));
    const strides = run.slice(1).map((m, i) => m - run[i]);
    expect(strides.filter((stride) => stride !== 1)).toEqual([]);
    expect([run[0], run.at(-1)]).toEqual([163, 340]);
  });

  it('marks 4 h, keeps the unit and steps evenly in hours at every width', async () => {
    const { data, ticks } = await mountMainnetEpochs();
    for (const width of [1232, 976, 608, 358, 326]) {
      const shown = kept(ticks, data.length, width, true);
      const labels = shown.map((tick) => tick.label);
      expect(labels, `${width} px`).toContain('4.0');
      expect(labels.at(-1), `${width} px`).toBe('≥ 7.67 h');
      const round = shown.filter((tick) => !isEdge(tick.label));
      assertRegular(round.map((tick) => tick.label));
      // a linear axis: the same pixels for every hour between any two round ticks
      const perHour = round
        .slice(1)
        .map((tick, i) => (tick.x - round[i].x) / (Number(tick.label) - Number(round[i].label)));
      for (const step of perHour) expect(step, `${width} px`).toBeCloseTo(perHour[0], 6);
    }
  });

  // Review of main..fix/readability (2026-11-18, logic finding 2): the model filled the regular
  // run it found at the detail page's tick budget (163 → 340 min), and the index tile looked for
  // the run again at its own budget of 6. Its larger step swallowed the outliers, so the tile's run
  // ran 2.05 → 8.72 h, "6" stood on the 7.67 h bucket and "8" on the 8.42 h one, "≤ 2.05" was lost
  // and "≥ 17.22 h" said that 7.67, 8.42 and 8.72 h sat on a linear axis one minute apart. One run,
  // the model's; a tick budget chooses only the step.
  it('reads the Epoch durations tile on the run the model filled, at the tile’s own step', async () => {
    const { ChartSmallMultiple } = await vi.importActual<
      typeof import('@/components/charts/chart-small-multiple')
    >('@/components/charts/chart-small-multiple');
    const definition = CHART_BY_SLUG.get('epoch-time-distribution')!;
    vi.mocked(api.getEpochTimeDistributionChart).mockResolvedValue({
      title: 'Epoch Time Distribution',
      yAxisLabel: 'Epochs',
      data: MAINNET_EPOCH_MINUTES.map(([m, n]) => ({
        date: (m / 60).toFixed(2),
        value: String(n),
      })),
    });
    render(<ChartSmallMultiple definition={definition} />);
    await screen.findByRole('img', { name: 'Epoch durations' });
    const props = chartProps.filter((entry) => entry.title === 'Epoch durations').at(-1)!;
    const data = props.data as { label: string }[];
    const ticks = props.xTicks as Tick[];
    // the detail page's edges: the run between them is every minute from 163 to 340
    expect(ticks[0].label).toBe('≤ 2.05');
    expect(ticks.at(-1)!.label).toBe('≥ 7.67 h');
    const round = ticks.slice(1, -1);
    expect(round.length).toBeGreaterThan(2);
    expect(round.length).toBeLessThanOrEqual(6);
    assertRegular(round.map((tick) => tick.label));
    // every round label stands on its own bucket: within the minute, inside the run
    for (const tick of round) {
      expect(minute(data[tick.index].label), tick.label).toBeGreaterThanOrEqual(163);
      expect(minute(data[tick.index].label), tick.label).toBeLessThanOrEqual(340);
      expect(
        Math.abs(minute(data[tick.index].label) - Number(tick.label) * 60),
        tick.label
      ).toBeLessThan(1);
    }
  });

  it('labels Block intervals at round seconds, the unit once', async () => {
    vi.mocked(api.getBlockTimeDistributionChart).mockResolvedValue({
      title: 'Block Time Distribution',
      yAxisLabel: 'Block Ratio (%)',
      data: Array.from({ length: 501 }, (_, i) => ({
        date: `${Math.floor(i / 10)}.${i % 10}`,
        value: '0.199',
      })),
    });
    render(<ChartDetail slug="block-time-distribution" />);
    await screen.findByRole('img', { name: 'Block intervals' });
    expect(ticksOf('Block intervals').map((tick) => tick.label)).toEqual([
      '0',
      '10',
      '20',
      '30',
      '40',
      '50 s',
    ]);
  });

  it('writes one prefix per difficulty tick', async () => {
    vi.mocked(api.getDifficultyChart).mockResolvedValue({
      title: 'Difficulty',
      yAxisLabel: 'Difficulty',
      data: [
        { date: '2026-11-12', value: '5229340000000000000' },
        { date: '2026-11-13', value: '5729340000000000000' },
      ],
    });
    render(<ChartDetail slug="difficulty" />);
    await screen.findByRole('img', { name: 'Difficulty' });
    const props = chartProps.filter((entry) => entry.title === 'Difficulty').at(-1)!;
    const formatAxis = props.formatAxis as ((value: number, step: number) => string) | undefined;
    expect(formatAxis?.(6000, 1000)).toBe('6,000');
    expect(formatAxis?.(5500, 500)).toBe('5,500');
  });

  it('names the Cell size buckets with one prefix form and the unit once', async () => {
    vi.mocked(api.getCellSizeDistributionChart).mockResolvedValue({
      title: 'Cell Size Distribution',
      yAxisLabel: 'Live Cells',
      y2AxisLabel: 'Common Knowledge Size (CKB)',
      data: ['<100 CKB', '100-1k CKB', '1k-10k CKB', '10k-100k CKB', '100k-1m CKB', '>=1m CKB'].map(
        (date, i) => ({ date, value: String(600 - i * 100), value2: '1' })
      ),
      // the route's wholes (lane D1): 600 + 500 + … + 100 cells, 1 CKB in each bucket
      liveCells: '2100',
      occupiedCapacity: '6',
    });
    render(<ChartDetail slug="cell-size-distribution" />);
    await screen.findByRole('img', { name: 'Cell size' });
    expect(ticksOf('Cell size').map((tick) => tick.label)).toEqual([
      '< 100',
      '100 – 1k',
      '1k – 10k',
      '10k – 100k',
      '100k – 1M',
      '≥ 1M CKB',
    ]);
    // Re-pinned for lane D1 (rule 3): the bucket named in words in the figure's eyebrow (was the
    // readings group's "most common, < 100 CKB")
    expect(screen.getByTestId('figure-eyebrow')).toHaveTextContent('live cells of < 100 CKB');
  });
});
