import { readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { sourceFiles } from '@/__tests__/utils/source-scan';
import { exactSites, type ExactKind, type ExactSite } from '@/__tests__/utils/exact-figure-scan';

/**
 * Gate for the exact figure (lane exactness, part A, A8; CLAUDE.md "Numerical Precision" and the
 * lead's rulings): a figure the site states is the API's exact value, cut for display, never
 * rounded. In `app/`, `components/`, `hooks/` and `lib/` the gate reads each source's syntax tree
 * (`__tests__/utils/exact-figure-scan.ts`, which says what it counts) and finds every call that
 * rounds a number or reads a decimal into a float where its result may be written: `toFixed`,
 * `toPrecision`, `Math.round` / `floor` / `ceil` reaching text, `toLocaleString` or
 * `Intl.NumberFormat` given fraction or significant digits, `Number(…)` / `parseFloat(…)` reaching
 * text. Each is one of:
 *
 * - `drawing`: geometry — a width, a coordinate, a stroke, a colour channel, an error's pixels —
 *   never a figure a reader is told;
 * - `exact`: what is written is exact all the same (an integer read from its digits, a multiple of
 *   an axis step written at the decimals the step needs);
 * - `cut`: a floor that cuts a figure for display, the site's rule (a span's two largest units);
 * - `part B`: a site in a page lane's files (P1 chain, P2 assets, P3 lists and home), named here by
 *   its lane and read in part B, when those files are free: a stated figure there is a fault.
 *
 * Every site is on TOLERATED, by file, lane, kind and count, with its reason; a count is exact, so
 * a site that goes leaves the list, and a new one fails. The chart pages and the topic pages hold
 * no `part B` site: each of theirs is named.
 *
 * What the reader cannot see (its limits): a value handed to a function or a component and
 * written there (`formatValue={(n) => …}` counts only a call it can read in that function); names
 * are the file's, not a scope's (a name bound to a rounded value anywhere is read as bound
 * everywhere in the file); rounding done in BigInt arithmetic (`formatPercent`'s and `percentOf`'s
 * half up, part B's) or by a function of the site's own that is not one of these calls; a
 * `Number(…)` of a decimal string that is drawn and not written (not a site); and whether a
 * written float was exact before it was written (a template that writes `${x}` of a float made by
 * division is not a call it counts).
 */

const ROOT = join(__dirname, '..', '..');

type Lane = 'charts' | 'topics' | 'p1-chain' | 'p2-assets' | 'p3-lists' | 'shared';
type What = 'drawing' | 'exact' | 'cut' | 'part B';

interface Tolerated {
  path: string;
  lane: Lane;
  what: What;
  sites: Partial<Record<ExactKind, number>>;
  reason: string;
}

const TOLERATED: readonly Tolerated[] = [
  // ---- charts: named, none a stated figure ----
  {
    path: 'components/charts/chart-model.ts',
    lane: 'charts',
    what: 'exact',
    sites: { round: 1, toFixed: 1, 'number-to-text': 1 },
    reason:
      'axis ticks: the first and last multiple of a step in a run (Math.ceil / floor of an index), a bucket tick written at the decimals its step needs (stepDecimals), exactly the multiple, and an epoch or model-year label read as its integer for its place on the axis',
  },
  {
    path: 'lib/chart-ticks.ts',
    lane: 'charts',
    what: 'exact',
    sites: { round: 1, 'number-to-text': 1 },
    reason:
      "the axis steps: a tick's index in its run (Math.floor) and a step's decimal exponent read as its integer (stepDecimals)",
  },
  // ---- topics: named, none a stated figure ----
  {
    path: 'components/dao/dao-board.tsx',
    lane: 'topics',
    what: 'drawing',
    sites: { 'number-to-text': 1 },
    reason: "a phase-0 cycle window's bar height, % of the largest window's capacity",
  },
  {
    path: 'components/dao/dao-grows.tsx',
    lane: 'topics',
    what: 'drawing',
    sites: { round: 1 },
    reason: "a year's bar height in whole px, its share of 48 px; the figure beside it is exact",
  },
  {
    path: 'components/fiber/fiber-channel-timeline.tsx',
    lane: 'topics',
    what: 'drawing',
    sites: { round: 2 },
    reason: "the timeline svg's top and bottom in whole px around its words",
  },
  {
    path: 'components/fiber/fiber-network-graph.tsx',
    lane: 'topics',
    what: 'drawing',
    sites: { toFixed: 4 },
    reason:
      "an edge's stroke widths, and a party's x and reach in px in the error that says it runs off the canvas",
  },
  {
    path: 'components/fiber/fiber-split-bar.tsx',
    lane: 'topics',
    what: 'drawing',
    sites: { 'number-to-text': 1 },
    reason: "the split bar's left width, % at four places; the two amounts are written exact",
  },
  {
    path: 'lib/fiber-network-model.ts',
    lane: 'topics',
    what: 'drawing',
    sites: { round: 2 },
    reason: "a party's or an island's width in px in the error that says the canvas cannot hold it",
  },
  {
    path: 'components/ui/icicle-partition.tsx',
    lane: 'topics',
    what: 'drawing',
    sites: { round: 1 },
    reason: "the height of a row's labels in whole px (the DAO page's issuance icicle)",
  },
  // ---- shared modules ----
  {
    path: 'components/ui/share-bar.tsx',
    lane: 'shared',
    what: 'drawing',
    sites: { round: 1, 'number-to-text': 1 },
    reason:
      "a share bar's width, % of its scale at two places, and an API percentage read as its hundredths for the bar",
  },
  {
    path: 'components/ui/units.tsx',
    lane: 'shared',
    what: 'exact',
    sites: { round: 1 },
    reason: "the digit groups of a u64 in the count pattern's regular expression, not a figure",
  },
  {
    path: 'lib/format-age.ts',
    lane: 'shared',
    what: 'cut',
    sites: { round: 4 },
    reason:
      "a span's two largest units and an age's whole years and days, floored: cut, never rounded",
  },
  {
    path: 'lib/design-tokens.ts',
    lane: 'shared',
    what: 'drawing',
    sites: { round: 1 },
    reason: "a colour ramp's step, in thousandths",
  },
  {
    path: 'lib/palette.ts',
    lane: 'shared',
    what: 'drawing',
    sites: { round: 2 },
    reason: "a colour's alpha and a blended channel",
  },
  {
    path: 'lib/seal/brush.ts',
    lane: 'shared',
    what: 'drawing',
    sites: { round: 2 },
    reason: "a seal's brush coordinates (lib/seal, which no lane edits)",
  },
  {
    path: 'lib/seal/encode.ts',
    lane: 'shared',
    what: 'drawing',
    sites: { round: 1 },
    reason: "a seal's glyph index (lib/seal)",
  },
  {
    path: 'lib/seal/stones.ts',
    lane: 'shared',
    what: 'drawing',
    sites: { round: 2 },
    reason: "a stone's width and height in tenths of a px (lib/seal)",
  },
  {
    path: 'lib/shorten.ts',
    lane: 'shared',
    what: 'exact',
    sites: { 'number-to-text': 1 },
    reason: "an out point's index read from its digits, an integer",
  },
  {
    path: 'lib/utils.ts',
    lane: 'shared',
    what: 'exact',
    sites: { 'number-to-text': 1 },
    reason: 'a timestamp in ms read from its digits, an integer',
  },
  // ---- part B: the page lanes' files ----
  {
    path: 'app/blocks/[id]/uncles/[index]/client-page.tsx',
    lane: 'p1-chain',
    what: 'part B',
    sites: { 'number-to-text': 1 },
    reason: "an uncle's index read from the route's path",
  },
  {
    path: 'components/address/address-history.tsx',
    lane: 'p1-chain',
    what: 'part B',
    sites: { 'number-to-text': 2 },
    reason: "a capacity history's occupied and full capacity read into floats",
  },
  {
    path: 'components/address/address-holdings.tsx',
    lane: 'p1-chain',
    what: 'part B',
    sites: { 'number-to-text': 2 },
    reason: "a holding class's occupied capacity read into a float",
  },
  {
    path: 'components/ui/cell-bar.tsx',
    lane: 'p1-chain',
    what: 'part B',
    sites: { 'number-to-text': 5 },
    reason: "a cell bar's parts read into floats for their widths and words",
  },
  {
    path: 'components/ui/cell-tile.tsx',
    lane: 'p1-chain',
    what: 'part B',
    sites: { 'number-to-text': 2 },
    reason: "a cell tile's occupied share of its capacity",
  },
  {
    path: 'lib/cell-parts.ts',
    lane: 'p1-chain',
    what: 'part B',
    sites: { 'number-to-text': 1 },
    reason: "an ELF section's size read from the API's digits",
  },
  {
    path: 'components/asset/cell-versions.tsx',
    lane: 'p2-assets',
    what: 'part B',
    sites: { 'number-to-text': 1 },
    reason: "a version's capacity on the largest one's scale, half up at a ten-thousandth",
  },
  {
    path: 'components/asset/protocol-evidence.tsx',
    lane: 'p2-assets',
    what: 'part B',
    sites: { 'number-to-text': 1 },
    reason: "a field's index read from its key",
  },
  {
    path: 'components/ui/capacity-partition.tsx',
    lane: 'p2-assets',
    what: 'part B',
    sites: { 'number-to-text': 2 },
    reason: "a capacity partition's parts read into floats for their widths and words",
  },
  {
    path: 'components/ui/allele-gel.tsx',
    lane: 'p2-assets',
    what: 'part B',
    sites: { round: 2 },
    reason: "an allele gel's band positions and its measured width",
  },
  {
    path: 'components/ui/sequencing-gel.tsx',
    lane: 'p2-assets',
    what: 'part B',
    sites: { round: 1 },
    reason: "a sequencing gel's measured width",
  },
  {
    path: 'components/home/ckbytes-section.tsx',
    lane: 'p3-lists',
    what: 'part B',
    sites: { 'number-to-text': 2 },
    reason:
      'a CKBytes part and the circulating whole read into floats (formatDecimalBytes, part B)',
  },
  {
    path: 'components/home/mini-chart.tsx',
    lane: 'p3-lists',
    what: 'part B',
    sites: { round: 1, toFixed: 1 },
    reason: "a home chart's tick line height and a value at one place",
  },
  {
    path: 'components/home/network-section.tsx',
    lane: 'p3-lists',
    what: 'part B',
    sites: { toFixed: 3 },
    reason: "the home network charts' values at two places (block time, hash rate, uncle rate)",
  },
  {
    path: 'components/pipeline/pipeline-panel.tsx',
    lane: 'p3-lists',
    what: 'part B',
    sites: { toFixed: 6, 'number-to-text': 1 },
    reason: "the pipeline's strap path and a card's block number read from its attribute",
  },
  {
    path: 'components/pipeline/pipeline-tooltip.tsx',
    lane: 'p3-lists',
    what: 'part B',
    sites: { toFixed: 5 },
    reason: "a pool transaction's size, cycles and wait, rounded (MB, M, s)",
  },
  {
    path: 'components/pipeline/window-label.tsx',
    lane: 'p3-lists',
    what: 'part B',
    sites: { toFixed: 1 },
    reason: "the proposal window label's x",
  },
  {
    path: 'components/stats-cards.tsx',
    lane: 'p3-lists',
    what: 'part B',
    sites: { toFixed: 3 },
    reason: 'a stats card value in thousands and the sync progress, rounded',
  },
  {
    path: 'lib/chart-ticks.ts',
    lane: 'p3-lists',
    what: 'part B',
    sites: { toFixed: 4 },
    reason:
      "formatCompactSigned, a clipped bar's label on the home page's CKBytes charts, rounded (part B: formatCompactSigned where it labels a value)",
  },
  {
    path: 'lib/pipeline/encoding.ts',
    lane: 'p3-lists',
    what: 'part B',
    sites: { round: 1, toFixed: 1 },
    reason: "the pipeline's encoded timings and a coordinate",
  },
  {
    path: 'lib/pipeline/layout.ts',
    lane: 'p3-lists',
    what: 'part B',
    sites: { round: 2, toFixed: 4 },
    reason: "the pipeline's layout: slot counts and path coordinates",
  },
  {
    path: 'lib/pipeline/motion.ts',
    lane: 'p3-lists',
    what: 'part B',
    sites: { toFixed: 16, 'number-to-text': 2 },
    reason: "the pipeline's motion: transforms, opacities and a mark's centre read back",
  },
];

/** The sites of every source, by its path. */
function foundSites(): Map<string, ExactSite[]> {
  const found = new Map<string, ExactSite[]>();
  for (const path of sourceFiles(ROOT)) {
    const rel = relative(ROOT, path).split('\\').join('/');
    const sites = exactSites(
      readFileSync(path, 'utf8'),
      path.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
    );
    if (sites.length) found.set(rel, sites);
  }
  return found;
}

const KINDS: readonly ExactKind[] = [
  'toFixed',
  'toPrecision',
  'round',
  'fraction-digits',
  'number-to-text',
];

describe('the exact-figure gate (lane exactness, part A, A8)', () => {
  it('finds no rounding call that is not on the list, and every one on it, exactly', () => {
    const found = foundSites();
    const failures: string[] = [];
    const paths = new Set([...found.keys(), ...TOLERATED.map((t) => t.path)]);
    for (const path of paths) {
      const sites = found.get(path) ?? [];
      for (const kind of KINDS) {
        const n = sites.filter((s) => s.kind === kind).length;
        const allowed = TOLERATED.filter((t) => t.path === path).reduce(
          (sum, t) => sum + (t.sites[kind] ?? 0),
          0
        );
        if (n !== allowed)
          failures.push(
            `${path}: ${n} ${kind} site(s), ${allowed} tolerated — ${sites
              .filter((s) => s.kind === kind)
              .map((s) => `${s.line} ${s.text}`)
              .join('; ')}`
          );
      }
    }
    expect(failures).toEqual([]);
  });

  it('holds the chart and topic pages to named sites, none a stated figure', () => {
    expect(
      TOLERATED.filter((t) => (t.lane === 'charts' || t.lane === 'topics') && t.what === 'part B')
    ).toEqual([]);
    for (const t of TOLERATED) expect(t.reason.length, t.path).toBeGreaterThan(20);
  });

  it('pins its list by lane: it only shrinks', () => {
    const byLane = (lane: Lane) =>
      TOLERATED.filter((t) => t.lane === lane).reduce(
        (sum, t) => sum + Object.values(t.sites).reduce((a, b) => a + b, 0),
        0
      );
    expect({
      charts: byLane('charts'),
      topics: byLane('topics'),
      shared: byLane('shared'),
      'p1-chain': byLane('p1-chain'),
      'p2-assets': byLane('p2-assets'),
      'p3-lists': byLane('p3-lists'),
    }).toEqual({
      charts: 5,
      topics: 12,
      shared: 17,
      'p1-chain': 13,
      'p2-assets': 7,
      'p3-lists': 53,
    });
  });

  it('reads every form it counts (the reader is not blind)', () => {
    const kinds = (code: string) => exactSites(code).map((s) => `${s.line} ${s.kind}`);
    expect(
      kinds(
        [
          'const a = (1.25).toFixed(1);',
          'const b = x.toPrecision(3);',
          'const c = <span>{Math.round(share * 100)}%</span>;',
          'const d = `${Math.floor(n / 60)} min`;',
          'const held = Math.ceil(v);',
          'const e = `${held} px`;',
          "const f = v.toLocaleString('en-US', { maximumFractionDigits: 2 });",
          "const g = new Intl.NumberFormat('en-US', { minimumFractionDigits: 2 });",
          "const h = 'value ' + Number(text);",
          'const i = String(parseFloat(s));',
          'const j = <b>{(Math.round(r) / 100).toString()}</b>;',
        ].join('\n')
      )
    ).toEqual([
      '1 toFixed',
      '2 toPrecision',
      '3 round',
      '4 round',
      '5 round',
      '7 fraction-digits',
      '8 fraction-digits',
      '9 number-to-text',
      '10 number-to-text',
      '11 round',
    ]);
  });

  // lane exactness, part A2 (the test review's NOTE-2): the forms that can round a shown figure beside those above
  it('reads the other forms that round or read a float into a shown figure', () => {
    const kinds = (code: string) => exactSites(code).map((s) => `${s.line} ${s.kind}`);
    expect(
      kinds(
        [
          // a quotient written by the locale: three places at most, rounded
          "const a = (n / d).toLocaleString('en-US');",
          // a percent or a compact form rounds whatever it is given
          "const b = v.toLocaleString('en-US', { style: 'percent' });",
          "const c = new Intl.NumberFormat('en-US', { notation: 'compact' });",
          // a decimal read into a float, written
          'const d = `${Number.parseFloat(s)} CKB`;',
          'const e = `${+s} CKB`;',
          // a cut to an integer, written
          'const f = <span>{Math.trunc(x)}</span>;',
          // a rounded value in an attribute a reader is told: a title, a label
          'const g = <abbr title={Math.round(x)} />;',
          'const h = <b aria-label={String(Math.round(x))} />;',
        ].join('\n')
      )
    ).toEqual([
      '2 locale-rounds',
      '4 locale-rounds',
      '5 locale-rounds',
      '7 number-to-text',
      '8 number-to-text',
      '10 round',
      '12 round',
      '13 round',
    ]);
  });

  // lane exactness, part A2 (NOTE-2): a name's `toLocaleString` with no digit option is not counted, because the reader
  // cannot tell a float held by a name from an integer (an integer is exact under the locale; a float
  // is rounded at three places): its limits paragraph says so, and this pins that it does
  it('states, in its limits, the float a name holds that it cannot tell from an integer', () => {
    expect(exactSites("const k = v.toLocaleString('en-US');")).toEqual([]);
    const own = readFileSync(join(__dirname, 'exact-figure-gate.test.ts'), 'utf8');
    const limits = own.slice(0, own.indexOf('const ROOT'));
    expect(limits).toMatch(/rounds a float at three places/);
    expect(limits).toMatch(/a name's `toLocaleString`/);
  });

  it('passes what it does not count: geometry in an attribute, a value never written', () => {
    expect(
      exactSites(
        [
          'const w = Math.round(px);',
          '<rect width={w} x={Math.floor(x)} />;',
          'const n = Number(s);',
          'if (n > 3) go(n);',
          'const m = rows[Math.ceil(i / 2)];',
          '// Math.round(x) in a comment',
          "const t = 'Math.round(x) in a string';",
        ].join('\n')
      )
    ).toEqual([]);
  });
});
