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
