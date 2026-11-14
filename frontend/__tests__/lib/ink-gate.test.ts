import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';
import { TYPE_SCALE } from '@/lib/type-scale';

/**
 * Gate for the inks that carry words (UI review 2026-11-14, ruling R4; U-04, U-08, DY-02, DY-06):
 *
 * - the ghost ink (`text-text-ghost`, about 1.9:1 by night and 2:1 by day) is for the padding
 *   zeros of a number (the leading zeros, the trailing zeros of a fraction) and for pure
 *   decoration (a separator, an arrow, the `#` of a comment) — never for a word or a figure;
 * - an ink is never thinned by an alpha modifier (`text-text-dim/60`, `text-aqua/70`): a quieter
 *   ink is a named key of the palette, pinned at 4.5:1 (`__tests__/lib/palette.test.ts`);
 * - "set back" (a row or a tile that is context, not the subject) is drawn in the dim ink, never
 *   with element opacity; an `opacity-*` below 1 stands only where nothing it fades is a word
 *   (an icon, a mark) or where the state is a transient pointer focus, each listed with its reason.
 *   A disabled control (`disabled:opacity-*`) is the one variant allowed by rule. `opacity-0`
 *   hides; it does not fade.
 *
 * Each site the gate tolerates is a `path:line` → one-line reason, as in radius-scale.test.ts.
 */

const PADDING = 'padding zeros: the trailing zeros of an amount’s fraction';
const SEPARATOR = 'decoration: the separator between the crumbs or the parts of a line';
const ARROW = 'decoration: the aria-hidden arrow between two tiles of a mechanism chain';
const LEFT_B4 =
  'left for lane B4, which owns components/layout/**: a menu eyebrow or hint, listed in the B2 hand-back';

/** `text-text-ghost` sites, each the padding zeros of a number or pure decoration. */
const GHOST_ALLOWLIST: Record<string, string> = {
  'app/dao/depositors/page.tsx:29': PADDING,
  'app/fiber/channels/[id]/client-page.tsx:58': SEPARATOR,
  'components/activity-event-row.tsx:946': PADDING,
  'components/address/address-holdings.tsx:120': PADDING,
  'components/asset/asset-feed.tsx:417': SEPARATOR,
  'components/asset/detail-shell.tsx:152': SEPARATOR,
  'components/asset/genetics.tsx:762': ARROW,
  'components/asset/inventory-table.tsx:49': PADDING,
  'components/dao/dao-board.tsx:73': PADDING,
  'components/dao/dao-board.tsx:147': PADDING,
  'components/dao/dao-board.tsx:169': PADDING,
  'components/dao/dao-board.tsx:608': ARROW,
  'components/dao/dao-grows.tsx:233': PADDING,
  'components/dao/dao-history.tsx:137': PADDING,
  'components/fiber/fiber-channel-board.tsx:191': PADDING,
  'components/layout/network-switcher.tsx:91': LEFT_B4,
  'components/layout/theme-switcher.tsx:186': LEFT_B4,
  'components/layout/theme-switcher.tsx:245': LEFT_B4,
  'components/layout/theme-switcher.tsx:268': LEFT_B4,
  'components/scripts/script-family-page.tsx:917': SEPARATOR,
  'components/scripts/script-hero.tsx:24': PADDING,
  'components/transaction/script-group-matrix.tsx:178': ARROW,
  'components/ui/script-byte-map.tsx:157': ARROW,
};

const FORK_ALERT =
  'the deep-fork banner’s words on its cinnabar fill: their ink is the fill-ink rule’s (inkOnFill), listed in the B2 hand-back';

/** `text-<ink>/<n>` sites the gate tolerates. */
const ALPHA_INK_ALLOWLIST: Record<string, string> = {
  'components/deep-fork-alert.tsx:33': FORK_ALERT,
  'components/deep-fork-alert.tsx:42': FORK_ALERT,
  'components/deep-fork-alert.tsx:46': FORK_ALERT,
  'components/deep-fork-alert.tsx:50': FORK_ALERT,
  'components/deep-fork-alert.tsx:56': FORK_ALERT,
  'components/layout/network-switcher.tsx:76': LEFT_B4,
  'components/layout/network-switcher.tsx:126': LEFT_B4,
};

const LINKED =
  'a linked highlight: while the reader points at (or pins) one part, the others fade back; their own focus, not a set-back state';
const HOVER =
  'hover feedback on a control or a copyable hash: the pointer’s state, gone when it leaves';
const NO_WORDS = 'fades no word: the element is a mark, an icon or a bar';

/** `opacity-*` below 1 the gate tolerates. */
const OPACITY_ALLOWLIST: Record<string, string> = {
  'app/tx/[hash]/client-page.tsx:436': LINKED,
  'app/tx/[hash]/client-page.tsx:565': LINKED,
  'components/address/address-holdings.tsx:415': LINKED,
  'components/asset/cell-versions.tsx:118': LINKED,
  'components/asset/object-media.tsx:34': HOVER,
  'components/dao/dao-board.tsx:875': NO_WORDS,
  'components/dao/dao-history.tsx:136':
    'left: fraction digits in a history plot, a file the A5 follow-up is changing; listed in the B2 hand-back',
  'components/home/ckbytes-section.tsx:313': LINKED,
  'components/layout/nav-menu.tsx:147': NO_WORDS,
  'components/transaction/script-group-matrix.tsx:162': LINKED,
  'components/transaction/script-group-matrix.tsx:209': LINKED,
  'components/transaction/script-group-matrix.tsx:355': LINKED,
  'components/ui/capacity-utilization.tsx:108': LINKED,
  'components/ui/capacity-utilization.tsx:117': LINKED,
  'components/ui/composition-set.tsx:134': NO_WORDS,
  'components/ui/hex-display.tsx:185': HOVER,
  'components/ui/terminal-panel.tsx:85': NO_WORDS,
};

const SCAN_DIRS = ['app', 'components'];
/** The type scale's steps: `text-body/6` would be a size with a line height, not an ink. */
const SIZE_STEPS = new Set(Object.keys(TYPE_SCALE));

const GHOST = /(?<![\w-])(?:[\w-]+:)*text-text-ghost(?![\w-])/g;
/** A colour class with Tailwind's opacity modifier, in any variant: `hover:text-jade/80`. */
const ALPHA_INK = /(?<![\w-])((?:[\w-]+:)*)text-([a-z]+(?:-[a-z]+)*)\/(\d+|\[[^\]\s]+\])(?![\w-])/g;
/** An opacity class in any variant: `opacity-30`, `opacity-[.55]`, `group-hover:opacity-50`. */
const OPACITY = /(?<![\w-])((?:[\w-]+:)*)opacity-(\d+|\[[^\]\s]+\])(?![\w-])/g;

interface Offence {
  line: number;
  what: string;
}

/** The value of an opacity class as a fraction: `30` → 0.3, `[.55]` → 0.55. */
function opacityValue(raw: string): number {
  const value = raw.startsWith('[') ? Number(raw.slice(1, -1)) : Number(raw) / 100;
  if (!Number.isFinite(value)) throw new Error(`opacity-${raw} is not a number`);
  return value;
}

/** Every ghost-ink class, alpha-thinned ink and fading opacity in a source text. */
function inkOffences(code: string): { ghost: Offence[]; alpha: Offence[]; opacity: Offence[] } {
  const ghost: Offence[] = [];
  const alpha: Offence[] = [];
  const opacity: Offence[] = [];
  code.split('\n').forEach((text, i) => {
    for (const m of text.matchAll(GHOST)) ghost.push({ line: i + 1, what: m[0] });
    for (const m of text.matchAll(ALPHA_INK))
      if (!SIZE_STEPS.has(m[2])) alpha.push({ line: i + 1, what: m[0] });
    for (const m of text.matchAll(OPACITY)) {
      const value = opacityValue(m[2]);
      const disabled = m[1].split(':').includes('disabled');
      if (value > 0 && value < 1 && !disabled) opacity.push({ line: i + 1, what: m[0] });
    }
  });
  return { ghost, alpha, opacity };
}

function sourceFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(tsx|ts)$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(root, dir));
  return files;
}

describe('the ink gate finds what it must, and only that', () => {
  it('flags the ghost ink in any variant, never a look-alike class', () => {
    const code = [
      '<span className="text-text-ghost text-meta">at #1</span>',
      "const c = cn(zero ? 'sm:text-text-ghost' : 'text-text', 'hover:text-text-ghost');",
      '<i className="fill-text-ghost border-text-ghost text-text-ghostly bg-text-ghost" />',
    ].join('\n');
    expect(inkOffences(code).ghost).toEqual([
      { line: 1, what: 'text-text-ghost' },
      { line: 2, what: 'sm:text-text-ghost' },
      { line: 2, what: 'hover:text-text-ghost' },
    ]);
  });

  it('flags an ink thinned by an opacity modifier, never a size with a line height', () => {
    const code = [
      '<span className="text-text-dim/60 text-meta">occupied +61</span>',
      "const a = 'hover:text-jade/80 text-aqua/[0.7] text-white/70';",
      "const b = 'text-body/6 text-meta/[18px] bg-jade/10 border-text-dim/60 text-text-dim';",
    ].join('\n');
    expect(inkOffences(code).alpha).toEqual([
      { line: 1, what: 'text-text-dim/60' },
      { line: 2, what: 'hover:text-jade/80' },
      { line: 2, what: 'text-aqua/[0.7]' },
      { line: 2, what: 'text-white/70' },
    ]);
  });

  it('flags an opacity below 1 in any variant but disabled:, never 0, 100 or a look-alike', () => {
    const code = [
      "rowClassName={(tx) => (tx.isCellbase ? 'opacity-[.55]' : '')}",
      '<span className="opacity-[0.78]">{parts.fraction}</span>',
      "const d = cn(dimmed && 'opacity-30', 'group-hover:opacity-50 hover:opacity-90');",
      "const e = 'opacity-0 opacity-100 disabled:opacity-40 bg-opacity-50 transition-opacity';",
    ].join('\n');
    expect(inkOffences(code).opacity).toEqual([
      { line: 1, what: 'opacity-[.55]' },
      { line: 2, what: 'opacity-[0.78]' },
      { line: 3, what: 'opacity-30' },
      { line: 3, what: 'group-hover:opacity-50' },
      { line: 3, what: 'hover:opacity-90' },
    ]);
  });
});

describe('inks that carry words (R4)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const found = { ghost: [] as string[], alpha: [] as string[], opacity: [] as string[] };
  for (const file of files) {
    const path = relative(root, file);
    const offences = inkOffences(readFileSync(file, 'utf8'));
    for (const kind of ['ghost', 'alpha', 'opacity'] as const)
      for (const o of offences[kind]) found[kind].push(`${path}:${o.line} ${o.what}`);
  }
  const key = (offence: string) => offence.split(' ')[0];
  const rules = [
    ['ghost', GHOST_ALLOWLIST, 'the ghost ink on a word or a figure'],
    ['alpha', ALPHA_INK_ALLOWLIST, 'an ink thinned by an opacity modifier'],
    ['opacity', OPACITY_ALLOWLIST, 'an element faded by opacity'],
  ] as const;

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(300);
  });

  for (const [kind, allowlist, what] of rules) {
    it(`draws no ${what} outside the ${kind} allowlist`, () => {
      expect(found[kind].filter((o) => !(key(o) in allowlist))).toEqual([]);
    });

    it(`keeps the ${kind} allowlist reasoned and current`, () => {
      for (const [site, reason] of Object.entries(allowlist)) {
        expect(reason.trim().length, site).toBeGreaterThan(10);
        expect(found[kind].map(key), site).toContain(site);
      }
    });
  }
});
