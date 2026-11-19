/** Rows per page on a topic or list page (blocks, transactions, activities, inventory, …). */
export const DEFAULT_PAGE_SIZE = 50;

/**
 * Rows per page on an entity page (address, token, object, identity, script): the feeds and
 * holder lists under the hero. Twenty, not fifty, so the page's second tier — the evidence for
 * developers — stays within a few screens instead of ten (review 2026-11-03, item 2). A list page
 * is a list by nature and keeps {@link DEFAULT_PAGE_SIZE}; a phone reads twenty on both.
 */
export const ENTITY_PAGE_SIZE = 20;

/**
 * The pages a list of `total` rows takes at `pageSize` rows a page, both as a response sent them
 * (its `total` and its `limit`): the pager's `pages` (components/ui/pager.tsx), the whole pages
 * and one more for a remainder, by integer arithmetic. A list with no row takes none.
 */
export function pageCount(total: number, pageSize: number): number {
  if (!Number.isSafeInteger(total) || total < 0)
    throw new Error(`pageCount: ${total} is not a count of rows`);
  if (!Number.isSafeInteger(pageSize) || pageSize < 1)
    throw new Error(`pageCount: ${pageSize} is not a page's size`);
  const rest = total % pageSize;
  return (total - rest) / pageSize + (rest > 0 ? 1 : 0);
}

/** A ranked list's page in the figures its pager writes (`Pager` in `pages` mode). */
interface RankedPage {
  from: number;
  to: number;
  total: number;
  pageSize: number;
  page: number;
  pages: number;
}

/**
 * A ranked list's page in the pager's figures, all from the response the page draws: `page` is
 * the page it was asked as (the reader's cursor history when it was asked), the rows a page holds
 * are its `limit`, the rows shown its `data`, the rows in all its `total`. Nothing is guessed: a
 * ranked list's response carries its total, and one without is refused, naming the list; an empty
 * page has no rows to count from and is refused too (a list draws no pager for it).
 */
export function rankedPage(
  list: string,
  page: number,
  response: { data: readonly unknown[]; limit: number; total?: number | null }
): RankedPage {
  const { data, limit, total } = response;
  if (total === undefined || total === null)
    throw new Error(`${list}: a ranked list's page came without its total`);
  if (!Number.isSafeInteger(page) || page < 1)
    throw new Error(`${list}: ${page} is not a page of the list`);
  if (data.length === 0) throw new Error(`${list}: page ${page} holds no row to count from`);
  const from = (page - 1) * limit + 1;
  return {
    from,
    to: from + data.length - 1,
    total,
    pageSize: limit,
    page,
    pages: pageCount(total, limit),
  };
}
