import { describe, it, expect } from 'vitest';
import { ageSeconds, formatAgeTwoUnits, formatSpan } from '@/lib/format-age';
import { formatDurationMs, formatTimeAgo } from '@/lib/utils';

const NOW = Date.parse('2026-10-25T11:43:55Z');

describe('formatAgeTwoUnits (TX-18 facts rail)', () => {
  it('reads days and hours, as the Transaction board does ("2d 20h ago")', () => {
    expect(formatAgeTwoUnits('2026-10-22T14:43:55Z', NOW)).toBe('2d 21h ago');
    expect(formatAgeTwoUnits(NOW - (2 * 86400 + 20 * 3600 + 59) * 1000, NOW)).toBe('2d 20h ago');
  });
  it('reads hours and minutes, then minutes and seconds, then seconds', () => {
    expect(formatAgeTwoUnits(NOW - (3 * 3600 + 5 * 60) * 1000, NOW)).toBe('3h 5m ago');
    expect(formatAgeTwoUnits(NOW - (4 * 60 + 2) * 1000, NOW)).toBe('4m 2s ago');
    expect(formatAgeTwoUnits(NOW - 45 * 1000, NOW)).toBe('45s ago');
  });
  it('drops a zero second unit', () => {
    expect(formatAgeTwoUnits(NOW - 2 * 86400 * 1000, NOW)).toBe('2d ago');
  });
  // Review 45fd finding 3 (ruling D4): the node's clock and this device's are two clocks, and CKB
  // consensus itself accepts a block up to 15 s ahead of a node's clock.
  it('reads a time up to 15 s ahead of this device as no age at all', () => {
    expect(formatAgeTwoUnits(NOW + 2000, NOW)).toBe('0s ago');
    expect(formatAgeTwoUnits(NOW + 5000, NOW)).toBe('0s ago');
    expect(formatAgeTwoUnits(NOW + 15_000, NOW)).toBe('0s ago');
  });
  it('gives no relative age for a time further ahead: it says the device clock is behind', () => {
    expect(formatAgeTwoUnits(NOW + 15_001, NOW)).toBe("this device's clock is behind");
    expect(formatAgeTwoUnits(NOW + 16_000, NOW)).toBe("this device's clock is behind");
    expect(formatAgeTwoUnits(NOW + 10 * 60 * 1000, NOW)).toBe("this device's clock is behind");
  });
  it('still refuses a timestamp it cannot read', () => {
    expect(() => formatAgeTwoUnits('not a time', NOW)).toThrow('Invalid timestamp "not a time"');
  });
});

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
    expect(formatAgeTwoUnits(genesis, now)).toBe('6y 363d ago');
    expect(formatTimeAgo(genesis, now)).toBe('6y 363d ago');
    // a whole year, its day unit left out; a day short of it, still days and hours
    const year = Date.parse('2025-11-14T12:00:00+08:00');
    expect(formatAgeTwoUnits(year, now)).toBe('1y ago');
    expect(formatAgeTwoUnits(year + 86_400_000, now)).toBe('364d ago');
  });

  it('keeps the feed rows’ one unit under a year', () => {
    expect(formatTimeAgo(NOW - 45_000, NOW)).toBe('45s ago');
    expect(formatTimeAgo(NOW - (4 * 60 + 2) * 1000, NOW)).toBe('4m ago');
    expect(formatTimeAgo(NOW - 3 * 86_400_000, NOW)).toBe('3d ago');
  });
});
