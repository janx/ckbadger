import { formatBlockNumber } from '@/lib/format-block';
import type { ChainMode } from '@/lib/chain-mode';
import { fontPx, textPx } from '@/lib/type-metrics';
import type { TypeStep } from '@/lib/type-scale';

/**
 * The shell's budget at the type scale (readability plan 2026-11-18, lane F3-f): what the header,
 * the vitals strip and the footer need for their widest real content, computed from
 * lib/type-metrics.ts (every glyph 0.6 em) and the geometry their classes draw, so that a test can
 * state, per reference width, what stands in each line and that it fits. The status line's budget
 * is the app's own (lib/status-line-fit.ts: the line measures itself and keeps the parts its width
 * holds); what only the tests read lives here, beside them. `__tests__/components/shell-budget.test.tsx`
 * pins the classes each number below is read from, so a change of either side fails it.
 */

/** A width to the hundredth of a pixel, as the budgets are written (binary floating point aside). */
export function px(n: number): number {
  return Math.round(n * 100) / 100;
}

/** The width of a run of text at a step of the scale (the shell's text is JetBrains Mono). */
export function runPx(text: string, step: TypeStep): number {
  return textPx([...text].length, fontPx(step));
}

const sum = (xs: readonly number[]) => xs.reduce((a, b) => a + b, 0);

/** The widths every part of the shell is budgeted at, the doc's and the lead's measuring set. */
export const SHELL_WIDTHS = [1280, 1024, 768, 640, 390, 360, 320] as const;

/**
 * The page's content width at a viewport (tailwind.config.ts `container`): locked at 640, 768,
 * 1024 and 1280 from those widths, fluid below 640; a 16 px gutter, 24 px from lg.
 */
export function contentPx(viewport: number): number {
  if (viewport >= 1280) return 1280 - 48;
  if (viewport >= 1024) return 1024 - 48;
  if (viewport >= 768) return 768 - 32;
  if (viewport >= 640) return 640 - 32;
  return viewport - 32;
}

/**
 * The widest real content each part is budgeted for, as the site writes it.
 *
 * - A block height of nine digits (`#999,999,999`): mainnet stood at #21,045,159 on 2026-11-18, a
 *   block every 8.9 s or so, so the ninth digit is some 22 years away; eight is today's.
 * - An epoch number of five digits (15,317 on 2026-11-18, six epochs a day: 99,999 in about 38
 *   years).
 * - `index / length` of the longest epoch: CKB caps an epoch at 1,800 blocks (4 h at the 8 s least
 *   block interval), the index one less than the length.
 * - The time left as the API's `format_duration` (crates/api/src/utils/format.rs) writes its
 *   longest under 100 days: `23h 59m` (`Nd Nh` reaches seven characters only from ten days).
 * - The block time as the API writes it (two decimals) and the strip spaces it, under 100 s.
 * - The hash rate as `format_hash_rate` writes it: under 1,000 of each unit but EH/s, two
 *   decimals, a four-letter unit. The difficulty as `format_difficulty` does: the same, one letter.
 * - The live mark's longer word, `Not live`; the mode's longer words, `SYNCING` and `OFFLINE`.
 * - The network names the code knows (crates/common/src/hardfork.rs `NETWORK_*`: the orchestrator
 *   refuses any other and gives the frontend the canonical one), seven letters each.
 * - The release as built (Cargo.toml's 0.8.2; `releaseOf` drops the build's branch and commit) in
 *   its longer state, `· syncing`.
 */
export const WIDEST = {
  block: formatBlockNumber(999_999_999),
  blockToday: formatBlockNumber(99_999_999),
  epoch: (99_999).toLocaleString('en-US'),
  position: `${(1_799).toLocaleString('en-US')} / ${(1_800).toLocaleString('en-US')}`,
  left: '≈ 23h 59m left',
  blockTime: '99.99 s',
  hashRate: '999.99 PH/s',
  difficulty: '999.99 P',
  live: 'Not live',
  modes: ['LIVE', 'SYNCING', 'OFFLINE'] as const satisfies readonly ChainMode[],
  networks: ['mainnet', 'testnet'] as const,
  release: 'ckbadger 0.8.2 · syncing',
} as const;

// ---------------------------------------------------------------------------------------------
// The header row (components/layout/header.tsx, the search field components/search-bar.tsx)

/** The wordmark: the 8 px prompt square, its 10 px gap (`gap-2.5`), `ckbadger` at 15 px. */
export const WORDMARK_PX = 8 + 10 + textPx('ckbadger'.length, 15);

/** The row's gap between the wordmark, the nav and the search: `gap-2`, `md:gap-8`. */
const ROW_GAP = { belowMd: 8, fromMd: 32 } as const;

/** The nav's gap between entries: `gap-x-4`, `lg:gap-x-6`. */
const NAV_GAP = { md: 16, lg: 24 } as const;

/** A nav menu's chevron after its label: `gap-1` and the 12 px chevron (`h-3 w-3`). */
const MENU_CHEVRON_PX = 4 + 12;

/** A nav entry, its label as written (lowercased by CSS: the same length) and whether it is a menu. */
export interface NavEntry {
  label: string;
  menu: boolean;
}

/** A nav entry's width at the body step. */
export function navEntryPx(entry: NavEntry): number {
  return runPx(entry.label, 'body') + (entry.menu ? MENU_CHEVRON_PX : 0);
}

/** The eyebrow's tracking (`.eyebrow` in app/globals.css): 0.12 em after every glyph. */
export const EYEBROW_TRACKING_EM = 0.12;

/**
 * The network switcher in the nav, around the active network's name in the eyebrow (uppercase,
 * 0.12 em tracked, the meta step): the button's 1 px border and `px-2` each side, the 6 px dot,
 * two `gap-1`, the 12 px chevron; then the wrapper's `md:pr-2`, its `md:border-r` and its
 * `md:mr-1` outside it.
 */
export function switcherPx(name: string): number {
  const n = [...name].length;
  const meta = fontPx('meta');
  const label = textPx(n, meta) + n * EYEBROW_TRACKING_EM * meta;
  return 1 + 8 + 6 + 4 + label + 4 + 12 + 8 + 1 + 8 + 1 + 4;
}

/** The menu button below md (`h-10 w-10`) and the gap before the section word (`gap-2`). */
const MENU_BUTTON_PX = 40;
const SECTION_WORD_GAP = 8;

/** The theme switcher from md (`h-9 w-9`) and the gap before it (`gap-3`). */
const THEME_PX = 36;
const THEME_GAP = 12;

/** The field's cap: `max-w-[clamp(18rem,36vw,36rem)]`. */
export function fieldCapPx(viewport: number): number {
  return Math.min(Math.max(288, 0.36 * viewport), 576);
}

/**
 * The field's text box insets: the 1 px border and `pl-7` (the prompt square's room) on the left;
 * on the right `md:pr-9` (the `/` key's room, drawn from md) or `pr-3` below md, and the border.
 */
export const FIELD_INSETS = { fromMd: 1 + 28 + 36 + 1, belowMd: 1 + 28 + 12 + 1 } as const;

/** The field's placeholders (components/search-bar.tsx): 34 characters and 6. */
export const PLACEHOLDER = { long: 'search block / tx / address / cell', short: 'search' } as const;

/**
 * The field's floor: a field that holds its short placeholder and, from md, the `/` key's room
 * with their insets (RESPONSIVE_LAYOUT gives no other). 116.4 px from md, 92.4 below.
 */
export function fieldFloorPx(fromMd: boolean): number {
  return (fromMd ? FIELD_INSETS.fromMd : FIELD_INSETS.belowMd) + runPx(PLACEHOLDER.short, 'body');
}

export interface HeaderRow {
  /** The nav's width (with the switcher where it is drawn); 0 below md, where the menu has it. */
  nav: number;
  /** What the row leaves the search field, capped by the field's clamp. */
  field: number;
  floor: number;
  placeholder: 'long' | 'short';
}

/**
 * The header row at a viewport: from md the wordmark, the nav (the switcher first when two
 * networks are configured), the field and the theme switcher; below md the wordmark, the menu
 * button, from 640 the section word (`sm:inline`), and the field.
 */
export function headerRow(
  viewport: number,
  {
    nav,
    switcher,
    word,
  }: { nav: readonly NavEntry[]; switcher: string | null; word: string | null }
): HeaderRow {
  const content = contentPx(viewport);
  const fromMd = viewport >= 768;
  let navPx = 0;
  let room: number;
  if (fromMd) {
    const items = nav.map(navEntryPx);
    if (switcher !== null) items.unshift(switcherPx(switcher));
    navPx = sum(items) + (items.length - 1) * (viewport >= 1024 ? NAV_GAP.lg : NAV_GAP.md);
    room = content - WORDMARK_PX - 2 * ROW_GAP.fromMd - navPx - THEME_GAP - THEME_PX;
  } else {
    const wordPx = viewport >= 640 && word !== null ? SECTION_WORD_GAP + runPx(word, 'body') : 0;
    room = content - WORDMARK_PX - 2 * ROW_GAP.belowMd - MENU_BUTTON_PX - wordPx;
  }
  const field = px(Math.min(fieldCapPx(viewport), room));
  const box = field - (fromMd ? FIELD_INSETS.fromMd : FIELD_INSETS.belowMd);
  return {
    nav: px(navPx),
    field,
    floor: px(fieldFloorPx(fromMd)),
    placeholder: box >= runPx(PLACEHOLDER.long, 'body') ? 'long' : 'short',
  };
}

// ---------------------------------------------------------------------------------------------
// The vitals strip (components/stats-bar.tsx)

/** One line of a strip or a footer: its items' names and its width with the gaps between them. */
export interface Line {
  items: string[];
  px: number;
}

/** Items laid out as a wrapping flex row lays them: each on the line it fits, in order. */
export function wrapLines(
  items: readonly { name: string; px: number }[],
  gap: number,
  line: number
): Line[] {
  const lines: Line[] = [];
  for (const item of items) {
    const last = lines[lines.length - 1];
    if (last && px(last.px + gap + item.px) <= line) {
      last.items.push(item.name);
      last.px = px(last.px + gap + item.px);
    } else lines.push({ items: [item.name], px: px(item.px) });
  }
  return lines;
}

/** A vital's label and value: the label at `label`, the value at the body step, `gap` apart. */
function pairPx(label: string, value: string, labelStep: TypeStep, gap: number): number {
  return runPx(label, labelStep) + gap + runPx(value, 'body');
}

/** The epoch's two groups: its number with its bar, and its position with the time left. */
export function epochGroupsPx(
  fromMd: boolean,
  cells: 5 | 10,
  content: { epoch: string; position: string; left: string } = WIDEST
): { bar: number; position: number } {
  const gap = fromMd ? 8 : 6;
  const step: TypeStep = fromMd ? 'small' : 'meta';
  return {
    bar:
      runPx('Epoch', step) + gap + runPx(content.epoch, 'body') + gap + textPx(cells, fontPx(step)),
    position: runPx(content.position, step) + gap + runPx(content.left, step),
  };
}

/** The live mark: its 7 px dot, and from 640 (`max-sm:sr-only` in the one line) `gap-2` and the word. */
function livePx(word: boolean): number {
  return 7 + (word ? 8 + runPx(WIDEST.live, 'small') : 0);
}

/**
 * The strip from md: one wrapping row 24 px apart (`md:gap-x-6`), labels at the small step, values
 * at the body step 8 px after them (`md:gap-2`), the ten-cell bar, Live at its end.
 */
export function stripFromMd(viewport: number, block: string = WIDEST.block): Line[] {
  const epoch = epochGroupsPx(true, 10);
  return wrapLines(
    [
      { name: 'block', px: pairPx('Block', block, 'small', 8) },
      { name: 'epoch', px: epoch.bar + 8 + epoch.position },
      { name: 'block time', px: pairPx('Block time', WIDEST.blockTime, 'small', 8) },
      { name: 'hash rate', px: pairPx('Hash rate', WIDEST.hashRate, 'small', 8) },
      { name: 'difficulty', px: pairPx('Difficulty', WIDEST.difficulty, 'small', 8) },
      { name: 'live', px: livePx(true) },
    ],
    24,
    contentPx(viewport)
  );
}

/**
 * The strip below md on every page but the home page: one wrapping line 12 px apart (`gap-x-3`),
 * the block, the epoch with its bar (five cells below 640, ten from it), and the live mark (its dot
 * alone below 640) where the page holds the socket.
 */
export function stripLine(
  viewport: number,
  { block, live }: { block: string; live: boolean }
): Line[] {
  const sm = viewport >= 640;
  const items = [
    { name: 'block', px: pairPx('Block', block, 'meta', 6) },
    { name: 'epoch', px: epochGroupsPx(false, sm ? 10 : 5).bar },
  ];
  if (live) items.push({ name: 'live', px: livePx(sm) });
  return wrapLines(items, 12, contentPx(viewport));
}

/** A mining figure below md: label over value below 640 (its width the wider), beside it from 640. */
function miningPx(label: string, value: string, sm: boolean): number {
  return sm
    ? pairPx(label, value, 'meta', 6)
    : Math.max(runPx(label, 'meta'), runPx(value, 'body'));
}

export interface StripLines {
  /** Line 1: the block (tracks 1–2, 1–3 from 640) and Live (the last track). */
  first: { block: number; live: number; blockRoom: number; liveRoom: number };
  /** Line 2: the epoch across the line, as one line or as its two groups on two. */
  epoch: Line[];
  /** Line 3: the three mining figures, one track each. */
  mining: { px: number[]; tracks: number[] };
}

/**
 * The home page's strip below md (`grid`, `gap-x-4`): three equal tracks below 640, whose sizes
 * grow to a figure that needs more (a `1fr` track is at least its content); from 640 three tracks
 * of the mining figures' own widths and a fourth that takes the rest.
 */
export function stripLines(viewport: number, block: string = WIDEST.block): StripLines {
  const line = contentPx(viewport);
  const sm = viewport >= 640;
  const mining = [
    miningPx('Block time', WIDEST.blockTime, sm),
    miningPx('Hash rate', WIDEST.hashRate, sm),
    miningPx('Difficulty', WIDEST.difficulty, sm),
  ].map(px);
  const live = livePx(true);
  let tracks: number[];
  if (sm) {
    tracks = [...mining, line - sum(mining) - 3 * 16];
  } else {
    // the fr rule: a track whose content needs more than its share keeps its content; the others
    // share what is left
    const need = [mining[0], mining[1], Math.max(mining[2], live)];
    const fixed = new Set<number>();
    for (;;) {
      if (fixed.size === need.length) {
        tracks = need;
        break;
      }
      const free = line - 2 * 16 - sum([...fixed].map((i) => need[i]));
      const share = free / (3 - fixed.size);
      const grow = need.findIndex((n, i) => !fixed.has(i) && n > share);
      if (grow === -1) {
        tracks = need.map((n, i) => (fixed.has(i) ? n : share));
        break;
      }
      fixed.add(grow);
    }
  }
  const blockRoom = sm ? sum(tracks.slice(0, 3)) + 2 * 16 : tracks[0] + 16 + tracks[1];
  const groups = epochGroupsPx(false, sm ? 10 : 5);
  return {
    first: {
      block: px(pairPx('Block', block, 'meta', 6)),
      live: px(live),
      blockRoom: px(blockRoom),
      liveRoom: px(tracks[tracks.length - 1]),
    },
    epoch: wrapLines(
      [
        { name: 'epoch and bar', px: groups.bar },
        { name: 'position and time left', px: groups.position },
      ],
      6,
      line
    ),
    mining: { px: mining, tracks: tracks.slice(0, 3).map(px) },
  };
}

// ---------------------------------------------------------------------------------------------
// The status line as drawn (components/layout/status-line.tsx), for the component's own tests

/** Whether an element of the line is drawn at a viewport below md or from it, by its classes. */
function drawn(el: Element, fromMd: boolean): boolean {
  if (el.classList.contains('sr-only')) return false;
  if (!el.classList.contains('hidden')) return true;
  return fromMd && ['md:flex', 'md:inline', 'md:block'].some((c) => el.classList.contains(c));
}

/** The characters of the text an element draws (an `sr-only` run is no box in the line). */
function drawnChars(node: Node, fromMd: boolean): number {
  if (node.nodeType === Node.TEXT_NODE) return [...(node.textContent ?? '')].length;
  if (node instanceof Element && !drawn(node, fromMd)) return 0;
  return sum(Array.from(node.childNodes).map((c) => drawnChars(c, fromMd)));
}

/** The flex items of a box that are drawn: its elements that are drawn and its non-empty texts. */
function drawnRuns(el: Element, fromMd: boolean): Node[] {
  return Array.from(el.childNodes).filter((c) =>
    c.nodeType === Node.TEXT_NODE
      ? (c.textContent ?? '') !== ''
      : c instanceof Element && drawn(c, fromMd)
  );
}

/**
 * The width the status line's drawn parts take, measured as the type model measures them at the
 * line's meta step with the insets its classes give: the mode block `px-2.5` each side; a segment
 * `px-3` each side, its 1 px line, `gap-1.5` between its runs; the right-hand group (`ml-auto`)
 * the sum of its segments. What a class hides at the viewport, and an `sr-only` run, take no room.
 */
export function drawnStatusLinePx(bar: HTMLElement, fromMd: boolean): number {
  const meta = fontPx('meta');
  const partPx = (el: Element): number => {
    if (!drawn(el, fromMd)) return 0;
    if (el.getAttribute('data-testid') === 'status-mode')
      return textPx(drawnChars(el, fromMd), meta) + 20;
    if (el.classList.contains('ml-auto')) return sum(Array.from(el.children).map(partPx));
    const runs = drawnRuns(el, fromMd);
    return textPx(drawnChars(el, fromMd), meta) + 6 * (runs.length - 1) + 2 * 12 + 1;
  };
  return px(sum(Array.from(bar.children).map(partPx)));
}

// ---------------------------------------------------------------------------------------------
// The footer (components/layout/site-footer.tsx)

/**
 * The footer's links (text-small, `gap-x-5`), as many to a line as the line holds; "Keyboard
 * shortcuts" from md only. Below md a line of its own says the release and the clock's zone, drawn
 * 2ch to the left so the `· ` before each line's first token is clipped.
 */
export function footerLines(viewport: number, entries: readonly string[]): Line[] {
  return wrapLines(
    entries.map((e) => ({ name: e, px: runPx(e, 'small') })),
    20,
    contentPx(viewport)
  );
}

/** The release-and-zone line below md, as one line (`ckbadger 0.8.2 · asia/shanghai`). */
export function releaseAndZonePx(release: string, zone: string): number {
  return runPx(`ckbadger ${release} · ${zone}`, 'small');
}
