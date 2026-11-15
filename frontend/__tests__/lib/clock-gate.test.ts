import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the ages on screen (lane E4, the review's item H): an age is a statement that goes
 * stale, so every age the pages write reads the page's one clock (lib/clock.ts) through its hooks
 * in components/ui/utc8-date.tsx (`useClockText`, `useAge`, `ClockText`, `useClockTime`,
 * `Utc8Age`, `Utc8Date`); the status line's clock reads it too. An age computed from a read of the
 * wall clock is written once and stands still between data updates ("20s ago" for minutes).
 *
 * The gate is exact because the age formatters take no default time: `formatTimeAgo`,
 * `formatAgeOneUnit`, `formatAgeTwoUnits`, `ageInOneUnit` and `ageSeconds` need their `now`, so
 * the compiler finds every caller, and a caller's time can only come from the clock's hooks or a
 * read of the wall clock. So, in the TypeScript of every file under app/ and components/ (the
 * TypeScript parser reads code, not comments or strings):
 *
 * - no read of the wall clock: `Date.now` (called or passed), `new Date` with no argument, `Date()`
 *   called, `performance.now`; the reads that are not a time on screen (a request's parameter, a
 *   drawing's scale) are listed per file, in order, each with its reason and an anchor that must
 *   stand on its line, in the style of the ink gate's allowlists;
 * - no use of the clock's own parts (`clockNow`, `subscribeClock`) outside its hooks and the
 *   status line's clock: a `clockNow()` read in a render without subscribing is a frozen age too.
 *
 * And the age formatters themselves (lib/format-age.ts, lib/utils.ts) read no wall clock at all.
 *
 * Outside the scan, read by hand (lane E4's hand-back): lib/ and hooks/ modules that read the wall
 * clock for no age (a chart's request range, an export's stamp, animation frames, the daily
 * cards' "yesterday" bucket, which their data refetch moves).
 */

interface Allowed {
  reason: string;
  anchor: string;
}
type Allowlist = Readonly<Record<string, readonly Allowed[]>>;

/** Reads of the wall clock under app/ and components/ that write no time on screen. */
const WALL_CLOCK_ALLOWLIST: Allowlist = {
  'app/network/trends.tsx': [
    {
      reason:
        'a request parameter: the network history is read up to the second it is asked for; the page writes the days the API returns',
      anchor: 'Math.floor(Date.now() / 1000)',
    },
  ],
  'components/scripts/script-family-page.tsx': [
    {
      reason:
        "a drawing's scale: the holder timeline's right end (its axis says “today”); no age or duration is written from it",
      anchor: 'now={Date.now()}',
    },
  ],
};

/** The files that may use the clock's own parts: its hooks, and the status line's clock. */
const CLOCK_PARTS_ALLOWED = new Set([
  'components/ui/utc8-date.tsx',
  'components/layout/status-line.tsx',
]);
const CLOCK_PARTS = new Set(['clockNow', 'subscribeClock']);

const SCAN_DIRS = ['app', 'components'];
/** The age formatters: no read of the wall clock at all, not even as a default. */
const FORMATTERS = ['lib/format-age.ts', 'lib/utils.ts'];

interface Site {
  path: string;
  line: number;
  what: string;
  /** The site's own source line, where its entry's anchor must stand. */
  source: string;
}

/** `Date`, or a property reaching it (`globalThis.Date`, `window.Date`). */
const isDate = (e: ts.Expression) =>
  (ts.isIdentifier(e) && e.text === 'Date') ||
  (ts.isPropertyAccessExpression(e) && e.name.text === 'Date');
/** `performance`, or a property reaching it (`window.performance`). */
const isPerformance = (e: ts.Expression) =>
  (ts.isIdentifier(e) && e.text === 'performance') ||
  (ts.isPropertyAccessExpression(e) && e.name.text === 'performance');

/** Every read of the wall clock in a source text, and every use of the clock's own parts. */
function clockReads(code: string, fileName = 'x.tsx'): { wall: Site[]; parts: Site[] } {
  const sf = ts.createSourceFile(fileName, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const lines = code.split('\n');
  const wall: Site[] = [];
  const parts: Site[] = [];
  const site = (node: ts.Node, what: string): Site => {
    const line = sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
    return { path: fileName, line, what, source: lines[line - 1] };
  };
  const visit = (node: ts.Node) => {
    if (ts.isPropertyAccessExpression(node) && node.name.text === 'now') {
      if (isDate(node.expression)) wall.push(site(node, 'Date.now'));
      else if (isPerformance(node.expression)) wall.push(site(node, 'performance.now'));
    } else if (
      ts.isNewExpression(node) &&
      isDate(node.expression) &&
      (node.arguments === undefined || node.arguments.length === 0)
    ) {
      wall.push(site(node, 'new Date()'));
    } else if (ts.isCallExpression(node) && isDate(node.expression)) {
      wall.push(site(node, 'Date()'));
    } else if (ts.isIdentifier(node) && CLOCK_PARTS.has(node.text)) {
      parts.push(site(node, node.text));
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return { wall, parts };
}

/**
 * What the sites break in the allowlist: a site with no reason (a file not listed, or one more
 * site than reasons), a site whose line lacks its reason's anchor, a file with fewer sites than
 * reasons, a reason too short to say why.
 */
function allowlistFailures(sites: readonly Site[], allowlist: Allowlist): string[] {
  const byPath = new Map<string, Site[]>();
  for (const s of sites) byPath.set(s.path, [...(byPath.get(s.path) ?? []), s]);
  const out: string[] = [];
  for (const [path, list] of byPath) {
    const entries = allowlist[path] ?? [];
    list.forEach((s, i) => {
      if (i >= entries.length)
        out.push(
          `${path}:${s.line} ${s.what}: no reason for read ${i + 1} of ${list.length} (${entries.length} listed) — read the one clock (components/ui/utc8-date.tsx) for an age`
        );
      else if (!s.source.includes(entries[i].anchor))
        out.push(
          `${path}:${s.line} ${s.what}: its line lacks the anchor "${entries[i].anchor}" of its reason (${entries[i].reason})`
        );
    });
  }
  for (const [path, entries] of Object.entries(allowlist)) {
    const list = byPath.get(path) ?? [];
    if (list.length < entries.length)
      out.push(
        `${path}: ${entries.length} reasons for ${list.length} reads (${list.map((s) => `${s.line} ${s.what}`).join(', ') || 'none'}): drop the reason of each read that went`
      );
    for (const { reason, anchor } of entries) {
      if (reason.trim().length <= 10)
        out.push(`${path}: a reason too short to say why: "${reason}"`);
      if (anchor.trim() === '') out.push(`${path}: an empty anchor names no read`);
    }
  }
  return out;
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

describe('the clock gate finds what it must, and only that', () => {
  const wall = (code: string) => clockReads(code).wall.map((s) => s.what);
  const parts = (code: string) => clockReads(code).parts.map((s) => s.what);

  it('finds each read of the wall clock, called or passed', () => {
    expect(wall('const a = Date.now();')).toEqual(['Date.now']);
    expect(wall('const read = Date.now;')).toEqual(['Date.now']);
    expect(wall('useState(() => Date.now())')).toEqual(['Date.now']);
    expect(wall('const d = new Date();')).toEqual(['new Date()']);
    expect(wall('const d = new Date;')).toEqual(['new Date()']);
    expect(wall('const s = Date();')).toEqual(['Date()']);
    expect(wall('const t = performance.now();')).toEqual(['performance.now']);
    expect(wall('const t = window.performance.now();')).toEqual(['performance.now']);
    expect(wall('const t = globalThis.Date.now();')).toEqual(['Date.now']);
    expect(wall('<Row now={Date.now()} />')).toEqual(['Date.now']);
  });

  it('leaves a time that is not now, a comment and a string alone', () => {
    expect(wall('const d = new Date(timestamp);')).toEqual([]);
    expect(wall('const d = new Date(Number(x)).toISOString();')).toEqual([]);
    expect(wall('const t = Date.parse(x); const u = Date.UTC(2026, 0, 1);')).toEqual([]);
    expect(wall('// Date.now() would freeze the age\nconst a = 1;')).toEqual([]);
    expect(wall("const words = 'Date.now() and new Date()';")).toEqual([]);
    expect(wall('/* performance.now() */ const b = now;')).toEqual([]);
    expect(wall('const later = clock.now();')).toEqual([]);
  });

  it('finds the clock’s own parts wherever they are named', () => {
    expect(parts("import { clockNow, subscribeClock } from '@/lib/clock';")).toEqual([
      'clockNow',
      'subscribeClock',
    ]);
    expect(parts('formatTimeAgo(t, clockNow())')).toEqual(['clockNow']);
    expect(parts('useSyncExternalStore(subscribeClock, read, read)')).toEqual(['subscribeClock']);
    expect(parts('useClockText((now) => formatTimeAgo(t, now))')).toEqual([]);
  });

  it('states the line of each read', () => {
    const [read] = clockReads('const a = 1;\n\nconst b = Date.now();').wall;
    expect(read.line).toBe(3);
    expect(read.source).toBe('const b = Date.now();');
  });

  it('fails a read with no reason, a reason whose read went, and an anchor off the line', () => {
    const at = (path: string, source: string, line = 1): Site => ({
      path,
      line,
      what: 'Date.now',
      source,
    });
    const reason = 'a request parameter, not a time on screen';
    const list: Allowlist = { 'a.tsx': [{ reason, anchor: 'nowSeconds' }] };
    expect(allowlistFailures([at('a.tsx', 'return nowSeconds(Date.now());')], list)).toEqual([]);
    // one more read than reasons
    expect(
      allowlistFailures(
        [at('a.tsx', 'return nowSeconds(Date.now());'), at('a.tsx', 'x = Date.now()', 2)],
        list
      )
    ).toHaveLength(1);
    // a file not listed
    expect(allowlistFailures([at('b.tsx', 'x = Date.now()')], list)[0]).toMatch(
      /^b\.tsx:1 Date\.now: no reason/
    );
    // the listed read went: its reason stays behind
    expect(allowlistFailures([], list)[0]).toMatch(/^a\.tsx: 1 reasons for 0 reads/);
    // the n-th reason's anchor must stand on the n-th read's line
    expect(allowlistFailures([at('a.tsx', 'const age = Date.now() - t;')], list)[0]).toMatch(
      /lacks the anchor "nowSeconds"/
    );
    expect(
      allowlistFailures([at('a.tsx', 'nowSeconds(Date.now())')], {
        'a.tsx': [{ reason: 'short', anchor: 'nowSeconds' }],
      })
    ).toEqual(['a.tsx: a reason too short to say why: "short"']);
  });
});

describe('every age on screen reads the one clock', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root).map((path) => ({
    path: relative(root, path),
    code: readFileSync(path, 'utf8'),
  }));

  it('scans the pages and the components', () => {
    expect(files.length).toBeGreaterThan(250);
    expect(files.map((f) => f.path)).toContain('components/ui/utc8-date.tsx');
  });

  it('reads the wall clock under app/ and components/ only where no time on screen comes of it', () => {
    const sites = files.flatMap(({ path, code }) => clockReads(code, path).wall);
    expect(allowlistFailures(sites, WALL_CLOCK_ALLOWLIST)).toEqual([]);
  });

  it('uses the clock’s own parts only in its hooks and the status line’s clock', () => {
    const outside = files
      .flatMap(({ path, code }) => clockReads(code, path).parts)
      .filter((s) => !CLOCK_PARTS_ALLOWED.has(s.path))
      .map((s) => `${s.path}:${s.line} ${s.what}`);
    expect(outside).toEqual([]);
    for (const path of CLOCK_PARTS_ALLOWED)
      expect(files.find((f) => f.path === path)?.code).toMatch(/\bsubscribeClock\b/);
  });

  it('gives the age formatters no wall clock to fall back on', () => {
    const reads = FORMATTERS.flatMap((path) =>
      clockReads(readFileSync(join(root, path), 'utf8'), path).wall.map(
        (s) => `${s.path}:${s.line} ${s.what}`
      )
    );
    expect(reads).toEqual([]);
  });
});
