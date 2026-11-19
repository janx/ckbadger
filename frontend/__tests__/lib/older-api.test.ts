// The third read's La N6 (with S5's rule): a page on an API one version older refuses by name or
// draws truthfully. Every field this plan added to a route is read through one reader, which
// refuses its absence in one sentence: "<what>: the API sends no <field>: it is older than this
// page", as the chart models say it.
import { describe, expect, it } from 'vitest';
import { olderApi, sentField } from '@/lib/older-api';

describe('the one reader of a field an older API does not send', () => {
  it('says what does not send which field, and that the API is older than this page', () => {
    expect(olderApi('Scripts', 'roleCounts.both').message).toBe(
      'Scripts: the API sends no roleCounts.both: it is older than this page'
    );
  });

  it('reads a field the API sends, a null included, and refuses one it does not', () => {
    const row = { a: 1, b: null } as { a: number; b: string | null; c: number };
    expect(sentField(row, 'a', 'a row')).toBe(1);
    expect(sentField(row, 'b', 'a row')).toBeNull();
    expect(() => sentField(row, 'c', 'a row')).toThrow(
      'a row: the API sends no c: it is older than this page'
    );
  });
});
