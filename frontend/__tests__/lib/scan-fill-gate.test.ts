import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the scan (readability R1, Gate F0; the owner: by night the scan is cut into a figure's
 * fills, and no layer lies over the text). The scan is a property of a fill, never of a word:
 *
 * - the class `.scan-fill` (app/globals.css: the material `fill-scan` as a mask image) and the
 *   `mask` attribute of an SVG element stand only on an element that holds no words: not SVG text
 *   (`<text>`, `<tspan>`, `<textPath>`), not an element whose JSX children, as written, contain
 *   text (non-blank JSX text, an expression that is not JSX, such an element further down through
 *   `?:`, `&&`, `||`, `??` and the JSX inside a `.map(…)`), and not a component, which may draw
 *   words the scan cannot see; `<title>` and the paint servers paint nothing — the manner of
 *   `__tests__/lib/ink-gate.test.ts`;
 * - the class is written only on an element's `className`, where the gate sees what it holds (a
 *   constant or a helper that carried it would hide its element), and the scan's CSS variable
 *   (`--m-fill-scan`) is named nowhere in the source and read by the `.scan-fill` rule alone;
 * - the elements that carry the scan are listed below by file: a new one is a decision (the
 *   seals, the pipeline's plates and strap, a tile and a word never take it), and one that went
 *   leaves a count behind.
 *
 * A `mask` is the scan's when its value reads the result of `useScanFill()`
 * (components/ui/scan-fill.tsx) or a name bound to anything read from it; every other `mask` (the
 * pipeline strap's fade) is held to the same rule of no words, and is not counted. A JSX spread
 * that reads the scan or writes a `mask`, and any other prop that reads the scan (a component's
 * `fill={scan}`), are masks too. Limits: the scan's result passed into a function, or a mask set
 * through a `style` object, is followed only where it reads a scan name in this file.
 */

const SCAN_DIRS = ['app', 'components', 'hooks', 'lib'];
const CLASS = /(?<![\w-])scan-fill(?![\w-])/;
const CLASS_ALL = new RegExp(CLASS.source, 'g');
/** The scan's CSS variable (the material `fill-scan`, `lib/palette.ts`). */
const FILL_VAR_ALL = /--m-fill-scan(?![\w-])/g;
const TEXT_TAGS = new Set(['text', 'tspan', 'textPath']);
const UNPAINTED_TAGS = new Set([
  'title',
  'desc',
  'defs',
  'clipPath',
  'mask',
  'pattern',
  'linearGradient',
  'radialGradient',
  'filter',
  'marker',
  'symbol',
  'style',
]);

/**
 * The elements that carry the scan, by file: the HTML fills that take `.scan-fill` and the SVG
 * fills that take the scan's mask. The address page's balance bar is not here: its parts print
 * their share inside the fill, and a wordless layer under the label would stripe the ground right
 * behind the words. The vitals strip's epoch bar is not here: it is set in block characters, text.
 */
const SCAN_SITES: Readonly<Record<string, { fills: number; masks: number }>> = {
  // the address's cells table: a cell's occupied share of its capacity
  'components/address/address-history.tsx': { fills: 1, masks: 0 },
  // a token's holders: a holder's share of the holders
  'components/asset/token-holders.tsx': { fills: 1, masks: 0 },
  // a collection's fully on-chain share
  'components/asset/spore-detail.tsx': { fills: 1, masks: 0 },
  // the stacked resource bar of a narrow column: overhead, user transactions
  'components/blocks/block-visual.tsx': { fills: 2, masks: 0 },
  // a chart page's and a chart card's share rows
  'components/charts/chart-detail.tsx': { fills: 1, masks: 0 },
  'components/charts/chart-small-multiple.tsx': { fills: 1, masks: 0 },
  // the network page's distributions
  'app/network/distributions.tsx': { fills: 1, masks: 0 },
  // the home page's ranked shares; its bars and signed bars
  'components/home/ranked-bars.tsx': { fills: 1, masks: 0 },
  'components/home/mini-chart.tsx': { fills: 0, masks: 2 },
  // the list bars (blocks, transactions, scripts): every segment
  'components/ui/aligned-bar.tsx': { fills: 1, masks: 0 },
  // a gene's top variant share
  'components/ui/allele-gel.tsx': { fills: 1, masks: 0 },
  // the capacity partition, its two forms
  'components/ui/capacity-partition.tsx': { fills: 2, masks: 0 },
  // a cell's occupied share, under the tile's words
  'components/ui/cell-tile.tsx': { fills: 1, masks: 0 },
  // the shared chart: a ranked composition's bars (HTML); the areas and the bars (SVG)
  'components/ui/chart.tsx': { fills: 1, masks: 2 },
  // a holder phone row's share
  'components/ui/share-bar.tsx': { fills: 1, masks: 0 },
};

type Jsx = ts.JsxElement | ts.JsxSelfClosingElement | ts.JsxFragment;

interface Site {
  line: number;
  what: string;
  /** Whether the element holds words (or may: a component). */
  words: boolean;
  /** For a `mask`: whether it is the scan's (it reads `useScanFill()`). */
  scan: boolean;
}

/** The fills, the masks and the stray class names of one TSX source. */
function scanSites(code: string): { fills: Site[]; masks: Site[]; stray: number[] } {
  const source = ts.createSourceFile(
    'scan.tsx',
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX
  );
  const text = (n: ts.Node) => n.getText(source).replace(/\s+/g, ' ');
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const isJsx = (n: ts.Node): n is Jsx =>
    ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n) || ts.isJsxFragment(n);
  const opening = (n: ts.JsxElement | ts.JsxSelfClosingElement) =>
    ts.isJsxElement(n) ? n.openingElement : n;
  const tagOf = (n: ts.JsxElement | ts.JsxSelfClosingElement) => text(opening(n).tagName);
  const isComponent = (tag: string) => /^[A-Z]/.test(tag) || tag.includes('.');
  const unwrap = (e: ts.Expression): ts.Expression => {
    while (
      ts.isParenthesizedExpression(e) ||
      ts.isAsExpression(e) ||
      ts.isSatisfiesExpression(e) ||
      ts.isNonNullExpression(e)
    )
      e = e.expression;
    return e;
  };
  const isNothing = (e: ts.Expression) =>
    e.kind === ts.SyntaxKind.NullKeyword ||
    e.kind === ts.SyntaxKind.TrueKeyword ||
    e.kind === ts.SyntaxKind.FalseKeyword ||
    (ts.isIdentifier(e) && e.text === 'undefined');

  // ---- does an element hold words ----
  const exprHoldsWords = (e0: ts.Expression | undefined): boolean => {
    if (!e0) return false;
    const e = unwrap(e0);
    if (isJsx(e)) return holdsWords(e);
    if (ts.isConditionalExpression(e))
      return exprHoldsWords(e.whenTrue) || exprHoldsWords(e.whenFalse);
    if (ts.isBinaryExpression(e)) {
      const op = e.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return exprHoldsWords(e.right);
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken)
        return exprHoldsWords(e.left) || exprHoldsWords(e.right);
    }
    if (isNothing(e)) return false;
    // JSX inside a call (`.map(…)`): what that JSX holds; else a value React writes as text
    const jsx: Jsx[] = [];
    const find = (n: ts.Node): void => {
      if (isJsx(n)) jsx.push(n);
      else ts.forEachChild(n, find);
    };
    find(e);
    return jsx.length ? jsx.some(holdsWords) : true;
  };
  const childHoldsWords = (c: ts.JsxChild): boolean =>
    ts.isJsxText(c)
      ? /\S/.test(c.text)
      : ts.isJsxExpression(c)
        ? exprHoldsWords(c.expression)
        : holdsWords(c);
  function holdsWords(n: Jsx): boolean {
    if (ts.isJsxFragment(n)) return n.children.some(childHoldsWords);
    const tag = tagOf(n);
    if (TEXT_TAGS.has(tag)) return true;
    if (UNPAINTED_TAGS.has(tag)) return false;
    if (ts.isJsxSelfClosingElement(n)) return isComponent(tag);
    return n.children.some(childHoldsWords);
  }
  /** An element that carries a paint of its own: a component may draw words, so it counts. */
  const carriesWords = (n: ts.JsxElement | ts.JsxSelfClosingElement) =>
    isComponent(tagOf(n)) || holdsWords(n);

  // ---- the names bound to useScanFill(), and to anything read from one (`const m = scan?.mask`) ----
  const scanNames = new Set<string>();
  const readsScan = (e: ts.Node): boolean =>
    (ts.isIdentifier(e) && scanNames.has(e.text)) || (ts.forEachChild(e, readsScan) ?? false);
  const bind = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer) {
      const init = unwrap(node.initializer);
      if ((ts.isCallExpression(init) && text(init.expression) === 'useScanFill') || readsScan(init))
        scanNames.add(node.name.text);
    }
    ts.forEachChild(node, bind);
  };
  bind(source);

  // ---- the sites, and every class name the sites account for ----
  const fills: Site[] = [];
  const masks: Site[] = [];
  const seen = new Set<number>();
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = tagOf(node);
      for (const attr of opening(node).attributes.properties) {
        // A spread is a mask when it reads the scan or writes a `mask`: held to the same rule.
        if (ts.isJsxSpreadAttribute(attr)) {
          const spread = unwrap(attr.expression);
          const writesMask =
            ts.isObjectLiteralExpression(spread) &&
            spread.properties.some((p) => p.name !== undefined && text(p.name) === 'mask');
          if (readsScan(attr.expression) || writesMask)
            masks.push({
              line: lineOf(attr),
              what: `<${tag}> ${text(attr)}`,
              words: carriesWords(node),
              scan: readsScan(attr.expression),
            });
          continue;
        }
        if (!attr.initializer) continue;
        const name = text(attr.name);
        // The scan handed on under another prop (a component's `fill={scan}`) is its mask too.
        if (name !== 'className' && name !== 'mask' && readsScan(attr.initializer)) {
          masks.push({
            line: lineOf(attr),
            what: `<${tag}> ${text(attr)}`,
            words: carriesWords(node),
            scan: true,
          });
          continue;
        }
        if (name === 'className') {
          const found = [...attr.getText(source).matchAll(CLASS_ALL)];
          if (!found.length) continue;
          for (const m of found) seen.add(attr.getStart(source) + m.index);
          fills.push({
            line: lineOf(attr),
            what: `<${tag}> ${text(attr)}`,
            words: carriesWords(node),
            scan: true,
          });
        }
        if (name === 'mask')
          masks.push({
            line: lineOf(attr),
            what: `<${tag}> ${text(attr)}`,
            words: carriesWords(node),
            scan: readsScan(attr.initializer),
          });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);

  // a class name anywhere else in the code (not in a comment) is one the gate cannot judge
  const stray: number[] = [];
  const scanTokens = (node: ts.Node): void => {
    // a module's path names a file, not a class (`@/components/ui/scan-fill`)
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) return;
    if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) return;
    if (
      ts.isStringLiteralLike(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node) ||
      ts.isJsxText(node)
    ) {
      const start = node.getStart(source);
      for (const m of node.getText(source).matchAll(CLASS_ALL))
        if (!seen.has(start + m.index)) stray.push(lineOf(node));
      // the scan's CSS variable is read by the `.scan-fill` rule alone (cssRulesReading below)
      stray.push(...[...node.getText(source).matchAll(FILL_VAR_ALL)].map(() => lineOf(node)));
    }
    ts.forEachChild(node, scanTokens);
  };
  scanTokens(source);
  return { fills, masks, stray };
}

function sourceFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.tsx?$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(root, dir));
  return files;
}

/** The selectors of the CSS rules (inside an at-rule too) whose declarations read `variable`. */
function cssRulesReading(css: string, variable: string): string[] {
  const code = css.replace(/\/\*[\s\S]*?\*\//g, '');
  return [...code.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter(([, , body]) => body.includes(`var(${variable})`))
    .map(([, selector]) => selector.trim().replace(/\s+/g, ' '));
}

describe('the scan gate finds what it must, and only that', () => {
  const sites = (lines: string[]) => scanSites(lines.join('\n'));

  it('flags the class on an element that holds words, a component, or SVG text', () => {
    const { fills } = sites([
      '<span className="scan-fill h-full" style={{ width }} />;',
      '<div className="bg-jade scan-fill">{label}</div>;',
      '<span className={cn("scan-fill", on && "px-2")}>DAO free <b>{share}</b></span>;',
      '<div className="scan-fill">{parts.map((p) => <span key={p}>{p}</span>)}</div>;',
      '<Bar className="scan-fill" />;',
      '<text className="scan-fill" />;',
      '<div className="scan-fill">{on && <i className="h-1" />}</div>;',
      '<div className="scan-fills scan-fill-x" />;',
    ]);
    expect(fills.map((s) => [s.line, s.words])).toEqual([
      [1, false],
      [2, true],
      [3, true],
      [4, true],
      [5, true],
      [6, true],
      [7, false],
    ]);
  });

  it('flags a mask on words, and tells the scan’s mask from another', () => {
    const { masks } = sites([
      'const scan = useScanFill();',
      'const fade = `url(#${id}-mask)`;',
      '<path d={d} mask={scan?.mask} />;',
      '<g mask={scan?.mask}>{bars.map((b) => <rect key={b} />)}</g>;',
      '<g mask={scan?.mask}><rect /><text>{v}</text></g>;',
      '<g mask={scan?.mask}>{label}</g>;',
      '<path d={d} mask={fade} />;',
      '<g mask={fade}><ClipMark text={t} /></g>;',
    ]);
    expect(masks.map((s) => [s.line, s.words, s.scan])).toEqual([
      [3, false, true],
      [4, false, true],
      [5, true, true],
      [6, true, true],
      [7, false, false],
      [8, true, false],
    ]);
  });

  // Review of main..fix/readability (2026-11-18, test finding 7): the gate read only the `mask`
  // and `className` attributes, so the scan's mask passed by a JSX spread, held in another name,
  // or handed to a component under another prop was not seen, nor the scan's CSS variable named in
  // the source.
  it('reads the scan’s mask through a spread, another name and another prop', () => {
    const { masks } = sites([
      'const scan = useScanFill();',
      'const m = scan?.mask;',
      '<g {...{ mask: scan?.mask }}><text>{v}</text></g>;',
      '<path d={d} {...{ mask: m }} />;',
      '<g mask={m}>{label}</g>;',
      '<Bars fill={scan} />;',
      '<rect width={w} {...rest} />;',
    ]);
    expect(masks.map((s) => [s.line, s.words, s.scan])).toEqual([
      [3, true, true],
      [4, false, true],
      [5, true, true],
      [6, true, true],
    ]);
  });

  it('flags the scan’s CSS variable named anywhere in the source', () => {
    const { stray } = sites([
      "<div style={{ maskImage: 'var(--m-fill-scan)' }}>{label}</div>;",
      "const FADE = 'var(--m-fill-scan)';",
      "const key = materials['fill-scan'];",
    ]);
    expect(stray).toEqual([1, 2]);
  });

  it('flags the class name where the gate cannot see its element: a constant, a helper, a call', () => {
    const { stray } = sites([
      "const FILL = 'scan-fill h-1';",
      '<span className={FILL} />;',
      "const cls = cn('bg-jade', `scan-fill ${x}`);",
      '// a comment may name scan-fill',
      '<span className="scan-fill" />;',
      "import { useScanFill } from '@/components/ui/scan-fill';",
      "const Lazy = lazy(() => import('@/components/ui/scan-fill'));",
      'const id = `scan-fill${n}`;',
    ]);
    expect(stray).toEqual([1, 3, 8]);
  });

  it('reads which CSS rules read the scan’s variable, in an at-rule too, comments aside', () => {
    expect(
      cssRulesReading(
        [
          '/* .x { mask-image: var(--m-fill-scan); } */',
          '.scan-fill {',
          '  mask-image: var(--m-fill-scan);',
          '}',
          '@media (min-width: 640px) {',
          '  .title, h1 { -webkit-mask-image: var(--m-fill-scan); }',
          '  .other { mask-image: none; }',
          '}',
        ].join('\n'),
        '--m-fill-scan'
      )
    ).toEqual(['.scan-fill', '.title, h1']);
  });
});

describe('the scan stands on fills, never on words (R1)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const found = files.map((file) => ({
    path: relative(root, file),
    ...scanSites(readFileSync(file, 'utf8')),
  }));

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(250);
  });

  it('sets the class and every SVG mask on an element that holds no words', () => {
    const wordy = found.flatMap(({ path, fills, masks }) =>
      [...fills, ...masks].filter((s) => s.words).map((s) => `${path}:${s.line} ${s.what}`)
    );
    expect(wordy).toEqual([]);
  });

  it('writes the class only on an element’s className', () => {
    const stray = found.flatMap(({ path, stray }) => stray.map((line) => `${path}:${line}`));
    expect(stray).toEqual([]);
  });

  it('carries the scan where the board put it, and nowhere else', () => {
    const carried: Record<string, { fills: number; masks: number }> = {};
    for (const { path, fills, masks } of found) {
      const scanMasks = masks.filter((s) => s.scan).length;
      if (fills.length || scanMasks) carried[path] = { fills: fills.length, masks: scanMasks };
    }
    expect(carried).toEqual(SCAN_SITES);
  });

  // Review of main..fix/readability (2026-11-18, test finding 7): another rule reading the scan's
  // variable would lay the scan on whatever it selects, unseen by the source gate.
  it('reads the scan’s CSS variable in the .scan-fill rule alone', () => {
    const sheets: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) walk(path);
        else if (name.endsWith('.css')) sheets.push(path);
      }
    };
    for (const dir of SCAN_DIRS) walk(join(root, dir));
    const rules = sheets.flatMap((path) =>
      cssRulesReading(readFileSync(path, 'utf8'), '--m-fill-scan').map(
        (selector) => `${relative(root, path)} ${selector}`
      )
    );
    expect(rules).toEqual(['app/globals.css .scan-fill']);
  });
});
