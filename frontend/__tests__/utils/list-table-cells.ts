import { CELL_HORIZONTAL_CLASSES } from '@/components/ui/list-table';

/**
 * What a list table's cells hold, read from the rendered DOM by its classes: jsdom lays nothing
 * out, so a cell's padding and how its text may break are what its classes and its ancestors'
 * classes say. The rules are ListTable's ("A cell holds its content", components/ui/list-table.tsx),
 * written here a second time, from the CSS each class sets, not from the component's constants:
 *
 * - padding: every head and cell carries exactly the table's horizontal classes (`pl-0`, the gap,
 *   none after the last column) and no other class that takes a horizontal pixel (a padding, a
 *   margin, a width, a side border): a column's budget is its content and the gap, no more.
 * - a figure never breaks inside itself: no run of characters with a digit in it stands where a
 *   word may break where it overflows (`overflow-wrap: break-word` / `anywhere`,
 *   `word-break: break-all`) unless it is held on one line (`white-space: nowrap`): a figure one
 *   pixel wider than its box shows the shortfall, never "#21,045,84" over "5".
 * - a whole id (a run of 40 characters or more) stands in a box that clips or scrolls it, or wraps
 *   where its own box lets it: it is wider than any column.
 * - words may always break between them: two or more words in one unbreakable run (under
 *   `white-space: nowrap`), in a cell of a column that is not numeric, in no box that clips them and
 *   in no chip, are a phrase the cell cannot hold where it is narrower than they are: they draw over
 *   the next column ("this device's clock is behind" in a 94 px Age column; a 234 px headline in a
 *   217 px What it did).
 *
 * Exempt, each a unit budgeted whole by its column: a numeric column's cell (one figure with its
 * label: `reward 9,999.99999999`, `Not yet run` under a bar's head) and a bordered chip (a row tag,
 * a composition badge: `data-row-tag`, `data-composition`).
 */

export interface CellFault {
  /** The table's accessible name. */
  table: string;
  /** Where: the column head's text, or the row and column of a cell. */
  where: string;
  rule: 'padding' | 'figure breaks' | 'long run does not break' | 'words do not break';
  text: string;
}

/** A class that takes a horizontal pixel of a cell, read without its variant and its `!`. */
const HORIZONTAL_SPACE =
  /^-?(p|px|pl|pr|ps|pe|m|mx|ml|mr|ms|me|w|min-w|max-w|size)-|^border(-[xlrse])?(-(\d+|\[[^\]]+\]))?$|^\[(padding|margin|width|min-width|max-width|border)(-(left|right|inline|inline-start|inline-end))?:/;

/** The class without its state or breakpoint variants (`last:`, `md:`) and without its `!`. */
function bare(cls: string): string {
  return cls.replace(/^([a-z0-9-]+:)*/, '').replace(/^!/, '');
}

/** `min-w-0` lets a cell shrink and takes no pixel (ListTable writes it on every cell). */
function horizontalClasses(el: Element): string[] {
  return Array.from(el.classList).filter((c) => c !== 'min-w-0' && HORIZONTAL_SPACE.test(bare(c)));
}

type WhiteSpace = 'nowrap' | 'normal' | null;

/**
 * The white-space an element's own style or classes set, null if they set none. A breakpoint
 * variant (`md:whitespace-nowrap`) is not read: a table is drawn at every width it fits.
 */
function ownWhiteSpace(el: Element): WhiteSpace {
  const style = (el as HTMLElement).style?.whiteSpace;
  if (style) return /nowrap|^pre$/.test(style) ? 'nowrap' : 'normal';
  let found: WhiteSpace = null;
  let important = false;
  for (const cls of Array.from(el.classList)) {
    if (/^(sm|md|lg|xl|2xl|max-[a-z]+):/.test(cls)) continue;
    const imp = cls.startsWith('!');
    const c = cls.replace(/^!/, '');
    const value: WhiteSpace =
      c === 'whitespace-nowrap' || c === 'truncate' || c === 'whitespace-pre'
        ? 'nowrap'
        : /^whitespace-(normal|pre-line|pre-wrap|break-spaces)$/.test(c)
          ? 'normal'
          : null;
    if (value === null) continue;
    if (imp || !important) {
      found = value;
      important = imp;
    }
  }
  return found;
}

/** Whether an element lets a word break where it overflows, or says it may not; null if neither. */
function ownWordBreak(el: Element): 'anywhere' | 'normal' | null {
  const classes = Array.from(el.classList).map(bare);
  if (classes.some((c) => c === 'break-normal' || c === '[overflow-wrap:normal]')) return 'normal';
  if (
    classes.some((c) =>
      ['break-words', 'break-all', '[overflow-wrap:anywhere]', '[word-break:break-all]'].includes(c)
    )
  )
    return 'anywhere';
  return null;
}

/** A box that clips or scrolls what overflows it, so nothing in it draws outside the cell. */
function clips(el: Element): boolean {
  return Array.from(el.classList)
    .map(bare)
    .some((c) =>
      /^(truncate|overflow-hidden|overflow-x-hidden|overflow-clip|overflow-x-clip|overflow-auto|overflow-x-auto|overflow-scroll|overflow-x-scroll|line-clamp-\d+)$/.test(
        c
      )
    );
}

/** A bordered chip: one unit its column budgets whole, as a figure is. */
function chip(el: Element): boolean {
  return el.hasAttribute('data-row-tag') || el.hasAttribute('data-composition');
}

/** The elements from a text node's parent up to and including its cell. */
function path(node: Node, cell: Element): Element[] {
  const out: Element[] = [];
  for (let el = node.parentElement; el; el = el.parentElement) {
    out.push(el);
    if (el === cell) break;
  }
  return out;
}

function hidden(els: readonly Element[]): boolean {
  return els.some((el) => el.classList.contains('sr-only') || el.hasAttribute('hidden'));
}

/**
 * A run this long is a whole id (a hash is 66 characters, an address 46 and more), wider than any
 * column.
 */
const LONG_RUN = 40;

/** The words of a text that hold two letters or more: a phrase, not a figure's unit or sign. */
function words(text: string): string[] {
  return text.split(/\s+/).filter((w) => /[A-Za-z]{2,}/.test(w));
}

function paddingFault(table: string, where: string, el: Element): CellFault | null {
  const extra = horizontalClasses(el).filter((c) => !CELL_HORIZONTAL_CLASSES.includes(c));
  const missing = CELL_HORIZONTAL_CLASSES.filter((c) => !el.classList.contains(c));
  if (!extra.length && !missing.length) return null;
  return { table, where, rule: 'padding', text: `+[${extra.join(' ')}] −[${missing.join(' ')}]` };
}

/** Every way the cells of a ListTable table break its rules, in reading order. */
export function cellFaults(table: Element): CellFault[] {
  const name = table.getAttribute('aria-label') ?? '(unnamed table)';
  const faults: CellFault[] = [];
  const heads = Array.from(table.querySelectorAll('thead th'));
  const headText = (i: number) => heads[i]?.textContent?.trim() || `column ${i + 1}`;
  for (const [i, th] of heads.entries()) {
    const fault = paddingFault(name, `head ${headText(i)}`, th);
    if (fault) faults.push(fault);
  }
  for (const [r, tr] of Array.from(table.querySelectorAll('tbody tr')).entries()) {
    for (const [i, td] of Array.from(tr.children).entries()) {
      const where = `row ${r + 1}, ${headText(i)}`;
      const fault = paddingFault(name, where, td);
      if (fault) faults.push(fault);
      const numeric = td.classList.contains('tabular-nums') && td.classList.contains('text-right');
      const walker = td.ownerDocument.createTreeWalker(td, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = node.textContent ?? '';
        if (!text.trim()) continue;
        const up = path(node, td);
        if (hidden(up) || up.some(clips)) continue;
        const space = up.map(ownWhiteSpace).find((w) => w !== null) ?? 'normal';
        const breakWord = up.map(ownWordBreak).find((w) => w !== null) ?? 'normal';
        for (const token of text.split(/\s+/).filter(Boolean)) {
          if (token.length >= LONG_RUN) {
            if (space === 'nowrap' || breakWord !== 'anywhere')
              faults.push({ table: name, where, rule: 'long run does not break', text: token });
          } else if (/\d/.test(token) && space !== 'nowrap' && breakWord === 'anywhere') {
            faults.push({ table: name, where, rule: 'figure breaks', text: token });
          }
        }
        if (space === 'nowrap' && !numeric && !up.some(chip) && words(text).length >= 2)
          faults.push({ table: name, where, rule: 'words do not break', text: text.trim() });
      }
    }
  }
  return faults;
}

/** Every ListTable table under `root` (its container carries `data-list-form`). */
export function listTables(root: ParentNode = document): HTMLTableElement[] {
  return Array.from(root.querySelectorAll<HTMLTableElement>('[data-list-form] > table'));
}

/** Each fault as one line, for an error that names where every one is. */
export function faultLines(faults: readonly CellFault[]): string[] {
  return faults.map((f) => `${f.table} · ${f.where} · ${f.rule}: ${JSON.stringify(f.text)}`);
}
