import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, within } from '../utils/test-utils';
import { atViewportWidth } from '../utils/viewport';
import {
  PLACEHOLDER,
  SHELL_WIDTHS,
  WIDEST,
  WORDMARK_PX,
  contentPx,
  fieldFloorPx,
  footerLines,
  headerRow,
  navEntryPx,
  px,
  releaseAndZonePx,
  runPx,
  stripFromMd,
  stripLine,
  stripLines,
  switcherPx,
  type NavEntry,
} from '../utils/shell-budget';
import {
  boxPx,
  containerContentPx,
  gapPx,
  inFlowAt,
  insetsPx,
  outerPx,
} from '../utils/tailwind-box';
import { Header, navItems } from '@/components/layout/header';
import {
  FIELD_INSETS_PX,
  headerFieldRoomPx,
  headerNavEntryPx,
  headerNavPx,
  headerRowForm,
  headerSwitcherPx,
} from '@/lib/header-fit';
import { SiteFooter } from '@/components/layout/site-footer';
import { VitalsStrip } from '@/components/stats-bar';
import {
  NAVBAR_DROPDOWN_TRIGGER_CLASS,
  NAVBAR_LINK_CLASS,
} from '@/components/layout/navbar-dropdown-styles';

// The network switcher reads the search and hash it keeps across a switch; the shortcuts push.
const push = vi.hoisted(() => vi.fn());
vi.mock('@/src/navigation', () => ({
  useRouter: () => ({ push, replace: vi.fn(), prefetch: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
  useSearchAndHash: () => '',
  usePathname: () => '/',
  useParams: () => ({}),
}));

/**
 * The shell budgeted from the type scale (readability plan 2026-11-18, lane F3-f; the rules in
 * docs/RESPONSIVE_LAYOUT.md "Rules for the shared pieces"): per reference width, for one network
 * and for two (the switcher in the nav from md), what stands in each line of the header, the vitals
 * strip and the footer, that it fits, and where a part wraps, shortens or leaves. The arithmetic is
 * `__tests__/utils/shell-budget.ts` at the scale (meta 12, small 13, body 14 px; a glyph 0.6 em);
 * the last block pins the classes it reads. The status line's budget is the line's own
 * (lib/status-line-fit.ts) and is tested beside it.
 */

/** The nav as the header draws it from md: its links and its two menus. */
const NAV: NavEntry[] = navItems.map((item) => ({ label: item.label, menu: 'children' in item }));

/** The widest word the header writes beside the menu button from 640: the current section. */
const SECTION_WORDS = [
  'Home',
  ...navItems.flatMap((item) =>
    'children' in item ? item.children.map((c) => c.label) : [item.label]
  ),
];
const WIDEST_WORD = SECTION_WORDS.reduce((a, b) => (b.length > a.length ? b : a));

/** The widest network name the switcher can show. */
const WIDEST_NETWORK = WIDEST.networks.reduce((a, b) => (b.length > a.length ? b : a));

describe('the shell measured as Chrome measured it (one network, the scale at 12 / 13 / 14 px)', () => {
  it('measures the nav entries and the search field the lead measured on the new-size build', () => {
    // the lead's Chrome reading (lanes/checks/shell.cjs): dao 25, activities 100, inventory 92,
    // scripts 59, charts 50
    expect(NAV.map((e) => [e.label, px(navEntryPx(e))])).toEqual([
      ['DAO', 25.2],
      ['Activities', 100],
      ['Inventory', 91.6],
      ['Scripts', 58.8],
      ['Charts', 50.4],
    ]);
    expect(WORDMARK_PX).toBe(90);
    // the field: 460.8 at 1280, 352 at 1024, 144 at 768, 288 at 640, 212 / 182 at 390 / 360, 142
    // at 320 — the section word of `/blocks` beside the menu button from 640
    const field = SHELL_WIDTHS.map(
      (w) => headerRow(w, { nav: NAV, switcher: null, word: 'Blocks' }).field
    );
    expect(field).toEqual([460.8, 352, 144, 288, 212, 182, 142]);
  });

  it('measures the switcher with its eyebrow tracking: 117.48 px with its margin, not 107', () => {
    // `MAINNET` at 12 px, 0.12 em after each glyph: 7 × 8.64 = 60.48; the button's border, padding,
    // dot, gaps and chevron 44; the wrapper's pr-2, border-r, mr-1 13
    expect(px(switcherPx(WIDEST_NETWORK))).toBe(117.48);
  });
});

describe('the header row per width, one network and two', () => {
  /**
   * What the row leaves the field and which placeholder it holds (ruling on F3-f, option B: the nav
   * stands in the row while the field keeps its floor, 116.4 px from md; where it would not, the
   * nav goes into the menu and the row takes its form below md: the wordmark, the menu button,
   * the section word, the field). With two networks the switcher stands first in the nav; in the
   * row that would leave the field 10.52 px at 768–1023 (the container is 736 px across that
   * band), so there the nav is in the menu and the field is its cap: 288 px at 768, 36vw above,
   * the long placeholder from 977 px (36vw ≥ 351.6). With one network the nav stands at 768 (144
   * px ≥ 116.4). Below md the switcher is a row of the menu, as it always was.
   */
  const WIDTHS = [1280, 1024, 1023, 900, 768, 640, 390, 360, 320] as const;
  const TABLE: Record<
    (typeof WIDTHS)[number],
    [one: [number, 'long' | 'short'], two: [number, 'long' | 'short']]
  > = {
    1280: [
      [460.8, 'long'],
      [460.8, 'long'],
    ],
    1024: [
      [352, 'long'],
      [210.52, 'short'],
    ],
    1023: [
      [144, 'short'],
      [368.28, 'long'],
    ],
    900: [
      [144, 'short'],
      [324, 'short'],
    ],
    768: [
      [144, 'short'],
      [288, 'short'],
    ],
    640: [
      [288, 'short'],
      [288, 'short'],
    ],
    390: [
      [212, 'short'],
      [212, 'short'],
    ],
    360: [
      [182, 'short'],
      [182, 'short'],
    ],
    320: [
      [142, 'short'],
      [142, 'short'],
    ],
  };

  it.each(WIDTHS.flatMap((w) => [1, 2].map((n) => [w, n] as const)))(
    'at %i px with %i network(s): the field, its placeholder, its floor',
    (width, networks) => {
      const row = headerRow(width, {
        nav: NAV,
        switcher: networks === 2 ? WIDEST_NETWORK : null,
        word: WIDEST_WORD,
      });
      const [field, placeholder] = TABLE[width][networks - 1];
      // the nav stands in the row from md but with two networks across 768–1023
      const menu = width < 768 || (networks === 2 && width < 1024);
      expect(row.form).toBe(menu ? 'menu' : 'nav');
      expect(row.field).toBe(field);
      expect(row.placeholder).toBe(placeholder);
      expect(row.field).toBeGreaterThanOrEqual(row.floor);
    }
  );

  it('floors the field at its short placeholder and, from md, the "/" key: 116.4 px, 92.4 below md', () => {
    expect(px(fieldFloorPx(true))).toBe(116.4);
    expect(px(fieldFloorPx(false))).toBe(92.4);
    // the long placeholder needs a field of 351.6 px from md: 285.6 and the 66 px insets
    expect(px(runPx(PLACEHOLDER.long, 'body') + 66)).toBe(351.6);
    // the widest section word, "Transactions", beside the menu button at 640 leaves the field its
    // 288 px cap: 608 − 90 − 8 − 40 − 8 − 100.8 − 8 = 353.2 of room
    expect(WIDEST_WORD).toBe('Transactions');
    // below md the row has its phone form: at 640–767 (608 px) the nav would leave the field 16 px
    expect(
      px(
        headerFieldRoomPx(
          'nav',
          contentPx(640),
          { nav: NAV, switcher: null, word: null },
          { fromSm: true, fromLg: false }
        )
      )
    ).toBe(16);
    expect(px(contentPx(640) - WORDMARK_PX - 8 - 40 - 8 - runPx(WIDEST_WORD, 'body') - 8)).toBe(
      353.2
    );
  });
});

/**
 * The header's row measured at `row` px (its content box: the container's width less its gutter)
 * in a window `viewport` px wide. jsdom lays nothing out: the row reports `row`, every other box
 * nothing, so only the row's form is decided by a measurement.
 */
function atRow(viewport: number, row: number) {
  atViewportWidth(viewport);
  const widthOf = (el: Element) =>
    el.querySelector(':scope > nav[aria-label="Main"]') !== null ? row : 0;
  class ResizeObserverStub {
    constructor(private readonly cb: ResizeObserverCallback) {}
    observe(el: Element) {
      this.cb(
        [{ contentRect: { width: widthOf(el) } } as ResizeObserverEntry],
        this as unknown as ResizeObserver
      );
    }
    unobserve() {}
    disconnect() {}
  }
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement
  ) {
    const width = widthOf(this);
    return { width, height: 0, top: 0, left: 0, right: width, bottom: 0, x: 0, y: 0 } as DOMRect;
  });
}

const TWO = { networks: [{ name: 'mainnet' }, { name: 'testnet' }], defaultNetwork: 'mainnet' };

/** The header's parts by what they are: the row, the nav, the menu button's group, the theme. */
function header() {
  const banner = screen.getByRole('banner');
  const nav = within(banner).getByRole('navigation', { name: 'Main' });
  const toggle = within(banner).getByRole('button', { name: 'Toggle menu' });
  return {
    banner,
    row: nav.parentElement as HTMLElement,
    nav,
    menuGroup: toggle.parentElement as HTMLElement,
    toggle,
    theme: within(banner).getByTestId('theme-switcher'),
  };
}

/** Tailwind's breakpoints the header's classes use. */
const BREAKPOINTS = { sm: 640, md: 768, lg: 1024 } as const;

/** The classes of an element that apply in the window jsdom has (`atViewportWidth`). */
function applied(el: HTMLElement): string[] {
  return Array.from(el.classList).flatMap((c) => {
    const at = /^(sm|md|lg):(.+)$/.exec(c);
    if (!at) return [c];
    return window.innerWidth >= BREAKPOINTS[at[1] as keyof typeof BREAKPOINTS] ? [at[2]] : [];
  });
}

/** Drawn in this window, by its classes: the last display class that applies is not `hidden`. */
function drawn(el: HTMLElement): boolean {
  const display = applied(el).filter((c) => ['hidden', 'flex', 'block', 'inline'].includes(c));
  return display.at(-1) !== 'hidden';
}

/** The row's gap in this window: 8 px (`gap-2`) or 32 (`gap-8`). */
function rowGap(el: HTMLElement): number {
  const gap = applied(el).filter((c) => c === 'gap-2' || c === 'gap-8');
  return gap.at(-1) === 'gap-8' ? 32 : 8;
}

describe('the header row takes its form from its measured width (ruling on F3-f, option B)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    delete window.__CKBADGER_RUNTIME_CONFIG__;
    push.mockReset();
  });

  it.each([768, 900, 1023])(
    'puts the nav into the menu at %i px with two networks: in the row the field would keep 10.52 px',
    (viewport) => {
      window.__CKBADGER_RUNTIME_CONFIG__ = TWO;
      atRow(viewport, contentPx(viewport));
      render(<Header />);
      const h = header();
      expect(drawn(h.nav)).toBe(false);
      expect(drawn(h.menuGroup)).toBe(true);
      expect(drawn(h.theme)).toBe(false);
      // the row's form below md: the 8 px gap; its height the nav form's, so nothing below moves
      expect(rowGap(h.row)).toBe(8);
      expect(h.row).toHaveClass('min-h-[52px]', 'lg:min-h-[60px]');
      expect(h.row.className).not.toMatch(/\b(?:h|max-h|min-h)-(?!\[52px\]|\[60px\])/);
    }
  );

  it('keeps the nav in the row at 768 px with one network (144 px ≥ 116.4) and at 1024 with two', () => {
    for (const [viewport, config] of [
      [768, undefined],
      [1024, TWO],
    ] as const) {
      if (config) window.__CKBADGER_RUNTIME_CONFIG__ = config;
      atRow(viewport, contentPx(viewport));
      const { unmount } = render(<Header />);
      const h = header();
      expect(drawn(h.nav), `${viewport}`).toBe(true);
      expect(drawn(h.menuGroup), `${viewport}`).toBe(false);
      expect(rowGap(h.row)).toBe(32);
      expect(h.row).toHaveClass('min-h-[52px]', 'lg:min-h-[60px]');
      expect(drawn(h.theme)).toBe(true);
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  it('opens the phone menu as it is in the md band: the same entries in the same order, the switcher a row', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = TWO;
    atRow(900, contentPx(900));
    render(<Header />);
    fireEvent.click(header().toggle);
    const phoneNav = screen
      .getAllByRole('navigation')
      .find((n) => n.getAttribute('aria-label') !== 'Main')!;
    const panel = phoneNav.parentElement as HTMLElement;
    expect(drawn(panel)).toBe(true);
    const switcher = within(phoneNav).getByTestId('network-switcher');
    expect(switcher).toHaveAttribute('data-placement', 'menu');
    expect(
      Array.from(phoneNav.querySelectorAll('a, [data-testid="network-switcher"], span.block')).map(
        (el) =>
          el.getAttribute('data-testid') === 'network-switcher' ? 'switcher' : el.textContent
      )
    ).toEqual([
      'Home',
      'switcher',
      'DAO',
      'Activities',
      'Transactions',
      'Blocks',
      'Forks',
      'Inventory',
      'Tokens',
      'Objects',
      'Identities',
      'Scripts',
      'Charts',
    ]);
    expect(within(phoneNav).getByTestId('theme-segments')).toBeInTheDocument();
    // it closes as it does below md: on a choice, and on its button
    fireEvent.click(within(phoneNav).getByRole('link', { name: 'Scripts' }));
    expect(screen.getAllByRole('navigation')).toHaveLength(1);
    fireEvent.click(header().toggle);
    expect(screen.getAllByRole('navigation')).toHaveLength(2);
    fireEvent.click(header().toggle);
    expect(screen.getAllByRole('navigation')).toHaveLength(1);
  });

  it('draws the form it has today before the row is measured: the nav from md, the menu below it', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = TWO;
    atViewportWidth(768);
    const { unmount } = render(<Header />);
    let h = header();
    expect(drawn(h.nav)).toBe(true);
    expect(drawn(h.menuGroup)).toBe(false);
    unmount();
    atViewportWidth(390);
    render(<Header />);
    h = header();
    expect(drawn(h.nav)).toBe(false);
    expect(drawn(h.menuGroup)).toBe(true);
  });

  it('keeps the keys in both forms: "/" focuses the field, "g b" opens the blocks, the field after the nav or the toggle', () => {
    for (const viewport of [900, 1280]) {
      window.__CKBADGER_RUNTIME_CONFIG__ = TWO;
      atRow(viewport, contentPx(viewport));
      const { unmount } = render(<Header />);
      const field = within(screen.getByRole('banner')).getByRole('textbox');
      act(() => {
        fireEvent.keyDown(window, { key: '/' });
      });
      expect(field, `${viewport}`).toHaveFocus();
      act(() => field.blur());
      act(() => {
        fireEvent.keyDown(window, { key: 'g' });
      });
      act(() => {
        fireEvent.keyDown(window, { key: 'b' });
      });
      expect(push, `${viewport}`).toHaveBeenCalledWith('/blocks');
      // in reading order the wordmark, then the nav or the menu button (whichever is drawn), then
      // the field
      const h = header();
      const lead = drawn(h.nav) ? h.nav : h.toggle;
      expect(drawn(h.nav), `${viewport}`).toBe(viewport >= 1024);
      expect(
        lead.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING,
        `${viewport}`
      ).toBeTruthy();
      unmount();
      push.mockReset();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });
});

/**
 * The header as drawn against its budget (review-fb-tests finding 2, for lib/header-fit.ts as for
 * the status line): the budget's frame constants are numbers written beside the classes of six
 * components (header, wordmark, network switcher, nav menu, theme switcher, search bar), and the
 * class test below pins most of those classes by name only — a chevron's size, the gap before it, a
 * link's padding, the field's border and any class added beside them were pinned by nothing. Here
 * each part of the rendered header is read off its classes (__tests__/utils/tailwind-box.ts:
 * Tailwind's theme, app/globals.css, never lib/header-fit.ts) at each width and network count, and
 * compared with the budget: the wordmark, each nav entry, the switcher with its margin, the nav,
 * the field's insets, and what the row leaves the field in the form the header drew.
 */
describe('the header as drawn is the header its budget counts', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  const sum = (xs: readonly number[]) => xs.reduce((a, b) => a + b, 0);
  const width = (el: Element, viewport: number) => {
    const w = outerPx(el, viewport);
    if (w === null) throw new Error(`<${el.tagName}> is not drawn at ${viewport}`);
    return w;
  };

  /** What the drawn row leaves the field: the row less its other parts, their gaps and the theme. */
  function drawnFieldRoom(row: HTMLElement, field: HTMLElement, rowPx: number, viewport: number) {
    const drawnIn = (el: Element) => Array.from(el.children).filter((c) => inFlowAt(c, viewport));
    const group = Array.from(row.children).find((c) => c.contains(field))!;
    const fieldBox = Array.from(group.children).find((c) => c.contains(field))!;
    const parts = drawnIn(row);
    const beside = drawnIn(group).filter((c) => c !== fieldBox);
    return (
      rowPx -
      sum(parts.filter((c) => c !== group).map((c) => width(c, viewport))) -
      (parts.length - 1) * gapPx(row, viewport) -
      sum(beside.map((c) => width(c, viewport))) -
      beside.length * gapPx(group, viewport)
    );
  }

  const CASES = [
    [1280, 1],
    [1024, 1],
    [768, 1],
    [640, 1],
    [390, 1],
    [360, 1],
    [1280, 2],
    [1024, 2],
    [900, 2],
    [768, 2],
    [390, 2],
  ] as const;

  it.each(CASES)('at %i px with %i network(s)', (viewport, networks) => {
    if (networks === 2) window.__CKBADGER_RUNTIME_CONFIG__ = TWO;
    const rowPx = containerContentPx(viewport);
    atRow(viewport, rowPx);
    render(<Header />);
    const h = header();
    const at = { fromSm: viewport >= 640, fromMd: viewport >= 768, fromLg: viewport >= 1024 };
    const parts = {
      nav: NAV,
      switcher: networks === 2 ? 'mainnet' : null,
      // the route is `/`: the section word is "Home"
      word: 'Home',
    };
    const form = headerRowForm(rowPx, parts, at);
    expect(h.row).toHaveAttribute('data-header-form', form);
    // the wordmark: the 8 px square, its gap, `ckbadger` at 15 px
    const wordmark = within(h.banner).getByRole('link', { name: 'CKBadger home' });
    expect(boxPx(wordmark, viewport)).toBeCloseTo(WORDMARK_PX, 9);
    // the nav, entry by entry, then whole, where it stands in the row
    if (form === 'nav') {
      const drawnEntries = Array.from(h.nav.children).filter((c) => inFlowAt(c, viewport));
      const budget = [
        ...(parts.switcher === null ? [] : [headerSwitcherPx(parts.switcher)]),
        ...NAV.map(headerNavEntryPx),
      ];
      drawnEntries.forEach((entry, i) =>
        expect(width(entry, viewport), `${entry.textContent}`).toBeCloseTo(budget[i], 9)
      );
      expect(drawnEntries).toHaveLength(budget.length);
      expect(boxPx(h.nav, viewport)).toBeCloseTo(headerNavPx(parts, at.fromLg), 9);
    } else expect(inFlowAt(h.nav, viewport)).toBe(false);
    // the field's text box insets: the border, the prompt's room, the `/` key's from md
    const field = within(h.banner).getByRole('textbox');
    expect(insetsPx(field, viewport).box).toBe(
      at.fromMd ? FIELD_INSETS_PX.fromMd : FIELD_INSETS_PX.belowMd
    );
    // what the row leaves the field in the form it drew
    expect(drawnFieldRoom(h.row, field, rowPx, viewport)).toBeCloseTo(
      headerFieldRoomPx(form, rowPx, parts, at),
      9
    );
  });
});

describe('the vitals strip per width, at its widest real content', () => {
  it('is one row at 1280 and wraps as before at 1024 and 768, each item whole', () => {
    expect(stripFromMd(1280)).toEqual([
      {
        items: ['block', 'epoch', 'block time', 'hash rate', 'difficulty', 'live'],
        px: 1223.8,
      },
    ]);
    expect(stripFromMd(1024)).toEqual([
      { items: ['block', 'epoch', 'block time', 'hash rate'], px: 945.2 },
      { items: ['difficulty', 'live'], px: 254.6 },
    ]);
    expect(stripFromMd(768)).toEqual([
      { items: ['block', 'epoch'], px: 581.8 },
      { items: ['block time', 'hash rate', 'difficulty', 'live'], px: 618 },
    ]);
    for (const w of [1280, 1024, 768])
      for (const line of stripFromMd(w)) expect(line.px).toBeLessThanOrEqual(contentPx(w));
  });

  it('holds one line below md on every page but home: the block and the epoch with its bar', () => {
    // nine digits, the widest; the live mark is drawn only where the page holds the realtime
    // socket, which only the home page does, and the home page has its three lines
    const nine = { block: WIDEST.block, live: false };
    expect(stripLine(640, nine)).toEqual([{ items: ['block', 'epoch'], px: 325.2 }]);
    expect(stripLine(390, nine)).toEqual([{ items: ['block', 'epoch'], px: 289.2 }]);
    expect(stripLine(360, nine)).toEqual([{ items: ['block', 'epoch'], px: 289.2 }]);
    // with the live dot it would be 308.2 of 328 at 360
    expect(stripLine(360, { ...nine, live: true })).toEqual([
      { items: ['block', 'epoch', 'live'], px: 308.2 },
    ]);
    // at 320 eight digits hold the line (280.8 of 288); the ninth puts the epoch under the block
    expect(stripLine(320, { block: WIDEST.blockToday, live: false })).toEqual([
      { items: ['block', 'epoch'], px: 280.8 },
    ]);
    expect(stripLine(320, nine)).toEqual([
      { items: ['block'], px: 142.8 },
      { items: ['epoch'], px: 134.4 },
    ]);
  });

  it('holds the home page’s three lines below md; the epoch takes two from 360 down', () => {
    const at = Object.fromEntries([640, 390, 360, 320].map((w) => [w, stripLines(w)]));
    for (const [w, s] of Object.entries(at)) {
      // line 1: the block in its tracks, Live in the last
      expect(s.first.block, `${w}`).toBeLessThanOrEqual(s.first.blockRoom);
      expect(s.first.live, `${w}`).toBeLessThanOrEqual(s.first.liveRoom);
      // line 3: each figure in its track, the tracks within the line
      s.mining.px.forEach((m, i) => expect(m, `${w}`).toBeLessThanOrEqual(s.mining.tracks[i]));
      for (const line of s.epoch) expect(line.px, `${w}`).toBeLessThanOrEqual(contentPx(Number(w)));
    }
    expect(at[640].epoch).toEqual([
      { items: ['epoch and bar', 'position and time left'], px: 376.8 },
    ]);
    expect(at[390].epoch).toEqual([
      { items: ['epoch and bar', 'position and time left'], px: 340.8 },
    ]);
    for (const w of [360, 320])
      expect(at[w].epoch).toEqual([
        { items: ['epoch and bar'], px: 134.4 },
        { items: ['position and time left'], px: 200.4 },
      ]);
    // at 320 the hash rate's value (92.4) is wider than a third of the line (85.33): its track
    // keeps it and the other two share the rest, 81.8 each
    expect(at[320].mining.tracks).toEqual([81.8, 92.4, 81.8]);
    expect(at[360].mining.tracks).toEqual([98.67, 98.67, 98.67]);
    expect(at[640].mining.tracks).toEqual([136.8, 163.2, 145.2]);
  });
});

describe('the footer per width', () => {
  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  /** The footer's entries as it draws them, in order. */
  function entries(): string[] {
    window.__CKBADGER_RUNTIME_CONFIG__ = { buildVersion: '0.8.2@f8947b8d3915' };
    render(<SiteFooter />);
    const nav = within(screen.getByRole('contentinfo')).getByRole('navigation', { name: 'Footer' });
    return Array.from(nav.querySelectorAll('a, button')).map((el) => el.textContent ?? '');
  }

  it('holds its links in one line from 1024, two at 768 (by 0.2 px), one at 640, two below', () => {
    const all = entries();
    expect(all.at(-1)).toBe('Keyboard shortcuts');
    const phone = all.slice(0, -1);
    expect(footerLines(1280, all)).toEqual([{ items: all, px: 736.2 }]);
    expect(footerLines(1024, all)).toEqual([{ items: all, px: 736.2 }]);
    // at 768 the seven need 736.2 of 736 px: "Keyboard shortcuts" takes a line of its own
    expect(footerLines(768, all)).toEqual([
      { items: phone, px: 575.8 },
      { items: ['Keyboard shortcuts'], px: 140.4 },
    ]);
    expect(footerLines(640, phone)).toEqual([{ items: phone, px: 575.8 }]);
    for (const w of [390, 360, 320])
      expect(footerLines(w, phone)).toEqual([
        { items: ['Hardforks', 'Network', 'Fiber channels'], px: 274 },
        { items: ['Fiber dashboard ↗', 'cknerv ↗', 'Web5 ↗'], px: 281.8 },
      ]);
  });

  it('says the release and the zone on one line below md where the zone is at most 25 characters at 360', () => {
    expect(px(releaseAndZonePx('0.8.2', 'asia/shanghai'))).toBe(234);
    // 328 px hold 42 glyphs at 7.8: `ckbadger 0.8.2 · ` is 17 of them
    const longest = (w: number) => Math.floor(px(contentPx(w) / runPx('0', 'small'))) - 17;
    expect(longest(360)).toBe(25);
    expect(longest(320)).toBe(19);
    expect(releaseAndZonePx('0.8.2', 'x'.repeat(25))).toBeLessThanOrEqual(contentPx(360));
  });
});

describe('the classes and sources the budget reads', () => {
  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  const css = () => readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');

  it('reads the network names the code knows from the crate that defines them', () => {
    const rust = readFileSync(
      resolve(__dirname, '../../../crates/common/src/hardfork.rs'),
      'utf-8'
    );
    const names = [...rust.matchAll(/pub const NETWORK_\w+: &str = "([^"]+)";/g)].map((m) => m[1]);
    expect(names).toEqual([...WIDEST.networks]);
  });

  it('reads the wordmark at 15 px and the eyebrow tracked 0.12 em', () => {
    expect(css()).toMatch(/\.wordmark \{\s*font-size: 15px;/);
    expect(css()).toMatch(/\.eyebrow \{[^}]*letter-spacing: 0\.12em;/);
    expect(css()).toMatch(/\.prompt-square,[^{]*\{[^}]*width: 8px;/);
  });

  it('reads the header row’s gaps, the nav’s, the field’s cap and insets, the menu button and the theme', () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
    render(<Header />);
    const banner = screen.getByRole('banner');
    const nav = within(banner).getByRole('navigation', { name: 'Main' });
    const row = nav.parentElement as HTMLElement;
    // unmeasured in jsdom's 1024 px window, the row draws its form of the breakpoint: the nav
    expect(row).toHaveAttribute('data-header-form', 'nav');
    expect(row).toHaveClass('flex', 'gap-8');
    expect(nav).toHaveClass('gap-x-4', 'lg:gap-x-6', 'shrink-0');
    expect(NAVBAR_LINK_CLASS.split(' ')).toContain('text-body');
    // the switcher: the eyebrow at the meta step, its border, px-2, gap-1; its wrapper's room
    expect(NAVBAR_DROPDOWN_TRIGGER_CLASS.split(' ')).toEqual(
      expect.arrayContaining(['eyebrow', 'text-meta', 'border', 'px-2', 'gap-1'])
    );
    const switcher = within(nav).getByTestId('network-switcher');
    expect(switcher).toHaveAttribute('data-placement', 'nav');
    expect(switcher).toHaveClass('mr-1', 'border-r', 'pr-2');
    const input = within(banner).getByRole('textbox');
    expect(input).toHaveClass('well', 'text-body', 'pl-7', 'pr-3', 'md:pr-9');
    const fieldBox = input.closest('form')!.parentElement!.parentElement as HTMLElement;
    expect(fieldBox).toHaveClass('w-full', 'max-w-[clamp(18rem,36vw,36rem)]');
    expect(fieldBox.parentElement).toHaveClass('flex-1', 'gap-3');
    expect(within(banner).getByRole('button', { name: /^Theme:/ })).toHaveClass('h-9', 'w-9');
    const menu = within(banner).getByRole('button', { name: 'Toggle menu' });
    expect(menu).toHaveClass('h-10', 'w-10');
    expect(menu.parentElement).toHaveClass('gap-2');
    expect(within(banner).getByRole('link', { name: 'CKBadger home' })).toHaveClass('gap-2.5');
  });

  it('reads the strip’s labels at meta (small from md), values at body, their gaps', async () => {
    render(<VitalsStrip />);
    const block = await screen.findByRole('link', { name: /^Block #/ });
    const [label, value] = Array.from(block.children);
    expect(label).toHaveClass('text-meta', 'md:text-small');
    expect(value).toHaveClass('text-body');
    expect(block).toHaveClass('gap-1.5', 'md:gap-2');
    const epoch = screen.getByTestId('vitals-epoch');
    expect(epoch).toHaveClass('gap-x-1.5', 'md:gap-x-2');
    expect(screen.getByTestId('vitals-epoch-bar')).toHaveClass('text-meta', 'md:text-small');
  });

  it('reads the footer’s links at the small step, 20 px apart', () => {
    render(<SiteFooter />);
    const footer = screen.getByRole('contentinfo');
    expect(footer.firstElementChild).toHaveClass('text-small');
    expect(within(footer).getByRole('navigation', { name: 'Footer' })).toHaveClass('gap-x-5');
  });
});
