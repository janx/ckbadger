import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { virtualProgram } from '@/__tests__/utils/virtual-program';

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
 *
 * And where a size is chosen (readability 2026-11-18, R9; `docs/RESPONSIVE_LAYOUT.md`, "Formats"):
 * `head` is for an h1 and a title line only, prose taking the row form; `tight` only below `sm`
 * and in an SVG or tile label under 200 px. A choice is a string literal `'head'` or `'tight'` that
 * the TypeScript checker reads as an id's size: its contextual type holds `'row'` beside it (a
 * renderer's `size`, a helper's `size: ShortSize` parameter or default, a `?:` of sizes, TitleId's
 * `form`). Every choice is listed per file in order with its reason and an anchor that must stand
 * in the site's own source (its line, and the JSX tag it is an attribute of), as the ink gate
 * lists its sites; a `head` reason is a title's. Limits: a literal with no contextual type
 * (`const s = 'tight'` then passed on) is not read; it is the type of the place it lands in that
 * makes it a size.
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

const NUMBER_TAIL =
  'a block number’s last digits as a label inside a drawing whose ruler or column states the leading ones: a number, not an id or a text label';

/** Cuts joined to an ellipsis the gate tolerates. */
const SLICED_ALLOWLIST: Allowlist = {
  // the proposal ruler's tick labels ("…312")
  'components/transaction/proposal-ruler.tsx': [NUMBER_TAIL],
  // hashShort's head and tail
  'lib/ai/markdown-renderer.ts': [AGENT_BUNDLE, AGENT_BUNDLE],
  // the missing and the empty witness groups' code hashes
  // the pipeline's block cards and rows ("…9312")
  'lib/pipeline/layout.ts': [NUMBER_TAIL],
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

/** One choice of a size the gate tolerates: why it stands, and a text on its line or its tag. */
interface Allowed {
  reason: string;
  anchor: string;
}
type AnchoredAllowlist = Readonly<Record<string, readonly Allowed[]>>;

const TITLE =
  'an h1 or a title line: the page’s own name, the one place an id takes the head size (R9)';
const title = (anchor: string): Allowed => ({ reason: TITLE, anchor });

/** `head` chosen: in the title renderers only. */
const HEAD_ALLOWLIST: AnchoredAllowlist = {
  // C13: the page is named by its short address, else its lock hash
  'app/address/[addr]/client-page.tsx': [
    title("shortenAddress(a.address, 'head')"),
    title("shortenHash(a.lockScriptHash, 'head')"),
  ],
  // the h1's measured line: the head form where it holds it whole
  'components/ui/title-id.tsx': [title("form('head')"), title("line >= needed ? 'head'")],
};

const PHONE_ROW =
  'a phone row: the narrow form of its list, drawn where its table does not fit; the row writes its ids at the tight size and the table form is not drawn beside it';
const BELOW_SM =
  'the rule itself: tight below sm, the row form from sm, for a phone row also drawn in a desktop column and a tile under 200 px only on a phone';
const SVG_LABEL =
  'a label in an SVG drawing, or the fitted text of one, measured in characters against the drawing';
const TILE_LABEL = 'a label in a tile under 200 px';
const CHART_LABEL =
  'a chart’s label for a series or a bar, beside its mark: the series names are cut to the chart';
const NARROW_LIST =
  'the narrow form of a figure, drawn where its matrix does not fit: its ids at the tight size, the matrix form not drawn beside it';
const GEL =
  'a gene’s raw bytes in the sequencing gel: one tight form in the wide gel, its gene list and the gene’s name; the gel’s columns are measured in these characters';
const TAB = 'a browser tab’s title: a tab is narrow (C1), so it names an id at the tight size';
const phoneRow = (anchor: string): Allowed => ({ reason: PHONE_ROW, anchor });
const svgLabel = (anchor: string): Allowed => ({ reason: SVG_LABEL, anchor });
const tileLabel = (anchor: string): Allowed => ({ reason: TILE_LABEL, anchor });
const gel = (anchor: string): Allowed => ({ reason: GEL, anchor });

/** `tight` chosen: below sm, or in a label under 200 px; at desktop width only with its arithmetic. */
const TIGHT_ALLOWLIST: AnchoredAllowlist = {
  'app/blocks/page.tsx': [
    {
      reason:
        'the hash beside the block number in the blocks table’s key cell: at the row size (17 characters at 12 px, 122.4 px; tight, 11 characters, 79.2) the Block column needs 273 px and the table 1,236, wider than the 1,232 px container at 1280',
      anchor: 'hash={block.hash}',
    },
    phoneRow("miner(block, 'tight')"),
    phoneRow('hash={block.hash}'),
    {
      reason:
        'the hash under the block number in the 608 px table’s Height column: the row size (17 characters at 12 px, 122.4 px) is wider than the column’s 92.4 px of text, the height at 14 px; the same tight form as the wide table',
      anchor: 'hash={block.hash}',
    },
  ],
  'app/dao/depositors/page.tsx': [phoneRow('<Depositor row={d} size="tight" />')],
  'app/forks/page.tsx': [phoneRow('<Miners event={event} size="tight" />')],
  // the folded peer row below 560 px of container (from it, one line at the row size)
  'app/network/nodes-table.tsx': [phoneRow('value={peer.peerId}')],
  'app/scripts/page.tsx': [phoneRow("shortenHash(s.codeHash, 'tight')")],
  'app/transactions/page.tsx': [phoneRow('hash={tx.hash}')],
  'components/address/address-history.tsx': [
    phoneRow('<TxHash tx={tx} size="tight" />'),
    phoneRow('<CellOutPoint cell={c} size="tight" />'),
    phoneRow('<CellContains cell={c} size="tight" />'),
  ],
  'components/asset/asset-feed.tsx': [phoneRow('hash={r.hash}')],
  // the genotype chart's overlay lines, named on the chart
  'components/asset/collection-objects.tsx': [
    svgLabel("objectName(item.number, item.sporeId, 'tight')"),
  ],
  // the mechanism chain's tiles
  'components/asset/genetics.tsx': [
    tileLabel('value={step.decoderReference.hash}'),
    tileLabel('value={step.decoderBinaryHash}'),
  ],
  'components/asset/inventory-table.tsx': [
    phoneRow('hash={a.id}'),
    phoneRow('hash={a.id}'),
    phoneRow('hash={a.id}'),
  ],
  // the uncles' labels in the 178 px tiles of "Position in the chain"
  'components/blocks/block-position.tsx': [
    svgLabel("shortenHash(uncle.hash, 'tight')"),
    svgLabel("shortenHash(uncle.parentHash, 'tight')"),
    svgLabel("shortenHash(uncle.hash, 'tight')"),
  ],
  'components/blocks/block-transactions-table.tsx': [phoneRow('hash={tx.hash}')],
  // a ranking's name for two series that share a name ("MEMES · 0x5cb3…b4fa", CH-29)
  'components/charts/chart-model.ts': [
    { reason: CHART_LABEL, anchor: "shortenHash(fullSeriesId(entry.id), 'tight')" },
  ],
  // the compact card of the depositors (DAO-22)
  'components/dao/dao-board.tsx': [
    phoneRow("ownerLink(o, 'tight')"),
    phoneRow("lockLink(o, 'tight')"),
  ],
  // an unnamed miner under a compact tile of the fork tree, where the row form (93.6 px at 12 px)
  // does not fit the 88 px column of a 72 px tile (review F-B, logic finding 4)
  'components/forks/fork-tree.tsx': [svgLabel("fits('row') ? 'row' : 'tight'")],
  // the holders' segments on the timeline, each the longest name that fits its segment's px
  'components/scripts/holder-timeline.tsx': [
    svgLabel("holderName(holder.custody, 'tight')"),
    svgLabel("shortenHash(holder.custody.lockArgs!, 'tight')"),
  ],
  'components/scripts/script-family-board.tsx': [
    {
      reason:
        'the history readout under the picture at desktop width: tier 1’s prose budget (160 characters, __tests__/utils/hierarchy.ts) holds it at the tight size and the row size passes it (162 for JoyID); the readout is P2’s and B7’s to rework',
      anchor: "holderName(deployment.custody, 'tight')",
    },
  ],
  'components/scripts/script-family-page.tsx': [
    phoneRow('value={r.referenceHash}'),
    phoneRow("depOf(r, 'tight')"),
    {
      reason:
        'the versions table’s Held by column at desktop width: with the code cell at the row size a version keeps the row’s 90 visible characters (table-rows T6) only at the tight size (86; 92 at the row size)',
      anchor: "holderName(custody, 'tight')",
    },
    phoneRow("signedText(d, 'tight')"),
    phoneRow("outpointLink(first, 'tight')"),
    phoneRow("deploymentStatus(first, 'tight')"),
    phoneRow("outpointLink(d, 'tight')"),
    phoneRow("deploymentStatus(d, 'tight')"),
    phoneRow('value={cell.txHash}'),
    phoneRow('<CellOwner cell={cell} size="tight" />'),
    phoneRow('<AlsoGuardedBy cell={cell} size="tight" />'),
  ],
  'components/scripts/script-newest-cells.tsx': [
    phoneRow("key(row, 'tight')"),
    phoneRow('<Owner cell={row.cell} size="tight" />'),
    phoneRow("whatOf(row, 'tight')"),
    phoneRow('<Created cell={row.cell} detail={row.detail} size="tight" />'),
  ],
  // the three-line card, drawn below 480 px of container: a phone
  'components/transaction/balance-changes.tsx': [phoneRow("form === 'three-line' ? 'tight'")],
  // the narrow list of the script groups: args and the code cell
  'components/transaction/script-group-matrix.tsx': [
    { reason: NARROW_LIST, anchor: "g.sub('tight')" },
    { reason: NARROW_LIST, anchor: "g.code('tight')" },
    { reason: NARROW_LIST, anchor: "g.code('tight')" },
  ],
  'components/transaction/transaction-list-table.tsx': [
    phoneRow("whatNote(tx.activity!, 'tight')"),
    phoneRow('hash={tx.hash}'),
  ],
  'components/ui/cell-tile.tsx': [
    {
      reason:
        'a cell tile’s out point beside its label: the narrowest tile text column is 203 px (the transaction’s sheets at 1024), where "output 12" and a row-size out point (29 characters at 12 px, 208.8 px) do not stand on one line',
      anchor: 'value={cell.outPoint.txHash}',
    },
  ],
  'components/ui/sequencing-gel.tsx': [
    gel("shortenHash(g.raw, 'tight')"),
    gel("shortenHash(gene.raw, 'tight')"),
    gel("shortenHash(g.raw, 'tight')"),
    gel("shortenHash(g.raw, 'tight')"),
    gel('value={g.raw}'),
    gel("shortenHash(g.raw, 'tight')"),
  ],
  'hooks/useTightBelowSm.ts': [{ reason: BELOW_SM, anchor: "? 'row' : 'tight'" }],
  // the home page's script share bars, each named beside its bar
  'lib/activity-stats.ts': [
    { reason: CHART_LABEL, anchor: "shortenHash(entry.codeHash, 'tight')" },
  ],
  'lib/dob-genes.ts': [
    {
      reason:
        'a Bitcoin inscription id in a trait’s text, at the tight size the inscription matcher of this module reads back',
      anchor: "shortenHash(inscription.txid, 'tight')",
    },
  ],
  'lib/use-document-title.ts': [
    { reason: TAB, anchor: "shortenHash(hash, 'tight')" },
    { reason: TAB, anchor: "shortenAddress(address, 'tight')" },
  ],
};

interface SizeSite {
  path: string;
  line: number;
  what: string;
  /** The site's line, and the lines up from it to the JSX tag it is an attribute of. */
  source: string;
}

/** A site's own source: its line, and when it is an attribute on a line of its own, its tag's. */
function sizeSiteSource(lines: readonly string[], line: number): string {
  const at = line - 1;
  let start = at;
  while (start > 0 && at - start < 6 && !lines[start].includes('<')) start--;
  if (!lines[start].includes('<')) start = at;
  return lines.slice(start, at + 1).join('\n');
}

/**
 * The choices of `head` and `tight` in a program's files: every string literal of the two whose
 * contextual type, as the checker reads it, holds `'row'` and itself (an id's size).
 */
function sizeChoices(
  program: ts.Program,
  files: readonly string[],
  rel: (file: string) => string
): { head: SizeSite[]; tight: SizeSite[] } {
  const checker = program.getTypeChecker();
  const out = { head: [] as SizeSite[], tight: [] as SizeSite[] };
  for (const file of files) {
    const source = program.getSourceFile(file);
    if (!source) throw new Error(`the program has no source for ${file}`);
    const lines = source.text.split('\n');
    const visit = (node: ts.Node) => {
      if (
        (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) &&
        (node.text === 'head' || node.text === 'tight') &&
        !ts.isLiteralTypeNode(node.parent)
      ) {
        const type = checker.getContextualType(node);
        const members = type ? (type.isUnion() ? type.types : [type]) : [];
        const sizes = new Set(members.flatMap((m) => (m.isStringLiteral() ? [m.value] : [])));
        if (sizes.has('row') && sizes.has(node.text)) {
          const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
          out[node.text].push({
            path: rel(file),
            line,
            what: lines[line - 1].trim().slice(0, 80),
            source: sizeSiteSource(lines, line),
          });
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return out;
}

/** What the choices break in their allowlist: as the ink gate's, by file, order and anchor. */
function anchoredFailures(sites: readonly SizeSite[], allowlist: AnchoredAllowlist): string[] {
  const byPath = new Map<string, SizeSite[]>();
  for (const site of sites) byPath.set(site.path, [...(byPath.get(site.path) ?? []), site]);
  const out: string[] = [];
  for (const [path, list] of byPath) {
    const entries = allowlist[path] ?? [];
    list.forEach((site, i) => {
      if (i >= entries.length)
        out.push(
          `${path}:${site.line} ${site.what}: no reason for site ${i + 1} of ${list.length} (${entries.length} listed)`
        );
      else if (!site.source.includes(entries[i].anchor))
        out.push(
          `${path}:${site.line} ${site.what}: its source lacks the anchor "${entries[i].anchor}" of its reason (${entries[i].reason})`
        );
    });
  }
  for (const [path, entries] of Object.entries(allowlist)) {
    const list = byPath.get(path) ?? [];
    if (list.length < entries.length)
      out.push(
        `${path}: ${entries.length} reasons for ${list.length} sites (${list.map((s) => `${s.line} ${s.what}`).join(', ') || 'none'}): drop the reason of each site that went`
      );
    for (const { reason, anchor } of entries) {
      if (reason.trim().length <= 10)
        out.push(`${path}: a reason too short to say why: "${reason}"`);
      if (anchor.trim() === '') out.push(`${path}: an empty anchor names no site`);
    }
  }
  return out;
}

describe('the size gate reads a size where the checker does, and only there', () => {
  const program = virtualProgram({
    'lib/shorten.ts': "export type ShortSize = 'head' | 'row' | 'tight';",
    'a.tsx': [
      "import type { ShortSize } from '@/lib/shorten';",
      'declare function shortenHash(v: string, size: ShortSize): string;',
      'function Hash(props: { hash: string; size: ShortSize }) { return null as unknown as never; }',
      "type Tone = 'head' | 'body';",
      "declare function spread(part: 'head' | 'tail'): string;",
      'declare const wide: boolean;',
      "const a = shortenHash('0x', 'tight');",
      'const b = <Hash hash="0x" size="tight" />;',
      "const c = <Hash hash=\"0x\" size={wide ? 'row' : 'tight'} />;",
      "const d: { tone: Tone } = { tone: 'head' };",
      "const e = spread('head');",
      "function f(size: ShortSize = 'head') { return size; }",
      "const g: ShortSize = wide ? 'head' : 'row';",
      "const h = 'tight';",
      "type T = { size: ShortSize | 'tight' };",
    ].join('\n'),
  });
  const found = sizeChoices(program, ['/v/a.tsx'], (file) => file.slice(3));

  it('finds a size passed to a helper and a renderer, in a ?: of sizes, a default and a typed value', () => {
    expect(found.tight.map((s) => s.line)).toEqual([7, 8, 9]);
    expect(found.head.map((s) => s.line)).toEqual([12, 13]);
  });

  it('reads no other union that holds the word (a tone, a part of a form), no bare literal, no type', () => {
    const lines = [...found.head, ...found.tight].map((s) => s.line);
    for (const line of [10, 11, 14, 15]) expect(lines).not.toContain(line);
  });

  it('keys a choice by its file, its order and its anchor', () => {
    const site = (line: number, source: string): SizeSite => ({
      path: 'a.tsx',
      line,
      what: source,
      source,
    });
    const list: AnchoredAllowlist = { 'a.tsx': [phoneRow("miner(block, 'tight')")] };
    expect(anchoredFailures([site(3, "{miner(block, 'tight')}")], list)).toEqual([]);
    expect(anchoredFailures([site(9, "{miner(block, 'tight')}")], list)).toEqual([]);
    expect(anchoredFailures([site(3, "{owner(row, 'tight')}")], list)).toEqual([
      `a.tsx:3 {owner(row, 'tight')}: its source lacks the anchor "miner(block, 'tight')" of its reason (${PHONE_ROW})`,
    ]);
    expect(
      anchoredFailures([site(3, "{miner(block, 'tight')}"), site(5, "{miner(b, 'tight')}")], list)
    ).toEqual([`a.tsx:5 {miner(b, 'tight')}: no reason for site 2 of 2 (1 listed)`]);
    expect(anchoredFailures([], list)).toEqual([
      'a.tsx: 1 reasons for 0 sites (none): drop the reason of each site that went',
    ]);
  });

  it('reads an attribute on a line of its own with its tag', () => {
    const lines = ['<Hash', '  hash={block.hash}', '  size="tight"', '/>'];
    expect(sizeSiteSource(lines, 3)).toBe('<Hash\n  hash={block.hash}\n  size="tight"');
  });
});

describe('head for a title, tight below sm and in a label under 200 px (R9)', () => {
  const root = join(__dirname, '..', '..');
  const rel = (file: string) => relative(root, file).split('\\').join('/');
  const files = sourceFiles(root).filter((file) => relative(root, file) !== OWNER);
  const parsed = ts.getParsedCommandLineOfConfigFile(join(root, 'tsconfig.json'), undefined, {
    ...ts.sys,
    onUnRecoverableConfigFileDiagnostic: (diagnostic) => {
      throw new Error(ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'));
    },
  });
  if (!parsed) throw new Error('tsconfig.json did not parse');
  const program = ts.createProgram({
    rootNames: files,
    options: { ...parsed.options, incremental: false, noEmit: true },
  });
  const found = sizeChoices(program, files, rel);

  it('reads the choices of the source', () => {
    expect(found.tight.length).toBeGreaterThan(20);
    expect(found.head.length).toBeGreaterThan(0);
  });

  it('chooses head only in the title renderers', () => {
    for (const entries of Object.values(HEAD_ALLOWLIST))
      for (const { reason } of entries) expect(reason).toBe(TITLE);
    expect(anchoredFailures(found.head, HEAD_ALLOWLIST)).toEqual([]);
  });

  it('chooses tight only where its reason is listed', () => {
    expect(anchoredFailures(found.tight, TIGHT_ALLOWLIST)).toEqual([]);
  });
});
