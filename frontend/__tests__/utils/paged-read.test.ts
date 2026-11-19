import { describe, expect, it } from 'vitest';
import { pagedRead } from '@/__tests__/utils/paged-read';

const page = (n: number, hasMore: boolean, total?: number, next: string | null = null) => ({
  data: Array.from({ length: n }, (_, i) => i),
  limit: 20,
  hasMore,
  nextCursor: next,
  total,
});

describe('pagedRead: a test’s page agrees with itself', () => {
  it('passes a full page with more and a short last page', () => {
    expect(pagedRead(page(20, true, 25, 'c1')).data).toHaveLength(20);
    expect(pagedRead(page(5, false, 25), 20).data).toHaveLength(5);
  });

  it('refuses the page that hid the address crash: 14 rows of 20 with more after it', () => {
    expect(() => pagedRead(page(14, true, 40, 'c1'))).toThrow(
      'pagedRead: a page of 14 rows at 20 a page is short and says more rows follow'
    );
  });

  it('refuses more rows than a page, more with no cursor, a last page with one, a short count', () => {
    expect(() => pagedRead(page(21, true, 40, 'c1'))).toThrow('holds more rows than a page');
    expect(() => pagedRead(page(20, true, 40))).toThrow('names no next cursor');
    expect(() => pagedRead(page(5, false, 25, 'c2'))).toThrow(
      'is the last and names a next cursor'
    );
    expect(() => pagedRead(page(5, false, 24), 20)).toThrow(
      'counts 24 rows in all, fewer than the 25 shown'
    );
  });
});
