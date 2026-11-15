import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { droppedGenesCaption, traitDisplay, type DroppedGene } from '@/lib/dob-genes';
import { TraitText } from '@/components/asset/trait-text';

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
      // up to the element that prints it: the JSX element it is a child of, or the one TraitText
      // stands in when it is TraitText's text; a call in another attribute (a title, an aria label)
      // or one whose result a function body keeps for later (a row's title) prints nothing here
      let up: ts.Node = node;
      let printed = true;
      let host: ts.JsxElement | null = null;
      while (up && !ts.isSourceFile(up)) {
        if (ts.isJsxAttribute(up)) {
          const el = up.parent.parent;
          if (up.name.getText(source) === 'text' && tagName(el, source) === 'TraitText')
            host = ts.isJsxElement(el.parent) ? el.parent : null;
          else printed = false;
          break;
        }
        if (ts.isVariableDeclaration(up) || ts.isBlock(up)) {
          printed = false;
          break;
        }
        if (ts.isJsxExpression(up) && ts.isJsxElement(up.parent)) {
          host = up.parent;
          break;
        }
        up = up.parent;
      }
      if (host && printed) {
        const { className, title } = attrs(host.openingElement, source);
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

/**
 * The lead's second Chrome pass (B3 round 2, 2026-11-15): on the cluster page at 390 and 360 the
 * dd of "How an object is read" set "inscription 020172…cb69i0" as "inscription 020172…" /
 * "cb69i0": after the ellipsis UAX #14 allows a line break. A shortened id inside a trait value is
 * one unit, made at render (NoBreak), with no character added to the text; the value still wraps
 * between its words and third-party text still breaks anywhere.
 */
describe('a shortened id inside a trait value is one unit', () => {
  const units = (container: HTMLElement) =>
    Array.from(container.querySelectorAll('[data-nobreak]')).map((el) => el.textContent);

  it('holds an inscription id together and adds nothing to the text', () => {
    const value = traitDisplay(`btcfs://020172${'89'.repeat(27)}cb69i0`);
    expect(value).toBe('inscription 020172…cb69i0');
    const { container } = render(
      <dd>
        <TraitText text={value} />
      </dd>
    );
    expect(units(container)).toEqual(['020172…cb69i0']);
    expect(container.textContent).toBe(value);
  });

  it('holds every id of a sentence, the words between them free to wrap', () => {
    const one = traitDisplay(`btcfs://a94617${'95'.repeat(27)}79e0i0`);
    const text = `${one} for 3 · inscription 020172…cb69i12 for 2`;
    const { container } = render(
      <p>
        <TraitText text={text} />
      </p>
    );
    expect(units(container)).toEqual(['a94617…79e0i0', '020172…cb69i12']);
    expect(container.textContent).toBe(text);
  });

  it('leaves third-party text and counts as plain text', () => {
    const long = `btcfs://${'x'.repeat(292)}`;
    const mimic = `deadbeef…${'c'.repeat(200)}`;
    for (const text of [long, '5 inscriptions', mimic, 'wait…what', 'GOLD']) {
      const { container, unmount } = render(
        <p>
          <TraitText text={text} />
        </p>
      );
      expect(units(container)).toEqual([]);
      expect(container.textContent).toBe(text);
      unmount();
    }
  });
});

/**
 * Every place that prints trait text (a trait value or a sentence built from one) prints it
 * through TraitText, the one rendering point that makes a shortened id inside it a unit; a bare
 * call printed as a JSX child or in a phone row's lines is refused.
 */
const TRAIT_TEXT =
  /^(traitDisplay|geneCellText|droppedGenesCaption|inscriptionText|foundAcross|ruleWords|valueOf)$/;
function tagName(el: ts.Node, source: ts.SourceFile): string | null {
  return ts.isJsxOpeningElement(el) || ts.isJsxSelfClosingElement(el)
    ? el.tagName.getText(source)
    : null;
}
function bareTraitText(path: string): string[] {
  const code = readFileSync(path, 'utf8');
  const source = ts.createSourceFile(path, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const out: string[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isCallExpression(node) && TRAIT_TEXT.test(node.expression.getText(source))) {
      let up: ts.Node = node.parent;
      let bare = false;
      while (up && !ts.isSourceFile(up)) {
        if (ts.isJsxAttribute(up)) {
          const name = up.name.getText(source);
          const el = up.parent.parent;
          if (name === 'text' && tagName(el, source) === 'TraitText') break;
          // a phone row's lines print their parts; any other attribute (a title) prints nothing
          bare = name === 'lines';
          break;
        }
        if (ts.isVariableDeclaration(up) || ts.isBlock(up) || ts.isFunctionDeclaration(up)) break;
        if (ts.isJsxExpression(up) && (ts.isJsxElement(up.parent) || ts.isJsxFragment(up.parent))) {
          // SVG text never wraps (its labels are cut to their budgets) and a <title> is a tooltip
          const tag = ts.isJsxElement(up.parent) ? tagName(up.parent.openingElement, source) : null;
          bare = !['text', 'tspan', 'title'].includes(tag ?? '');
          break;
        }
        up = up.parent;
      }
      if (bare) {
        const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
        out.push(`${relative(ROOT, path)}:${line} ${node.expression.getText(source)}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

describe('trait text has one rendering point', () => {
  const sources = [...files(join(ROOT, 'components')), ...files(join(ROOT, 'app'))];

  it('prints every trait value and every sentence built from one through TraitText', () => {
    expect(sources.flatMap(bareTraitText)).toEqual([]);
  });
});

/**
 * B3 round 3 (the lead's sweep for plain words cut in the middle): `break-all` breaks inside any
 * word, so it belongs only to an element that prints raw text (a URI, a hex dump). An element that
 * takes it while holding words, its own or a descendant's in the same JSX (literal text of two or
 * more words, or text beside a ShortId / NoBreak unit), is refused: the media box's reading
 * ("inscript / ion i0") and a list item whose break-all reached its notice. Round 4 reads words
 * built in an expression too: a literal or a template's text of two or more words, and a value named
 * for prose (`message`, `apiMessage`, `error`, `mismatch`, `reason`). What it cannot read: words a
 * call returns, a value otherwise named, and words a component from another file prints, which
 * inherit the class all the same.
 */
const BREAK_ALL = /(^|[\s'"`])break-all($|[\s'"`])/;
function breakAllOnWords(path: string): string[] {
  const code = readFileSync(path, 'utf8');
  const source = ts.createSourceFile(path, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  // the same file's JSX consts (`const label = <>…</>`) and components (`function Reading()`),
  // followed where an element prints them
  const consts = new Map<string, ts.Node>();
  const components = new Map<string, ts.Node>();
  const index = (n: ts.Node) => {
    if (ts.isVariableDeclaration(n) && n.initializer && ts.isIdentifier(n.name)) {
      const init = ts.isParenthesizedExpression(n.initializer)
        ? n.initializer.expression
        : n.initializer;
      if (ts.isJsxElement(init) || ts.isJsxFragment(init) || ts.isJsxSelfClosingElement(init))
        consts.set(n.name.text, init);
    }
    if (ts.isFunctionDeclaration(n) && n.name && /^[A-Z]/.test(n.name.text) && n.body)
      components.set(n.name.text, n.body);
    ts.forEachChild(n, index);
  };
  index(source);
  const out: string[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) && BREAK_ALL.test(attrs(node.openingElement, source).className)) {
      let words = false;
      let unit = false;
      let text = false;
      const seen = new Set<ts.Node>();
      const scan = (n: ts.Node) => {
        if (seen.has(n)) return;
        seen.add(n);
        // an attribute (a class, a title) prints nothing; a call's arguments are its own business,
        // but a callback it is given renders (`items.map((x) => <li>…</li>)`)
        if (ts.isJsxAttribute(n)) return;
        if (ts.isCallExpression(n)) {
          scan(n.expression);
          for (const arg of n.arguments)
            if (ts.isArrowFunction(arg) || ts.isFunctionExpression(arg)) scan(arg);
          return;
        }
        if (ts.isJsxText(n)) {
          const t = n.getText(source).trim();
          if (t) text = true;
          if (t.split(/\s+/).filter(Boolean).length >= 2) words = true;
        }
        // B3 round 4: words built in an expression, a literal of two or more words (a fallback, a
        // template's text) or a value named for prose (an error's message, a mismatch, a reason)
        if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) {
          if (n.text.trim().split(/\s+/).filter(Boolean).length >= 2) words = true;
        }
        if (ts.isTemplateHead(n) || ts.isTemplateMiddle(n) || ts.isTemplateTail(n)) {
          if (n.text.trim().split(/\s+/).filter(Boolean).length >= 2) words = true;
        }
        if (
          (ts.isIdentifier(n) || ts.isPropertyAccessExpression(n)) &&
          ts.isJsxExpression(n.parent) &&
          /^(message|apiMessage|error|mismatch|reason)$/i.test(
            ts.isIdentifier(n) ? n.text : n.name.text
          )
        )
          words = true;
        if (ts.isJsxSelfClosingElement(n) || ts.isJsxOpeningElement(n)) {
          const tag = n.tagName.getText(source);
          if (tag === 'ShortId' || tag === 'NoBreak') unit = true;
          const body = components.get(tag);
          if (body) scan(body);
        }
        if (ts.isJsxExpression(n) && n.expression && ts.isIdentifier(n.expression)) {
          const init = consts.get(n.expression.text);
          if (init) scan(init);
        }
        ts.forEachChild(n, scan);
      };
      node.children.forEach(scan);
      if (words || (unit && text)) {
        const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
        out.push(`${relative(ROOT, path)}:${line}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

describe('break-all only on raw text', () => {
  const sources = [...files(join(ROOT, 'components')), ...files(join(ROOT, 'app'))];

  it('finds the class where it stands', () => {
    expect(
      sources.filter((f) => readFileSync(f, 'utf8').includes('break-all')).length
    ).toBeGreaterThan(5);
  });

  // B3 round 4: the seventeen sites reported in round 3 are cleared, and the list that kept them
  // is gone with them; every site anywhere is refused
  it('never puts break-all on an element that holds words', () => {
    expect(sources.flatMap(breakAllOnWords)).toEqual([]);
  });
});
