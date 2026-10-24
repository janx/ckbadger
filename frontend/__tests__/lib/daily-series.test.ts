import { describe, expect, it } from 'vitest';
import { chartNumber, dailyDeltas, dayLabel, splitByCompleteness } from '@/lib/daily-series';

describe('dayLabel', () => {
  it('labels ISO and compact days as MM/DD', () => {
    expect(dayLabel('2026-10-23')).toBe('10/23');
    expect(dayLabel('20261024')).toBe('10/24');
  });

  it('rejects anything else', () => {
    expect(() => dayLabel('10/23')).toThrow(/not a YYYY-MM-DD or YYYYMMDD day/);
  });
});

describe('chartNumber', () => {
  it('reads a numeric chart value and names the day of one that is not', () => {
    expect(chartNumber({ date: '2026-10-23', value: '9.57' })).toBe(9.57);
    expect(() => chartNumber({ date: '2026-10-23', value: 'n/a' })).toThrow(
      'chart value "n/a" on 2026-10-23 is not a number'
    );
    expect(() => chartNumber({ date: '2026-10-23', value: '' })).toThrow(/not a number/);
  });
});

describe('dailyDeltas', () => {
  const series = [
    { date: '2026-10-20', value: '8131000000' },
    { date: '2026-10-21', value: '8131306075.44505907' },
    { date: '2026-10-22', value: '8131474441.92489293' },
    { date: '2026-10-23', value: '8128895616.437997' },
  ];

  it('derives exact day-over-day changes in shannons over the last N days', () => {
    expect(dailyDeltas(series, 2)).toEqual([
      { date: '2026-10-22', delta: BigInt('16836647983386') },
      { date: '2026-10-23', delta: BigInt('-257882548689593') },
    ]);
  });

  it('uses every point a shorter series has', () => {
    expect(dailyDeltas(series, 30)).toHaveLength(3);
    expect(dailyDeltas(series.slice(0, 1), 30)).toEqual([]);
  });

  it('throws on a missing day instead of folding two days into one bar', () => {
    const gap = [series[0], series[2]];
    expect(() => dailyDeltas(gap, 30)).toThrow('daily series skips from 2026-10-20 to 2026-10-22');
  });

  it('throws on a malformed amount', () => {
    expect(() =>
      dailyDeltas(
        [
          { date: '2026-10-20', value: '1' },
          { date: '2026-10-21', value: 'NaN' },
        ],
        30
      )
    ).toThrow(/not a CKB amount/);
  });
});

describe('splitByCompleteness', () => {
  const b = (label: string, value: number, complete: boolean) => ({ label, value, complete });

  it('splits on the complete flags, never on position', () => {
    const hourly = [b('08:00', 650, true), b('09:00', 731, true), b('10:00', 726, false)];
    expect(splitByCompleteness(hourly, 'hourlyData')).toEqual({
      complete: hourly.slice(0, 2),
      inProgress: hourly.slice(2),
    });
  });

  it('keeps more than one trailing bucket in progress (the previous hour, for the first minutes)', () => {
    const hourly = [b('08:00', 650, true), b('09:00', 731, false), b('10:00', 12, false)];
    expect(splitByCompleteness(hourly, 'hourlyData')).toEqual({
      complete: hourly.slice(0, 1),
      inProgress: hourly.slice(1),
    });
  });

  it('allows an empty series: before any block, or before a fresh build has its stats', () => {
    expect(splitByCompleteness([], 'dailyData')).toEqual({ complete: [], inProgress: [] });
  });

  it('allows a series of only in-progress buckets', () => {
    const only = [b('10/24', 14122, false)];
    expect(splitByCompleteness(only, 'dailyData')).toEqual({ complete: [], inProgress: only });
  });

  it('fails fast when a complete bucket follows an in-progress one', () => {
    expect(() =>
      splitByCompleteness(
        [b('08:00', 1, false), b('09:00', 2, true), b('10:00', 3, false)],
        'hourlyData'
      )
    ).toThrow('hourlyData: complete bucket 09:00 follows in-progress bucket 08:00');
  });

  it('fails fast when the newest bucket is marked complete', () => {
    expect(() =>
      splitByCompleteness([b('10/23', 1, true), b('10/24', 2, true)], 'dailyData')
    ).toThrow(
      'dailyData: the newest bucket 10/24 is marked complete; the newest bucket is always in progress'
    );
  });
});
