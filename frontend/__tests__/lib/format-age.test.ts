import { describe, it, expect } from 'vitest';
import { ageSeconds, formatSpan } from '@/lib/format-age';
import { formatDurationMs, formatTimeAgo } from '@/lib/utils';

const NOW = Date.parse('2026-10-25T11:43:55Z');

// Lane D2 (gate D0′): the transaction's facts rail, the one reader of the two-unit age
// (`formatAgeTwoUnits`, "2d 20h ago"), became the status strip, whose time takes the site's one
// age (`Utc8Age`, "2d ago"). The formatter had no user left, so it went, and its tests with it
// (the dead-exports gate's rule); the rules they also held — up to 15 s ahead reads as no age,
// further ahead as the device clock behind, an unreadable time refused — are `ageSeconds`'s and
// stay pinned below; the calendar years are pinned through the one form (`formatTimeAgo`).

describe('ageSeconds (the one rule for a relative age)', () => {
  it('is whole seconds since the time, truncated', () => {
    expect(ageSeconds(NOW - 45_999, NOW)).toEqual({ kind: 'age', seconds: 45 });
    expect(ageSeconds(String(NOW - 60_000), NOW)).toEqual({ kind: 'age', seconds: 60 });
    expect(ageSeconds(new Date(NOW), NOW)).toEqual({ kind: 'age', seconds: 0 });
  });
  it('reads up to 15 s ahead as zero, and further ahead as the device clock behind', () => {
    expect(ageSeconds(NOW + 15_000, NOW)).toEqual({ kind: 'age', seconds: 0 });
    expect(ageSeconds(NOW + 15_001, NOW)).toEqual({ kind: 'clock-behind' });
  });
  it('throws on an unreadable timestamp', () => {
    expect(() => ageSeconds('2026-13-45', NOW)).toThrow('Invalid timestamp "2026-13-45"');
  });
});

// U-15 (UI review 2026-11-14): one duration and age form, the notation of format-age: a number and
// its unit letter (y d h m s), the two largest units, a zero second unit left out. "26 m 14 s" and
// "52 s on chain" took another; the genesis block's age read "2555d ago".
describe('one duration and age form (U-15)', () => {
  it('writes a span in its two largest units, the unit letter after the number', () => {
    expect(formatSpan(26 * 60 + 14)).toBe('26m 14s');
    expect(formatSpan(52)).toBe('52s');
    expect(formatSpan(0)).toBe('0s');
    expect(formatSpan(3 * 86400 + 4 * 3600 + 59)).toBe('3d 4h');
    expect(formatSpan(2 * 3600)).toBe('2h');
    expect(formatSpan(2555 * 86400)).toBe('2555d');
    expect(() => formatSpan(-1)).toThrow('invalid span -1');
  });

  it('writes a duration in milliseconds the same way, under a second in ms', () => {
    expect(formatDurationMs((26 * 60 + 14) * 1000 + 999)).toBe('26m 14s');
    expect(formatDurationMs(52_000)).toBe('52s');
    expect(formatDurationMs(450)).toBe('450ms');
  });

  it('writes an age of a year or more in calendar years and days on the UTC+8 clock', () => {
    // the genesis block, 2019-11-16 05:21 UTC+8, read on 2026-11-14 12:00 UTC+8: 2,555 days
    const genesis = Date.parse('2019-11-16T05:21:00+08:00');
    const now = Date.parse('2026-11-14T12:00:00+08:00');
    expect(formatTimeAgo(genesis, now)).toBe('6y 363d ago');
    // a whole year, its day unit left out; a day short of it, still days. Re-pinned on purpose
    // (lane D2): read through the one form, `formatTimeAgo` (it was `formatAgeTwoUnits`, gone
    // with the facts rail), whose years are the same calendar years
    const year = Date.parse('2025-11-14T12:00:00+08:00');
    expect(formatTimeAgo(year, now)).toBe('1y ago');
    expect(formatTimeAgo(year + 86_400_000, now)).toBe('364d ago');
  });

  it('keeps the feed rows’ one unit under a year', () => {
    expect(formatTimeAgo(NOW - 45_000, NOW)).toBe('45s ago');
    expect(formatTimeAgo(NOW - (4 * 60 + 2) * 1000, NOW)).toBe('4m ago');
    expect(formatTimeAgo(NOW - 3 * 86_400_000, NOW)).toBe('3d ago');
  });
});
