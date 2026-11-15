import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { droppedGenesCaption, traitDisplay, type DroppedGene } from '@/lib/dob-genes';

/**
 * The lead's Chrome finding (B3, 2026-11-15): the cluster and object pages of the "秦晖" collection
 * overflowed by 1,400–2,300 px. Its Source gene's one option is FIVE btcfs URIs joined by ';'
 * (374 characters, from GET /spore/clusters/<id>/insights); traitDisplay knew one inscription and
 * printed anything else beginning btcfs:// as it is, so the gel's caption carried 374 characters
 * with no break opportunity. A list of inscriptions is said in words, through the count rule; what
 * traitDisplay cannot describe is printed as it is, and every place that prints a trait value lets
 * it break anywhere (or clips it, its whole text in the title): third-party text never overflows.
 */
const REAL_LIST =
  'btcfs://968badc6c87622e5e9412adde79cc431480eb03500e7e7aea656624ccfb8e77ai0;' +
  'btcfs://a0135609448ac38bb6fde6332200215ba3a69df7ca8bf1bae8e5f32fc460cad8i0;' +
  'btcfs://d72ad6527381e527db2e253a8786707e941e9f37c3a1f39291efec961728a159i0;' +
  'btcfs://f76292e56c454ef04c6a186f31db603e9fb3cf6c33b9de36019569a5ec3b89e0i0;' +
  'btcfs://06a79fe027f6645aa74c3860e12bb6ce25fe3e7a324f132715a1ef4842cc2117i0';

describe('a trait value that points at inscriptions', () => {
  it('says a list of inscriptions in words, through the count rule', () => {
    expect(REAL_LIST).toHaveLength(374);
    expect(traitDisplay(REAL_LIST)).toBe('5 inscriptions');
    expect(traitDisplay(REAL_LIST.split(';').slice(0, 2).join(';'))).toBe('2 inscriptions');
  });

  it('keeps one inscription by its id, inside the gel’s 27-character value budget', () => {
    const one = REAL_LIST.split(';')[0];
    expect(traitDisplay(one)).toBe('inscription 968bad…e77ai0');
    expect(traitDisplay(one).length).toBeLessThanOrEqual(27);
  });

  it('prints as it is a list with one element that is not an inscription', () => {
    const broken = `${REAL_LIST.split(';').slice(0, 2).join(';')};btcfs://not-an-inscription`;
    expect(traitDisplay(broken)).toBe(broken);
  });

  it('prints as it is a long btcfs text that is not an inscription', () => {
    const text = `btcfs://${'x'.repeat(292)}`;
    expect(text).toHaveLength(300);
    expect(traitDisplay(text)).toBe(text);
  });
});

describe('the caption built from the real pattern', () => {
  // The collection's three genes with one value for every object: Type, Author, Source.
  const dropped = (source: string): DroppedGene[] => [
    {
      gene: 'prev.type',
      label: 'Type',
      reason: 'single',
      variants: 1,
      value: traitDisplay('image'),
    },
    { gene: 'Author', label: 'Author', reason: 'single', variants: 1, value: traitDisplay('秦晖') },
    { gene: 'Source', label: 'Source', reason: 'single', variants: 1, value: traitDisplay(source) },
  ];

  it('stays a sentence: under 120 characters, no run longer than 15 without a break', () => {
    const caption = droppedGenesCaption(dropped(REAL_LIST), 1000);
    expect(caption).toBe(
      'Type is image, Author is 秦晖 and Source is 5 inscriptions for every object, so none of them gets a lane.'
    );
    expect(caption.length).toBeLessThan(120);
    // the longest run with no space (a no-break space joins a count to its noun, as everywhere)
    expect(Math.max(...caption.split(' ').map((w) => w.length))).toBeLessThanOrEqual(15);
  });
});

/**
 * Every place that prints a trait value (traitDisplay, the gene cell's geneCellText) or the caption
 * built from them (droppedGenesCaption) lets it break anywhere, or clips it with the whole text in
 * its title. Read from the source: the element around the call, as written.
 */
const ROOT = join(__dirname, '..', '..');
const PRINTERS = /^(traitDisplay|geneCellText|droppedGenesCaption)$/;
function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : /\.tsx$/.test(name) ? [path] : [];
  });
}

/** The class text and the title of a JSX element, as written. */
function attrs(el: ts.JsxOpeningLikeElement, source: ts.SourceFile) {
  const out = { className: '', title: false };
  for (const a of el.attributes.properties) {
    if (!ts.isJsxAttribute(a)) continue;
    const name = a.name.getText(source);
    if (name === 'className') out.className = a.initializer?.getText(source) ?? '';
    if (name === 'title') out.title = true;
  }
  return out;
}

function unwrappedPrinters(path: string): string[] {
  const code = readFileSync(path, 'utf8');
  const source = ts.createSourceFile(path, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const out: string[] = [];
  const visit = (node: ts.Node) => {
    if (
      ts.isCallExpression(node) &&
      PRINTERS.test(node.expression.getText(source)) &&
      !/^function /.test(node.parent.getText(source))
    ) {
      // up to the JSX child that prints it; a call in an attribute (a title, an aria label) or one
      // whose result a function body keeps for later (a row's title) prints nothing here
      let up: ts.Node = node;
      let printed = true;
      while (up && !(ts.isJsxExpression(up) && ts.isJsxElement(up.parent))) {
        if (ts.isJsxAttribute(up) || ts.isVariableDeclaration(up) || ts.isBlock(up))
          printed = false;
        up = up.parent;
      }
      if (up) up = up.parent;
      if (up && printed) {
        const { className, title } = attrs((up as ts.JsxElement).openingElement, source);
        const wraps = className.includes('[overflow-wrap:anywhere]');
        const clips = /\btruncate\b/.test(className) && title;
        if (!wraps && !clips) {
          const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
          out.push(`${relative(ROOT, path)}:${line} ${node.expression.getText(source)}`);
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

describe('third-party text never overflows', () => {
  const sources = [...files(join(ROOT, 'components')), ...files(join(ROOT, 'app'))];

  it('reads the pages that print trait values', () => {
    const printing = sources.filter((f) =>
      /traitDisplay\(|droppedGenesCaption\(/.test(readFileSync(f, 'utf8'))
    );
    expect(printing.length).toBeGreaterThanOrEqual(3);
  });

  it('prints every trait value where it breaks anywhere, or clips it with its title', () => {
    expect(sources.flatMap(unwrappedPrinters)).toEqual([]);
  });
});
