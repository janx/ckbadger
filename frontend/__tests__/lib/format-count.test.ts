import { describe, expect, it } from 'vitest';
import { formatCount } from '@/lib/format-count';

describe('formatCount', () => {
  it('agrees the noun with the count (FF-D6, TX-34)', () => {
    expect(formatCount(1, 'block')).toBe('1 block');
    expect(formatCount(2, 'block')).toBe('2 blocks');
    expect(formatCount(0, 'cell')).toBe('0 cells');
    expect(formatCount(1234, 'confirmation')).toBe('1,234 confirmations');
    expect(formatCount(1, 'entry', 'entries')).toBe('1 entry');
    expect(formatCount(3, 'entry', 'entries')).toBe('3 entries');
  });
  it('rejects a count that is not a non-negative integer', () => {
    expect(() => formatCount(-1, 'block')).toThrow('Invalid count -1 for block');
    expect(() => formatCount(1.5, 'block')).toThrow('Invalid count 1.5 for block');
  });
});
