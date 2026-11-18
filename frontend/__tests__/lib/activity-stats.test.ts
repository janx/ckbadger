import { dailyActivities } from '@/__tests__/msw/fixtures/home';
import { describe, expect, it } from 'vitest';
import {
  activityTypeShares,
  activityFilterCounts,
  completedYesterdayActivities,
  completedYesterdayTransactions,
  nonCellbaseCount,
  previousChainDay,
  formatShare,
  scriptLabel,
  scriptShares,
  type ActivityCounts,
} from '@/lib/activity-stats';

/** One kind per transaction: the kinds sum to activityCount, the one total the API sends. */
const counts: ActivityCounts = {
  activityCount: 7115,
  kindCounts: { dao: 7, token: 0, object: 0, identity: 10, protocol: 1, script: 5795, ckb: 1302 },
};

describe('activityTypeShares', () => {
  it('ranks the kinds by count as shares of the transactions and hides empty kinds', () => {
    const rows = activityTypeShares(counts);
    expect(rows.map((r) => [r.label, r.count])).toEqual([
      ['Script call', 5795],
      ['Transfer', 1302],
      ['Identity', 10],
      ['DAO', 7],
      ['Protocol', 1],
    ]);
    // A partition: the shares of the transactions sum to one.
    expect(rows.reduce((s, r) => s + r.share, 0)).toBeCloseTo(1, 12);
  });

  it('has no rows without activity', () => {
    expect(
      activityTypeShares({
        activityCount: 0,
        kindCounts: { dao: 0, token: 0, object: 0, identity: 0, protocol: 0, script: 0, ckb: 0 },
      })
    ).toEqual([]);
  });
});

describe('scriptShares', () => {
  const entries = [
    { codeHash: '0xaaaa000000000000', name: '.bit Time Info', count: 4395 },
    { codeHash: '0xbbbb000000000000', name: '.bit Time Index State', count: 4395 },
    { codeHash: '0xcccc000000000000', name: 'Bitcoin SPV Type Lock', count: 240 },
    { codeHash: '0xdddd000000000000', name: '.bit Lock', count: 20 },
    { codeHash: '0xeeee000000000000', name: null, count: 9 },
    { codeHash: '0xffff000000000000', name: '  ', count: 3 },
    { codeHash: '0x1111000000000000', name: 'quiet', count: 0 },
    { codeHash: '0x2222000000000000', name: 'rare', count: 1 },
  ];

  it('keeps the top five and folds the rest into one row', () => {
    const s = scriptShares(entries);
    expect(s.total).toBe(9063);
    expect(s.top.map((r) => r.label)).toEqual([
      '.bit Time Info',
      '.bit Time Index State',
      'Bitcoin SPV Type Lock',
      '.bit Lock',
      // U-09 (re-pinned): a share bar's label in the home stats card, the tight size
      '0xeeee…0000',
    ]);
    expect(s.others).toEqual({ scripts: 2, count: 4, share: 4 / 9063 });
  });

  it('has no others row when every script fits', () => {
    expect(scriptShares(entries.slice(0, 2)).others).toBeNull();
    expect(scriptShares([])).toEqual({ total: 0, top: [], others: null });
  });
});

describe('scriptLabel', () => {
  it('uses the name when known, else a short code hash', () => {
    expect(
      scriptLabel({
        name: 'xUDT',
        codeHash: '0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95',
      })
    ).toBe('xUDT');
    expect(
      scriptLabel({
        name: null,
        codeHash: '0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95',
      })
    ).toBe('0x50bd…1b95');
  });
});

// CH-14 and the review's item I (re-pinned on purpose): a share is the one percentage of its
// count in the total, two decimals, never a false 0.00% or 100.00% (it was one decimal of a float
// share, '<0.1%' below 0.05 %)
describe('formatShare', () => {
  it('prints the one percentage of a count in the total', () => {
    expect(formatShare(814, 1000)).toBe('81.40%');
    expect(formatShare(14, 10000)).toBe('0.14%');
    expect(formatShare(2, 100000)).toBe('<0.01%');
    expect(formatShare(999_999, 1_000_000)).toBe('>99.99%');
    expect(formatShare(0, 10)).toBe('0.00%');
    expect(formatShare(10, 10)).toBe('100.00%');
  });

  it('rejects a count that is not a part of the total', () => {
    expect(() => formatShare(12, 10)).toThrow(/not a part of/);
    expect(() => formatShare(1.5, 10)).toThrow(/not a part of/);
    expect(() => formatShare(1, 0)).toThrow();
  });
});

describe('exact completed day counts', () => {
  const now = Date.parse('2026-10-25T17:00:00Z');
  it('uses the indexer UTC+8 day, including midnight and year boundaries', () => {
    expect(previousChainDay(now)).toBe('2026-10-25');
    expect(previousChainDay(Date.parse('2026-12-31T17:00:00Z'))).toBe('2026-12-31');
    expect(
      completedYesterdayActivities([{ ...dailyActivities[0], date: '20261025' }], now)?.date
    ).toBe('20261025');
  });
  it('selects only the exact complete transaction bucket and never a latest-bucket substitute', () => {
    const base = { currentHour: 0, currentDay: 999, hourlyData: [] };
    expect(
      completedYesterdayTransactions(
        {
          ...base,
          dailyData: [
            { label: '10/24', value: 9, complete: true },
            { label: '10/25', value: 10, complete: false },
          ],
        },
        now
      )
    ).toBeUndefined();
    expect(
      completedYesterdayTransactions(
        { ...base, dailyData: [{ label: '10/25', value: 0, complete: true }] },
        now
      )
    ).toBe(0);
  });
  it('gives each filter chip its kind count, Protocols included, and All when known', () => {
    expect(activityFilterCounts(counts, 7115)).toEqual({
      all: 7115,
      dao: 7,
      token: 0,
      object: 0,
      identity: 10,
      protocol: 1,
      script: 5795,
      ckb: 1302,
    });
    expect(activityFilterCounts(counts).all).toBeUndefined();
    expect(nonCellbaseCount(25, 10, '2026-10-25')).toBe(15);
    expect(() => nonCellbaseCount(9, 10, '2026-10-25')).toThrow(
      'Invalid transaction/cellbase counts'
    );
  });
});
