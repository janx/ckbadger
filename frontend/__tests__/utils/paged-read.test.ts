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

  // The second read's MUST 1: the checker refused the shape the API sends for an address's page
  // one, the pool's rows (each with its poolStatus) above a page of committed rows, so no test could
  // serve it. It counts the committed rows against the page size, and still refuses what the API
  // cannot send: pool rows past page one, a pool row below a committed one, more than 200 of them.
  it('passes page one’s pool rows above a full page of committed rows, and above a last page', () => {
    const pool = (n: number) => Array.from({ length: n }, () => ({ poolStatus: 'pending' }));
    const rows = (n: number) => Array.from({ length: n }, (_, i) => ({ i }));
    const one = (pooled: number, n: number, more: boolean, total?: number) => ({
      data: [...pool(pooled), ...rows(n)],
      limit: 20,
      hasMore: more,
      nextCursor: more ? 'c1' : null,
      total,
    });
    expect(pagedRead(one(1, 20, true, 40)).data).toHaveLength(21);
    expect(pagedRead(one(2, 19, false, 19)).data).toHaveLength(21);
    expect(pagedRead(one(200, 20, true)).data).toHaveLength(220);
    expect(() => pagedRead(one(2, 21, true))).toThrow(
      'pagedRead: a page of 21 rows at 20 a page (besides the pool’s 2) holds more rows than a page'
    );
    expect(() => pagedRead(one(1, 20, true), 20)).toThrow(
      'pagedRead: the pool’s rows past page one'
    );
    expect(() => pagedRead({ ...one(0, 1, false), data: [...rows(1), ...pool(1)] })).toThrow(
      'pagedRead: a pool row below a committed row'
    );
    expect(() => pagedRead(one(201, 0, false))).toThrow(
      'pagedRead: 201 pool rows, more than the 200 the API sends'
    );
    // the pool's rows are not counted in the total, which counts committed rows
    expect(() => pagedRead(one(5, 19, false, 19))).not.toThrow();
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
