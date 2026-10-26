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
  totalActivities,
  type ActivityCounts,
} from '@/lib/activity-stats';

const counts: ActivityCounts = {
  transferCount: 1302,
  daoDepositCount: 4,
  daoWithdrawRequestCount: 3,
  daoWithdrawCompleteCount: 1,
  tokenCount: 0,
  objectCount: 0,
  identityCount: 10,
  scriptCallCount: 5795,
};

describe('totalActivities', () => {
  it('sums every typed activity', () => {
    expect(totalActivities(counts)).toBe(7115);
  });
});

describe('activityTypeShares', () => {
  it('ranks the types by count, folds the three DAO steps into one row and hides empty types', () => {
    const rows = activityTypeShares(counts);
    expect(rows.map((r) => [r.label, r.count])).toEqual([
      ['Script call', 5795],
      ['Transfer', 1302],
      ['Identity', 10],
      ['DAO', 8],
    ]);
    expect(rows.reduce((s, r) => s + r.share, 0)).toBeCloseTo(1, 12);
  });

  it('has no rows without activity', () => {
    expect(
      activityTypeShares({
        ...counts,
        transferCount: 0,
        daoDepositCount: 0,
        daoWithdrawRequestCount: 0,
        daoWithdrawCompleteCount: 0,
        identityCount: 0,
        scriptCallCount: 0,
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
      '0xeeee0000…',
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
    ).toBe('0x50bd8d66…');
  });
});

describe('formatShare', () => {
  it('prints one decimal and marks tiny shares', () => {
    expect(formatShare(0.814)).toBe('81.4%');
    expect(formatShare(0.0014)).toBe('0.1%');
    expect(formatShare(0.0002)).toBe('<0.1%');
    expect(formatShare(0)).toBe('0.0%');
  });

  it('rejects a share outside [0, 1]', () => {
    expect(() => formatShare(1.2)).toThrow(/not in \[0, 1\]/);
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
  it('keeps unique totals separate from overlapping occurrences', () => {
    expect(activityFilterCounts(counts, 10).all).toBe(10);
    expect(activityFilterCounts(counts).all).toBeUndefined();
    expect(nonCellbaseCount(25, 10, '2026-10-25')).toBe(15);
    expect(() => nonCellbaseCount(9, 10, '2026-10-25')).toThrow(
      'Invalid transaction/cellbase counts'
    );
  });
});
