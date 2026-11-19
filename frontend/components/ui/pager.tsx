'use client';

import { formatCount, groupCount } from '@/lib/format-count';
import { cn } from '@/lib/utils';

/** A ranked list in pages: the rows shown, the rows in all, a page's size, the page and the pages. */
interface PagesProps {
  mode: 'pages';
  /** The list it pages, for a screen reader (`tokens`). */
  label: string;
  /** The first and the last row shown, counted from 1. */
  from: number;
  to: number;
  /** The rows in all. */
  total: number;
  /** The rows a page holds. */
  pageSize: number;
  page: number;
  pages: number;
  onPrevious: () => void;
  onNext: () => void;
}

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
    if (!onNewer && !onOlder) return null;
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

  const { label, from, to, total, pageSize, page, pages, onPrevious, onNext } = props;
  for (const [what, n] of Object.entries({ from, to, total, pageSize, page, pages }))
    whole(label, what, n);
  if (pages < 1) throw new Error(`Pager "${label}": ${pages} pages`);
  if (pages === 1) return null;
  if (page < 1 || page > pages) throw new Error(`Pager "${label}": page ${page} of ${pages}`);
  if (from < 1 || from > to || to > total)
    throw new Error(`Pager "${label}": rows ${from}–${to} of ${total}`);
  if (to - from + 1 > pageSize)
    throw new Error(`Pager "${label}": ${to - from + 1} rows on a page of ${pageSize}`);
  return (
    <nav aria-label={label} className={LINE}>
      {/* one unit: the range never breaks at its dash, nor the line before its "·" */}
      <span className="mr-auto whitespace-nowrap">
        {groupCount(from)}–{groupCount(to)} of {groupCount(total)}
        <span className="hidden sm:inline"> · {groupCount(pageSize)} a page</span>
      </span>
      <span className="flex flex-wrap items-center gap-x-[18px]">
        <Go words="← prev" onGo={page > 1 ? onPrevious : undefined} />
        <At page={page} pages={pages} />
        <Go words="next →" onGo={page < pages ? onNext : undefined} />
      </span>
    </nav>
  );
}
