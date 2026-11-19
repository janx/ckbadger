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
  /** The cursor of the page after it, where one follows. */
  nextCursor: string | null;
  /** The rows in all, where the API counts them (absent, or null while not derived). */
  total?: number | null;
}

/**
 * Where a response was asked for in its walk (lane cd-fixes-controls, item 1): the page, the
 * cursor and the page size it was asked at (none where the list asks for none of its own); `kept`
 * marks a page merged from reads asked at different places (`mergeAsked`).
 */
export interface AskedAt {
  readonly page: number;
  readonly cursor: string | undefined;
  readonly size: number | undefined;
  readonly kept?: true;
}

declare const ASKED: unique symbol;

/**
 * A response that carries where it was asked for. Only a walk's `ask`
 * (hooks/useCursorPagination) makes one, through `stampAsked`: the page a response was asked for
 * travels with it, and the helpers below take no page number of their own, so no caller can pair
 * a walk's page with a response of another page (a type error, not a convention). The brand is a
 * type alone; the stamp is a plain field, which the query cache's structural sharing keeps.
 */
export type Asked<R> = R & { readonly asked: AskedAt; readonly [ASKED]: true };

/** For a walk's `ask` alone (hooks/useCursorPagination): a response, stamped where it was asked. */
export function stampAsked<R extends object>(read: R, at: AskedAt): Asked<R> {
  return { ...read, asked: at } as Asked<R>;
}

/**
 * A page merged from several reads (a script's cells across its references), asked where all its
 * reads were; where they were not all asked at one place (one still kept from another page while
 * the others load), it is kept data, which offers no next. Null where no read has answered.
 */
export function mergeAsked<R extends object>(
  reads: readonly (Asked<object> | undefined)[],
  merged: R
): Asked<R> | null {
  const at = reads.filter((read): read is Asked<object> => read !== undefined).map((r) => r.asked);
  if (at.length === 0) return null;
  const [first] = at;
  const one = at.every(
    (a) => a.page === first.page && a.cursor === first.cursor && a.size === first.size && !a.kept
  );
  return stampAsked(merged, one ? first : { ...first, kept: true });
}

/**
 * A walk through a cursor-paged list (hooks/useCursorPagination): where it stands, its
 * ways, and `ask`, which reads a page and stamps it where it was asked.
 */
export interface CursorWalk {
  readonly cursor: string | undefined;
  readonly page: number;
  /** The page size the walk asks at (none where the list asks for none of its own). */
  readonly size: number | undefined;
  readonly hasPrevious: boolean;
  /** To the page after the one this response was asked for, where the walk still stands on it. */
  goToNext(read: Asked<{ nextCursor: string | null }>): void;
  goToPrevious(): void;
  reset(): void;
  ask<R extends object>(fetch: (cursor: string | undefined) => Promise<R>): () => Promise<Asked<R>>;
}

/** The pages a count of rows fills at the page size the response states. */
function pageCount(list: string, total: number, limit: number): number {
  if (!Number.isSafeInteger(limit) || limit < 1)
    throw new Error(`${list}: a page size of ${limit} rows`);
  if (!Number.isSafeInteger(total) || total < 0) throw new Error(`${list}: ${total} rows in all`);
  return Math.ceil(total / limit);
}

/**
 * What a response says of itself, checked once for both helpers: the stamp (a response no walk
 * asked for is refused), the page size it served (a whole number of rows: the API clamps a limit
 * to 1–100 and states back the size it served, and the rows before a page are counted at it), its
 * rows (no more than a page), its count (a whole number), and its next cursor (one where more
 * follows, never the cursor it was asked with: an API fault, L1 NOTE 8). And whether it is kept
 * data: a response asked at another page, cursor or page size than the walk stands on, shown while
 * the walk's page loads; it states its own page and offers no next.
 */
function readOf(list: string, read: Asked<CursorPageRead>, walk: CursorWalk) {
  const at = read?.asked;
  if (!at)
    throw new Error(`${list}: a response no walk asked for (a page number is the response's own)`);
  if (!Number.isSafeInteger(read.limit) || read.limit < 1)
    throw new Error(`${list}: a page size of ${read.limit} rows`);
  if (read.data.length > read.limit)
    throw new Error(`${list}: ${read.data.length} rows on a page of ${read.limit}`);
  const total = read.total === undefined || read.total === null ? null : read.total;
  const pages = total === null ? null : pageCount(list, total, read.limit);
  if (read.hasMore && !read.nextCursor)
    throw new Error(`${list}: a page with more rows after it and no next cursor`);
  if (read.hasMore && read.nextCursor === at.cursor)
    throw new Error(
      `${list}: the next cursor repeats the cursor page ${at.page} was asked with (${at.cursor})`
    );
  const kept =
    Boolean(at.kept) || at.page !== walk.page || at.cursor !== walk.cursor || at.size !== walk.size;
  return {
    page: at.page,
    total,
    pages,
    onPrevious: walk.hasPrevious ? walk.goToPrevious : undefined,
    onNext: read.hasMore && !kept ? () => walk.goToNext(read) : undefined,
  };
}

/** Where a page of a ranked list stands, as the pager's `pages` mode draws it. */
export interface RankedPage {
  /** The page whose rows are drawn: the one its response was asked for. */
  page: number;
  /** The page size the response served. */
  pageSize: number;
  /**
   * The rows the page shows, counted from 1; null where the list moved under the reader (an empty
   * page past the first, rows past the count, a page past the pages).
   */
  rows: { from: number; to: number } | null;
  /** The rows in all and the pages they fill, where the page's own response counts them. */
  count: { total: number; pages: number } | null;
  /** A way back, where the walk has one. */
  onPrevious?: () => void;
  /** A way on: the response's own "more", on the page the walk stands on (never kept data). */
  onNext?: () => void;
}

/**
 * Where a page of a ranked cursor-paged list stands, for the pager's `pages` mode
 * (components/ui/pager.tsx), from its response alone (lane cd-fixes-controls, item 1: the page is
 * the one the response was asked for, which travels with it): the rows before it are the full
 * pages before it, each the page size the response states, so it shows rows `from`–`to`; where
 * the response counts the rows, the count and the ⌈total ÷ limit⌉ pages they fill. Null where the
 * list is one page (the first, with nothing after it): no pager is drawn. Its ways to go are the
 * walk's: back where it has a way back, on by the response's own "more" where the walk stands on
 * the response's page (kept data offers no next).
 *
 * States live data reaches, not faults: a list whose response counts no rows (an ordered list, a
 * search) is paged by its rows and its own "more" (lane c2-finish); and a list that moved under
 * the reader (item 2: an empty page past the first; rows past the count; a page past the pages,
 * which the arithmetic (page − 1) × size cannot know: it holds only if the list stood still
 * between the reads) has no rows and no count, its place and its ways. A count is the page's own
 * response's alone: one read at one tip, never a figure another read supplied.
 */
export function rankedPageAt(
  list: string,
  read: Asked<CursorPageRead>,
  walk: CursorWalk
): RankedPage | null {
  const at = readOf(list, read, walk);
  const n = read.data.length;
  if (at.page === 1 && !read.hasMore) return null;
  if (at.page === 1 && n === 0)
    throw new Error(`${list}: an empty first page with more rows after it`);
  const ways = { onPrevious: at.onPrevious, onNext: at.onNext };
  const moved = { page: at.page, pageSize: read.limit, rows: null, count: null, ...ways };
  if (n === 0) return moved;
  const from = (at.page - 1) * read.limit + 1;
  const to = from + n - 1;
  if (at.total !== null && (to > at.total || at.page > at.pages!)) {
    // the first page's rows are its own count's first rows: past it, the response contradicts
    // itself, at one tip
    if (at.page === 1)
      throw new Error(`${list}: rows ${from}–${to} on the first page of ${at.total}`);
    return moved;
  }
  return {
    page: at.page,
    pageSize: read.limit,
    rows: { from, to },
    count: at.total === null ? null : { total: at.total, pages: at.pages! },
    ...ways,
  };
}

/** Where a page of a list in time stands, as the pager's `time` mode draws it. */
export interface TimePage {
  /** The page whose rows are drawn: the one its response was asked for. */
  page: number;
  /** The pages in all, where the response counts the rows and the page lies within them. */
  pages?: number;
  /** Toward the newest end, where the walk has a way back. */
  onNewer?: () => void;
  /** Toward the oldest end: the response's own "more", on the page the walk stands on. */
  onOlder?: () => void;
}

/**
 * Where a page of a cursor-paged list in time stands, for the pager's `time` mode, from its
 * response alone: the page it was asked for and, where the response counts the rows, the
 * ⌈total ÷ limit⌉ pages they fill (none where the page lies past them: the list moved under the
 * reader). Null where the list is one page (the first, with nothing after it): no pager is drawn.
 */
export function timePageAt(
  list: string,
  read: Asked<CursorPageRead>,
  walk: CursorWalk
): TimePage | null {
  const at = readOf(list, read, walk);
  if (at.page === 1 && !read.hasMore) return null;
  const within = at.pages !== null && at.page <= at.pages;
  return {
    page: at.page,
    ...(within ? { pages: at.pages! } : {}),
    onNewer: at.onPrevious,
    onOlder: at.onNext,
  };
}
