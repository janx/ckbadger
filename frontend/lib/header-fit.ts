import { fontPx, textPx } from '@/lib/type-metrics';

/**
 * What the header's row holds at its width (readability plan 2026-11-18, lane F3-f; the ruling on
 * its stopped point, option B; the rule in docs/RESPONSIVE_LAYOUT.md "Header"). One rule from md
 * up: the nav stands in the row while the search field keeps at least its floor; where it would
 * not, the nav goes into the menu and the row takes the form it has below md — the wordmark, the
 * menu button, the section word, the field — with the network switcher a row of that menu. The
 * header measures its row (its width less the page's gutter) and budgets its parts here, each
 * from what it writes (lib/type-metrics.ts, a glyph 0.6 em) and the classes that draw it
 * (components/layout/header.tsx, network-switcher.tsx, search-bar.tsx, wordmark.tsx), so a nav
 * entry, a longer label or a second network is weighed by the same rule. At the type scale of
 * 2026-11-18: one network keeps the nav from 768 px (the field 144 px, its floor 116.4); with two
 * the switcher (117.48 px with its margin) would leave the field 10.52 px across 768–1023, so there
 * the nav is in the menu; from 1024 it stands (210.52).
 */

/** The search field's placeholders (components/search-bar.tsx): lowercase, as a command is typed. */
export const SEARCH_PLACEHOLDER = 'search block / tx / address / cell';
/** The placeholder of a field whose text box cannot hold the long one: 6 characters, 50.4 px. */
export const SEARCH_PLACEHOLDER_SHORT = 'search';

/** A nav entry: its label as written (lowercased by CSS, the same length), and whether it opens a menu. */
export interface HeaderNavEntry {
  label: string;
  menu: boolean;
}

/** What the row holds besides the wordmark and the field. */
export interface HeaderRowParts {
  /** The nav's entries in order. */
  nav: readonly HeaderNavEntry[];
  /** The active network's name where the switcher is drawn (two networks or more), else null. */
  switcher: string | null;
  /** The section word beside the menu button (drawn from sm), or null on a page with none. */
  word: string | null;
}

/** The row's two forms: the nav standing in it, or the nav in the menu (the form below md). */
export type HeaderRowForm = 'nav' | 'menu';

/** Where the row is drawn: the breakpoints its classes follow. */
export interface HeaderViewport {
  fromSm: boolean;
  fromMd: boolean;
  fromLg: boolean;
}

/** The wordmark: the 8 px prompt square, its `gap-2.5`, `ckbadger` at its 15 px (`.wordmark`). */
export const WORDMARK_PX = 8 + 10 + textPx('ckbadger'.length, 15);

/** The gap between the row's parts: `gap-8` with the nav standing in it, `gap-2` in the menu form. */
const ROW_GAP_PX: Record<HeaderRowForm, number> = { nav: 32, menu: 8 };

/** The gap between the nav's entries: `gap-x-4`, `lg:gap-x-6`. */
const NAV_GAP_PX = { md: 16, lg: 24 } as const;

/** A nav menu's chevron after its label: `gap-1` and the 12 px chevron. */
const MENU_CHEVRON_PX = 4 + 12;

/** The eyebrow's tracking (`.eyebrow` in app/globals.css): 0.12 em after every glyph. */
const EYEBROW_TRACKING_EM = 0.12;

/**
 * The switcher in the nav around its name: the trigger's 1 px border and `px-2` each side, the
 * 6 px dot, two `gap-1`, the 12 px chevron; the nav placement's `pr-2`, `border-r` and `mr-1`.
 */
const SWITCHER_FRAME_PX = 1 + 8 + 6 + 4 + 4 + 12 + 8 + 1 + 8 + 1 + 4;

/** The menu button (`h-10 w-10`) and the gap before the section word (`gap-2`). */
const MENU_BUTTON_PX = 40;
const WORD_GAP_PX = 8;

/** The theme switcher in the nav form (`h-9 w-9`) and the gap before it (`gap-3`). */
const THEME_PX = 36;
const THEME_GAP_PX = 12;

/**
 * The field's text box insets: the 1 px border and `pl-7` (the prompt square's room) on the left;
 * on the right `md:pr-9` (the `/` key's room, drawn from md) or `pr-3` below md, and the border.
 */
export const FIELD_INSETS_PX = { fromMd: 1 + 28 + 36 + 1, belowMd: 1 + 28 + 12 + 1 } as const;

/** A nav entry's width at the body step (`NAVBAR_LINK_CLASS`). */
export function headerNavEntryPx(entry: HeaderNavEntry): number {
  return textPx([...entry.label].length, fontPx('body')) + (entry.menu ? MENU_CHEVRON_PX : 0);
}

/** The switcher with its name in the eyebrow (uppercase, tracked 0.12 em, the meta step). */
export function headerSwitcherPx(name: string): number {
  const n = [...name].length;
  const meta = fontPx('meta');
  return textPx(n, meta) + n * EYEBROW_TRACKING_EM * meta + SWITCHER_FRAME_PX;
}

/** The nav as it stands in the row: the switcher first where it is drawn, then the entries. */
export function headerNavPx(parts: HeaderRowParts, fromLg: boolean): number {
  const items = parts.nav.map(headerNavEntryPx);
  if (parts.switcher !== null) items.unshift(headerSwitcherPx(parts.switcher));
  const gap = fromLg ? NAV_GAP_PX.lg : NAV_GAP_PX.md;
  return items.reduce((a, b) => a + b, 0) + (items.length - 1) * gap;
}

/**
 * The room a row `rowPx` wide leaves the field in a form, before the field's own cap: in the nav
 * form the wordmark, the nav and the theme switcher with their gaps; in the menu form the wordmark,
 * the menu button and, from sm, the section word with its gap.
 */
export function headerFieldRoomPx(
  form: HeaderRowForm,
  rowPx: number,
  parts: HeaderRowParts,
  { fromSm, fromLg }: Pick<HeaderViewport, 'fromSm' | 'fromLg'>
): number {
  const gaps = 2 * ROW_GAP_PX[form];
  if (form === 'nav')
    return rowPx - WORDMARK_PX - gaps - headerNavPx(parts, fromLg) - THEME_GAP_PX - THEME_PX;
  const word =
    fromSm && parts.word !== null
      ? WORD_GAP_PX + textPx([...parts.word].length, fontPx('body'))
      : 0;
  return rowPx - WORDMARK_PX - gaps - MENU_BUTTON_PX - word;
}

/**
 * The field's floor: a field that holds its short placeholder and, from md, the `/` key's room,
 * with their insets. 116.4 px from md, 92.4 below.
 */
export function headerFieldFloorPx(fromMd: boolean): number {
  return (
    (fromMd ? FIELD_INSETS_PX.fromMd : FIELD_INSETS_PX.belowMd) +
    textPx(SEARCH_PLACEHOLDER_SHORT.length, fontPx('body'))
  );
}

/**
 * The row's form: below md the menu; from md the nav where the row, measured, leaves the field its
 * floor beside it, the menu where it would not. Before the row is measured (the render before the
 * layout effect), the form of the breakpoint: the nav from md, as ListTable draws its table before
 * it measures. The field's cap never decides it: it is at least 288 px.
 */
export function headerRowForm(
  rowPx: number | null,
  parts: HeaderRowParts,
  { fromMd, fromLg }: Pick<HeaderViewport, 'fromMd' | 'fromLg'>
): HeaderRowForm {
  if (!fromMd) return 'menu';
  if (rowPx === null) return 'nav';
  const hundredth = (n: number) => Math.round(n * 100) / 100;
  const room = headerFieldRoomPx('nav', rowPx, parts, { fromSm: true, fromLg });
  return hundredth(room) >= hundredth(headerFieldFloorPx(true)) ? 'nav' : 'menu';
}
