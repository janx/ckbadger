import { describe, it, expect } from 'vitest';
import { ageSeconds, formatAgeTwoUnits } from '@/lib/format-age';

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
