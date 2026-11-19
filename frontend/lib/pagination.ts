/** Rows per page on a topic or list page (blocks, transactions, activities, inventory, …). */
export const DEFAULT_PAGE_SIZE = 50;

/**
 * Rows per page on an entity page (address, token, object, identity, script): the feeds and
 * holder lists under the hero. Twenty, not fifty, so the page's second tier — the evidence for
 * developers — stays within a few screens instead of ten (review 2026-11-03, item 2). A list page
 * is a list by nature and keeps {@link DEFAULT_PAGE_SIZE}; a phone reads twenty on both.
 */
export const ENTITY_PAGE_SIZE = 20;

/** A page of a cursor list as the pager's pages mode takes it (components/ui/pager.tsx). */
export interface CursorPageFigures {
  from: number;
  to: number;
  total: number;
  pageSize: number;
  page: number;
  pages: number;
}

/**
 * The figures of one page of a cursor list whose API sent its total, for the pager's pages mode,
 * which computes none: the page the walk is on (hooks/useCursorPagination), the page size the list
 * asked the API for, the rows this page holds and the API's total. The rows are counted from 1, so
 * page 2 of twenty a page starts at 21 and ends where its rows end; the pages are the total over
 * the page size, the last one partial. A list with no total is not paged this way: it is a list in
 * time (the pager's time mode) or a short list (its more mode).
 */
export function cursorPageFigures({
  page,
  pageSize,
  rows,
  total,
}: {
  page: number;
  pageSize: number;
  rows: number;
  total: number;
}): CursorPageFigures {
  const from = (page - 1) * pageSize + 1;
  return { from, to: from + rows - 1, total, pageSize, page, pages: Math.ceil(total / pageSize) };
}
