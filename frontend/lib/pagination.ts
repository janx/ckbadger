/** Rows per page on a topic or list page (blocks, transactions, activities, inventory, …). */
export const DEFAULT_PAGE_SIZE = 50;

/**
 * Rows per page on an entity page (address, token, object, identity, script): the feeds and
 * holder lists under the hero. Twenty, not fifty, so the page's second tier — the evidence for
 * developers — stays within a few screens instead of ten (review 2026-11-03, item 2). A list page
 * is a list by nature and keeps {@link DEFAULT_PAGE_SIZE}; a phone reads twenty on both.
 */
export const ENTITY_PAGE_SIZE = 20;

/** One page of a cursor-paged list as the API sent it: what a pager reads of it. */
export interface CursorPageRead {
  data: readonly unknown[];
  /** The page size the API served. */
  limit: number;
  hasMore: boolean;
  /** The rows in all, where the API counts them (absent, or null while not derived). */
  total?: number | null;
}

/** The pages a count of rows fills at the page size the response states. */
function pageCount(list: string, total: number, limit: number): number {
  if (!Number.isSafeInteger(limit) || limit < 1)
    throw new Error(`${list}: a page size of ${limit} rows`);
  if (!Number.isSafeInteger(total) || total < 0) throw new Error(`${list}: ${total} rows in all`);
  return Math.ceil(total / limit);
}

/**
 * Where a page of a ranked cursor-paged list stands, for the pager's `pages` mode
 * (components/ui/pager.tsx), from its response and the cursor's page (hooks/useCursorPagination):
 * the rows before it are the full pages before it, each the page size the response states, so it
 * shows rows `from`–`to` of the response's `total`, which fills ⌈total ÷ limit⌉ pages. Null where
 * the list is one page (the first, with nothing after it): no pager is drawn. A ranked list's
 * response that counts no rows is refused, as `pages` mode writes "of N".
 */
export function rankedPageAt(
  list: string,
  page: number,
  read: CursorPageRead
): {
  from: number;
  to: number;
  total: number;
  pageSize: number;
  page: number;
  pages: number;
} | null {
  if (page === 1 && !read.hasMore) return null;
  if (read.total === undefined || read.total === null)
    throw new Error(`${list}: page ${page} of a ranked list states no total`);
  const from = (page - 1) * read.limit + 1;
  return {
    from,
    to: from + read.data.length - 1,
    total: read.total,
    pageSize: read.limit,
    page,
    pages: pageCount(list, read.total, read.limit),
  };
}

/**
 * Where a page of a cursor-paged list in time stands, for the pager's `time` mode: the cursor's
 * page and, where the response counts the rows, the ⌈total ÷ limit⌉ pages they fill. Null where
 * the list is one page (the first, with nothing after it): no pager is drawn.
 */
export function timePageAt(
  list: string,
  page: number,
  read: CursorPageRead
): { page: number; pages?: number } | null {
  if (page === 1 && !read.hasMore) return null;
  if (read.total === undefined || read.total === null) return { page };
  return { page, pages: pageCount(list, read.total, read.limit) };
}
