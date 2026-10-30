import { describe, it, expect } from 'vitest';
import { formatAgeTwoUnits } from '@/lib/format-age';

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
  it('refuses a time in the future', () => {
    expect(() => formatAgeTwoUnits(NOW + 5000, NOW)).toThrow('is after now');
  });
});
