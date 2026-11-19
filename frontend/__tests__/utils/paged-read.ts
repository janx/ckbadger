/**
 * A page of a cursor-paged list as a test serves it, checked to agree with itself (lane
 * cd-fixes-controls, item 1): a fixture that contradicts itself hid a crash (14 rows on a page of
 * 20 that said more rows followed, with a count of 40, could not show what a real last page
 * does). A page holds no more rows than its page size; a page shorter than its size is the last,
 * with no more after it; a page with more after it names its next cursor; and a count holds at
 * least the rows shown up to this page (`shownBefore`, the rows of the pages before it).
 */
export function pagedRead<
  P extends {
    data: readonly unknown[];
    limit: number;
    hasMore: boolean;
    nextCursor: string | null;
    total?: number | null;
  },
>(page: P, shownBefore = 0): P {
  const n = page.data.length;
  const where = `a page of ${n} rows at ${page.limit} a page`;
  if (n > page.limit) throw new Error(`pagedRead: ${where} holds more rows than a page`);
  if (n < page.limit && page.hasMore)
    throw new Error(`pagedRead: ${where} is short and says more rows follow`);
  if (page.hasMore && !page.nextCursor)
    throw new Error(`pagedRead: ${where} says more rows follow and names no next cursor`);
  if (!page.hasMore && page.nextCursor)
    throw new Error(`pagedRead: ${where} is the last and names a next cursor`);
  if (page.total !== undefined && page.total !== null && page.total < shownBefore + n)
    throw new Error(
      `pagedRead: ${where} counts ${page.total} rows in all, fewer than the ${shownBefore + n} shown`
    );
  return page;
}
