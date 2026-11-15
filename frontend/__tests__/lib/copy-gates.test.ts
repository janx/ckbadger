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
   * every other count goes through formatCount, so "1 deposit", never "1 deposits". Each entry is
   * a site: its file and its expression with the text after it, so the same words in another file
   * are read again (the review's item C: an entry keyed by its text alone let "1 blocks" through
   * as "a decimal amount").
   */
  const NOT_COUNTS: readonly { file: string; text: string; why: string }[] = [
    // a name before its noun, not a count
    {
      file: 'app/fiber/channels/client-page.tsx',
      text: '${chosen.label} channels',
      why: 'the table is named by its state: "Open channels"',
    },
    {
      file: 'app/fiber/channels/client-page.tsx',
      text: '${chosen.label.toLowerCase()} channels observed.',
      why: 'the empty state names the state: "No open channels observed."',
    },
    {
      file: 'app/network/client-page.tsx',
      text: '{network} nodes',
      why: 'the network by name: "CKB mainnet nodes"',
    },
    {
      file: 'components/address/address-holdings.tsx',
      text: '${label} objects',
      why: 'the holdings label names the class: "Spore objects"',
    },
    {
      file: 'components/address/address-holdings.tsx',
      text: '${label} objects held here',
      why: 'an accessible name, the class by name',
    },
    {
      file: 'components/charts/chart-model.ts',
      text: '${definition.slug} holders at ',
      why: 'a reading context, the chart by its slug',
    },
    {
      file: 'components/scripts/script-family-board.tsx',
      text: '${family.name} cells',
      why: 'an axis named by its script: "CKB in JoyID cells"',
    },
    {
      file: 'components/scripts/script-family-board.tsx',
      text: '{family.name} cells name their code b',
      why: 'the script by name: "JoyID cells name their code by its data hash"',
    },
    {
      file: 'components/scripts/script-family-board.tsx',
      text: '{family.name} cells point at their co',
      why: 'the script by name: "JoyID cells point at their code by its type"',
    },
    {
      file: 'components/ui/cell-bar.tsx',
      text: '${key} bytes',
      why: 'a reading context, the part by its key: "capacity bytes"',
    },
    {
      file: 'components/ui/allele-gel.tsx',
      text: '${lane.gene} variants',
      why: 'a list named by its gene: "Color variants"',
    },
    {
      file: 'app/tx/[hash]/client-page.tsx',
      text: '${witness.index} bytes',
      why: 'a reading context, the witness by its index: "witness 3 bytes" (the witness\'s bytes)',
    },
    // a count in words with its verb, the noun chosen by the count ("one gene varies")
    {
      file: 'components/asset/collection-objects.tsx',
      text: '${word} genes vary',
      why: 'the plural branch of n === 1 ? "gene varies" : "genes vary"; formatCount writes no words',
    },
    // the plural branch of a choice that never reaches one
    {
      file: 'components/identity/identity-overview.tsx',
      text: '${listJoin(coins)} addresses.',
      why: 'two or more coins: none and one have their own sentences',
    },
    // a verb's ending, not a count: "Color declares bytes past its 53 bytes"
    {
      file: 'components/asset/genetics.tsx',
      text: "{clipped.length === 1 ? 's' : ''} bytes past its ",
      why: 'the "s" ends the verb "declare"; the bytes are not counted by it',
    },
    // a sum written as its parts, a ratio, a decimal amount, a range's far end
    {
      file: 'components/cell/cell-microscope.tsx',
      text: '${fmt(dataBytes.length)} bytes',
      why: 'a sum written as its parts, "61 + 0 bytes": the noun counts the sum',
    },
    {
      file: 'components/charts/chart-model.ts',
      text: '${denominator} blocks',
      why: 'a ratio, "1 / 40 blocks"',
    },
    {
      file: 'components/charts/chart-model.ts',
      text: '${formatScaledInteger(years, per, 2)} years',
      why: 'a decimal amount, "1.50 years" (a whole number of years goes through formatCount)',
    },
    {
      file: 'components/transaction/proposal-ruler.tsx',
      text: '${window.far} blocks',
      why: 'a range\'s far end, "the commit window is 2 to 10 blocks", never one block',
    },
  ];
  const isNotACount = (t: Pick<Text, 'file' | 'text'>) =>
    NOT_COUNTS.some((entry) => entry.file === t.file && entry.text === t.text);

  it('keys an exception by its site: the same words in another file are read again', () => {
    expect(
      isNotACount({ file: 'components/charts/chart-model.ts', text: '${denominator} blocks' })
    ).toBe(true);
    expect(
      isNotACount({ file: 'components/dao/dao-board.tsx', text: '${denominator} blocks' })
    ).toBe(false);
    // every entry still names a site the scan finds
    const found = new Set(files.flatMap(handCounts).map((t) => `${t.file} ${t.text}`));
    expect(NOT_COUNTS.map((e) => `${e.file} ${e.text}`).filter((k) => !found.has(k))).toEqual([]);
  });

  it('writes every count with its noun through the one plural rule (formatCount)', () => {
    const offenders = files
      .flatMap(handCounts)
      .filter((t) => !isNotACount(t))
      .map(where);
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
