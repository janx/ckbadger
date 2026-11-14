import { describe, expect, it } from 'vitest';
import { formatCount } from '@/lib/format-count';

describe('formatCount', () => {
  // Re-pinned with U-16: the count and its noun are joined by a no-break space (one unit).
  it('agrees the noun with the count (FF-D6, TX-34)', () => {
    expect(formatCount(1, 'block')).toBe('1\u00a0block');
    expect(formatCount(2, 'block')).toBe('2\u00a0blocks');
    expect(formatCount(0, 'cell')).toBe('0\u00a0cells');
    expect(formatCount(1234, 'confirmation')).toBe('1,234\u00a0confirmations');
    expect(formatCount(1, 'entry', 'entries')).toBe('1\u00a0entry');
    expect(formatCount(3, 'entry', 'entries')).toBe('3\u00a0entries');
  });
  it('rejects a count that is not a non-negative integer', () => {
    expect(() => formatCount(-1, 'block')).toThrow('Invalid count -1 for block');
    expect(() => formatCount(1.5, 'block')).toThrow('Invalid count 1.5 for block');
  });
});
