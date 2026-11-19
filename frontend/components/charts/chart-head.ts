import type { ChartPoint } from '@/components/ui/chart';
import type { ChartDefinition } from '@/components/charts/chart-catalogue';
import {
  axisOutliers,
  axisStyle,
  cutDecimal,
  cutRatio,
  cutShare,
  exactDelta,
  exactSum,
  formatChartExact,
  formatChartHead,
  formatEpochLabel,
  type ChartModel,
} from '@/components/charts/chart-model';
import { exactSignedDecimalRatio } from '@/lib/exact-integer';
import { formatDateWindow, withUnit } from '@/lib/format-unit';
import { formatCount } from '@/lib/format-count';

/**
 * A chart page's head (lane D1, the chart-hero board): the figure named in words, where it is read,
 * and one sentence that says what the series did. The page and its index card read the same head
 * (CH-07), so a card states the page's own figure under the page's own words.
 *
 * Every figure is the API's exact value cut for display (`formatChartHead`: a CKB amount in the
 * summary form, a rate or a duration at two decimals, a count whole), and every figure a sentence
 * computes — a fall from a peak, a change on the day before, a share of a whole — is computed from
 * the exact values and cut, never rounded and never computed from a cut figure. Nothing is smoothed:
 * a sentence reads the series' own points.
 */
export interface ChartHead {
  /** What the figure is, in words (lowercase, the eyebrow). */
  eyebrow: string;
  /** Where it is read: a day, a window, a model year; null where the eyebrow says it. */
  at: string | null;
  /**
   * The figure; null where the source has none to give (a model with no today). `title` is its
   * exact value in words, every digit, for its hover title on the page and the card.
   */
  figure: { raw: string; display: string; unit: string; title: string } | null;
  /** One sentence: what the series did. */
  sentence: string;
}

type Ratio = readonly [bigint, bigint];

/** A head as a slug's function writes it: a figure's title defaults to its exact value. */
type HeadDraft = Omit<ChartHead, 'figure'> & {
  figure: (Omit<NonNullable<ChartHead['figure']>, 'title'> & { title?: string }) | null;
};

const ratio = (value: string, context: string): Ratio => exactSignedDecimalRatio(value, context);
/** -1, 0 or 1 as `a` is under, at or over `b`, exactly. */
function compare(a: Ratio, b: Ratio): number {
  const left = a[0] * b[1];
  const right = b[0] * a[1];
  return left === right ? 0 : left > right ? 1 : -1;
}

/** A unit's base for formatting: "% of all CKB" reads as a percentage. */
const baseUnit = (unit: string) => (unit.startsWith('%') ? '%' : unit);

/** A figure in a sentence: cut for display, with its unit (a count's unit is its sentence's word). */
function stated(raw: string, unit: string, countWord = false): string {
  const shown = formatChartHead(raw, baseUnit(unit));
  return COUNT_WORDS.has(unit) && !countWord ? shown : withUnit(shown, unit);
}

const COUNT_WORDS = new Set([
  'transactions',
  'addresses',
  'activities',
  'cells',
  'blocks',
  'epochs',
  'holders',
]);

/** The value of `key` at a point, exactly. */
const at = (point: ChartPoint, key: string): Ratio =>
  ratio(point.values[key], `head at ${point.label}`);

/** The point dated a year (365 days) before `date`, if the series has it. */
function yearBefore(points: readonly ChartPoint[], date: string): ChartPoint | undefined {
  const target = new Date(Date.parse(`${date}T00:00:00Z`) - 365 * 86_400_000)
    .toISOString()
    .slice(0, 10);
  return points.find((point) => point.label === target);
}

/** "Up from 21.74% a year before", "Down from …", "Unchanged from …" — exact comparison. */
function fromYearBefore(now: Ratio, before: Ratio, beforeText: string): string {
  const c = compare(now, before);
  return `${c > 0 ? 'Up' : c < 0 ? 'Down' : 'Unchanged'} from ${beforeText} a year before`;
}

/**
 * The newest point and how it stands against the past (a daily flow, a noisy rate): the most (or
 * the least) since the last day that reached it, else on record; and the series' peak, from the
 * API, unless the newest point is that peak.
 */
function sinceSentence(
  model: ChartModel,
  words: { more: string; less: string; span: string }
): string {
  const { points, unit } = model;
  const key = model.series[0].key;
  const last = points.at(-1)!;
  const now = at(last, key);
  const previous = points.at(-2);
  const up = !previous || compare(now, at(previous, key)) >= 0;
  let since: string | null = null;
  for (let i = points.length - 2; i >= 0; i--) {
    const c = compare(at(points[i], key), now);
    if (up ? c >= 0 : c <= 0) {
      since = points[i].label;
      break;
    }
  }
  const head = `The ${up ? words.more : words.less}${words.span} ${since ? `since ${since}` : 'on record'}`;
  const peak = model.peak;
  const atPeak = up && since === null;
  return `${head}${peak && !atPeak ? `; the peak was ${stated(peak.raw, unit)} on ${peak.label}` : ''}.`;
}

/**
 * A level that moves slowly against its peak (the board's hash rate): how far under the peak it is,
 * as a share of the peak, and its change on the day before; a percentage says its peak and its
 * change in points instead of a share of a share.
 */
function peakSentence(model: ChartModel): string {
  const { points, unit } = model;
  const key = model.series[0].key;
  const last = points.at(-1)!;
  const previous = points.at(-2);
  const peak = model.peak;
  const change = previous
    ? exactDelta(previous.values[key], last.values[key], `${model.title} change`)
    : null;
  const percent = unit === '%';
  // "on the day before" where the point before is the calendar day before; else since its date
  const since =
    previous && dayBefore(previous.label, last.label)
      ? 'on the day before'
      : `since ${previous?.label}`;
  const changeText =
    change === null
      ? ''
      : change === '0'
        ? `unchanged ${since === 'on the day before' ? 'from the day before' : since}`
        : `${percent ? `${cutDecimal(change.slice(1), 2)} points` : stated(change.slice(1), unit, true)} ${change.startsWith('-') ? 'down' : 'up'} ${since}`;
  if (!peak) return `${changeText.charAt(0).toUpperCase()}${changeText.slice(1)}.`;
  if (percent)
    return `Its peak was ${stated(peak.raw, unit)} on ${peak.label}${changeText ? `; ${changeText}` : ''}.`;
  const [pn, pd] = ratio(peak.raw, 'head peak');
  const [ln, ld] = at(last, key);
  const gap = pn * ld - ln * pd;
  if (gap <= BigInt(0)) return `At its peak${changeText ? `, ${changeText}` : ''}.`;
  const fall = withUnit(cutRatio(gap * BigInt(100), pn * ld, 2), '%');
  return `${fall} under its peak of ${stated(peak.raw, unit, true)} on ${peak.label}${changeText ? `, and ${changeText}` : ''}.`;
}

/** Over a target on how many of the newest points (a consensus target, CH-12). */
function targetSentence(model: ChartModel, window: number, noun: string, peakToo: boolean): string {
  if (!model.target) throw new Error(`${model.title}: a target sentence with no target`);
  const key = model.series[0].key;
  const recent = model.points.slice(-window);
  const over = recent.filter((point) => compare(at(point, key), model.target!.ratio) > 0).length;
  const peak =
    peakToo && model.peak
      ? `; the peak was ${stated(model.peak.raw, model.unit)} on ${model.peak.label}`
      : '';
  return `Over the ${model.target.text} target ${noun === 'day' ? 'on' : 'in'} ${over} of the last ${formatCount(recent.length, noun)}${peak}.`;
}

/** Whether `earlier` is the calendar day before `later` (two dated labels). */
function dayBefore(earlier: string, later: string): boolean {
  const a = Date.parse(`${earlier}T00:00:00Z`);
  const b = Date.parse(`${later}T00:00:00Z`);
  return Number.isFinite(a) && Number.isFinite(b) && b - a === 86_400_000;
}

const capitalise = (text: string) => `${text.charAt(0).toUpperCase()}${text.slice(1)}`;

/** The keys of a point's two largest values among `keys`, largest first (exact; ties keep order). */
function largestTwo(point: ChartPoint, keys: readonly string[]): [string, string] {
  const sorted = [...keys].sort((a, b) => compare(at(point, b), at(point, a)));
  return [sorted[0], sorted[1]];
}

/** Activity kinds and knowledge kinds in a sentence's words. */
const KIND_WORDS: Record<string, string> = {
  scriptCall: 'script calls',
  transfer: 'CKB transfers',
  dao: 'Nervos DAO',
  token: 'tokens',
  object: 'objects',
  identity: 'identities',
};
const KNOWLEDGE_WORDS: Record<string, { name: string; one: boolean }> = {
  transfer: { name: 'plain CKB', one: true },
  dao: { name: 'the Nervos DAO', one: true },
  udt: { name: 'tokens', one: false },
  nftSpore: { name: 'objects', one: false },
  otherContracts: { name: 'other scripts', one: false },
};
const word = (table: Record<string, string>, key: string) => {
  const name = table[key];
  if (name === undefined) throw new Error(`chart head: no words for ${key}`);
  return name;
};

/** The figure of a dated line or stack at its newest point, as `model.latest` reads it. */
function latestFigure(model: ChartModel, unit = model.unit): HeadDraft['figure'] {
  if (!model.latest) return null;
  return {
    raw: model.latest.raw,
    display: formatChartHead(model.latest.raw, baseUnit(unit)),
    unit,
  };
}

function dated(
  model: ChartModel,
  eyebrow: string,
  sentence: (model: ChartModel) => string,
  unit?: string
): HeadDraft {
  return {
    eyebrow,
    at: model.latest?.label ?? null,
    figure: latestFigure(model, unit),
    sentence: model.points.length ? sentence(model) : 'No indexed observations yet.',
  };
}

const FLOW_COUNT = { more: 'most', less: 'fewest', span: ' in a day' };
const FLOW_CKB = { more: 'most', less: 'least', span: ' in a day' };
const RATE = { more: 'highest', less: 'lowest', span: '' };
const DURATION = { more: 'longest', less: 'shortest', span: '' };

type HeadOf = (definition: ChartDefinition, model: ChartModel) => HeadDraft;

/**
 * A point's whole as the route states it, which every share the page states of the point divides
 * by (the D1 hand-back, item 2; chart-model.ts): never the page's sum of its bands.
 */
function wholeAt(point: ChartPoint, context: string): string {
  if (point.whole === undefined) throw new Error(`${context}: no stated whole at ${point.label}`);
  return point.whole;
}

/** The share a ranking's five largest hold of the newest day's whole, from the API's bands. */
function fiveLargest(model: ChartModel, entity: string): string {
  const history = model.historicalShares?.[0];
  const last = history?.points.at(-1);
  if (!history || !last) return 'No indexed observations yet.';
  const keys = history.series.map((entry) => entry.key);
  const whole = wholeAt(last, `${model.title} whole`);
  const rest = keys.find((key) => key === 'rest' || key === 'others');
  const named = keys.filter((key) => key !== rest);
  const held = exactSum(
    named.map((key) => last.values[key]),
    `${model.title} named`
  );
  return `The ${named.length === 5 ? 'five' : String(named.length)} largest ${entity}s hold ${cutShare(held, whole, `${model.title} named`)} of occupied CKB.`;
}

function rankedHead(entity: string): HeadOf {
  return (_definition, model) => {
    const leader = model.rankedRows?.find((row) => !row.remainder);
    return {
      eyebrow: leader ? `${leader.label}, the largest ${entity}` : `the largest ${entity}`,
      at: model.latest?.label ?? null,
      figure: latestFigure(model),
      sentence: fiveLargest(model, entity),
    };
  };
}

const HEADS: Record<string, HeadOf> = {
  'transaction-count': (_, m) =>
    dated(m, 'confirmed in a day', (x) => sinceSentence(x, FLOW_COUNT)),
  'active-addresses': (_, m) => dated(m, 'active in a day', (x) => sinceSentence(x, FLOW_COUNT)),
  'ckb-volume': (_, m) => dated(m, 'moved in a day', (x) => sinceSentence(x, FLOW_CKB)),
  'activity-volume': (_, m) => dated(m, 'recorded in a day', (x) => sinceSentence(x, FLOW_COUNT)),
  'activity-type-breakdown': (_, m) => {
    const last = m.points.at(-1);
    if (!last) return dated(m, 'the largest kind in a day', () => '');
    const keys = m.series.map((entry) => entry.key);
    const [first, second] = largestTwo(last, keys);
    // the day's kind occurrences, the route's: kinds overlap, so they are no count of activities
    const whole = wholeAt(last, 'activities whole');
    return {
      eyebrow: `${word(KIND_WORDS, first)}, the largest kind in a day`,
      at: last.label,
      figure: {
        raw: last.values[first],
        display: formatChartHead(last.values[first], m.unit),
        unit: m.unit,
      },
      sentence: `${capitalise(word(KIND_WORDS, first))} were ${cutShare(last.values[first], whole, 'activities')} of the day's kind occurrences, ${word(KIND_WORDS, second)} ${cutShare(last.values[second], whole, 'activities')}.`,
    };
  },
  'cell-count': (_, m) => dated(m, 'live', peakSentence),
  'hodl-wave': (_, m) => {
    const last = m.points.at(-1);
    const oldest = m.series[0].key;
    const young = m.series.at(-1)!.key;
    const head = dated(m, 'unmoved for 3 years or more', () => '', '% of all CKB');
    if (!last) return head;
    const before = yearBefore(m.points, last.label);
    const moved = `${withUnit(cutDecimal(last.values[young], 2), '%')} of all CKB moved within the last month`;
    return {
      ...head,
      sentence: before
        ? `${fromYearBefore(at(last, oldest), at(before, oldest), withUnit(cutDecimal(before.values[oldest], 2), '%'))}; ${moved}.`
        : `${capitalise(moved)}.`,
    };
  },
  'total-supply': (_, m) => {
    const last = m.points.at(-1);
    const head = dated(m, 'circulating', () => '');
    if (!last) return head;
    // C, the day's total issuance, the route's: its bands do not sum to it
    const whole = wholeAt(last, 'supply whole');
    return {
      ...head,
      figure: {
        raw: last.values.circulating,
        display: formatChartHead(last.values.circulating, 'CKB'),
        unit: 'CKB',
      },
      sentence: `Of the ${stated(whole, 'CKB')} issued, ${cutShare(last.values.nervosdao, whole, 'supply')} is deposited in the Nervos DAO and ${cutShare(last.values.burnt, whole, 'supply')} is burnt.`,
    };
  },
  'total-deposit': (_, m) => dated(m, 'deposited in the Nervos DAO', peakSentence),
  'daily-deposit': (_, m) => dated(m, 'deposited in a day', (x) => sinceSentence(x, FLOW_CKB)),
  'nominal-apc': (_, m) => {
    const first = m.points[0];
    const last = m.points.at(-1);
    // the API's today alone, never a stand-in sample (CH-03)
    const today = m.today ?? null;
    return {
      eyebrow: 'the model today',
      at: today?.label ?? null,
      figure: today
        ? {
            raw: today.values.primary,
            display: cutDecimal(today.values.primary, 2),
            unit: '% a year',
          }
        : null,
      sentence:
        first && last
          ? `The model falls from ${withUnit(cutDecimal(first.values.primary, 2), '%')} at genesis to ${withUnit(cutDecimal(last.values.primary, 2), '%')} in year ${last.label}.`
          : 'No model samples yet.',
    };
  },
  'inflation-rate': (_, m) =>
    dated(m, 'nominal, trailing 365 days', (x) => {
      const last = x.points.at(-1)!;
      const before = yearBefore(x.points, last.label);
      const real = `real inflation, without the secondary issuance, is ${withUnit(cutDecimal(last.values.companion, 2), '%')}`;
      return before
        ? `${fromYearBefore(at(last, 'primary'), at(before, 'primary'), withUnit(cutDecimal(before.values.primary, 2), '%'))}; ${real}.`
        : `${capitalise(real)}.`;
    }),
  'secondary-issuance': (_, m) =>
    dated(m, 'issued since genesis', (x) => {
      const last = x.points.at(-1)!;
      const whole = wholeAt(last, 'issuance whole');
      const share = (key: string) => cutShare(last.values[key], whole, 'issuance');
      return `By destination, ${share('burnt')} is burnt, ${share('compensation')} paid to DAO depositors as compensation and ${share('mining')} to miners.`;
    }),
  'circulation-ratio': (_, m) =>
    dated(m, 'deposited in the Nervos DAO', peakSentence, '% of circulating CKB'),
  'knowledge-size': (_, m) => dated(m, 'occupied by live cells', peakSentence),
  'common-knowledge-composition': (_, m) => {
    const last = m.points.at(-1);
    const head = dated(m, 'the largest kind', () => '');
    if (!last) return head;
    const keys = m.series.map((entry) => entry.key);
    const [first, second] = largestTwo(last, keys);
    const whole = wholeAt(last, 'knowledge whole');
    const one = KNOWLEDGE_WORDS[first];
    const two = KNOWLEDGE_WORDS[second];
    if (!one || !two) throw new Error(`chart head: no words for ${first} or ${second}`);
    return {
      eyebrow: `${one.name}, the largest kind`,
      at: last.label,
      figure: {
        raw: last.values[first],
        display: formatChartHead(last.values[first], 'CKB'),
        unit: 'CKB',
      },
      sentence: `${capitalise(one.name)} ${one.one ? 'holds' : 'hold'} ${cutShare(last.values[first], whole, 'knowledge')} of occupied CKB, ${two.name} ${cutShare(last.values[second], whole, 'knowledge')}.`,
    };
  },
  'most-utilized-scripts': rankedHead('script'),
  'most-utilized-assets': rankedHead('asset'),
  'cell-size-distribution': (_, m) => {
    const lead = m.reading;
    // the live cells and the CKB they occupy, the route's wholes
    if (m.whole === undefined) throw new Error('cell size: no stated live cells');
    const counts = m.whole;
    const ckb = m.secondary?.points ?? [];
    const most = ckb.length
      ? ckb.reduce((best, point) =>
          compare(at(point, 'secondary'), at(best, 'secondary')) > 0 ? point : best
        )
      : null;
    return {
      eyebrow: lead ? `live cells of ${lead.label}` : 'live cells',
      at: null,
      figure: lead
        ? {
            raw: lead.values.primary,
            display: cutShare(lead.values.primary, counts, 'cells').replace(/%$/, ''),
            unit: '% of live cells',
            // a share's exact value is its two counts
            title: `${formatChartExact(lead.values.primary, 'cells')} of ${formatChartExact(counts, 'cells')} live cells`,
          }
        : null,
      sentence: most
        ? `The most occupied CKB is in cells of ${most.label}, ${cutShare(
            most.values.secondary,
            (() => {
              if (m.secondary?.whole === undefined)
                throw new Error('cell size: no stated occupied CKB');
              return m.secondary.whole;
            })(),
            'cell CKB'
          )} of it.`
        : 'No indexed observations yet.',
    };
  },
  'capacity-turnover-ratio': (_, m) =>
    dated(m, 'live capacity moved in the week', (x) => sinceSentence(x, RATE)),
  'address-cohort-retention': (_, m) => {
    const largest = m.points.length
      ? m.points.reduce((best, point) =>
          compare(at(point, 'primary'), at(best, 'primary')) > 0 ? point : best
        )
      : null;
    return {
      eyebrow: m.reading
        ? `occupied share of its balance, the ${m.reading.label} cohort`
        : 'occupied share of its balance',
      at: null,
      figure: m.reading
        ? {
            raw: m.reading.values.primary,
            display: cutDecimal(m.reading.values.primary, 2),
            unit: '%',
          }
        : null,
      sentence: largest
        ? `The ${largest.label} cohort keeps the largest share, ${withUnit(cutDecimal(largest.values.primary, 2), '%')}.`
        : 'No cohorts yet.',
    };
  },
  'hash-rate': (_, m) => dated(m, 'hash rate', peakSentence),
  difficulty: (_, m) => dated(m, "difficulty, the day's average", peakSentence),
  'uncle-rate': (_, m) => dated(m, 'uncle rate', (x) => targetSentence(x, 30, 'day', true)),
  'miner-address-distribution': (_, m) => {
    const rows = m.rankedRows ?? [];
    const window = m.window;
    const second = rows[1];
    return {
      eyebrow: rows[0] ? `${rows[0].label}, the largest miner` : 'the largest miner',
      at: window ? formatDateWindow(window.from, window.to) : null,
      figure: rows[0]
        ? { raw: rows[0].value, display: formatChartHead(rows[0].value, 'blocks'), unit: 'blocks' }
        : null,
      sentence:
        window && second
          ? `Of the window's ${formatCount(window.totalBlocks, 'block')}, the next largest miner, ${second.label}, found ${formatChartHead(second.value, 'blocks')}.`
          : 'One miner found every block in the window.',
    };
  },
  'average-block-time': (_, m) =>
    dated(m, "block time, the day's average", (x) => sinceSentence(x, DURATION)),
  'epoch-time-length': (_, m) => {
    const last = m.points.at(-1);
    return {
      eyebrow: last
        ? `epoch #${formatEpochLabel(last.label, 'epoch-time-length')} took`
        : 'an epoch took',
      at: null,
      figure: latestFigure(m),
      sentence: last ? targetSentence(m, 42, 'epoch', false) : 'No completed epochs yet.',
    };
  },
  'block-time-distribution': (_, m) => {
    const mode = m.reading;
    return {
      eyebrow: 'the most common interval',
      at: 'last 42 epochs',
      figure: mode ? { raw: mode.label, display: mode.label, unit: 's' } : null,
      sentence: mode
        ? `${withUnit(cutDecimal(mode.values.primary, 2), '%')} of blocks came ${mode.label} s after their parent, more than at any other tenth of a second.`
        : 'No blocks in the window yet.',
    };
  },
  'epoch-time-distribution': (_, m) => {
    const mode = m.reading;
    const taken = m.points.filter(
      (point) => compare(at(point, 'primary'), [BigInt(0), BigInt(1)]) > 0
    );
    return {
      eyebrow: 'the most common duration',
      at: null,
      figure: mode ? { raw: mode.label, display: mode.label, unit: 'h' } : null,
      sentence: taken.length
        ? `The shortest epoch took ${taken[0].label} h and the longest ${taken.at(-1)!.label} h.`
        : 'No completed epochs yet.',
    };
  },
};

/**
 * The head of a chart page and of its index card: one for both (CH-07). A figure's title is its
 * exact value with its unit, every digit (a bucket's figure is its label, a number too): what is
 * not a number fails here, with the model, and the page says its data is invalid.
 */
export function chartHead(definition: ChartDefinition, model: ChartModel): ChartHead {
  const draft = HEADS[definition.slug];
  if (!draft) throw new Error(`${definition.slug}: no chart head`);
  const head = draft(definition, model);
  const figure = head.figure;
  return {
    ...head,
    figure: figure && {
      ...figure,
      title:
        figure.title ??
        `${formatChartExact(figure.raw, baseUnit(figure.unit))}${figure.unit.startsWith('%') ? '' : ' '}${figure.unit}`,
    },
  };
}

/**
 * What a chart page writes over its axis where outliers leave it (rule 6): the one point above the
 * scale by its label and exact value, or how many and the largest. Each value is cut for display;
 * on a chart of two series the series is named.
 */
export function capNote(
  model: ChartModel,
  points: readonly ChartPoint[],
  indexes: readonly number[],
  label: (label: string) => string,
  noun: string
): string {
  const valueAt = (point: ChartPoint) => {
    // a stack is above the scale by its whole, the day's, as the route states it
    if (model.kind === 'stacked') {
      const whole = wholeAt(point, `${model.title} cap`);
      return { ratio: ratio(whole, 'cap whole'), text: stated(whole, model.unit, true) };
    }
    const key = model.series.reduce((best, entry) =>
      compare(at(point, entry.key), at(point, best.key)) > 0 ? entry : best
    );
    const named = model.series.length > 1 ? `${key.label.toLowerCase()} ` : '';
    return {
      ratio: at(point, key.key),
      text: `${named}${stated(point.values[key.key], model.unit, true)}`,
    };
  };
  const shown = indexes.map((index) => ({ point: points[index], ...valueAt(points[index]) }));
  const largest = shown.reduce((best, entry) =>
    compare(entry.ratio, best.ratio) > 0 ? entry : best
  );
  const named = `${label(largest.point.label)} · ${largest.text}`;
  return shown.length === 1
    ? `${named}, above this scale`
    : `${formatCount(shown.length, noun)} above this scale; the largest, ${named}`;
}

/**
 * How a chart is drawn until its reader changes it (the chart-hero board, CH-08 and rule 6; the D1
 * hand-back, item 3), for its page and its index card alike: the catalogue's log scale where a line
 * spans orders of magnitude and every value is above zero, else linear, its outliers left off the
 * axis and named over the plot. `points` and `series` are the ones drawn (a range, a card's);
 * `scale` is the reader's choice where the page offers one.
 */
export function chartDrawing(
  definition: ChartDefinition,
  model: ChartModel,
  points: readonly ChartPoint[],
  series: ChartModel['series'],
  scale?: 'linear' | 'log'
): {
  supportsLog: boolean;
  yScale: 'linear' | 'log';
  axisCap: { value: number; note: string } | undefined;
} {
  const supportsLog =
    model.kind === 'line' &&
    model.points.length > 0 &&
    model.points.every((point) =>
      model.series.every(
        (entry) =>
          exactSignedDecimalRatio(point.values[entry.key], `${definition.slug} log scale`)[0] >
          BigInt(0)
      )
    );
  const chosen = scale ?? (definition.logScale ? 'log' : 'linear');
  const yScale = supportsLog && chosen === 'log' ? 'log' : 'linear';
  const style = axisStyle(definition);
  // Rule 6: an outlier does not set a linear axis; its value is named over the plot.
  const outliers =
    yScale === 'linear' && !model.yDomain && (style === 'date' || style === 'epoch')
      ? axisOutliers(points, series, model.kind)
      : null;
  return {
    supportsLog,
    yScale,
    axisCap: outliers
      ? {
          value: outliers.cap,
          note: capNote(
            model,
            points,
            outliers.indexes,
            (label) => (style === 'epoch' ? `#${formatEpochLabel(label, definition.slug)}` : label),
            style === 'epoch' ? 'epoch' : 'day'
          ),
        }
      : undefined,
  };
}
