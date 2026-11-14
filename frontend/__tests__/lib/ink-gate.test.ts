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
 * The sites the gate tolerates are listed per file, in the ORDER they occur in it: the n-th site
 * of a kind in a file has the n-th reason. A line number would break on every lane that moves a
 * line; an order survives that, and an added or a removed site still fails (a site with no reason,
 * or a reason with no site), the message naming the path, the line and the class.
 */

const PADDING = 'padding zeros: the trailing zeros of an amount’s fraction';
const SEPARATOR = 'decoration: the separator between the crumbs or the parts of a line';
const ARROW = 'decoration: the aria-hidden arrow between two tiles of a mechanism chain';

/** Per file, in order: the reasons of its sites of one kind. */
type Allowlist = Readonly<Record<string, readonly string[]>>;

/** `text-text-ghost` sites, each the padding zeros of a number or pure decoration. */
const GHOST_ALLOWLIST: Allowlist = {
  'app/dao/depositors/page.tsx': [PADDING],
  'app/fiber/channels/[id]/client-page.tsx': [SEPARATOR],
  'components/activity-event-row.tsx': [PADDING],
  'components/address/address-holdings.tsx': [PADDING],
  'components/asset/asset-feed.tsx': [SEPARATOR],
  'components/asset/detail-shell.tsx': [SEPARATOR],
  'components/asset/genetics.tsx': [ARROW],
  'components/asset/inventory-table.tsx': [PADDING],
  // Amount's zeros, the deposited figure's, the 24 h change's; the mechanism chain's arrow
  'components/dao/dao-board.tsx': [PADDING, PADDING, PADDING, ARROW],
  'components/dao/dao-grows.tsx': [PADDING],
  'components/dao/dao-history.tsx': [PADDING],
  'components/fiber/fiber-channel-board.tsx': [PADDING],
  'components/scripts/script-family-page.tsx': [SEPARATOR],
  'components/scripts/script-hero.tsx': [PADDING],
  'components/transaction/script-group-matrix.tsx': [ARROW],
  'components/ui/script-byte-map.tsx': [ARROW],
};

/** `text-<ink>/<n>` sites the gate tolerates: none. */
const ALPHA_INK_ALLOWLIST: Allowlist = {};

const LINKED =
  'a linked highlight: while the reader points at (or pins) one part, the others fade back; their own focus, not a set-back state';
const HOVER =
  'hover feedback on a control or a copyable hash: the pointer’s state, gone when it leaves';
const NO_WORDS = 'fades no word: the element is a mark, an icon or a bar';

/** `opacity-*` below 1 the gate tolerates. */
const OPACITY_ALLOWLIST: Allowlist = {
  // the witness rows, the dependency rows
  'app/tx/[hash]/client-page.tsx': [LINKED, LINKED],
  'components/address/address-holdings.tsx': [LINKED],
  'components/asset/cell-versions.tsx': [LINKED],
  'components/asset/object-media.tsx': [HOVER],
  // the deposit-window bars: the first day at 90 %, the others at 50 %
  'components/dao/dao-board.tsx': [NO_WORDS, NO_WORDS],
  'components/home/ckbytes-section.tsx': [LINKED],
  'components/layout/nav-menu.tsx': [NO_WORDS],
  // the cell headers, the group columns, the group cards
  'components/transaction/script-group-matrix.tsx': [LINKED, LINKED, LINKED],
  'components/ui/composition-set.tsx': [NO_WORDS],
  'components/ui/hex-display.tsx': [HOVER],
  'components/ui/terminal-panel.tsx': [NO_WORDS],
};

interface Site {
  path: string;
  line: number;
  what: string;
}

/**
 * What the sites of one kind break in its allowlist: a site with no reason (a file not listed, or
 * one more site than reasons), a file with fewer sites than reasons, a reason too short to say why.
 * Each names the path, the line (for the message only) and the class.
 */
function allowlistFailures(sites: readonly Site[], allowlist: Allowlist): string[] {
  const byPath = new Map<string, Site[]>();
  for (const site of sites) byPath.set(site.path, [...(byPath.get(site.path) ?? []), site]);
  const out: string[] = [];
  for (const [path, list] of byPath) {
    const reasons = allowlist[path] ?? [];
    list.forEach((site, i) => {
      if (i >= reasons.length)
        out.push(
          `${path}:${site.line} ${site.what}: no reason for site ${i + 1} of ${list.length} (${reasons.length} listed)`
        );
    });
  }
  for (const [path, reasons] of Object.entries(allowlist)) {
    const list = byPath.get(path) ?? [];
    if (list.length < reasons.length)
      out.push(
        `${path}: ${reasons.length} reasons for ${list.length} sites (${list.map((s) => `${s.line} ${s.what}`).join(', ') || 'none'}): drop the reason of each site that went`
      );
    for (const reason of reasons)
      if (reason.trim().length <= 10)
        out.push(`${path}: a reason too short to say why: "${reason}"`);
  }
  return out;
}

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

describe('the allowlists key a site by its file and its order, not its line', () => {
  const reason = 'decoration: a separator between two parts of a line';
  const list: Allowlist = { 'components/a.tsx': [reason, reason] };
  const site = (line: number): Site => ({
    path: 'components/a.tsx',
    line,
    what: 'text-text-ghost',
  });

  it('passes the same sites moved to other lines', () => {
    expect(allowlistFailures([site(10), site(20)], list)).toEqual([]);
    expect(allowlistFailures([site(14), site(31)], list)).toEqual([]);
  });

  it('fails an added site, naming its path, line and class', () => {
    expect(allowlistFailures([site(10), site(20), site(25)], list)).toEqual([
      'components/a.tsx:25 text-text-ghost: no reason for site 3 of 3 (2 listed)',
    ]);
    expect(
      allowlistFailures([{ path: 'components/b.tsx', line: 7, what: 'text-text-ghost' }], list)
    ).toEqual([
      'components/b.tsx:7 text-text-ghost: no reason for site 1 of 1 (0 listed)',
      'components/a.tsx: 2 reasons for 0 sites (none): drop the reason of each site that went',
    ]);
  });

  it('fails a removed site, naming the file and the sites left', () => {
    expect(allowlistFailures([site(20)], list)).toEqual([
      'components/a.tsx: 2 reasons for 1 sites (20 text-text-ghost): drop the reason of each site that went',
    ]);
  });

  it('fails a reason too short to say why', () => {
    expect(allowlistFailures([site(1)], { 'components/a.tsx': ['zeros'] })).toEqual([
      'components/a.tsx: a reason too short to say why: "zeros"',
    ]);
  });
});

describe('inks that carry words (R4)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const found = { ghost: [] as Site[], alpha: [] as Site[], opacity: [] as Site[] };
  for (const file of files) {
    const path = relative(root, file);
    const offences = inkOffences(readFileSync(file, 'utf8'));
    for (const kind of ['ghost', 'alpha', 'opacity'] as const)
      for (const o of offences[kind]) found[kind].push({ path, ...o });
  }
  const rules = [
    ['ghost', GHOST_ALLOWLIST, 'the ghost ink on a word or a figure'],
    ['alpha', ALPHA_INK_ALLOWLIST, 'an ink thinned by an opacity modifier'],
    ['opacity', OPACITY_ALLOWLIST, 'an element faded by opacity'],
  ] as const;

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(250);
  });

  for (const [kind, allowlist, what] of rules)
    it(`draws no ${what} but the ${kind} allowlist's, each with its reason`, () => {
      expect(allowlistFailures(found[kind], allowlist)).toEqual([]);
    });
});
