import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '../../utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { ACTIVITY_KINDS_SLUG, CHARTS } from '@/components/charts/chart-catalogue';
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
  /** Every slug's sentence, word for word (T's NOTE 7). */
  sentence: string;
}

const d400 = days(400);
const EXPECT: Record<string, Expect> = {
  'transaction-count': {
    eyebrow: 'confirmed in a day · 2026-11-18',
    figure: '12,000 transactions',
    sentence: `The most in a day since 2026-11-16; the peak was 1,111,902 on ${d400[100]}.`,
  },
  'active-addresses': {
    eyebrow: 'active in a day · 2026-11-18',
    figure: '2,575 addresses',
    sentence: `The fewest in a day since 2026-11-16; the peak was 1,111,902 on ${d400[100]}.`,
  },
  'ckb-volume': {
    eyebrow: 'moved in a day · 2026-11-18',
    figure: '302,594,731 CKB',
    sentence: 'The most in a day on record.',
  },
  // re-pinned on purpose (F-6 merged): the kinds partition the day's activities, so the series is
  // its activities again and needs no note (lane D1's second follow-up had made it "counted in a
  // day", "6,177 kind counts" and the note "An activity of two kinds counts under both.")
  'activity-volume': {
    eyebrow: 'recorded in a day · 2026-11-18',
    figure: '6,177 activities',
    sentence: `The fewest in a day on record; the peak was 1,111,902 on ${d400[100]}.`,
  },
  'activity-type-breakdown': {
    eyebrow: 'script calls, the largest kind in a day · 2026-11-18',
    figure: '5,425 activities',
    // re-pinned on purpose (F-6 merged): the whole is the day's activities, which the kinds
    // partition (it was "… of the day's kind counts, …" with the overlap's note, and before that
    // "of the day's kind occurrences")
    sentence: "Script calls were 88.47% of the day's activities, CKB transfers 10.25%.",
  },
  'cell-count': {
    eyebrow: 'live · 2026-11-18',
    figure: '1,000,020 cells',
    sentence: 'At its peak, 10 cells up on the day before.',
  },
  'hodl-wave': {
    eyebrow: 'unmoved for 3 years or more · 2026-11-18',
    figure: '27.37% of all CKB',
    sentence: 'Up from 21.74% a year before; 12.87% of all CKB moved within the last month.',
  },
  // lane D1, second follow-up, item 1: circulating is the protocol circulation, C − genesis burnt
  // − S, the route's (it was the band outside the DAO, 6,000); the sentence states C once, what
  // circulates, of that what is in the DAO, and what is burnt (it was "Of the 12,000 CKB issued,
  // 25.00% is deposited in the Nervos DAO and 8.33% is burnt."); re-pinned for lane exactness, part A2 (M1): the
  // deposits are named as the DAO band is, "of it deposited in the Nervos DAO" (was "of it in")
  'total-supply': {
    eyebrow: 'circulating · 2026-11-18',
    figure: '8,000 CKB',
    sentence:
      // re-pinned for lane exactness, part B1: of what circulates, the deposits and the withdrawing apart (100 of
      // 8,000 is 1.25 %)
      'Of the 12,000 CKB issued, 66.66% circulates, 37.50% of it deposited in the Nervos DAO and 1.25% withdrawing from it; 29.16% is burnt.',
  },
  'total-deposit': {
    eyebrow: 'deposited in the Nervos DAO · 2026-11-18',
    figure: '8,191,619,544 CKB',
    sentence:
      '26.80% under its peak of 11,190,757,666 CKB on 2026-11-16, and 1,619,544 CKB up on the day before.',
  },
  'daily-deposit': {
    eyebrow: 'deposited in a day · 2026-11-18',
    figure: '12,232,783 CKB',
    sentence: 'The most in a day on record.',
  },
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
  // lane D1's third round: one whole, the protocol total its destinations make (S1: the fixture's 9,397,611,890
  // disagreed with them), and compensation accrued, claimed or not (S2: it said "paid")
  'secondary-issuance': {
    eyebrow: 'issued since genesis · 2026-11-18',
    figure: '9,417,500,000 CKB',
    sentence:
      'By destination, 72.39% is burnt, 18.05% accrued to DAO depositors as compensation, claimed or unclaimed, and 9.55% went to miners.',
  },
  // lane D1's third round (S9): the figures as shown, no difference of two cut values (it said "0.01 points down")
  'circulation-ratio': {
    eyebrow: 'deposited in the Nervos DAO · 2026-11-18',
    figure: '16.37% of circulating CKB',
    sentence: 'Its peak was 35.89% on 2026-11-16; 16.37%, down from 16.38% the day before.',
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
  'cell-size-distribution': {
    eyebrow: 'live cells of < 100 CKB',
    figure: '90.11% of live cells',
    sentence: 'The most occupied CKB is in cells of 100k – 1M CKB, 33.90% of it.',
  },
  'capacity-turnover-ratio': {
    eyebrow: 'live capacity moved in the week · 2026-11-18',
    figure: '2.17%',
    sentence: 'The lowest on record; the peak was 29.51% on 2026-11-15.',
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
  difficulty: {
    eyebrow: "difficulty, the day's average · 2026-11-18",
    figure: '788.78 P',
    sentence: '84.91% under its peak of 5,229.34 P on 2026-11-16, and 0.78 P up on the day before.',
  },
  'uncle-rate': {
    eyebrow: 'uncle rate · 2026-11-18',
    figure: '2.00%',
    sentence: `Over the 2.50% target on 12 of the last 30 days; the peak was 3.00% on ${days(40)[10]}.`,
  },
  'miner-address-distribution': {
    eyebrow: 'F2Pool, the largest miner · 2026-11-12 → 2026-11-18',
    figure: '41,816 blocks',
    sentence: "Of the window's 60,583 blocks, the next largest miner, ckb1…cdefghij, found 9,000.",
  },
  'average-block-time': {
    eyebrow: "block time, the day's average · 2026-11-18",
    figure: '9.95 s',
    sentence: `The longest since ${days(200)[150]}; the peak was 116.32 s on ${days(200)[1]}.`,
  },
  'epoch-time-length': {
    eyebrow: 'epoch #299 took',
    figure: '4.05 h',
    // lane D1's third round (M1): counted from the exact ms, epoch 298's 17 s over the target too (it was 21)
    sentence: 'Over the 4.00 h target in 22 of the last 42 epochs.',
  },
  'block-time-distribution': {
    eyebrow: 'the most common interval · last 42 epochs',
    figure: '1.3 s',
    // lane D1's third round (S4): what lies past the drawn 50 s is said
    sentence:
      '1.03% of blocks came 1.3 s after their parent, more than at any other tenth of a second; 12 blocks came more than 50 s after it.',
  },
  'epoch-time-distribution': {
    eyebrow: 'the most common duration',
    // re-pinned for the ruling of 2026-11-19: minute 241 is 4.01 h, cut (was half up, 4.02)
    figure: '4.01 h',
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
      // a count and its noun are held by a no-break space (the units gate): compared as a space
      if (expected.sentence) expect(words.replace(/\u00a0/g, ' ')).toBe(expected.sentence);
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
        // Rule 5: at most five bands. The lead's ruling (lane D1's third round, T's MUST-1): Activity
        // by type keeps every kind, the site's closed set, each with its class colour and its
        // filter chip, none of them "others"; five and "others" would hide a named kind behind a
        // word. It is the owner's to confirm at the gate; the allowance is that one slug's, by
        // name (ACTIVITY_KINDS_SLUG), and every other stack, classed or not, is held at five.
        // Re-pinned on purpose (F-6 merged): seven kinds, the protocols' their own (it was six).
        expect(series.length).toBeLessThanOrEqual(definition.slug === ACTIVITY_KINDS_SLUG ? 7 : 5);
        if (definition.slug === ACTIVITY_KINDS_SLUG) expect(series).toHaveLength(7);
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
    const cap = chart.axisCap as { value: number; note: string };
    expect(cap.note.replace(/\u00a0/g, ' ')).toBe(
      '2 epochs above this scale; the largest, #0 · 50.48 h'
    );
    expect(cap.value).toBeLessThan(17.21);
    expect(cap.value).toBeGreaterThanOrEqual(4.05);
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
    // one band, one spelling: the table names the ages as the band ends do, the route's words
    // (lane D1's third round, N5: it wrote "6 m – 1 y" beside "6 to 12 months")
    expect(
      within(table)
        .getAllByRole('row')
        .slice(1)
        .map((row) => row.querySelector('td')?.textContent)
    ).toEqual([
      '3 years or more',
      '1 to 3 years',
      '6 to 12 months',
      '3 to 6 months',
      '1 to 3 months',
      '1 week to 1 month',
      '1 day to 1 week',
      'under 1 day',
    ]);
  });

  // The figure fits a 360 px phone (328 px of line): at 26 px a digit is 15.6 px and the unit, at
  // 22 px, 13.2 px a glyph, so "16.37% of circulating CKB" is 5 × 15.6 + 20 × 13.2 = 342 px, over
  // the line by 14 px when held on one line. The unit's words wrap between themselves; the figure
  // and the unit's first word stay together (a no-break space, or the % sign against the digits).
  it('lets a long unit wrap under its figure on a phone, never the figure from its unit', async () => {
    const view = render(<ChartDetail slug="circulation-ratio" />);
    const value = await screen.findByTestId('figure-value');
    // it wraps: no nowrap on the figure or inside it by any form the nowrap gate knows (a class,
    // Tailwind's text-nowrap, an inline white-space or text-wrap, a NoBreak's span), and its
    // unit's words are joined by plain spaces (T's NOTE 6: one class name's absence was pinned)
    for (const el of [value, ...value.querySelectorAll<HTMLElement>('*')]) {
      expect(el).not.toHaveClass('whitespace-nowrap');
      expect(el).not.toHaveClass('text-nowrap');
      expect(el.style.whiteSpace).not.toBe('nowrap');
      expect(el.style.getPropertyValue('text-wrap')).not.toBe('nowrap');
    }
    expect(value.textContent).toBe('16.37% of circulating CKB');
    view.unmount();
    render(<ChartDetail slug="hash-rate" />);
    const rate = await screen.findByTestId('figure-value');
    expect(rate.textContent).toBe('81.05\u00a0PH/s');
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
