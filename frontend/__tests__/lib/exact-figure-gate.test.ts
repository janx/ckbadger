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
 * `toPrecision`, `Math.round` / `floor` / `ceil` / `trunc` reaching text, `toLocaleString` or
 * `Intl.NumberFormat` given fraction or significant digits, or a percent or compact style, or a
 * `toLocaleString` of a quotient, `Number(…)` / `parseFloat(…)` / `Number.parseFloat(…)` / a unary
 * `+` reaching text, a rounded value in a title or an accessible name (lane exactness, part A2, the test review's
 * NOTE-2, added the last five). Each is one of:
 *
 * - `drawing`: geometry — a width, a coordinate, a stroke, a colour channel, an error's pixels —
 *   never a figure a reader is told;
 * - `exact`: what is written is exact all the same (an integer read from its digits, a multiple of
 *   an axis step written at the decimals the step needs);
 * - `cut`: a floor that cuts a figure for display, the site's rule (a span's two largest units).
 *
 * Every site is on TOLERATED, by file, lane, kind and count, with its reason; a count is exact, so
 * a site that goes leaves the list, and a new one fails. No site is unread (lane exactness, part
 * B7): the page lanes' sites part A left as `part B` were each read in part B3, the stated figures
 * among them cut (the home network charts' unused float path, the clipped bar's label, the pool
 * tooltip's sizes, cycles and wait, the sync banner's rates and progress) and the rest named.
 *
 * What the reader cannot see (its limits): a name's `toLocaleString` (or a `format` of an
 * `Intl.NumberFormat`) with no digit option, which rounds a float at three places where it is exact
 * on an integer or a BigInt, and a name does not say which it holds (every such call sampled in the
 * chart and topic files is on an integer or a BigInt; a quotient written so is counted); a value
 * handed to a function or a component and written there, a component's text prop among them
 * (`label={Math.round(x)}`; `formatValue={(n) => …}` counts only a call it can read in that
 * function); names
 * are the file's, not a scope's (a name bound to a rounded value anywhere is read as bound
 * everywhere in the file); rounding done in BigInt arithmetic (a `+ divisor / 2` before a
 * quotient: part B took the shown ones out, `formatPercent`'s and `percentOf`'s in B2 and
 * `formatDecimalBytes`' in B3, and the two left are a tick's grid in lib/chart-ticks.ts, exact
 * multiples of its step, and a version bar's width in components/asset/cell-versions.tsx) or by a
 * function of the site's own that is not one of these calls; a float cut in its own text (a
 * `cutFloat` of a float's shortest decimal is exact to that decimal, which the float may not
 * be); a
 * `Number(…)` of a decimal string that is drawn and not written (not a site); and whether a
 * written float was exact before it was written (a template that writes `${x}` of a float made by
 * division is not a call it counts).
 */

const ROOT = join(__dirname, '..', '..');

type Lane = 'charts' | 'topics' | 'p1-chain' | 'p2-assets' | 'p3-lists' | 'shared';
type What = 'drawing' | 'exact' | 'cut';

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
    path: 'lib/format-asset.ts',
    lane: 'shared',
    what: 'exact',
    sites: { 'locale-rounds': 1 },
    reason:
      "a BigInt quotient written by the locale (lane exactness, part A2: the reader counts a quotient's toLocaleString and cannot tell a BigInt from a float): a token amount's whole units, the amount over 10^decimals in BigInt, its fraction written beside it",
  },
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
  // ---- the page lanes' files, read in part B (lane exactness, B3): named, none a stated figure ----
  {
    path: 'app/blocks/[id]/uncles/[index]/client-page.tsx',
    lane: 'p1-chain',
    what: 'exact',
    sites: { 'number-to-text': 1 },
    reason:
      "an uncle's index read from the route's path, an integer the page checks (its pattern and Number.isSafeInteger) before it reads it",
  },
  {
    path: 'components/address/address-history.tsx',
    lane: 'p1-chain',
    what: 'drawing',
    sites: { 'number-to-text': 2 },
    reason:
      "a capacity history row's occupied bar, its width occupied ÷ capacity in %; the figures beside it are written exact (the lists lane's file, read, not edited)",
  },
  {
    path: 'components/address/address-holdings.tsx',
    lane: 'p1-chain',
    what: 'drawing',
    sites: { 'number-to-text': 2 },
    reason:
      "a holding class's part of the occupied bar, its width in %; the class's figures are written exact in its legend",
  },
  {
    path: 'components/ui/cell-bar.tsx',
    lane: 'p1-chain',
    what: 'drawing',
    sites: { 'number-to-text': 5 },
    reason:
      "a cell bar's parts and its occupied share as widths in %; each part's figure is in its title and the bar's aria label, exact",
  },
  {
    path: 'components/ui/cell-tile.tsx',
    lane: 'p1-chain',
    what: 'drawing',
    sites: { 'number-to-text': 2 },
    reason:
      "a cell tile's occupied bar, its width occupied ÷ capacity in %; the occupied bytes are written whole beside it",
  },
  {
    path: 'lib/cell-parts.ts',
    lane: 'p1-chain',
    what: 'exact',
    sites: { 'number-to-text': 1 },
    reason:
      "an ELF section's size read from the API's digits, an integer of bytes far under 2^53 (a cell's data), placed and written exactly",
  },
  {
    path: 'lib/tx-detail-model.ts',
    lane: 'p1-chain',
    what: 'exact',
    sites: { 'locale-rounds': 1 },
    reason:
      "a BigInt quotient written by the locale (lane exactness, part A2: the reader counts a quotient's toLocaleString and cannot tell a BigInt from a float): a column head's whole CKB, shannons over 10^8 in BigInt, an integer the locale writes exactly",
  },
  {
    path: 'components/scripts/script-family-page.tsx',
    lane: 'p1-chain',
    what: 'exact',
    sites: { 'locale-rounds': 1 },
    reason:
      "a BigInt quotient written by the locale (lane exactness, part A2: the reader counts a quotient's toLocaleString and cannot tell a BigInt from a float): a cell's whole bytes, shannons over 10^8 in BigInt after the check that they are whole",
  },
  {
    path: 'components/asset/object-hero.tsx',
    lane: 'p2-assets',
    what: 'exact',
    sites: { 'locale-rounds': 1 },
    reason:
      "a BigInt quotient written by the locale (lane exactness, part A2: the reader counts a quotient's toLocaleString and cannot tell a BigInt from a float): a cell's whole CKB, shannons over 10^8 in BigInt where they divide exactly",
  },
  {
    path: 'components/asset/cell-versions.tsx',
    lane: 'p2-assets',
    what: 'drawing',
    sites: { 'number-to-text': 1 },
    reason:
      "a version's capacity bar, its width on the largest version's scale in thousandths of a %; the capacity is written exact beside it",
  },
  {
    path: 'components/asset/protocol-evidence.tsx',
    lane: 'p2-assets',
    what: 'exact',
    sites: { 'number-to-text': 1 },
    reason: "a field's index read from its key ('field-N'), an integer index into the fields",
  },
  {
    path: 'components/ui/capacity-partition.tsx',
    lane: 'p2-assets',
    what: 'drawing',
    sites: { 'number-to-text': 2 },
    reason:
      "a capacity partition's parts as widths in % of their sum; each part's figure is written exact in its legend",
  },
  {
    path: 'components/ui/allele-gel.tsx',
    lane: 'p2-assets',
    what: 'drawing',
    sites: { round: 2 },
    reason: "an allele gel's band colour step between two inks, and its measured width in whole px",
  },
  {
    path: 'components/ui/sequencing-gel.tsx',
    lane: 'p2-assets',
    what: 'drawing',
    sites: { round: 1 },
    reason: "a sequencing gel's measured width in whole px",
  },
  {
    path: 'components/home/ckbytes-section.tsx',
    lane: 'p3-lists',
    what: 'drawing',
    sites: { 'number-to-text': 2 },
    reason:
      "a CKBytes part's width in the allocation bar, its share of the circulating whole in %; each part's figure is written exact, and its bytes cut (formatDecimalBytes, part B3)",
  },
  {
    path: 'components/home/mini-chart.tsx',
    lane: 'p3-lists',
    what: 'drawing',
    sites: { round: 1, toFixed: 1 },
    reason:
      "a home chart's height under its tick labels' descent in whole px, and its svg coordinates at a tenth of a px; every figure it writes is a point's exact display or compact label (part B3)",
  },
  {
    path: 'components/pipeline/pipeline-panel.tsx',
    lane: 'p3-lists',
    what: 'drawing',
    sites: { toFixed: 6 },
    reason: "the reward strap's path coordinates at a tenth of a px",
  },
  {
    path: 'components/pipeline/pipeline-panel.tsx',
    lane: 'p3-lists',
    what: 'exact',
    sites: { 'number-to-text': 1 },
    reason:
      "a card's block number read back from its data attribute, an integer, to find its card (and to name it in the error where none is)",
  },
  {
    path: 'components/pipeline/window-label.tsx',
    lane: 'p3-lists',
    what: 'drawing',
    sites: { toFixed: 1 },
    reason: "the proposal window label's x at a tenth of a px",
  },
  {
    path: 'lib/pipeline/encoding.ts',
    lane: 'p3-lists',
    what: 'drawing',
    sites: { round: 1, toFixed: 1 },
    reason:
      "a colour mix's share in tenths of a %, and a cellbase diamond's path coordinates at a tenth of a px",
  },
  {
    path: 'lib/pipeline/layout.ts',
    lane: 'p3-lists',
    what: 'drawing',
    sites: { round: 2, toFixed: 4 },
    reason:
      'path coordinates at a tenth of a px, how many arrowheads a gap holds, and how many field rows a plate keeps under its uncles',
  },
  {
    path: 'lib/pipeline/motion.ts',
    lane: 'p3-lists',
    what: 'drawing',
    sites: { toFixed: 16, 'number-to-text': 2 },
    reason:
      "the pipeline's motion: transforms and opacities in svg attributes, and a mark's centre read back from its cx and cy",
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
  'locale-rounds',
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

  // lane exactness, part B7: every site is named (drawing, exact or cut); no lane's is left unread
  it('holds every page to named sites, none a stated figure', () => {
    expect(
      TOLERATED.filter((t) => !['drawing', 'exact', 'cut'].includes(t.what)).map((t) => t.path)
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
      // re-pinned for lane exactness, part A2 (the test review's NOTE-2): the reader now counts a quotient's
      // toLocaleString, and four BigInt quotients, exact, are named (shared 17, p1-chain 13 and
      // p2-assets 7 before); no site was added to the code
      shared: 18,
      'p1-chain': 15,
      'p2-assets': 8,
      // lane exactness, part B3: 53 → 38, the stated figures cut (the network charts' float path
      // 3, the clipped bar's label 4, the pool tooltip 5, the sync banner 3); the rest named
      'p3-lists': 38,
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
      '1 locale-rounds',
      '2 locale-rounds',
      '3 locale-rounds',
      '4 number-to-text',
      '5 number-to-text',
      '6 round',
      '7 round',
      '8 round',
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
