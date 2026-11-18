import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
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
import { Header, navItems } from '@/components/layout/header';
import { SiteFooter } from '@/components/layout/site-footer';
import { VitalsStrip } from '@/components/stats-bar';
import {
  NAVBAR_DROPDOWN_TRIGGER_CLASS,
  NAVBAR_LINK_CLASS,
} from '@/components/layout/navbar-dropdown-styles';

// The network switcher reads the search and hash it keeps across a switch.
vi.mock('@/src/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn(), prefetch: vi.fn() }),
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
   * What the row leaves the field and which placeholder it holds. With two networks the switcher
   * stands first in the nav from md; below md it is a row of the phone menu and the header row is
   * the same as with one. OPEN: from 768 to 1023 with two networks the row leaves the field 10.52
   * px, under its 116.4 px floor — RESPONSIVE_LAYOUT does not say whether the nav links or the
   * switcher yield, so no arrangement is chosen here (the lane's hand-back gives the options).
   */
  const TABLE: Record<number, [one: [number, 'long' | 'short'], two: [number, 'long' | 'short']]> =
    {
      1280: [
        [460.8, 'long'],
        [460.8, 'long'],
      ],
      1024: [
        [352, 'long'],
        [210.52, 'short'],
      ],
      768: [
        [144, 'short'],
        [10.52, 'short'],
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
  const OPEN = (width: number, networks: number) => networks === 2 && width >= 768 && width < 1024;

  it.each(SHELL_WIDTHS.flatMap((w) => [1, 2].map((n) => [w, n] as const)))(
    'at %i px with %i network(s): the field, its placeholder, its floor',
    (width, networks) => {
      const row = headerRow(width, {
        nav: NAV,
        switcher: networks === 2 && width >= 768 ? WIDEST_NETWORK : null,
        word: WIDEST_WORD,
      });
      const [field, placeholder] = TABLE[width][networks - 1];
      expect(row.field).toBe(field);
      expect(row.placeholder).toBe(placeholder);
      if (OPEN(width, networks)) expect(row.field).toBeLessThan(row.floor);
      else expect(row.field).toBeGreaterThanOrEqual(row.floor);
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
    expect(px(contentPx(640) - WORDMARK_PX - 8 - 40 - 8 - runPx(WIDEST_WORD, 'body') - 8)).toBe(
      353.2
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
    expect(row).toHaveClass('flex', 'gap-2', 'md:gap-8');
    expect(nav).toHaveClass('gap-x-4', 'lg:gap-x-6', 'shrink-0');
    expect(NAVBAR_LINK_CLASS.split(' ')).toContain('text-body');
    // the switcher: the eyebrow at the meta step, its border, px-2, gap-1; its wrapper's room
    expect(NAVBAR_DROPDOWN_TRIGGER_CLASS.split(' ')).toEqual(
      expect.arrayContaining(['eyebrow', 'text-meta', 'border', 'px-2', 'gap-1'])
    );
    expect(screen.getByTestId('network-switcher')).toHaveClass('md:mr-1', 'md:border-r', 'md:pr-2');
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
