import { CELL_HORIZONTAL_CLASSES, cellClassOwner } from '@/components/ui/list-table';
import { SCREENS, stylesheetValue } from '@/__tests__/utils/tailwind-box';

/**
 * What a list table's cells hold, read from the rendered DOM by its classes: jsdom lays nothing
 * out, so a cell's padding and how its text may break are what its classes and its ancestors'
 * classes say. The rules are ListTable's ("A cell holds its content", components/ui/list-table.tsx):
 *
 * - its classes: every head and cell carries the table's horizontal classes (`pl-0`, the gap, none
 *   after the last column), and every other class on it is the table's own or one a column may set,
 *   by the table's one rule (`cellClassOwner`, imported, never written a second time here): a
 *   column's budget is its content and the gap, no more.
 * - a figure never breaks inside itself: no run of characters with a digit in it stands where a
 *   word may break where it overflows (`overflow-wrap: break-word` / `anywhere`,
 *   `word-break: break-all` / `break-word`, as a class, an arbitrary property, an inline style or
 *   a stylesheet rule) unless it is held on one line (`white-space: nowrap`): a figure one pixel
 *   wider than its box shows the shortfall, never "#21,045,84" over "5".
 * - a whole id (a run of 40 characters or more) stands in a box that clips or scrolls it, or wraps
 *   where its own box lets it: it is wider than any column.
 * - words may always break between them: two or more words in one unbreakable run (the text an
 *   element that sets `white-space: nowrap` holds, across its text nodes, but for a descendant
 *   that sets its own white-space again), in a cell of a column that is not numeric, in no box that
 *   clips them and in no chip, are a phrase the cell cannot hold where it is narrower than they
 *   are: they draw over the next column ("this device's clock is behind" in a 94 px Age column; a
 *   234 px headline in a 217 px What it did). A breakpoint's white-space is read at every width: a
 *   table is drawn wherever it fits, so a phrase unbreakable at any width is a fault.
 *
 * Exempt, each a unit budgeted whole by its column: a numeric column's cell (one figure with its
 * label: `reward 9,999.99999999`, `Not yet run` under a bar's head) and a bordered chip (a row tag,
 * a composition badge: `data-row-tag`, `data-composition`).
 *
 * What it reads: every ListTable table (`listTables`) on the page when a test ends (__tests__/setup.ts),
 * a table being a `[data-list-form]` container's own `<table>`; a container of the table forms
 * whose table is not its child is refused, not passed. What it does not see: a table a test
 * unmounts itself before it ends (the width loops do), and a white-space or a word break under a
 * state variant (`hover:`), which it refuses.
 */

export interface CellFault {
  /** The table's accessible name. */
  table: string;
  /** Where: the column head's text, or the row and column of a cell. */
  where: string;
  rule: 'cell class' | 'figure breaks' | 'long run does not break' | 'words do not break';
  text: string;
}

/** A class split into its variants and its utility, outside brackets (`md:pr-9`, `[&_a]:x`). */
function variantsOf(cls: string): { variants: string[]; utility: string } {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < cls.length; i++) {
    if (cls[i] === '[') depth++;
    else if (cls[i] === ']') depth--;
    else if (cls[i] === ':' && depth === 0) {
      parts.push(cls.slice(start, i));
      start = i + 1;
    }
  }
  return { variants: parts, utility: cls.slice(start) };
}

/** A variant that names widths: a screen, below one, or an arbitrary breakpoint. */
const isBreakpoint = (variant: string) =>
  SCREENS.some(([name]) => variant === name || variant === `max-${name}`) ||
  /^(min|max)-\[[^\]]+\]$/.test(variant);

type BreakProp = 'white-space' | 'overflow-wrap' | 'word-break';

/**
 * The value a class sets for a property a line breaks by — a Tailwind utility, an arbitrary
 * property, or a class of app/globals.css — read at every width (a breakpoint's class applies at
 * some width a table is drawn at), or undefined when it sets none. A state variant on such a class
 * is refused: where it holds is not a width the check can read.
 */
function classValue(
  cls: string,
  prop: BreakProp
): { value: string; important: boolean } | undefined {
  const { variants, utility } = variantsOf(cls);
  const bare = utility.replace(/^!/, '');
  const value = READ[prop](bare) ?? (variants.length ? undefined : stylesheetValue(bare, prop));
  if (value === undefined) return undefined;
  const state = variants.find((v) => !isBreakpoint(v));
  if (state !== undefined)
    throw new Error(
      `list-table-cells: ${cls} sets how a line breaks under the state ${state}:, which the check cannot place at a width`
    );
  return { value, important: utility.startsWith('!') };
}

function whiteSpaceOf(utility: string): string | undefined {
  if (utility === 'truncate') return 'nowrap';
  const m = /^whitespace-(normal|nowrap|pre|pre-line|pre-wrap|break-spaces)$/.exec(utility);
  if (m) return m[1];
  return /^\[white-space:([a-z-]+)\]$/.exec(utility)?.[1];
}

function overflowWrapOf(utility: string): string | undefined {
  if (utility === 'break-words') return 'break-word';
  if (utility === 'break-normal') return 'normal';
  return /^\[overflow-wrap:([a-z-]+)\]$/.exec(utility)?.[1];
}

function wordBreakOf(utility: string): string | undefined {
  if (utility === 'break-all') return 'break-all';
  if (utility === 'break-keep') return 'keep-all';
  if (utility === 'break-normal') return 'normal';
  return /^\[word-break:([a-z-]+)\]$/.exec(utility)?.[1];
}

const READ: Record<BreakProp, (utility: string) => string | undefined> = {
  'white-space': whiteSpaceOf,
  'overflow-wrap': overflowWrapOf,
  'word-break': wordBreakOf,
};

/**
 * The values an element's own style and classes set for a property, the inline style over every
 * class and an `!important` class over the rest; every width's value of the winning kind.
 */
function ownValues(el: Element, prop: BreakProp): string[] {
  const style = (el as HTMLElement).style?.getPropertyValue(prop);
  if (style) return [style];
  const found = Array.from(el.classList)
    .map((cls) => classValue(cls, prop))
    .filter((v): v is { value: string; important: boolean } => v !== undefined);
  const important = found.filter((v) => v.important);
  return (important.length ? important : found).map((v) => v.value);
}

type WhiteSpace = 'nowrap' | 'normal' | null;

/** The white-space an element sets itself, null if none: nowrap where it is nowrap at any width. */
function ownWhiteSpace(el: Element): WhiteSpace {
  const values = ownValues(el, 'white-space');
  if (!values.length) return null;
  return values.some((v) => /nowrap|^pre$/.test(v)) ? 'nowrap' : 'normal';
}

/** Whether an element lets a word break where it overflows, or says it may not; null if neither. */
function ownWordBreak(el: Element): 'anywhere' | 'normal' | null {
  const wrap = ownValues(el, 'overflow-wrap');
  const word = ownValues(el, 'word-break');
  if (wrap.some((v) => v !== 'normal') || word.some((v) => v === 'break-all' || v === 'break-word'))
    return 'anywhere';
  if (wrap.includes('normal')) return 'normal';
  return null;
}

/** A box that clips or scrolls what overflows it, so nothing in it draws outside the cell. */
function clips(el: Element): boolean {
  return Array.from(el.classList)
    .map((c) => variantsOf(c).utility.replace(/^!/, ''))
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

function classFault(table: string, where: string, el: Element): CellFault | null {
  const extra = Array.from(el.classList).filter((c) => cellClassOwner(c) === null);
  const missing = CELL_HORIZONTAL_CLASSES.filter((c) => !el.classList.contains(c));
  if (!extra.length && !missing.length) return null;
  return {
    table,
    where,
    rule: 'cell class',
    text: `+[${extra.join(' ')}] −[${missing.join(' ')}]`,
  };
}

/** Every way the cells of a ListTable table break its rules, in reading order. */
export function cellFaults(table: Element): CellFault[] {
  const name = table.getAttribute('aria-label') ?? '(unnamed table)';
  const faults: CellFault[] = [];
  const heads = Array.from(table.querySelectorAll('thead th'));
  const headText = (i: number) => heads[i]?.textContent?.trim() || `column ${i + 1}`;
  for (const [i, th] of heads.entries()) {
    const fault = classFault(name, `head ${headText(i)}`, th);
    if (fault) faults.push(fault);
  }
  for (const [r, tr] of Array.from(table.querySelectorAll('tbody tr')).entries()) {
    for (const [i, td] of Array.from(tr.children).entries()) {
      const where = `row ${r + 1}, ${headText(i)}`;
      const fault = classFault(name, where, td);
      if (fault) faults.push(fault);
      const numeric = td.classList.contains('tabular-nums') && td.classList.contains('text-right');
      // a nowrap run's text, gathered across its text nodes, by the element that holds it on one line
      const runs = new Map<Element, string>();
      const walker = td.ownerDocument.createTreeWalker(td, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = node.textContent ?? '';
        if (!text.trim()) {
          // a space between two parts of a run belongs to it
          const owner = path(node, td).find((el) => ownWhiteSpace(el) !== null);
          if (owner && ownWhiteSpace(owner) === 'nowrap' && runs.has(owner))
            runs.set(owner, runs.get(owner)! + text);
          continue;
        }
        const up = path(node, td);
        if (hidden(up) || up.some(clips)) continue;
        const owner = up.find((el) => ownWhiteSpace(el) !== null);
        const space = owner ? ownWhiteSpace(owner)! : 'normal';
        const breakWord = up.map(ownWordBreak).find((w) => w !== null) ?? 'normal';
        for (const token of text.split(/\s+/).filter(Boolean)) {
          if (token.length >= LONG_RUN) {
            if (space === 'nowrap' || breakWord !== 'anywhere')
              faults.push({ table: name, where, rule: 'long run does not break', text: token });
          } else if (/\d/.test(token) && space !== 'nowrap' && breakWord === 'anywhere') {
            faults.push({ table: name, where, rule: 'figure breaks', text: token });
          }
        }
        if (space === 'nowrap' && !numeric && !up.some(chip))
          runs.set(owner!, (runs.get(owner!) ?? '') + text);
      }
      for (const run of runs.values())
        if (words(run).length >= 2)
          faults.push({
            table: name,
            where,
            rule: 'words do not break',
            text: run.replace(/\s+/g, ' ').trim(),
          });
    }
  }
  return faults;
}

/**
 * Every ListTable table under `root`: the `<table>` a `[data-list-form]` container holds as its
 * child. A container in a table form with neither its table nor its empty-list sentence as a child
 * is refused: a wrapper around the table would hide it from the check.
 */
export function listTables(root: ParentNode = document): HTMLTableElement[] {
  for (const form of Array.from(
    root.querySelectorAll('[data-list-form="table"], [data-list-form="narrow-table"]')
  ))
    // its children, read one by one (jsdom's `:scope >` matches a grandchild too)
    if (!Array.from(form.children).some((child) => ['TABLE', 'P'].includes(child.tagName)))
      throw new Error(
        `list-table-cells: a [data-list-form="${form.getAttribute('data-list-form')}"] holds no <table> of its own; the check cannot read a table wrapped in another box`
      );
  return Array.from(root.querySelectorAll<HTMLTableElement>('[data-list-form] > table'));
}

/** Each fault as one line, for an error that names where every one is. */
export function faultLines(faults: readonly CellFault[]): string[] {
  return faults.map((f) => `${f.table} · ${f.where} · ${f.rule}: ${JSON.stringify(f.text)}`);
}

/** The after-test check (__tests__/setup.ts): every ListTable table on the page, or an error naming each fault. */
export function assertListTables(root: ParentNode = document): void {
  const faults = listTables(root).flatMap(cellFaults);
  if (faults.length)
    throw new Error(
      `A list table's cells break its rules (components/ui/list-table.tsx, "A cell holds its content"):\n${faultLines(faults).join('\n')}`
    );
}
