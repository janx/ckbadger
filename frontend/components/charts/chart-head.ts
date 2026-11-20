import type { ChartPoint } from '@/components/ui/chart';
import { ACTIVITY_COUNT_WORDS, type ChartDefinition } from '@/components/charts/chart-catalogue';
import {
  axisOutliers,
  axisStyle,
  cutDecimal,
  cutRatio,
  cutShare,
  exactDelta,
  formatChartExact,
  formatChartHead,
  formatEpochLabel,
  type ChartModel,
} from '@/components/charts/chart-model';
import { exactSignedDecimalRatio, formatExactDecimal } from '@/lib/exact-integer';
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
 *
 * A percentage series (unit `%`) is a share its route cuts at its precision: the route sends the
 * share, not its operands. Its sentence does no arithmetic between two of its values; it states the
 * figures as shown and says up, down or the same by comparing those figures (`shown`), claiming
 * nothing of a digit it does not show (lane D1's third round, S9).
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
  'kind counts',
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

/**
 * A percentage as the page shows it, cut at two decimals, as a ratio to compare (S9): what a
 * sentence compares of a share series is the figures it states.
 */
function shown(value: string): Ratio {
  const [n, d] = ratio(value, 'shown percentage');
  // BigInt division cuts toward zero, as cutDecimal cuts the digits it writes
  return [(n * BigInt(100)) / d, BigInt(100)];
}

/** A percentage as a sentence writes it: cut at two decimals, with its sign. */
const percentText = (value: string) => withUnit(cutDecimal(value, 2), '%');

/**
 * "Up from 21.74% a year before", "Down from …", "The same 22.22% as a year before": a share series
 * compared by the figures shown (N3: it said "Unchanged" of two different shares).
 */
function fromYearBefore(now: string, before: string): string {
  const c = compare(shown(now), shown(before));
  const beforeText = percentText(before);
  return c === 0
    ? `The same ${beforeText} as a year before`
    : `${c > 0 ? 'Up' : 'Down'} from ${beforeText} a year before`;
}

/** The point before as a sentence names it: the day before, or its date where days are missing. */
const beforeWords = (previous: ChartPoint, last: ChartPoint) =>
  dayBefore(previous.label, last.label) ? 'the day before' : `on ${previous.label}`;

/**
 * The newest point and how it stands against the past (a daily flow, a noisy rate). Against the
 * point before: the same (S6: an equal day is its own case, never "the most since" it); else the
 * most (or the least) since the last day that reached it, else on record. Then the series' peak,
 * from the API, unless the newest point stands at it. One point is the first on record. A
 * percentage series compares the figures it shows (S9).
 */
function sinceSentence(
  model: ChartModel,
  words: { more: string; less: string; span: string }
): string {
  const { points, unit } = model;
  const key = model.series[0].key;
  const percent = unit === '%';
  const value = (point: ChartPoint): Ratio => (percent ? shown(point.values[key]) : at(point, key));
  const last = points.at(-1)!;
  const now = value(last);
  const previous = points.at(-2);
  if (!previous) return 'The first day on record.';
  // a series whose every day equals the newest is no record, and a day of nothing is never "the
  // most" (lane exactness, part A2, S3: two days of 0 read "The same as the day before, the most in a day on record.")
  if (points.every((point) => compare(value(point), now) === 0)) return sameEveryDay(model);
  // the newest point stands at the record where no point of the series is above it; the peak, the
  // route's, is named only where it does not
  const atRecord = points.every((point) => compare(value(point), now) <= 0);
  const peak = model.peak;
  const peakText =
    atRecord || !peak ? '' : `; the peak was ${stated(peak.raw, unit)} on ${peak.label}`;
  const step = compare(now, value(previous));
  if (step === 0) {
    const same = `The same${percent ? ` ${stated(last.values[key], unit)}` : ''} as ${beforeWords(previous, last)}`;
    return atRecord ? `${same}, the ${words.more}${words.span} on record.` : `${same}${peakText}.`;
  }
  const up = step > 0;
  let since: string | null = null;
  for (let i = points.length - 2; i >= 0; i--) {
    const c = compare(value(points[i]), now);
    if (up ? c >= 0 : c <= 0) {
      since = points[i].label;
      break;
    }
  }
  return `The ${up ? words.more : words.less}${words.span} ${since ? `since ${since}` : 'on record'}${peakText}.`;
}

/**
 * A series whose every day equals the newest: "The same every day on record.", a percentage with its
 * figure (lane exactness, part A2, S3).
 */
function sameEveryDay(model: ChartModel): string {
  const last = model.points.at(-1)!;
  return model.unit === '%'
    ? `The same ${stated(last.values[model.series[0].key], model.unit)} every day on record.`
    : 'The same every day on record.';
}

/**
 * A level that moves slowly against its peak (the board's hash rate): how far under the peak it is,
 * as a share of the peak, and its change on the day before. A percentage states its peak, its
 * figure and the day before's, and says up, down or the same by the figures it shows (S9: never a
 * difference of two cut shares, "0.00 points up"). A peak under the newest point is the route's
 * fault, and fails (N1).
 */
function peakSentence(model: ChartModel): string {
  const { points, unit } = model;
  const key = model.series[0].key;
  const last = points.at(-1)!;
  const previous = points.at(-2);
  const routePeak = model.peak;
  if (routePeak && compare(ratio(routePeak.raw, 'head peak'), at(last, key)) < 0)
    throw new Error(
      `${model.title}: peak ${routePeak.raw} on ${routePeak.label} is under the newest point, ${last.values[key]} on ${last.label}`
    );
  const percent = unit === '%';
  // a flat level is the same every day; a peak of zero is no peak (lane exactness, part A2, S3: two days of 0 read "At
  // its peak, unchanged from the day before.")
  const figure = (point: ChartPoint): Ratio =>
    percent ? shown(point.values[key]) : at(point, key);
  if (previous && points.every((point) => compare(figure(point), figure(last)) === 0))
    return sameEveryDay(model);
  const peak =
    model.peak && ratio(model.peak.raw, 'head peak')[0] !== BigInt(0) ? model.peak : null;
  if (percent) {
    const peakText = peak ? `Its peak was ${stated(peak.raw, unit)} on ${peak.label}` : '';
    if (!previous) return peak ? `${peakText}.` : 'The first day on record.';
    const c = compare(shown(last.values[key]), shown(previous.values[key]));
    const now = stated(last.values[key], unit);
    const before = beforeWords(previous, last);
    const change =
      c === 0
        ? `the same ${now} as ${before}`
        : `${now}, ${c > 0 ? 'up' : 'down'} from ${stated(previous.values[key], unit)} ${before}`;
    return peak ? `${peakText}; ${change}.` : `${capitalise(change)}.`;
  }
  const change = previous
    ? exactDelta(previous.values[key], last.values[key], `${model.title} change`)
    : null;
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
        : `${stated(change.slice(1), unit, true)} ${change.startsWith('-') ? 'down' : 'up'} ${since}`;
  if (!peak) return changeText ? `${capitalise(changeText)}.` : 'The first day on record.';
  const [pn, pd] = ratio(peak.raw, 'head peak');
  const [ln, ld] = at(last, key);
  const gap = pn * ld - ln * pd;
  if (gap === BigInt(0)) return `At its peak${changeText ? `, ${changeText}` : ''}.`;
  const fall = withUnit(cutRatio(gap * BigInt(100), pn * ld, 2), '%');
  return `${fall} under its peak of ${stated(peak.raw, unit, true)} on ${peak.label}${changeText ? `, and ${changeText}` : ''}.`;
}

/**
 * Over a target on how many of the newest points (a consensus target, CH-12), counted exactly on
 * the route's integers: a duration by its whole ms against the target's (M1: an epoch 17 s over 4 h
 * is over it); the uncle rate by each day's uncles and blocks against the target's fraction (lane
 * exactness, part A, A2: a day shown at 2.50 % can be over one in forty; it compared the shown
 * share). A target with neither is the route's fault.
 */
function targetSentence(model: ChartModel, window: number, noun: string, peakToo: boolean): string {
  const target = model.target;
  if (!target) throw new Error(`${model.title}: a target sentence with no target`);
  const recent = model.points.slice(-window);
  const isOver = (point: ChartPoint): boolean => {
    if (target.ms !== undefined) {
      return BigInt(msAt(point, model.title)) > target.ms;
    }
    if (target.fraction !== undefined) {
      const { uncles, blocks } = point.values;
      if (uncles === undefined || blocks === undefined)
        throw new Error(`${model.title}: no uncles or blocks at ${point.label}`);
      const [numerator, denominator] = target.fraction;
      return BigInt(uncles) * denominator > BigInt(blocks) * numerator;
    }
    throw new Error(`${model.title}: a target compared neither in ms nor in uncles and blocks`);
  };
  const over = recent.filter(isOver).length;
  const peak =
    peakToo && model.peak
      ? `; the peak was ${stated(model.peak.raw, model.unit)} on ${model.peak.label}`
      : '';
  return `Over the ${target.text} target ${noun === 'day' ? 'on' : 'in'} ${over} of the last ${formatCount(recent.length, noun)}${peak}.`;
}

/** Whether `earlier` is the calendar day before `later` (two dated labels). */
function dayBefore(earlier: string, later: string): boolean {
  const a = Date.parse(`${earlier}T00:00:00Z`);
  const b = Date.parse(`${later}T00:00:00Z`);
  return Number.isFinite(a) && Number.isFinite(b) && b - a === 86_400_000;
}

const capitalise = (text: string) => `${text.charAt(0).toUpperCase()}${text.slice(1)}`;

/**
 * The keys of a point's largest value among `keys`, every one tied at it, in `keys`' order, and the
 * largest after them (exact; lane exactness, part A2, N9: a tie at the top named one kind "the largest").
 */
function largestKinds(
  point: ChartPoint,
  keys: readonly string[]
): { top: string[]; next: string | null } {
  const sorted = [...keys].sort((a, b) => compare(at(point, b), at(point, a)));
  const top = sorted.filter((key) => compare(at(point, key), at(point, sorted[0])) === 0);
  return { top, next: sorted[top.length] ?? null };
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

/**
 * The share a ranking's five largest hold of the newest day's whole: the ranks drawn as their own
 * bands, their total the route's (`drawnTotal`, M2: the page summed them).
 */
function fiveLargest(model: ChartModel, entity: string): string {
  const history = model.historicalShares?.[0];
  const last = history?.points.at(-1);
  if (!history || !last) return 'No indexed observations yet.';
  const keys = history.series.map((entry) => entry.key);
  const whole = wholeAt(last, `${model.title} whole`);
  const rest = keys.find((key) => key === 'rest' || key === 'others');
  const named = keys.filter((key) => key !== rest);
  if (history.drawnTotal === undefined)
    throw new Error(`${model.title}: no drawn total at ${last.label}`);
  const held = history.drawnTotal;
  if (exactSignedDecimalRatio(whole, `${model.title} whole`)[0] === BigInt(0))
    return `No occupied CKB on ${last.label}.`;
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

/** The one miner of a window, who found every block of it, or the route is broken (S7). */
function oneMiner(blocks: string, totalBlocks: number): string {
  if (blocks !== String(totalBlocks))
    throw new Error(`miners: one miner of ${blocks} blocks in a window of ${totalBlocks}`);
  return 'One miner found every block in the window.';
}

/** A duration's whole ms at a point, as the route sends it (M1). */
function msAt(point: ChartPoint, context: string): string {
  const ms = point.values.ms;
  if (ms === undefined) throw new Error(`${context}: no ms at ${point.label}`);
  return ms;
}

/** Labels in a list: "1.3 s", "1.3 s and 1.4 s", "1.3 s, 1.4 s and 1.5 s". */
/** A tie at a histogram's top is listed up to this many, else counted (lane exactness, part A2, S4: it listed 500). */
const TIES_LISTED = 3;

function listed(items: readonly string[]): string {
  return items.length < 2
    ? (items[0] ?? '')
    : `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`;
}

const HEADS: Record<string, HeadOf> = {
  'transaction-count': (_, m) =>
    dated(m, 'confirmed in a day', (x) => sinceSentence(x, FLOW_COUNT)),
  'active-addresses': (_, m) => dated(m, 'active in a day', (x) => sinceSentence(x, FLOW_COUNT)),
  'ckb-volume': (_, m) => dated(m, 'moved in a day', (x) => sinceSentence(x, FLOW_CKB)),
  // counted, not recorded: the series is the day's kind counts (ACTIVITY_COUNT_WORDS)
  'activity-volume': (_, m) => dated(m, 'counted in a day', (x) => sinceSentence(x, FLOW_COUNT)),
  'activity-type-breakdown': (_, m) => {
    const last = m.points.at(-1);
    if (!last) return dated(m, 'the largest kind in a day', () => '');
    const keys = m.series.map((entry) => entry.key);
    // the day's kind occurrences, the route's: kinds overlap, so they are no count of activities
    const whole = wholeAt(last, 'activities whole');
    // a whole of zero (a day of cellbases only): no largest kind and no share, words for the
    // empty day, as the band ends write a dash (S8: one rule)
    if (ratio(whole, 'activities whole')[0] === BigInt(0))
      return {
        eyebrow: 'the largest kind in a day',
        at: last.label,
        figure: null,
        sentence: 'No activity of any kind on the day.',
      };
    const { top, next } = largestKinds(last, keys);
    const first = top[0];
    const names = listed(top.map((key) => word(KIND_WORDS, key)));
    const tied = top.length > 1;
    return {
      eyebrow: `${names}, the largest kind${tied ? 's' : ''} in a day`,
      at: last.label,
      figure: {
        raw: last.values[first],
        display: formatChartHead(last.values[first], m.unit),
        unit: m.unit,
      },
      sentence: `${capitalise(names)} were ${cutShare(last.values[first], whole, 'activities')}${tied ? ' each' : ''} of the day's ${ACTIVITY_COUNT_WORDS.noun}${next ? `, ${word(KIND_WORDS, next)} ${cutShare(last.values[next], whole, 'activities')}` : ''}.`,
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
    const moved = `${percentText(last.values[young])} of all CKB moved within the last month`;
    return {
      ...head,
      sentence: before
        ? `${fromYearBefore(last.values[oldest], before.values[oldest])}; ${moved}.`
        : `${capitalise(moved)}.`,
    };
  },
  // Circulating is the protocol circulation, C − genesis burnt − S, the route's (lane D1, second
  // follow-up): the site's one circulating amount. The sentence states C once, what circulates, of
  // that what is in the DAO, and what is burnt, each share cut from the route's figures.
  'total-supply': (_, m) => {
    const last = m.points.at(-1);
    const head = dated(m, 'circulating', () => '');
    if (!last || !m.latest) return head;
    const whole = wholeAt(last, 'supply whole');
    const circulating = m.latest.raw;
    return {
      ...head,
      sentence: `Of the ${stated(whole, 'CKB')} issued, ${cutShare(circulating, whole, 'supply')} circulates, ${cutShare(last.values.daoDeposited, circulating, 'supply')} of it deposited in the Nervos DAO and ${cutShare(last.values.daoWithdrawing, circulating, 'supply')} withdrawing from it; ${cutShare(last.values.burnt, whole, 'supply')} is burnt.`,
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
        ? `${fromYearBefore(last.values.primary, before.values.primary)}; ${real}.`
        : `${capitalise(real)}.`;
    }),
  'secondary-issuance': (_, m) =>
    dated(m, 'issued since genesis', (x) => {
      const last = x.points.at(-1)!;
      const whole = wholeAt(last, 'issuance whole');
      const share = (key: string) => cutShare(last.values[key], whole, 'issuance');
      // compensation accrues to depositors whether or not they have claimed it (S2: "paid" said
      // the unclaimed part was paid; the Supply chart names that part "not yet claimed")
      return `By destination, ${share('burnt')} is burnt, ${share('compensation')} accrued to DAO depositors as compensation, claimed or unclaimed, and ${share('mining')} went to miners.`;
    }),
  'circulation-ratio': (_, m) =>
    dated(m, 'deposited in the Nervos DAO', peakSentence, '% of circulating CKB'),
  'knowledge-size': (_, m) => dated(m, 'occupied by live cells', peakSentence),
  'common-knowledge-composition': (_, m) => {
    const last = m.points.at(-1);
    const head = dated(m, 'the largest kind', () => '');
    if (!last) return head;
    const keys = m.series.map((entry) => entry.key);
    const { top, next } = largestKinds(last, keys);
    const first = top[0];
    const whole = wholeAt(last, 'knowledge whole');
    const words = (key: string) => {
      const found = KNOWLEDGE_WORDS[key];
      if (!found) throw new Error(`chart head: no words for ${key}`);
      return found;
    };
    const names = listed(top.map((key) => words(key).name));
    const tied = top.length > 1;
    return {
      eyebrow: `${names}, the largest kind${tied ? 's' : ''}`,
      at: last.label,
      figure: {
        raw: last.values[first],
        display: formatChartHead(last.values[first], 'CKB'),
        unit: 'CKB',
      },
      sentence: `${capitalise(names)} ${!tied && words(first).one ? 'holds' : 'hold'} ${cutShare(last.values[first], whole, 'knowledge')}${tied ? ' each' : ''} of occupied CKB${next ? `, ${words(next).name} ${cutShare(last.values[next], whole, 'knowledge')}` : ''}.`,
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
    // the complete cohorts: the newest one, the snapshot's month, is still filling (N6)
    const complete = m.points.slice(0, -1);
    const largest = complete.length
      ? complete.reduce((best, point) =>
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
        ? `The ${largest.label} cohort keeps the largest share, ${percentText(largest.values.primary)}.`
        : m.points.length
          ? 'No complete cohort yet.'
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
      sentence: !rows.length
        ? 'No blocks in the window.'
        : second
          ? `Of the window's ${formatCount(window!.totalBlocks, 'block')}, the next largest miner, ${second.label}, found ${formatChartHead(second.value, 'blocks')}.`
          : oneMiner(rows[0].value, window!.totalBlocks),
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
      // the figure cut from the epoch's whole ms; its title the ms, every digit (M1)
      figure: last
        ? {
            ...latestFigure(m)!,
            title: `${formatExactDecimal(msAt(last, m.title), `${m.title} ms`)} ms`,
          }
        : null,
      sentence: last ? targetSentence(m, 42, 'epoch', false) : 'No completed epochs yet.',
    };
  },
  'block-time-distribution': (_, m) => {
    const mode = m.reading;
    // the window the route counted: its epochs, every interval, those past the drawn 50 s (S4)
    const window = m.intervalWindow;
    if (!window) throw new Error('block intervals: no window from the route');
    // every bucket tied with the most common one is named (S5), a long tie counted (lane exactness, part A2, S4)
    const ties = m.readingTies ?? [];
    const beyond = window.beyondRange
      ? `; ${formatCount(window.beyondRange, 'block')} came more than 50 s after it`
      : '';
    const tied =
      ties.length === 0
        ? ', more than at any other tenth of a second'
        : ties.length <= TIES_LISTED
          ? `, as many as ${listed(ties.map((tie) => `${tie} s`))} after it, more than at any other tenth of a second`
          : `, as many as at ${ties.length} other tenths of a second, more than at any of the rest`;
    return {
      eyebrow: 'the most common interval',
      // no window of 0 epochs on a chain in its first (lane exactness, part A2, N4)
      at: window.epochs ? `last ${formatCount(window.epochs, 'epoch')}` : null,
      figure: mode ? { raw: mode.label, display: mode.label, unit: 's' } : null,
      sentence: mode
        ? `${percentText(mode.values.primary)} of blocks came ${mode.label} s after their parent${tied}${beyond}.`
        : window.intervals
          ? // every interval past the drawn range: no most common one under it, no figure (S4)
            `No interval under 50 s; ${formatCount(window.beyondRange, 'block')} came more than 50 s after their parent.`
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
        ? `The shortest epoch took ${taken[0].label} h and the longest ${taken.at(-1)!.label} h${
            // every duration tied with the most common one is named (S5), a long tie counted
            // (lane exactness, part A2, S4)
            mode && m.readingTies?.length
              ? m.readingTies.length <= TIES_LISTED
                ? `; as many took ${listed(m.readingTies.map((tie) => `${tie} h`))} as ${mode.label} h`
                : `; as many took each of ${m.readingTies.length} other durations as ${mode.label} h`
              : ''
          }.`
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
    // what the series counts, said once where it needs saying (ACTIVITY_COUNT_WORDS.note)
    // said where a figure counts (an empty day has none: S8)
    sentence:
      definition.countNote && model.points.length && figure
        ? `${head.sentence} ${definition.countNote}`
        : head.sentence,
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
      return {
        ratio: ratio(whole, 'cap whole'),
        text: stated(whole, model.wholeUnit ?? model.unit, true),
      };
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
