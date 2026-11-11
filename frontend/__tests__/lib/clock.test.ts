import { describe, expect, it } from 'vitest';
import { clockText, localZone } from '@/lib/clock';

describe('the status line clock', () => {
  it('writes local time as HH:MM:SS on the 24-hour clock, zero-padded', () => {
    expect(clockText(new Date(2026, 10, 11, 14, 2, 31))).toBe('14:02:31');
    expect(clockText(new Date(2026, 10, 11, 0, 0, 0))).toBe('00:00:00');
    expect(clockText(new Date(2026, 10, 11, 9, 5, 7, 999))).toBe('09:05:07');
  });

  it('names the local time zone', () => {
    expect(localZone()).toBe(Intl.DateTimeFormat().resolvedOptions().timeZone);
    expect(localZone().length).toBeGreaterThan(0);
  });
});
