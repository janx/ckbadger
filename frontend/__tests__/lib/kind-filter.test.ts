import { describe, expect, it } from 'vitest';
import { KIND_FILTER_LABEL, kindColor, kindFilterValues, kindWords } from '@/lib/kind-filter';

/**
 * The kind filter (controls lane C2, UI review PA-15): the activities stream and the transactions
 * list filter by kind with one set of words and marks, the controls board's; each list names its
 * kinds in its order.
 */
describe('the kind filter', () => {
  it('writes a list’s kinds in its order, in the board’s words, with their marks and counts', () => {
    expect(
      kindFilterValues(['all', 'ckb', 'script', 'cellbase', 'dao'], { all: 20, dao: 3, ckb: 0 })
    ).toEqual([
      { key: 'all', label: 'all', mark: undefined, count: 20 },
      { key: 'ckb', label: 'ckb transfers', mark: 'jade', count: 0 },
      { key: 'script', label: 'script calls', mark: 'slate-ckb', count: undefined },
      { key: 'cellbase', label: 'cellbase', mark: 'slate-other', count: undefined },
      { key: 'dao', label: 'nervos dao', mark: 'gold', count: 3 },
    ]);
    expect(kindFilterValues(['token', 'object', 'identity', 'protocol'], {})).toEqual([
      { key: 'token', label: 'tokens', mark: 'token', count: undefined },
      { key: 'object', label: 'objects', mark: 'object', count: undefined },
      { key: 'identity', label: 'identities', mark: 'identity', count: undefined },
      { key: 'protocol', label: 'protocols', mark: undefined, count: undefined },
    ]);
  });

  it('is named "kind", the label its phone select stands after', () => {
    expect(KIND_FILTER_LABEL).toBe('kind');
  });

  it('names a kind in a figure by its words, and colours it by its mark', () => {
    expect(kindWords('dao')).toBe('nervos dao');
    expect(kindColor('ckb')).toBe('var(--c-jade)');
    expect(kindColor('dao')).toBe('var(--c-gold)');
    // a kind with no mark takes the slate of the other scripts in the figure, as it drew it
    expect(kindColor('protocol')).toBe('var(--c-slate-other)');
  });
});
