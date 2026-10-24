import { describe, expect, it } from 'vitest';
import { chartNumber, dailyDeltas, dayLabel, splitInProgress } from '@/lib/daily-series';

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

describe('splitInProgress', () => {
  it('treats the last tx-stats bucket as the one in progress, whatever its label', () => {
    const hourly = [
      { label: '09:00', value: 731 },
      { label: '10:00', value: 726 },
      { label: '11:00', value: 326 },
    ];
    expect(splitInProgress(hourly, 'hourlyData')).toEqual({
      complete: hourly.slice(0, 2),
      inProgress: hourly[2],
    });
  });

  it('has no complete bucket when only the current one exists', () => {
    const only = [{ label: '10/24', value: 14122 }];
    expect(splitInProgress(only, 'dailyData')).toEqual({ complete: [], inProgress: only[0] });
  });

  it('throws on an empty series', () => {
    expect(() => splitInProgress([], 'hourlyData')).toThrow(
      'hourlyData has no buckets, not even the one in progress'
    );
  });
});
