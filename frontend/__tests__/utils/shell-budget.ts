import { WIDEST_BLOCK_NUMBER } from '@/lib/list-bounds';
import {
  FIELD_INSETS_PX,
  SEARCH_PLACEHOLDER,
  SEARCH_PLACEHOLDER_SHORT,
  WORDMARK_PX,
  headerFieldFloorPx,
  headerFieldRoomPx,
  headerNavEntryPx,
  headerNavPx,
  headerRowForm,
  headerSwitcherPx,
  type HeaderNavEntry,
  type HeaderRowForm,
} from '@/lib/header-fit';
import type { ChainMode } from '@/lib/chain-mode';
import { fontPx, textPx } from '@/lib/type-metrics';
import type { TypeStep } from '@/lib/type-scale';
import { contentPx as drawnContentPx } from './tailwind-box';

/**
 * The shell's budget at the type scale (readability plan 2026-11-18, lane F3-f): what the header,
 * the vitals strip and the footer need for their widest real content, computed from
 * lib/type-metrics.ts (every glyph 0.6 em) and the geometry their classes draw, so that a test can
 * state, per reference width, what stands in each line and that it fits. The status line's and the
 * header row's budgets are the app's own (lib/status-line-fit.ts, lib/header-fit.ts: each measures
 * its line and keeps the parts its width holds), read here at the reference widths; what only the
 * tests read, the strip's and the footer's, lives here, beside them. `__tests__/components/shell-budget.test.tsx`
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
 * - A block height as the lists bound it, one bound for one quantity (lib/list-bounds.ts
 *   `WIDEST_BLOCK_NUMBER`, `#99,999,999`, eight digits of record: mainnet stood at #21,045,159 on
 *   2026-11-18, a block every 8.9 s or so, so the ninth digit is some 22 years away).
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
  block: WIDEST_BLOCK_NUMBER,
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
// The header row (components/layout/header.tsx): its budget is the app's own, lib/header-fit.ts,
// read here at the reference widths with the container's content width for the row.

export { WORDMARK_PX };

/** A nav entry: its label as written and whether it opens a menu (lib/header-fit.ts). */
export type NavEntry = HeaderNavEntry;

/** A nav entry's width at the body step. */
export const navEntryPx = headerNavEntryPx;

/** The switcher in the nav, its name in the tracked eyebrow, with its margin. */
export const switcherPx = headerSwitcherPx;

/** The field's floor: its short placeholder and, from md, the `/` key's room, with their insets. */
export const fieldFloorPx = headerFieldFloorPx;

/** The field's placeholders (lib/header-fit.ts, drawn by components/search-bar.tsx). */
export const PLACEHOLDER = { long: SEARCH_PLACEHOLDER, short: SEARCH_PLACEHOLDER_SHORT } as const;

/** The field's cap: `max-w-[clamp(18rem,36vw,36rem)]`. */
export function fieldCapPx(viewport: number): number {
  return Math.min(Math.max(288, 0.36 * viewport), 576);
}

export interface HeaderRow {
  /** The row's form: the nav standing in it, or the nav in the menu (lib/header-fit.ts). */
  form: HeaderRowForm;
  /** The nav's width where it stands in the row; 0 in the menu form. */
  nav: number;
  /** What the row leaves the search field in its form, capped by the field's clamp. */
  field: number;
  floor: number;
  placeholder: 'long' | 'short';
}

/**
 * The header row at a viewport, its row measured at the container's content width: the form
 * lib/header-fit.ts decides, and what that form leaves the field.
 */
export function headerRow(
  viewport: number,
  parts: { nav: readonly NavEntry[]; switcher: string | null; word: string | null }
): HeaderRow {
  const row = contentPx(viewport);
  const at = { fromSm: viewport >= 640, fromMd: viewport >= 768, fromLg: viewport >= 1024 };
  const form = headerRowForm(row, parts, at);
  const field = px(Math.min(fieldCapPx(viewport), headerFieldRoomPx(form, row, parts, at)));
  const insets = at.fromMd ? FIELD_INSETS_PX.fromMd : FIELD_INSETS_PX.belowMd;
  return {
    form,
    nav: form === 'nav' ? px(headerNavPx(parts, at.fromLg)) : 0,
    field,
    floor: px(headerFieldFloorPx(at.fromMd)),
    placeholder: field - insets >= runPx(SEARCH_PLACEHOLDER, 'body') ? 'long' : 'short',
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

/**
 * The width the status line's drawn parts take in a window `viewport` px wide, read off the line
 * as drawn (__tests__/utils/tailwind-box.ts): each part's text at the step it inherits, 0.6 em a
 * glyph, and the insets its classes and its inline style give it, from Tailwind's theme and the
 * cascade, never from lib/status-line-fit.ts. What is hidden at the viewport, and an `sr-only`
 * run, take no room. A test that compares this with the line's budget compares two readings.
 */
export function drawnStatusLinePx(bar: HTMLElement, viewport: number): number {
  return px(drawnContentPx(bar, viewport));
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
