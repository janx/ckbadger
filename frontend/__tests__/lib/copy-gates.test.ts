import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * U-15 (UI review 2026-11-14): one wording for the same thing.
 *
 * - The ellipsis of a loading or pending string is "…": six still wrote "..." ("Loading graph...",
 *   "pending..."). A field's placeholder is a hint in the field, not page text, and is not read.
 * - An empty state is a sentence that names the thing and ends with a period: "No transactions."
 *   or "No transactions in this block." (was "No transactions" / "No transactions yet" / "No txns
 *   in this block" / "No data available" …). A "No …" that is a label, a value in a field that
 *   names its own subject ("Witness: No witness"), is not a sentence and is listed below.
 */
const root = join(__dirname, '..', '..');
const files: string[] = [];
const walk = (dir: string) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else if (/\.(tsx|ts)$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
  }
};
for (const dir of ['app', 'components']) walk(join(root, dir));

/** Attributes whose values are never read as page text, or are a field's placeholder. */
const SKIPPED_ATTR =
  /^(className|key|id|href|src|role|type|rel|target|style|testId|data-[\w-]+|\w+ClassName|viewBox|d|fill|stroke|transform|points|textAnchor|fontSize|paintOrder|placeholder)$/;

interface Text {
  file: string;
  line: number;
  /** The literal as written; a template's parts as they stand between its substitutions. */
  text: string;
  /** A template head or middle: more text follows its substitution. */
  continues: boolean;
}

/**
 * Text with more of its sentence after it: JSX text, or a string chosen in a JSX expression
 * (`{a ? 'No records are set' : …}`), followed by an expression or an element.
 */
function jsxContinues(node: ts.Node): boolean {
  let child: ts.Node = node;
  if (!ts.isJsxText(node)) {
    // up through the choice that picks the string, to the JSX expression that holds it
    let up = node.parent;
    while (ts.isConditionalExpression(up) || ts.isParenthesizedExpression(up)) up = up.parent;
    if (!ts.isJsxExpression(up)) return false;
    child = up;
  }
  const parent = child.parent;
  if (!ts.isJsxElement(parent) && !ts.isJsxFragment(parent)) return false;
  const siblings: readonly ts.Node[] = parent.children;
  const at = siblings.indexOf(child);
  return siblings
    .slice(at + 1)
    .some(
      (next) =>
        !(ts.isJsxText(next) && next.text.trim() === '') &&
        !(ts.isJsxExpression(next) && next.expression === undefined)
    );
}

function texts(file: string): Text[] {
  const code = readFileSync(file, 'utf8');
  const source = ts.createSourceFile(
    file,
    code,
    ts.ScriptTarget.Latest,
    true,
    file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const out: Text[] = [];
  const name = relative(root, file);
  const visit = (node: ts.Node) => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node) || ts.isLiteralTypeNode(node))
      return;
    // a thrown error is a developer's message, not page text
    if (ts.isNewExpression(node) && node.expression.getText(source) === 'Error') return;
    if (ts.isJsxAttribute(node) && SKIPPED_ATTR.test(node.name.getText(source))) return;
    if (
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node) ||
      ts.isJsxText(node)
    ) {
      const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
      out.push({
        file: name,
        line,
        text: node.text.replace(/\s+/g, ' ').trim(),
        continues:
          ts.isTemplateHead(node) ||
          ts.isTemplateMiddle(node) ||
          ((ts.isJsxText(node) || ts.isStringLiteral(node)) && jsxContinues(node)),
      });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

const all = files.flatMap(texts);

/** Nouns counted on screen; a count written before one by hand takes no plural rule. */
const COUNTED =
  'cells|transactions|deposits|withdrawal requests|binaries|transfers|epochs|sources|holders|blocks|peers|versions|objects|items|addresses|txs|inputs|outputs|records|members|channels|uncles|proposals|scripts|tokens|actions|days|nodes|participants|calls|classes|clusters|identities|bytes|deployments|owners|mints|burns|rounds|observations|keys|genes|variants|renewals|entries|labels|fields|rows|weeks|years|groups|events|parties|steps|decimals';

/**
 * Where a count is written straight before its noun: `{n} cells` in JSX, `${n} cells` in a
 * template — in page text and in what a reader hears or hovers (an accessible name, a title, an
 * alt); thrown errors aside.
 */
function handCounts(file: string): Text[] {
  const code = readFileSync(file, 'utf8');
  const source = ts.createSourceFile(
    file,
    code,
    ts.ScriptTarget.Latest,
    true,
    file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const noun = new RegExp(`^\\s(${COUNTED})\\b`);
  const out: Text[] = [];
  const name = relative(root, file);
  const at = (node: ts.Node, text: string) =>
    out.push({
      file: name,
      line: source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1,
      text,
      continues: false,
    });
  const visit = (node: ts.Node) => {
    if (ts.isImportDeclaration(node) || ts.isLiteralTypeNode(node)) return;
    if (ts.isNewExpression(node) && node.expression.getText(source) === 'Error') return;
    if (
      ts.isJsxAttribute(node) &&
      /^(className|key|href|data-[\w-]+)$/.test(node.name.getText(source))
    )
      return;
    if (ts.isTemplateSpan(node) && noun.test(node.literal.text))
      at(node, `\${${node.expression.getText(source)}}${node.literal.text.slice(0, 24)}`);
    if (ts.isJsxExpression(node) && node.expression && !ts.isJsxAttribute(node.parent)) {
      const siblings: readonly ts.Node[] =
        ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent) ? node.parent.children : [];
      const next = siblings[siblings.indexOf(node) + 1];
      if (next && ts.isJsxText(next) && noun.test(next.text.replace(/^\n\s*/, ' ')))
        at(node, `{${node.expression.getText(source)}}${next.text.slice(0, 24)}`);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}
const where = ({ file, line, text }: Text) => `${file}:${line} ${text}`;

describe('copy gates (U-15)', () => {
  it('reads the source it guards', () => {
    expect(all.length).toBeGreaterThan(1000);
  });

  it('ends a loading or pending string with "…", never "..."', () => {
    const offenders = all
      // a word, then three dots at the end of the string: "Loading graph...", "pending..."
      .filter(({ text }) => /[A-Za-z]\.\.\.$/.test(text))
      .map(where);
    expect(offenders).toEqual([]);
  });

  /**
   * "No …" strings that are labels, not sentences: each is a value in a field or a column that
   * names its own subject, or a chart's series label. Everything else that begins "No " ends
   * with a period.
   */
  const LABELS = new Set<string>([
    // a script group's witness column ("Witness #0 · 85 bytes" / "No witness")
    'No witness',
    // a cell's type field
    'No type script',
    // an ELF section field
    'No .comment section',
    // a block's miner message field
    'No readable text; see the raw bytes below',
    // the forks table's rollback column
    'No rollback at detection',
    // a dial failure's name in the network filter
    'No authenticated session before deadline',
    // a day's row title in the network shares (what the row holds)
    'No retained round observation',
    'No retained labels',
    // a script page's title when it names no deployment
    'No script deployments',
    // the deployments table's columns
    'No deployment observed',
    'No observed outpoint',
    // the pool's fee-rate figure
    'No fee rates',
    // a script group's code column
    'No matching dep',
    'No matching cell dep in transaction evidence',
    'No current code cell resolved',
    'No live code cell for this reference',
    'No live code cell for the current version',
    // an object's or item's owner field, one form (was "No live cell" on the m-NFT page)
    'No live owner',
    // an m-NFT's CKB field
    'No live cell',
    // a cluster's or an item's cell field
    'No current cluster cell',
    'No resulting item cell',
    // the placeholder figure's accessible name
    'No picture shown here',
    // a history readout's item for a day of an incomplete week
    'No completed weekly total for this day',
  ]);

  it('writes every empty state that begins "No " as a sentence ending with a period', () => {
    const offenders = all
      .filter(({ text, continues }) => /^No /.test(text) && !continues)
      .filter(({ text }) => !text.endsWith('.') && !LABELS.has(text))
      .map(where);
    expect(offenders).toEqual([]);
  });

  /**
   * Counts whose words are not the noun's count ("${family.name} cells" names a script's cells);
   * every other count goes through formatCount, so "1 deposit", never "1 deposits".
   */
  const NOT_COUNTS = new Set<string>([
    // a name before its noun, not a count: "Open channels", "CKB in JoyID cells", "mainnet nodes"
    '${chosen.label} channels',
    '${chosen.label.toLowerCase()} channels observed.',
    '{network} nodes',
    '${label} objects',
    '${definition.slug} holders at ',
    '${family.name} cells · occupied and fr',
    '${family.name} cells',
    '{family.name} cells name their code b',
    '{family.name} cells point at their co',
    '${key} bytes',
    // a count in words, its noun already chosen by it ("two genes vary", "four steps")
    '${word} genes vary',
    '${STEP_WORDS[steps.length]} steps',
    '${listJoin(coins)} addresses.',
    '${clipped} weeks cut',
    "{clipped.length === 1 ? 's' : ''} bytes past its ",
    // a sum written as its parts ("12 + 40 bytes"), a ratio ("1 / 3 blocks"), a decimal amount
    '${fmt(dataBytes.length)} bytes',
    '${denominator} blocks',
    '${formatScaledInteger(years, per, 2)} years',
    "${formatChartExact(leader.value, 'blocks')} blocks · ",
    // a developer's message for a missing witness, and a row's hover title
    '${witness.index} bytes',
    '${countText(script.cellsCount)} cells ever',
  ]);

  it('writes every count with its noun through the one plural rule (formatCount)', () => {
    const offenders = files
      .flatMap(handCounts)
      .map(where)
      .filter((line) => !NOT_COUNTS.has(line.replace(/^[^ ]+ /, '')));
    expect(offenders).toEqual([]);
  });

  it('says one thing one way: no "txns", no "No data available", no "… found"', () => {
    const offenders = all
      .filter(({ text }) => /^No /.test(text))
      .filter(({ text }) => /\btxns\b|^No data available|^No \w+( \w+)? found\b/.test(text))
      .map(where);
    expect(offenders).toEqual([]);
  });
});
