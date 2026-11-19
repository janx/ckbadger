import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';
import { chartHead } from '@/components/charts/chart-head';

/**
 * Lane D1, third round (the reviewers' S5–S9, N1, N3, N6 and T's NOTE 7): every sentence form of
 * the chart heads, on the edge series the reviewers raised — one point, an empty range, a tie, a
 * fall to zero, an equal day — pinned word for word on points this file states. A count is glued
 * to its noun by a no-break space (formatCount), written here as \u00a0.
 */

const def = (slug: string) => {
  const found = CHART_BY_SLUG.get(slug);
  if (!found) throw new Error(`no chart ${slug}`);
  return found;
};
const head = (slug: string, payload: unknown) =>
  chartHead(def(slug), chartModel(def(slug), payload as never));

/** A daily line: the values on consecutive days ending 2026-11-18, its peak the first largest. */
function daily(values: readonly string[], extra: Record<string, unknown> = {}) {
  const end = Date.parse('2026-11-18T00:00:00Z');
  const data = values.map((value, i) => ({
    date: new Date(end - (values.length - 1 - i) * 86_400_000).toISOString().slice(0, 10),
    value,
  }));
  let peak = data[0];
  for (const point of data) if (Number(point.value) > Number(peak?.value)) peak = point;
  return { title: 't', yAxisLabel: 't', data, peak: peak ?? null, peak2: null, ...extra };
}

describe('a flow’s day against the past (sinceSentence)', () => {
  const s = (values: string[]) => head('transaction-count', daily(values)).sentence;

  it('says the first day of a series is the first on record', () => {
    expect(s(['5'])).toBe('The first day on record.');
  });
  it('says nothing was recorded on an empty range', () => {
    expect(head('transaction-count', daily([])).sentence).toBe('No indexed observations yet.');
  });
  it('calls a fall to zero the fewest on record', () => {
    expect(s(['5', '9', '0'])).toBe('The fewest in a day on record; the peak was 9 on 2026-11-17.');
  });
  it('says a day equal to the day before is the same, not the most since it (S6)', () => {
    expect(s(['9', '4', '4'])).toBe('The same as the day before; the peak was 9 on 2026-11-16.');
    expect(s(['4', '9', '9'])).toBe('The same as the day before, the most in a day on record.');
  });
  // lane exactness, part A2 (S3): a series whose every day equals the newest has its own words, and a day of nothing
  // is never "the most … on record" (it read "The same as the day before, the most in a day on
  // record." of two days of 0)
  it('says a flat series is the same every day, and never calls a zero the most', () => {
    expect(s(['4', '4', '4'])).toBe('The same every day on record.');
    expect(s(['0', '0'])).toBe('The same every day on record.');
    expect(head('daily-deposit', daily(['0', '0'])).sentence).toBe('The same every day on record.');
    const payload = daily(['0', '0']);
    const turnover = head('capacity-turnover-ratio', {
      ...payload,
      data: payload.data.map((point) => ({ ...point, value2: point.value })),
      peak2: payload.peak,
    });
    expect(turnover.sentence).toBe('The same 0.00% every day on record.');
  });
  it('names a tie with an earlier day as the most since it, no peak beside it', () => {
    expect(s(['9', '4', '9'])).toBe('The most in a day since 2026-11-16.');
  });
  it('reads a percentage series by the figures it shows (S9: turnover)', () => {
    // 2.171181 and 2.17 shown alike; the day before 2.179 is 2.17 too: the same shown figure
    // the weekly turnover is the page's series (value2), its peak the route's peak2
    const payload = daily(['2.5', '2.179', '2.171181']);
    const turnover = head('capacity-turnover-ratio', {
      ...payload,
      data: payload.data.map((point) => ({ ...point, value2: point.value })),
      peak2: payload.peak,
    });
    expect(turnover.sentence).toBe(
      'The same 2.17% as the day before; the peak was 2.50% on 2026-11-16.'
    );
  });
});

describe('a level against its peak (peakSentence)', () => {
  const deposit = (values: string[], peak?: { date: string; value: string }) => {
    const payload = daily(values);
    return head('total-deposit', {
      ...payload,
      data: payload.data.map((point) => ({ ...point, value2: '1' })),
      ...(peak ? { peak } : {}),
    }).sentence;
  };

  it('stands at its peak on one point', () => {
    expect(deposit(['100'])).toBe('At its peak.');
  });
  // lane exactness, part A2 (S3): a flat level is the same every day; a peak of zero is never "at its peak" (two days of
  // 0 read "At its peak, unchanged from the day before.")
  it('says a flat level is the same every day, and a zero is no peak', () => {
    expect(deposit(['0', '0'])).toBe('The same every day on record.');
    expect(deposit(['150', '150'])).toBe('The same every day on record.');
    expect(deposit(['0'])).toBe('The first day on record.');
    expect(head('circulation-ratio', daily(['0', '0'])).sentence).toBe(
      'The same 0.00% every day on record.'
    );
    expect(head('circulation-ratio', daily(['0'])).sentence).toBe('The first day on record.');
  });
  it('says an equal day is unchanged', () => {
    expect(deposit(['200', '150', '150'])).toBe(
      '25.00% under its peak of 200 CKB on 2026-11-16, and unchanged from the day before.'
    );
  });
  it('says a fall to zero in full', () => {
    expect(deposit(['200', '0'])).toBe(
      '100.00% under its peak of 200 CKB on 2026-11-17, and 200 CKB down on the day before.'
    );
  });
  it('refuses a peak under the newest point, the route’s fault (N1)', () => {
    expect(() => deposit(['100', '150'], { date: '2026-11-17', value: '100' })).toThrow(
      'peak 100 on 2026-11-17 is under the newest point, 150 on 2026-11-18'
    );
  });
});

describe('a share series states its figures, never a difference of two cut ones (S9, N3)', () => {
  const ratio = (values: string[]) => head('circulation-ratio', daily(values)).sentence;

  it('says the same shown figure as the same, never "0.00 points up"', () => {
    expect(ratio(['35.8909', '16.3712', '16.3747'])).toBe(
      'Its peak was 35.89% on 2026-11-16; the same 16.37% as the day before.'
    );
  });
  it('states the day before’s figure and the way it went', () => {
    expect(ratio(['35.8909', '16.3844', '16.3744'])).toBe(
      'Its peak was 35.89% on 2026-11-16; 16.37%, down from 16.38% the day before.'
    );
    expect(ratio(['35.8909', '16.3612', '16.3747'])).toBe(
      'Its peak was 35.89% on 2026-11-16; 16.37%, up from 16.36% the day before.'
    );
  });
  it('says its peak alone on one point', () => {
    expect(ratio(['16.3744'])).toBe('Its peak was 16.37% on 2026-11-18.');
  });
  it('compares a year before by the figures shown', () => {
    const values = Array.from({ length: 366 }, (_, i) => (i === 0 ? '5.4852' : '5.4859'));
    const payload = daily(values);
    const inflation = head('inflation-rate', {
      ...payload,
      data: payload.data.map((point) => ({ ...point, value2: '3.3446' })),
    });
    expect(inflation.sentence).toBe(
      'The same 5.48% as a year before; real inflation, without the secondary issuance, is 3.34%.'
    );
  });
});

describe('over a target, counted exactly (M1)', () => {
  it('counts an epoch 17 s over the 4 h target, and not one of exactly 4 h', () => {
    const epochs = head('epoch-time-length', {
      title: 'Epoch Time Length',
      yAxisLabel: 'Milliseconds',
      y2AxisLabel: 'Blocks',
      data: ['14400000', '14417000', '14418000'].map((value, i) => ({
        date: String(i),
        value,
        value2: '1800',
      })),
      peak: { date: '2', value: '14418000' },
      peak2: { date: '0', value: '1800' },
      target: { seconds: 14_400, hours: '4.00' },
    });
    // 14,418,000 ms is 4.005 h: cut, 4.00 h (half up it read 4.01)
    expect(epochs.figure?.display).toBe('4.00');
    expect(epochs.figure?.title).toBe('14,418,000 ms');
    expect(epochs.sentence).toBe('Over the 4.00 h target in 2 of the last 3\u00a0epochs.');
  });
  // Re-pinned for lane exactness, part A (A2): the route sends each day's uncles and blocks, and the count is on
  // them, uncles × 40 > blocks × 1 for a target of one in forty: 40,001 uncles of 1,600,000 blocks
  // is 2.5000625 %, cut to the share the route sends (0.025000) and shown 2.50 %, and is over the
  // target (it compared the shown figure and said "0 of the last 1 day").
  it('counts the uncle rate over its target on the day’s uncles and blocks', () => {
    const uncles = head('uncle-rate', {
      ...daily(['0.025000']),
      orphanRateTarget: { numerator: 1, denominator: 40 },
      counts: [{ date: '2026-11-18', uncles: 40_001, blocks: 1_600_000 }],
    });
    expect(uncles.sentence).toBe(
      'Over the 2.50% target on 1 of the last 1\u00a0day; the peak was 2.50% on 2026-11-18.'
    );
  });
  // the test review's NOTE-1 (lane exactness, part A2): 40,000 uncles of 1,600,000 blocks is one in forty exactly, at
  // the target and not over it (`>=` on the integers or on the cut share would count it)
  it('does not count a day exactly at the uncle-rate target', () => {
    const uncles = head('uncle-rate', {
      ...daily(['0.025000']),
      orphanRateTarget: { numerator: 1, denominator: 40 },
      counts: [{ date: '2026-11-18', uncles: 40_000, blocks: 1_600_000 }],
    });
    expect(uncles.sentence).toBe(
      'Over the 2.50% target on 0 of the last 1\u00a0day; the peak was 2.50% on 2026-11-18.'
    );
  });
  it('refuses an uncle rate whose route sends no operands, an older API', () => {
    expect(() =>
      head('uncle-rate', {
        ...daily(['0.02']),
        orphanRateTarget: { numerator: 1, denominator: 40 },
      })
    ).toThrow('uncle-rate: the API sends no counts: it is older than this page');
  });
});

describe('the epoch target, from its seconds (lane exactness, part A2, N3)', () => {
  const epochs = (seconds: number, hours: string) =>
    chartModel(def('epoch-time-length'), {
      title: 'Epoch Time Length',
      yAxisLabel: 'Milliseconds',
      y2AxisLabel: 'Blocks',
      data: [{ date: '0', value: '14400000', value2: '1800' }],
      peak: { date: '0', value: '14400000' },
      peak2: { date: '0', value: '1800' },
      target: { seconds, hours },
    } as never);

  it('draws the line at the target’s seconds and states a target off the hundredths in seconds', () => {
    // a devnet target of 100 s is 0.02777… h: the line at it, not at the cut 0.02
    const devnet = epochs(100, '0.02');
    expect(devnet.referenceLines).toEqual([{ value: 100 / 3600, label: 'target 100 s' }]);
    expect(devnet.target?.text).toBe('100 s');
    expect(devnet.target?.ms).toBe(BigInt(100_000));
  });
  it('draws a target under 36 s, which cuts to 0.00 h, and does not refuse the page', () => {
    const short = epochs(30, '0.00');
    expect(short.referenceLines).toEqual([{ value: 30 / 3600, label: 'target 30 s' }]);
  });
  it('states the consensus 4 h target in hours, exactly', () => {
    expect(epochs(14_400, '4.00').referenceLines).toEqual([{ value: 4, label: 'target 4.00 h' }]);
  });
  it('refuses a target that is not a whole number of seconds', () => {
    expect(() => epochs(0, '0.00')).toThrow('epoch-time-length: invalid epoch duration target 0 s');
  });
});

describe('an older API’s durations (lane exactness, part A2, N2)', () => {
  it('says an API that sends seconds or hours, not whole ms, is older than this page', () => {
    expect(() =>
      chartModel(def('average-block-time'), {
        title: 'Average Block Time',
        yAxisLabel: 'Seconds',
        data: [{ date: '2026-11-18', value: '9.96' }],
      } as never)
    ).toThrow(
      'average-block-time at 2026-11-18: 9.96 is not whole ms: the API is older than this page'
    );
    expect(() =>
      chartModel(def('epoch-time-length'), {
        title: 'Epoch Time Length',
        yAxisLabel: 'Hours',
        y2AxisLabel: 'Blocks',
        data: [{ date: '0', value: '4.01', value2: '1800' }],
      } as never)
    ).toThrow('epoch-time-length at 0: 4.01 is not whole ms: the API is older than this page');
  });
});

describe('the miners’ window (S7)', () => {
  const miners = (data: { blocks: number }[], totalBlocks: number) =>
    head('miner-address-distribution', {
      title: 'Miners',
      windowDays: 7,
      fromDate: '2026-11-12',
      toDate: '2026-11-18',
      totalBlocks,
      // the route's figure: the blocks of every miner after the five largest
      restBlocks: totalBlocks - data.slice(0, 5).reduce((sum, { blocks }) => sum + blocks, 0),
      data: data.map(({ blocks }, i) => ({
        minerLockHash: `0x${String(i + 1).repeat(64)}`,
        address: null,
        minerName: `Pool ${i}`,
        blocksMined: blocks,
        percentage: '0',
      })),
    }).sentence;

  it('says an empty window has no blocks', () => {
    expect(miners([], 0)).toBe('No blocks in the window.');
  });
  it('says one miner found every block only where one did', () => {
    expect(miners([{ blocks: 10 }], 10)).toBe('One miner found every block in the window.');
  });
});

describe('a whole of zero (S8)', () => {
  it('writes an empty day of activity in words, its band ends a dash', () => {
    const definition = def('activity-type-breakdown');
    const kinds = ['transfer', 'dao', 'token', 'object', 'identity', 'scriptCall'];
    const model = chartModel(definition, {
      title: 'Activity by type',
      series: kinds.map((key) => ({ key, label: key, color: '#000' })),
      data: [
        {
          date: '2026-11-18',
          values: { ...Object.fromEntries(kinds.map((key) => [key, '0'])), activityCount: '0' },
        },
      ],
    } as never);
    const empty = chartHead(definition, model);
    expect(empty.eyebrow).toBe('the largest kind in a day');
    expect(empty.figure).toBeNull();
    expect(empty.sentence).toBe('No activity of any kind on the day.');
    expect(Object.values(model.bandEnds!)).toEqual(kinds.map(() => '—'));
  });
});

describe('a tie of the largest kinds (lane exactness, part A2, N9)', () => {
  it('names every kind tied the largest, and each one’s share', () => {
    const definition = def('activity-type-breakdown');
    const kinds = ['transfer', 'dao', 'token', 'object', 'identity', 'scriptCall'];
    const values: Record<string, string> = {
      transfer: '3',
      dao: '0',
      token: '3',
      object: '0',
      identity: '0',
      scriptCall: '2',
      activityCount: '8',
    };
    const tied = chartHead(
      definition,
      chartModel(definition, {
        title: 'Activity by type',
        series: kinds.map((key) => ({ key, label: key, color: '#000' })),
        data: [{ date: '2026-11-18', values }],
      } as never)
    );
    // 3 CKB transfers and 3 token actions of 8: neither is "the largest kind" alone
    expect(tied.eyebrow).toBe('CKB transfers and tokens, the largest kinds in a day');
    expect(tied.sentence).toBe(
      // re-pinned on purpose (F-6 merged): "of the day's activities", which the kinds partition, and
      // no note (it was "of the day's kind counts" and "An activity of two kinds counts under both.")
      "CKB transfers and tokens were 37.50% each of the day's activities, script calls 25.00%."
    );
  });
});

describe('a tie at the top (S5)', () => {
  it('names every block interval tied at the top', () => {
    const intervals = head('block-time-distribution', {
      title: 'Block Time Distribution (Last 42 Epochs)',
      yAxisLabel: 'Block Ratio (%)',
      data: Array.from({ length: 51 }, (_, i) => ({
        date: `${Math.floor(i / 10)}.${i % 10}`,
        value: i === 12 || i === 13 ? '1.03' : '0.5',
      })),
      epochs: 42,
      intervals: 75_000,
      beyondRange: 0,
      // each bucket's count, the route's (lane exactness, part A2, N5)
      bucketCounts: Array.from({ length: 51 }, (_, i) => (i === 12 || i === 13 ? 773 : 375)),
    });
    expect(intervals.at).toBe('last 42\u00a0epochs');
    expect(intervals.sentence).toBe(
      '1.03% of blocks came 1.2 s after their parent, as many as 1.3 s after it, more than at any other tenth of a second.'
    );
  });
  it('says what lies beyond the drawn range, and reads the window from the route (S4)', () => {
    const intervals = head('block-time-distribution', {
      title: 'Block Time Distribution (Last 7 Epochs)',
      yAxisLabel: 'Block Ratio (%)',
      data: Array.from({ length: 51 }, (_, i) => ({
        date: `${Math.floor(i / 10)}.${i % 10}`,
        value: i === 13 ? '1.03' : '0.5',
      })),
      epochs: 7,
      intervals: 12_000,
      beyondRange: 12,
      // each bucket's count, the route's (lane exactness, part A2, N5)
      bucketCounts: Array.from({ length: 51 }, (_, i) => (i === 13 ? 124 : 60)),
    });
    expect(intervals.at).toBe('last 7\u00a0epochs');
    expect(intervals.sentence).toBe(
      '1.03% of blocks came 1.3 s after their parent, more than at any other tenth of a second; 12\u00a0blocks came more than 50 s after it.'
    );
  });
  // lane exactness, part A2 (S4, N4, N5): block intervals read their buckets by the route's counts
  const intervalsOf = (
    counts: readonly number[],
    shares: readonly string[],
    window: { epochs: number; intervals: number; beyondRange: number }
  ) =>
    head('block-time-distribution', {
      title: 'Block Time Distribution',
      yAxisLabel: 'Block Ratio (%)',
      data: counts.map((_, i) => ({ date: `${Math.floor(i / 10)}.${i % 10}`, value: shares[i] })),
      ...window,
      bucketCounts: counts,
    });
  it('names no interval where none came under 50 s, and no figure (S4)', () => {
    const none = intervalsOf(
      Array.from({ length: 501 }, () => 0),
      Array.from({ length: 501 }, () => '0.000'),
      { epochs: 42, intervals: 12, beyondRange: 12 }
    );
    expect(none.figure).toBeNull();
    expect(none.sentence).toBe(
      'No interval under 50 s; 12\u00a0blocks came more than 50 s after their parent.'
    );
  });
  it('counts a long tie at the top instead of listing it (S4)', () => {
    // 1.2 s to 1.6 s each 3 of 15,000: 0.02 %, five buckets tied
    const counts = Array.from({ length: 51 }, (_, i) =>
      i >= 12 && i <= 16 ? 3 : i === 20 ? 1 : 0
    );
    const shares = counts.map((n) => (n === 3 ? '0.020' : n === 1 ? '0.006' : '0.000'));
    expect(
      intervalsOf(counts, shares, { epochs: 1, intervals: 15_000, beyondRange: 14_984 }).sentence
    ).toBe(
      '0.02% of blocks came 1.2 s after their parent, as many as at 4 other tenths of a second, more than at any of the rest; 14,984\u00a0blocks came more than 50 s after it.'
    );
  });
  it('reads the most common interval by its count where two cut shares are equal (N5)', () => {
    // 3,000 and 3,001 of 150,000 are both 2.000 % cut at three places; the counts tell them apart
    const counts = Array.from({ length: 51 }, (_, i) =>
      i === 12 ? 3_000 : i === 13 ? 3_001 : 2_879
    );
    const shares = counts.map((n) => (n === 2_879 ? '1.919' : '2.000'));
    const intervals = intervalsOf(counts, shares, {
      epochs: 42,
      intervals: 150_000,
      beyondRange: 0,
    });
    expect(intervals.figure?.display).toBe('1.3');
    expect(intervals.sentence).toBe(
      '2.00% of blocks came 1.3 s after their parent, more than at any other tenth of a second.'
    );
  });
  it('names no window of 0 epochs on a chain in its first (N4)', () => {
    const first = intervalsOf([], [], { epochs: 0, intervals: 0, beyondRange: 0 });
    expect(first.at).toBeNull();
    expect(first.sentence).toBe('No blocks in the window yet.');
  });
  it('refuses block intervals whose route sends no counts, an older API (N5)', () => {
    expect(() =>
      head('block-time-distribution', {
        title: 'Block Time Distribution',
        yAxisLabel: 'Block Ratio (%)',
        data: [{ date: '0.1', value: '100.000' }],
        epochs: 1,
        intervals: 1,
        beyondRange: 0,
      })
    ).toThrow('block-time-distribution: the API sends no bucketCounts: it is older than this page');
  });
  it('says the epoch durations tied at the top', () => {
    const durations = head('epoch-time-distribution', {
      title: 'Epoch Time Distribution',
      yAxisLabel: 'Epochs',
      data: [
        { date: '2.05', value: '1' },
        { date: '4.01', value: '628' },
        { date: '4.03', value: '628' },
        { date: '50.50', value: '1' },
      ],
    });
    expect(durations.sentence).toBe(
      'The shortest epoch took 2.05 h and the longest 50.50 h; as many took 4.03 h as 4.01 h.'
    );
  });
});

describe('the cohorts (N6)', () => {
  const cohorts = (data: [string, string][]) =>
    head('address-cohort-retention', {
      title: 'Cohorts',
      yAxisLabel: '%',
      y2AxisLabel: 'CKB',
      data: data.map(([date, value]) => ({ date, value, value2: '1' })),
    }).sentence;

  it('finds the largest share among the complete cohorts only', () => {
    // the newest cohort, 2026-11, is still filling: its 9.5 % is no complete cohort's
    expect(
      cohorts([
        ['2024-06', '7.895999'],
        ['2026-10', '0.013256'],
        ['2026-11', '9.5'],
      ])
    ).toBe('The 2024-06 cohort keeps the largest share, 7.89%.');
  });
  it('says there is no complete cohort where the one there is still filling', () => {
    expect(cohorts([['2026-11', '9.5']])).toBe('No complete cohort yet.');
  });
});
