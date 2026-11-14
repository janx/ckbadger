import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the one shortening rule (UI review 2026-11-14, U-09, PB-19): an id is shortened by
 * `lib/shorten.ts` and nowhere else, at one of three sizes, with the one ellipsis character.
 *
 * In `app/`, `components/`, `hooks/` and `lib/` (but `lib/shorten.ts`, which owns the rule):
 *
 * - no string, template or JSX text holds three dots (`...`): the ellipsis is `…`. Spread syntax,
 *   rest parameters, comments and regular expressions are not text and are not read;
 * - no `.slice(` / `.substring(` / `.substr(` result is joined to an ellipsis — in a template
 *   (`${id.slice(0, 10)}…`), a `+` (`id.slice(0, 10) + '…'`) or JSX text beside a JSX expression
 *   (`{id.slice(0, 12)}…{id.slice(-7)}`): a cut by hand is a size of its own;
 * - no id renderer is passed a length (`startChars`, `endChars` …): a renderer takes a size.
 *
 * The sites the gate tolerates are listed per file, in the ORDER they occur in it (as the ink
 * gate does): the n-th site of a kind in a file has the n-th reason, so a moved line passes and an
 * added or a removed site fails, the message naming the path, the line and the text.
 */

/** Per file, in order: the reasons of its sites of one kind. */
type Allowlist = Readonly<Record<string, readonly string[]>>;

const AGENT_BUNDLE =
  'the agent bundle’s markdown and raw text for LLMs (lib/ai), out of this lane: reported, not changed';

/** Three dots in text the gate tolerates. */
const DOTS_ALLOWLIST: Allowlist = {
  // hashShort's cut
  'lib/ai/markdown-renderer.ts': [AGENT_BUNDLE],
  // the witness insights read only by lib/ai: a byte preview's and an ASCII preview's cut, the
  // missing and the empty witness groups' code hashes
  'lib/witness-analysis.ts': [AGENT_BUNDLE, AGENT_BUNDLE, AGENT_BUNDLE, AGENT_BUNDLE],
};

/** Cuts joined to an ellipsis the gate tolerates. */
const SLICED_ALLOWLIST: Allowlist = {
  // hashShort's head and tail
  'lib/ai/markdown-renderer.ts': [AGENT_BUNDLE, AGENT_BUNDLE],
  // the missing and the empty witness groups' code hashes
  'lib/witness-analysis.ts': [AGENT_BUNDLE, AGENT_BUNDLE],
};

/** Lengths passed to a renderer the gate tolerates: none. */
const LENGTHS_ALLOWLIST: Allowlist = {};

/** The id renderers: each takes a size, never a length. */
const RENDERERS = new Set(['HexCode', 'Hash', 'Address', 'HexDisplay', 'ShortId']);
/** A prop that would be a length by hand. */
const LENGTH_PROP = /^(start|end|head|tail|max|min)?(Chars|Length|Len)$|^(start|end|head|tail)$/i;
const CUTS = new Set(['slice', 'substring', 'substr']);
const ELLIPSES = ['…', '...'];

interface Site {
  path: string;
  line: number;
  what: string;
}

interface Offence {
  line: number;
  what: string;
}

/**
 * What the sites of one kind break in its allowlist: a site with no reason (a file not listed, or
 * one more site than reasons), a file with fewer sites than reasons, a reason too short to say why.
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

const startsWithEllipsis = (text: string) => ELLIPSES.some((e) => text.trimStart().startsWith(e));
const endsWithEllipsis = (text: string) => ELLIPSES.some((e) => text.trimEnd().endsWith(e));

/** Whether an expression holds a `.slice(` / `.substring(` / `.substr(` call. */
function holdsCut(node: ts.Node): boolean {
  if (
    ts.isCallExpression(node) &&
    ts.isPropertyAccessExpression(node.expression) &&
    CUTS.has(node.expression.name.text)
  )
    return true;
  return ts.forEachChild(node, holdsCut) ?? false;
}

/** A string literal's text, when the node is one. */
function literalText(node: ts.Node): string | null {
  while (ts.isParenthesizedExpression(node)) node = node.expression;
  return ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) ? node.text : null;
}

/** The three-dot texts, the cuts joined to an ellipsis and the lengths passed to a renderer. */
function shortenOffences(
  path: string,
  code: string
): { dots: Offence[]; sliced: Offence[]; lengths: Offence[] } {
  const source = ts.createSourceFile(
    path,
    code,
    ts.ScriptTarget.Latest,
    true,
    path.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const lineOf = (node: ts.Node) =>
    source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
  const snippet = (node: ts.Node) => node.getText(source).replace(/\s+/g, ' ').slice(0, 60);
  const dots: Offence[] = [];
  const sliced: Offence[] = [];
  const lengths: Offence[] = [];

  const visit = (node: ts.Node) => {
    if (
      (ts.isStringLiteral(node) ||
        ts.isNoSubstitutionTemplateLiteral(node) ||
        ts.isTemplateHead(node) ||
        ts.isTemplateMiddle(node) ||
        ts.isTemplateTail(node) ||
        ts.isJsxText(node)) &&
      node.text.includes('...')
    )
      dots.push({ line: lineOf(node), what: snippet(node) });

    if (ts.isTemplateExpression(node)) {
      node.templateSpans.forEach((span, i) => {
        const before = i === 0 ? node.head.text : node.templateSpans[i - 1].literal.text;
        const after = span.literal.text;
        if (holdsCut(span.expression) && (endsWithEllipsis(before) || startsWithEllipsis(after)))
          sliced.push({ line: lineOf(span.expression), what: snippet(node) });
      });
    }

    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.PlusToken) {
      const left = literalText(node.right) !== null ? node.left : null;
      const right = literalText(node.left) !== null ? node.right : null;
      if (
        (left && holdsCut(left) && startsWithEllipsis(literalText(node.right)!)) ||
        (right && holdsCut(right) && endsWithEllipsis(literalText(node.left)!))
      )
        sliced.push({ line: lineOf(node), what: snippet(node) });
    }

    if (ts.isJsxElement(node) || ts.isJsxFragment(node)) {
      const children = node.children;
      children.forEach((child, i) => {
        if (!ts.isJsxExpression(child) || !child.expression || !holdsCut(child.expression)) return;
        const before = children[i - 1];
        const after = children[i + 1];
        if (
          (before && ts.isJsxText(before) && endsWithEllipsis(before.text)) ||
          (after && ts.isJsxText(after) && startsWithEllipsis(after.text))
        )
          sliced.push({ line: lineOf(child), what: snippet(child) });
      });
    }

    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = node.tagName.getText(source);
      if (RENDERERS.has(tag))
        for (const attribute of node.attributes.properties)
          if (ts.isJsxAttribute(attribute) && LENGTH_PROP.test(attribute.name.getText(source)))
            lengths.push({
              line: lineOf(attribute),
              what: `<${tag} ${attribute.name.getText(source)}>`,
            });
    }

    ts.forEachChild(node, visit);
  };
  visit(source);
  return { dots, sliced, lengths };
}

const SCAN_DIRS = ['app', 'components', 'hooks', 'lib'];
/** The module that owns the rule: the one place an id is cut and the ellipsis is written. */
const OWNER = 'lib/shorten.ts';

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

describe('the shortening gate finds what it must, and only that', () => {
  it('flags three dots in a string, a template and JSX text', () => {
    const code = [
      "const a = 'Loading...';",
      'const b = `${hash}...${tail}`;',
      'const c = <span>waiting for the node... the inputs</span>;',
      "const d = x.replace('...', '…');",
    ].join('\n');
    expect(shortenOffences('a.tsx', code).dots.map((o) => o.line)).toEqual([1, 2, 3, 4]);
  });

  it('reads no spread, rest, comment or regular expression as text', () => {
    const code = [
      '// a comment... with dots',
      '/* a block... comment */',
      'const a = [...list, ...more];',
      'const b = { ...props };',
      'const c = <Hash {...rest} size="row" />;',
      'function f(...args: string[]) { return args; }',
      'const r = /a...b/;',
      "const ok = 'Loading…';",
    ].join('\n');
    expect(shortenOffences('a.tsx', code).dots).toEqual([]);
  });

  it('flags a cut joined to an ellipsis in a template, a sum and JSX', () => {
    const code = [
      'const a = `${id.slice(0, 10)}…${id.slice(-6)}`;',
      'const b = `…${(party.address ?? party.lockHash).slice(-6)}`;',
      'const c = `version ${hash.substring(0, 10)}…`;',
      "const d = id.slice(0, 10) + '…';",
      "const e = '…' + id.substr(-6);",
      'const f = <span>{id.slice(0, 12)}…{id.slice(-7)}</span>;',
      'const g = <span>Code cell {tx.slice(0, 10)}…:{index}</span>;',
      'const h = `${group.codeHash.slice(0, 10)}... -> witness#${i}`;',
    ].join('\n');
    expect(shortenOffences('a.tsx', code).sliced.map((o) => o.line)).toEqual([
      1, 1, 2, 3, 4, 5, 6, 6, 7, 8,
    ]);
  });

  it('never flags a cut with no ellipsis beside it, nor an ellipsis with no cut', () => {
    const code = [
      'const a = `${id.slice(0, 10)}:${index}`;',
      "const b = `${shortenHash(id, 'row')}…`;",
      'const c = <span>{list.slice(0, 3).length} more</span>;',
      "const d = 'Loading…' + count;",
      'const e = <span>{id.slice(2)} · …</span>;',
    ].join('\n');
    expect(shortenOffences('a.tsx', code).sliced).toEqual([]);
  });

  it('flags a length passed to an id renderer, never a size or another component', () => {
    const code = [
      '<Hash hash={h} startChars={6} endChars={4} />',
      '<HexCode value={v} size="tight" headChars={8} />',
      '<Address address={a} size="row" />',
      '<Bar startChars={6} />',
    ].join('\n');
    expect(shortenOffences('a.tsx', code).lengths).toEqual([
      { line: 1, what: '<Hash startChars>' },
      { line: 1, what: '<Hash endChars>' },
      { line: 2, what: '<HexCode headChars>' },
    ]);
  });
});

describe('the allowlists key a site by its file and its order, not its line', () => {
  const reason = 'the agent bundle’s text for LLMs, out of this lane';
  const list: Allowlist = { 'lib/a.ts': [reason] };
  const site = (line: number): Site => ({ path: 'lib/a.ts', line, what: "'...'" });

  it('passes the same site moved, fails an added and a removed one', () => {
    expect(allowlistFailures([site(10)], list)).toEqual([]);
    expect(allowlistFailures([site(40)], list)).toEqual([]);
    expect(allowlistFailures([site(10), site(20)], list)).toEqual([
      "lib/a.ts:20 '...': no reason for site 2 of 2 (1 listed)",
    ]);
    expect(allowlistFailures([], list)).toEqual([
      'lib/a.ts: 1 reasons for 0 sites (none): drop the reason of each site that went',
    ]);
  });
});

describe('one shortening rule (U-09, PB-19)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root).filter((file) => relative(root, file) !== OWNER);
  const found = { dots: [] as Site[], sliced: [] as Site[], lengths: [] as Site[] };
  for (const file of files) {
    const path = relative(root, file);
    const offences = shortenOffences(path, readFileSync(file, 'utf8'));
    for (const kind of ['dots', 'sliced', 'lengths'] as const)
      for (const o of offences[kind]) found[kind].push({ path, ...o });
  }

  it('scans the source and owns the rule in one module', () => {
    expect(files.length).toBeGreaterThan(300);
    expect(readFileSync(join(root, OWNER), 'utf8')).toContain("ELLIPSIS = '…'");
  });

  const rules = [
    ['dots', DOTS_ALLOWLIST, 'three dots in a string, a template or JSX text'],
    ['sliced', SLICED_ALLOWLIST, 'cut by hand joined to an ellipsis'],
    ['lengths', LENGTHS_ALLOWLIST, 'length passed to an id renderer'],
  ] as const;
  for (const [kind, allowlist, what] of rules)
    it(`writes no ${what} but the ${kind} allowlist's, each with its reason`, () => {
      expect(allowlistFailures(found[kind], allowlist)).toEqual([]);
    });
});
