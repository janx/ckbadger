import { describe, expect, it, vi } from 'vitest';
import { clockText, localZone } from '@/lib/clock';

describe('the status line clock', () => {
  it('writes local time as HH:MM:SS on the 24-hour clock, zero-padded', () => {
    expect(clockText(new Date(2026, 10, 11, 14, 2, 31))).toBe('14:02:31');
    expect(clockText(new Date(2026, 10, 11, 0, 0, 0))).toBe('00:00:00');
    expect(clockText(new Date(2026, 10, 11, 9, 5, 7, 999))).toBe('09:05:07');
  });

  it('names the local time zone', () => {
    expect(localZone()).toBe(Intl.DateTimeFormat().resolvedOptions().timeZone);
    expect(localZone()!.length).toBeGreaterThan(0);
  });

  // A4 review 5: an engine may give no zone; then there is none to name (null), never "undefined"
  // or an empty name, and each place that writes the zone leaves it out.
  it('has no zone where Intl gives none', () => {
    const resolved = Intl.DateTimeFormat().resolvedOptions();
    for (const timeZone of [undefined, '']) {
      const spy = vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
        ...resolved,
        timeZone: timeZone as unknown as string,
      });
      expect(localZone()).toBeNull();
      spy.mockRestore();
    }
  });
});
