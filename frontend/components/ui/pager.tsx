'use client';

import { formatCount, groupCount } from '@/lib/format-count';
import type { RankedPage } from '@/lib/pagination';
import { cn } from '@/lib/utils';

/**
 * A ranked list in pages, as lib/pagination.ts's `rankedPageAt` gives it: the page whose rows are
 * drawn, the rows a page holds, the rows shown (none where the list moved under the reader) and,
 * where the page's own response counts the rows, the count and the pages; and its ways, back and
 * on, each there or not (the helper's, from the walk and the response: lane cd-fixes-controls).
 */
interface PagesProps extends RankedPage {
  mode: 'pages';
  /** The list it pages, for a screen reader (`tokens`). */
  label: string;
}

/** What a page of a list that moved under the reader says where its range stood. */
export const LIST_MOVED = 'the list changed as you paged';

/** A list in time, paged by a cursor: its order, the page where the caller knows it, either end. */
interface TimeProps {
  mode: 'time';
  label: string;
  /** The list's order in words (`newest first`). */
  order: string;
  page?: number;
  /** The pages in all, where the caller knows them. */
  pages?: number;
  /** Absent where the list is at its newest end. */
  onNewer?: () => void;
  /** Absent where the list is at its oldest end. */
  onOlder?: () => void;
}

/** A short list's own forms: the rest of it, the next of a long one. */
interface MoreProps {
  mode: 'more';
  /** `all 8 branch switches ↓`: the count of the whole list, its noun. */
  all?: { count: number; singular: string; plural?: string; onShow: () => void };
  /** `next 50 ↓`: the rows the next load brings. */
  next?: { count: number; onShow: () => void };
}

export type PagerProps = PagesProps | TimeProps | MoreProps;

/** A pager's line: 13 px words in the quiet ink, its parts 18 px apart, wrapping between them. */
const LINE = 'text-small text-text-dim flex flex-wrap items-center gap-x-[18px] gap-y-1';
/** A part's box: one unbroken run, 28 px high from 640 and 32 below it (a thumb's target). */
const PART = 'inline-flex h-8 items-center whitespace-nowrap sm:h-7';

/** A way to go: celadon words; an end that cannot be taken is quiet words, not a control. */
function Go({ words, onGo }: { words: string; onGo?: () => void }) {
  if (!onGo) return <span className={cn(PART, 'text-text-dim')}>{words}</span>;
  return (
    <button
      type="button"
      onClick={onGo}
      className={cn(PART, 'text-jade text-small underline-offset-4 hover:underline')}
    >
      {words}
    </button>
  );
}

/** Where the list is, in the reading ink: `page 3 of 37`, or `page 4` with no count of pages. */
function At({ page, pages }: { page?: number; pages?: number }) {
  if (page === undefined) return null;
  return (
    <span className={cn(PART, 'text-text')}>
      page {groupCount(page)}
      {pages !== undefined && ` of ${groupCount(pages)}`}
    </span>
  );
}

/** A whole figure the caller gave, refused when it is not one. */
function whole(label: string, what: string, n: number): void {
  if (!Number.isSafeInteger(n))
    throw new Error(`Pager "${label}": ${what} ${n} is not a whole number`);
}

/**
 * The pager (the controls board, gate D0′; UI review 2026-11-14, phase C): words, not boxes. Two
 * modes behind one face, the count or the order at the left in the quiet ink and the ways to go at
 * the right: pages, for a ranked list (`41–60 of 727 · 20 a page`, `← prev`, `page 3 of 37`,
 * `next →`; below 640 the page's size leaves the line, as on the board's phone), and a cursor in
 * time (`newest first`, `← newer`, `older →`). Where the list is, is the reading ink; a way to go,
 * celadon; an end that cannot be taken, quiet and not a control. Nothing is drawn for one page.
 * The short list's own forms are a third face of the same words: `all 8 branch switches ↓`,
 * `next 50 ↓`. Every figure is the caller's, grouped (`groupCount`): the pager computes none, and
 * refuses figures that do not hold together.
 */
export function Pager(props: PagerProps) {
  if (props.mode === 'more') {
    const { all, next } = props;
    if (!all && !next) return null;
    return (
      <div className={LINE}>
        {all && (
          <Go
            words={`all ${formatCount(all.count, all.singular, all.plural)} ↓`}
            onGo={all.onShow}
          />
        )}
        {next && <Go words={`next ${groupCount(next.count)} ↓`} onGo={next.onShow} />}
      </div>
    );
  }

  if (props.mode === 'time') {
    const { label, order, page, pages, onNewer, onOlder } = props;
    if (page !== undefined) whole(label, 'page', page);
    if (pages !== undefined) whole(label, 'pages', pages);
    if (pages !== undefined && page === undefined)
      throw new Error(`Pager "${label}": ${pages} pages and no page`);
    if (page !== undefined && pages !== undefined && (page < 1 || page > pages))
      throw new Error(`Pager "${label}": page ${page} of ${pages}`);
    // a list of one page draws no pager by the helper (timePageAt gives null); a page with neither
    // end to take is kept data while the next page loads, drawn where it stands (lane
    // cd-fixes-controls, the second read's NOTE 1)
    return (
      <nav aria-label={label} className={LINE}>
        <span className="mr-auto">{order}</span>
        <span className="flex flex-wrap items-center gap-x-[18px]">
          <Go words="← newer" onGo={onNewer} />
          <At page={page} pages={pages} />
          <Go words="older →" onGo={onOlder} />
        </span>
      </nav>
    );
  }

  const { label, page, pageSize, rows, count, onPrevious, onNext } = props;
  whole(label, 'page', page);
  whole(label, 'pageSize', pageSize);
  if (count) {
    whole(label, 'total', count.total);
    whole(label, 'pages', count.pages);
    if (count.pages < 1) throw new Error(`Pager "${label}": ${count.pages} pages`);
    if (page < 1 || page > count.pages)
      throw new Error(`Pager "${label}": page ${page} of ${count.pages}`);
  } else if (page < 1) throw new Error(`Pager "${label}": page ${page}`);
  if (rows) {
    whole(label, 'from', rows.from);
    whole(label, 'to', rows.to);
    const of = count ? ` of ${count.total}` : '';
    if (rows.from < 1 || rows.from > rows.to || (count && rows.to > count.total))
      throw new Error(`Pager "${label}": rows ${rows.from}–${rows.to}${of}`);
    if (rows.to - rows.from + 1 > pageSize)
      throw new Error(`Pager "${label}": ${rows.to - rows.from + 1} rows on a page of ${pageSize}`);
  } else if (page === 1)
    throw new Error(`Pager "${label}": an empty first page (a list with no rows draws no pager)`);
  return (
    <nav aria-label={label} className={LINE}>
      {/* one unit: the range never breaks at its dash, nor the line before its "·"; a page of a
          list that moved under the reader (lane cd-fixes-controls, item 2) shows no range: it
          says so, once, in the quiet ink */}
      {rows ? (
        <span className="mr-auto whitespace-nowrap">
          {groupCount(rows.from)}–{groupCount(rows.to)}
          {count && ` of ${groupCount(count.total)}`}
          <span className="hidden sm:inline"> · {groupCount(pageSize)} a page</span>
        </span>
      ) : (
        <span className="mr-auto">{LIST_MOVED}</span>
      )}
      <span className="flex flex-wrap items-center gap-x-[18px]">
        <Go words="← prev" onGo={onPrevious} />
        <At page={page} pages={count?.pages} />
        <Go words="next →" onGo={onNext} />
      </span>
    </nav>
  );
}
