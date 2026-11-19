import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '../../utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { CHARTS } from '@/components/charts/chart-catalogue';
import { api } from '@/lib/api';
import { CHART_FIXTURES, days } from './chart-hero-fixtures';

/**
 * Gate for the chart pages on the chart-hero board the owner passed (gate D0′, lane D1): the 29
 * pages are one template, so every rule of the board is held here on every slug.
 *
 * 1. The head: the h1 at 26 px (`text-index`), an eyebrow that names the figure in words (never a
 *    date over a figure), the figure at 40 px from 640 (`sm:text-display`), the chart under it.
 * 2. One sentence under the title says what the series did, from its own points, every figure cut
 *    and never rounded; it replaces the line of figures (no "Last change", no peak reading).
 * 3. A bucket chart's figure is the answer the chart is asked, named in words.
 * 4. Ticks inside the plot, bare, the unit said once in the plot's name.
 * 5. A stack draws at most five ordered bands (a ranking its five largest and the rest), each named
 *    at its right end with its latest figure, in solid fills.
 * 6. An outlier does not set the axis: it is named with its exact value.
 * 8. One controls row of the controls library: a range, a scale, the help mark; no raw control.
 */

vi.mock('@/lib/api', async () => {
  const { CHART_FIXTURES: fixtures } = await import('./chart-hero-fixtures');
  const methods = Object.fromEntries(
    Object.keys(fixtures).map((name) => [name, vi.fn(async () => fixtures[name])])
  );
  return {
    api: {
      ...methods,
      getNetworkStats: vi.fn(async () => ({ syncStatus: { chartDataMayBeIncomplete: false } })),
      getHardforks: vi.fn(async () => ({ events: [] })),
      getBlocksByDay: vi.fn(async (date: string) => ({
        date,
        startMs: 0,
        endMs: 86_400_000,
        first: { number: 20_000_000, hash: `0x${'01'.repeat(32)}` },
        last: { number: 20_008_000, hash: `0x${'02'.repeat(32)}` },
        blocksCount: 8001,
        complete: true,
      })),
    },
  };
});
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({
  ChartSmallMultiple: () => <div>Related preview</div>,
}));
const chartProps = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => {
    chartProps.push(props);
    return <div role="img" aria-label={props.title} />;
  },
}));

interface Expect {
  eyebrow: string;
  figure: string;
  sentence?: string;
}

const d400 = days(400);
const EXPECT: Record<string, Expect> = {
  'transaction-count': {
    eyebrow: 'confirmed in a day · 2026-11-18',
    figure: '12,000 transactions',
    sentence: `The most in a day since 2026-11-16; the peak was 1,111,902 on ${d400[100]}.`,
  },
  'active-addresses': { eyebrow: 'active in a day · 2026-11-18', figure: '2,575 addresses' },
  'ckb-volume': { eyebrow: 'moved in a day · 2026-11-18', figure: '302,594,731 CKB' },
  'activity-volume': { eyebrow: 'recorded in a day · 2026-11-18', figure: '6,177 activities' },
  'activity-type-breakdown': {
    eyebrow: 'script calls, the largest kind in a day · 2026-11-18',
    figure: '5,425 activities',
    sentence: "Script calls were 88.47% of the day's activities, CKB transfers 10.25%.",
  },
  'cell-count': { eyebrow: 'live · 2026-11-18', figure: '1,000,020 cells' },
  'hodl-wave': {
    eyebrow: 'unmoved for 3 years or more · 2026-11-18',
    figure: '27.37% of all CKB',
    sentence: 'Up from 21.74% a year before; 12.87% of all CKB moved within the last month.',
  },
  'total-supply': {
    eyebrow: 'circulating · 2026-11-18',
    figure: '6,000 CKB',
    sentence:
      'Of the 10,000 CKB issued, 29.99% is deposited in the Nervos DAO and 10.00% is burnt.',
  },
  'total-deposit': {
    eyebrow: 'deposited in the Nervos DAO · 2026-11-18',
    figure: '8,191,619,544 CKB',
  },
  'daily-deposit': { eyebrow: 'deposited in a day · 2026-11-18', figure: '12,232,783 CKB' },
  'nominal-apc': {
    eyebrow: 'the model today · year 7.01',
    figure: '2.36% a year',
    sentence: 'The model falls from 5.33% at genesis to 1.58% in year 20.',
  },
  'inflation-rate': {
    eyebrow: 'nominal, trailing 365 days · 2026-11-18',
    figure: '5.48%',
    sentence:
      'Down from 5.97% a year before; real inflation, without the secondary issuance, is 3.34%.',
  },
  'secondary-issuance': {
    eyebrow: 'issued since genesis · 2026-11-18',
    figure: '9,397,611,890 CKB',
    sentence:
      'By destination, 72.39% is burnt, 18.05% paid to DAO depositors as compensation and 9.55% to miners.',
  },
  'circulation-ratio': {
    eyebrow: 'deposited in the Nervos DAO · 2026-11-18',
    figure: '16.37% of circulating CKB',
  },
  'knowledge-size': {
    eyebrow: 'occupied by live cells · 2026-11-18',
    figure: '149,076,490 CKB',
    sentence:
      '17.34% under its peak of 180,360,561 CKB on 2026-11-16, and 76,490 CKB up on the day before.',
  },
  'common-knowledge-composition': {
    eyebrow: 'other scripts, the largest kind · 2026-11-18',
    figure: '44 CKB',
    sentence: 'Other scripts hold 44.00% of occupied CKB, plain CKB 40.00%.',
  },
  'most-utilized-scripts': {
    eyebrow: 'Default Lock, the largest script · 2026-11-18',
    figure: '600 CKB',
    sentence: 'The five largest scripts hold 90.90% of occupied CKB.',
  },
  'most-utilized-assets': {
    eyebrow: 'RUSD, the largest asset · 2026-11-18',
    figure: '600 CKB',
    sentence: 'The five largest assets hold 90.90% of occupied CKB.',
  },
  'cell-size-distribution': { eyebrow: 'live cells of < 100 CKB', figure: '90.11% of live cells' },
  'capacity-turnover-ratio': {
    eyebrow: 'live capacity moved in the week · 2026-11-18',
    figure: '2.17%',
  },
  'address-cohort-retention': {
    eyebrow: 'occupied share of its balance, the 2026-10 cohort',
    figure: '0.01%',
    sentence: 'The 2024-06 cohort keeps the largest share, 7.89%.',
  },
  'hash-rate': {
    eyebrow: 'hash rate · 2026-11-18',
    figure: '81.05 PH/s',
    sentence:
      '82.58% under its peak of 465.31 PH/s on 2024-12-27, and 1.16 PH/s up on the day before.',
  },
  difficulty: { eyebrow: "difficulty, the day's average · 2026-11-18", figure: '788.78 P' },
  'uncle-rate': {
    eyebrow: 'uncle rate · 2026-11-18',
    figure: '2.00%',
    sentence: `Over the 2.50% target on 12 of the last 30 days; the peak was 3.00% on ${days(40)[10]}.`,
  },
  'miner-address-distribution': {
    eyebrow: 'F2Pool, the largest miner · 2026-11-12 → 2026-11-18',
    figure: '41,816 blocks',
  },
  'average-block-time': {
    eyebrow: "block time, the day's average · 2026-11-18",
    figure: '9.95 s',
    sentence: `The longest since ${days(200)[150]}; the peak was 116.32 s on ${days(200)[1]}.`,
  },
  'epoch-time-length': {
    eyebrow: 'epoch #299 took',
    figure: '4.05 h',
    sentence: 'Over the 4.00 h target in 21 of the last 42 epochs.',
  },
  'block-time-distribution': {
    eyebrow: 'the most common interval · last 42 epochs',
    figure: '1.3 s',
    sentence:
      '1.03% of blocks came 1.3 s after their parent, more than at any other tenth of a second.',
  },
  'epoch-time-distribution': {
    eyebrow: 'the most common duration',
    figure: '4.02 h',
    sentence: 'The shortest epoch took 2.05 h and the longest 50.50 h.',
  },
};

/** Charts whose series spans orders of magnitude: drawn on a log scale by default (CH-08). */
const LOG_BY_DEFAULT = ['transaction-count', 'activity-volume', 'capacity-turnover-ratio'];

const mainChart = (title: string) => {
  const props = chartProps.filter((entry) => entry.title === title).at(-1);
  if (!props) throw new Error(`no chart ${title}`);
  return props;
};

describe('the chart pages on the chart-hero board (lane D1)', () => {
  beforeEach(() => {
    chartProps.length = 0;
  });

  it('pins a head for every one of the 29 pages', () => {
    expect(Object.keys(EXPECT).sort()).toEqual(CHARTS.map((chart) => chart.slug).sort());
  });

  for (const definition of CHARTS) {
    const expected = EXPECT[definition.slug];
    it(`${definition.slug}: the head, the sentence, the controls and the plot`, async () => {
      render(<ChartDetail slug={definition.slug} />);
      const h1 = await screen.findByRole('heading', { level: 1, name: definition.title });
      expect(h1).toHaveClass('text-index');

      // 1, 3. the figure named in words, then the figure at 40 px from 640
      const figure = document.querySelector('[data-role="chart-figure"]');
      expect(figure, 'the figure block').not.toBeNull();
      const eyebrow = within(figure as HTMLElement).getByTestId('figure-eyebrow');
      expect(eyebrow).toHaveTextContent(expected.eyebrow);
      expect(eyebrow.textContent).not.toMatch(/^\d{4}-/);
      const value = within(figure as HTMLElement).getByTestId('figure-value');
      expect(value).toHaveClass('text-index', 'sm:text-display');
      expect(value.textContent?.replace(/ /g, ' ')).toBe(expected.figure);

      // 2. one sentence, in place of the line of figures
      const sentence = document.querySelector('[data-role="chart-sentence"]');
      expect(sentence, 'the sentence').not.toBeNull();
      const words = sentence!.querySelector('[data-role="sentence-words"]')!.textContent!;
      expect(words).toMatch(/\.$/);
      if (expected.sentence) expect(words).toBe(expected.sentence);
      expect(screen.queryByText(/Last change/)).toBeNull();
      expect(document.querySelector('[aria-label$=" readings"]')).toBeNull();
      expect(
        within(sentence as HTMLElement).getByRole('link', { name: /How it is computed/ })
      ).toHaveAttribute('href', '#how-it-is-computed');

      // 8. the controls library only: no pressed toggle, no raw range button
      expect(document.querySelector('button[aria-pressed]')).toBeNull();
      const dated =
        definition.slug !== 'epoch-time-length' &&
        !/distribution|nominal-apc|cohort|most-utilized/.test(definition.slug);
      if (dated) {
        const range = screen.getByRole('radiogroup', { name: 'range' });
        expect(within(range).getByRole('radio', { name: 'all' })).toBeChecked();
      }
      expect(
        screen.getByRole('button', { name: 'How to read and zoom this chart' })
      ).toBeInTheDocument();

      if (definition.visual === 'ranked') return;
      const chart = mainChart(definition.title);
      // 4. ticks inside the plot, bare; the unit once, in the plot's name
      expect(chart.insideYTicks).toBeTruthy();
      expect(chart.bareTicks).toBe(true);
      expect(typeof chart.plotName).toBe('string');
      expect(chart.yScale ?? 'linear').toBe(
        LOG_BY_DEFAULT.includes(definition.slug) ? 'log' : 'linear'
      );
      // 5. a stack: at most five bands, each named at its right end, solid
      if (chart.kind === 'stacked') {
        const series = chart.series as { key: string }[];
        expect(series.length).toBeLessThanOrEqual(
          definition.slug === 'activity-type-breakdown' ? 6 : 5
        );
        expect(Object.keys(chart.bandEnds as Record<string, string>).sort()).toEqual(
          series.map((entry) => entry.key).sort()
        );
        expect(chart.solidBands).toBe(true);
      }
    });
  }

  it('names the genesis epochs over the axis instead of letting them set it (rule 6)', async () => {
    render(<ChartDetail slug="epoch-time-length" />);
    await screen.findByRole('heading', { level: 1, name: 'Epoch length' });
    const chart = mainChart('Epoch length');
    expect(chart.axisCap).toEqual({
      value: expect.any(Number),
      note: '2 epochs above this scale; the largest, #0 · 50.48 h',
    });
    expect((chart.axisCap as { value: number }).value).toBeLessThan(17.21);
    expect((chart.axisCap as { value: number }).value).toBeGreaterThanOrEqual(4.05);
  });

  it('draws the HODL wave in its five bands, oldest on the baseline, each figure the API’s (rule 5)', async () => {
    render(<ChartDetail slug="hodl-wave" />);
    await screen.findByRole('heading', { level: 1, name: 'HODL wave' });
    const chart = mainChart('HODL wave');
    expect((chart.series as { key: string; label: string }[]).map((s) => [s.key, s.label])).toEqual(
      [
        ['gt3y', '3 years or more'],
        ['1y3y', '1 to 3 years'],
        ['6m1y', '6 to 12 months'],
        ['1m6m', '1 to 6 months'],
        ['lt1m', 'under 1 month'],
      ]
    );
    expect(chart.bandEnds).toEqual({
      gt3y: '27.37%',
      '1y3y': '31.05%',
      '6m1y': '12.40%',
      '1m6m': '16.31%',
      lt1m: '12.87%',
    });
    // the eight ages stay in the table under the chart
    const table = screen.getByRole('table', { name: 'HODL wave, every band' });
    expect(within(table).getAllByRole('row')).toHaveLength(9);
  });

  it('draws a ranked history as its five largest and the API’s rest (rule 5)', async () => {
    render(<ChartDetail slug="most-utilized-scripts" />);
    await screen.findByRole('heading', { level: 1, name: 'Scripts' });
    const history = mainChart('Occupied common knowledge');
    const series = history.series as { key: string; label: string }[];
    expect(series.map((s) => s.label)).toEqual([
      'Default Lock',
      'Nervos DAO',
      'Omnilock',
      'xUDT',
      'Spore',
      'Others',
    ]);
    const data = history.data as { values: Record<string, string> }[];
    // the rest is the API's figure (ranks six and seven and its remainder), never a page's sum
    expect(data.at(-1)!.values.rest).toBe('200');
    expect(history.bandEnds).toMatchObject({ rest: '9.09%' });
  });
});
