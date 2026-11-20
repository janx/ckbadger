import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { LINK_LOOKS, linkLookClass, type LinkLook } from '@/components/ui/link';
import { TYPE_STEPS } from '@/lib/type-scale';

/**
 * Gate for the ink of a word's role (readability plan 2026-11-18, ruling R3; the colour
 * guidelines' "Text hierarchy"): a value, a link, a date, a number and a sentence that explains
 * read in the reading ink; the quiet ink (`text-text-dim`) is for a label, a repeated unit, a
 * separator, a note and a set-back row. Four kinds of element can be told from the source alone,
 * and none of them carries the quiet ink outside a `quiet` subtree (a set-back row):
 *
 * - `link`: a link renderer — `<a>`, `<Address>` or a component named `…Link` (`Link`, `AppLink`,
 *   `BlockLink`, `MinerLink`, …);
 * - `amount`: a `CkbAmount` or a `TokenAmount` that takes its parent's ink (`tone="inherit"`;
 *   a `TokenAmount`'s default);
 * - `td`: a table cell — a `<td>`, the `className` of a `ListTable` column (an object with
 *   `render` and `minWidth`), or the root element its `render` returns;
 * - `prose`: a paragraph, `<p>`;
 * - `line`: a loading or an empty-state line drawn in another element (a `div`, a `span`, …), by
 *   its own words (`LOADING_OR_EMPTY`: they end in the ellipsis, or open with "No"): a sentence
 *   wherever it is drawn (BRIEF-roles-rest part C; the round's ruling B27).
 *
 * An element carries the quiet ink by its own class (`text-text-dim` at rest: with no variant, or a
 * responsive one; `hover:text-text-dim` is a pointer's state; an arbitrary value naming the quiet
 * ink's variable, `text-[color:var(--c-text-dim)]`), by its own `style` colour (`color('text-dim')`
 * or a string naming that variable; `alpha(…)` and `fade(…)` are other inks) or, when it sets no
 * ink of its own, by the nearest ink its JSX ancestors in the same file set around it (a link in a
 * dim note). A class or a colour is read through `?:`, `&&`, `cn(…)`, template literals and the
 * file's own constants, so one that is quiet in one branch counts. A link's `look` is read as its
 * class (`linkLookClass`: the key and onward looks set an ink, inline and block keep the line's).
 * An element handed to a component as a prop (in the attribute, in an array or an object literal
 * there, or returned by a function there) takes the ink a component pinned in PROP_INKS gives that
 * prop, else the ink around the component: its own class, then its ancestors' (BRIEF-roles-rest
 * part D: the walk stopped at the attribute, and `Dotted parts={[<Link …/>]}` inside a quiet span
 * passed). A `quiet` class on the element or an ancestor exempts it (the set-back row draws every
 * word quiet on purpose), and every element that carries that class is pinned by file in
 * QUIET_SITES, so an exemption is a decision too.
 *
 * The sites tolerated today are listed per file, with their count per kind and a reason: this is
 * the work list of the page lanes (P1 chain, P2 assets, P3 lists, P4 topics), B7 (prose) and the
 * chart lanes. A new site fails (its file's count is exceeded, or the file is not listed) and so
 * does a site that went (its count must be lowered), so the list only shrinks. A paragraph that is
 * a note under a figure is quiet by rule: when a lane decides that a `<p>` is such a note, it keeps
 * its count here with the reason saying so, or draws the note as something other than a `<p>`.
 *
 * Limits, the routes a source gate cannot see: a class or a colour imported from another module
 * (only the file's own constants are read; `BOARD_INKS`, `inkStyle(…)` and a `--ink` set through
 * them are not followed); a component that inks its children or a prop and is not in PROP_INKS
 * (the ancestor walk sees only the elements of the file that carry a `className` or a `style`; a
 * component of another file, or one of this file that renders the link inside its own body, as
 * the home pipeline's `PipelineTip` in PromptHeading's args, is the Chrome measurement's to check);
 * ink set in CSS (`.prose-tier2 p`, `.comment` in app/globals.css; lane F3 step 2 takes them, and
 * a CSS check should then hold them); a `style` that is not an object literal; and, because the
 * list counts sites per kind and not by name, a new site that replaces a fixed one of the same
 * kind in the same file.
 */

type Kind = 'link' | 'amount' | 'td' | 'prose' | 'line';
const KINDS: readonly Kind[] = ['link', 'amount', 'td', 'prose', 'line'];

/**
 * A loading line or an empty-state line, by its own words (BRIEF-roles-rest part C; the ink and
 * size round's ruling B27: a loading line is a sentence, site-wide, and so is an empty-state line):
 * words that end in the ellipsis ("Loading peer evidence…", "Resolving code…", ": reading…", a
 * lone "…" where a value is being read), or that open with "No" or "no" and a word ("No results.",
 * "No type script", "no CKB moved for").
 */
const LOADING_OR_EMPTY = /…$|^no \S/i;

/** The lane whose pages a file belongs to (the hand-back groups the list by them). */
type Family = 'chain' | 'assets' | 'lists' | 'topics' | 'shared';

interface Tolerated {
  family: Family;
  /** The sites of each kind in the file, today. */
  sites: Partial<Record<Kind, number>>;
  /** Why they stand, in one line: what they are and who takes them. */
  reason: string;
}

/**
 * The pinned total of TOLERATED's sites by family, and of its files (lane cd-fixes-controls,
 * review-p-tests NOTE-4: the list pinned no total, so a file could join it with a count without
 * moving a figure; the link-look, controls and lock-words gates pin theirs). Each figure only falls.
 */
// chain 36 → 31, lists 43 → 36, topics 18 → 14, files 59 → 58 (the merge with the P3 follow-up's
// 2d, each site decided: the blocks list 5, the transactions page 5 (one with 2b), the activities
// page 1, the stream explorer 1, the DAO depositors 4, which left the list)
// chain 28 → 0, files 51 → 37, decided 15 → 18 in 6 → 9 files (lane P1, the ink and size roles:
// each chain file left the list in the commit that converted it)
const TOLERATED_BY_FAMILY: Readonly<Record<Family, number>> = {
  // chain 31 → 28, lists 35 → 23 (the third read's T NOTE-3): the sites the P3 follow-up's 2d
  // decided to stay quiet left this work list for DECIDED (the blocks list 2, the CKBytes note 1;
  // the activities page 1, the inventory 1, the transactions page 6, the stream explorer 4)
  chain: 0,
  // assets 33 → 0 (lane P2, the ink and size roles round): every site reads in the reading ink
  assets: 0,
  // lists 36 → 35 (lane P2's third round, item 10, at its merge with the base that pinned the
  // figures): components/scripts/script-hero.tsx's companion line reads in the reading ink, its one
  // prose site gone with its entry
  // lists 23 → 15 (BRIEF-ink-size-roles item 1, the inventory: its figures, states and sentences in the reading ink)
  // lists 15 → 0 (BRIEF-ink-size-roles item 1, the script pages: their cells' states and dashes and their empty lines in the reading ink, their loading lines decided)
  lists: 0,
  // topics 14 → 0, shared 9 → 0 (lane cd-fixes-controls, the ink and size round)
  topics: 0,
  shared: 0,
};
// 58 → 57 (lane P2's third round, item 10): components/scripts/script-hero.tsx left the list;
// 57 → 51 (the third read's T NOTE-3): the six files whose sites are decided went to DECIDED;
// 51 → 0 (the ink and size round, at the merge of its five lanes): the chain family's 14 files
// (lane P1), the assets family's 15 (lane P2), the lists family's 5 (lane P3: the inventory 2, the
// script pages 3) and the topics and shared families' 17 (lane cd-fixes-controls) left the list
const TOLERATED_FILES = 0;

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- chain ----
  // lane P1, the ink and size roles: the chain family's 28 sites left this list, 25 in the
  // reading ink (empty, loading and waiting lines, a table cell's content, notes that are
  // sentences) and 3 to DECIDED (an uncle row's label, the witness list's hint, the structure's
  // key): the address page 2, address-history 1, address-holdings 2, the block page 4, the uncle
  // page 1, block-position 2, block-transactions-table 1, the transaction page 8,
  // cell-graph-renderer 1, cell-graph 1, cell-microscope 1, cell-transactions 2, code-cell-board
  // 1, tx-structure 1
  // 'app/cell/[outpoint]/client-page.tsx' left the list (lane P1 follow-up): the DAO line, its
  // deposit block and its compensation, is values in the reading ink (link 4 → 1, prose 2 → 1 in
  // B7: the out-point sentence reads)
  // 'components/blocks/block-visual.tsx' left the list (lane P1 follow-up): the size-and-cycles
  // line under the blocks figure is a reading of it, its figures values, in the reading ink
  // 'components/blocks/reward-breakdown.tsx' left the list (lane P1 follow-up): the reward's
  // sentences (genesis has none; paid by a block's cellbase, not indexed yet) and "paid by the
  // cellbase of #N · 0x…" are statements, in the reading ink, their links with them
  // 'components/home/ranked-bars.tsx' left the list (lane P3: link 1 → 0; the ranked list's title
  // link is its key, the key look's ink)
  // 'components/transaction/script-group-matrix.tsx' left the list (lane P1 follow-up): a group's
  // runs line ("guards out 1 for the next spend · …") is a statement, in the reading ink, a group
  // only attached as one that ran (link 1 → 0 in lane P1: its script's name reads)
  // ---- assets ----
  // The assets family left the list (lane P2, the ink and size roles round): its 33 sites in 15
  // files read in the reading ink, each a sentence, a cell's content or a link in its look:
  // - empty and loading lines are sentences (the tokens page's two and its label credit; the asset
  //   feed's "No activities." twice; the history's no-daily line; cell versions' empty line; the
  //   collection's three; its stored description's; the objects' three; the read-state line's
  //   loading and unavailable forms; genetics' five; the m-NFT's composition line; the media
  //   placeholder's; the empty cell data; the burned object's no-live-cell line; the sub-names'
  //   none; the identity evidence's witness credit);
  // - a row's cycles state ("Not computed", "Calculation failed") is its table cell's content;
  // - links that stood in a quiet note or cell wear their look: a version's owner address (its
  //   label quiet beside it), an object's minting date in its row, the decoders' two links in the
  //   mechanism tile, the sample object's link (its label and byte count quiet beside it), the
  //   witness transaction in the credit
  // ---- lists ----
  // 'app/scripts/[name]/client-page.tsx' left the list (BRIEF-ink-size-roles items 1 and 3: td 2,
  // prose 4 → DECIDED prose 2): "Custody evidence unavailable" and "Evidence unavailable" are a
  // cell's content, "No code-cell deployments" and "No capacity history." empty-state sentences, the
  // reading ink; its two loading lines are decided
  // 'app/scripts/page.tsx' left the list (lane P3: prose 1 → 0; the code-class bar is named by
  // the summary sentence, in the reading ink, no quiet label over it)
  // 'components/asset/inventory-summary.tsx' left the list (BRIEF-ink-size-roles items 1 and 3:
  // link 1, prose 2 → 0): the totals line and the identities' line are sentences, the reading ink
  // at 13 px, the anchor's block link in their ink
  // 'components/asset/inventory-table.tsx' left the list (BRIEF-ink-size-roles item 1: td 5 → 0):
  // the supply, the H-multiplier and the occupied CKB are figures in cells, and "Unavailable" a
  // cell's content, the reading ink; the minted " / N" reads at the cell's 13 px, its slash quiet
  // components/scripts/script-code-evidence.tsx left the list (lane P2's follow-up, item 4): the unspendable note reads in the reading ink
  // components/scripts/script-family-board.tsx left the list (lane P2's follow-up, item 4): the handover line reads in the reading ink
  // 'components/scripts/script-family-page.tsx' left the list (BRIEF-ink-size-roles items 1 and 3:
  // td 5, prose 3 → DECIDED prose 2): the dashes, "No deployment observed" and "custody
  // unavailable" are a cell's content, "No capacity history." an empty-state sentence, the reading
  // ink; its two loading lines are decided
  // components/scripts/script-hero.tsx left the list (lane P2's third round, item 10): the hero's
  // companion line reads in the reading ink (the scripts list page no longer draws it)
  // 'components/scripts/script-newest-cells.tsx' left the list (BRIEF-ink-size-roles item 1:
  // prose 1 → DECIDED prose 1): its loading line is decided
  // components/transaction/transaction-list-table.tsx left the list (lane P3: td 3 → 0; the
  // cellbase reward left with the cellbases' own list, the cycles state with the cycles column, the
  // cells count reads in the reading ink)
  // ---- topics ---- (lane cd-fixes-controls, the ink and size round: 14 → 0, each converted)
  // the loading and refreshing lines of the Fiber channel and channels pages (3), the forks page,
  // the hardforks page, the advertisement graph (2), the network map, the trends (2) and the Fiber
  // step are short sentences of a state, in the reading ink; the MaxMind credit states the data's
  // source, in the reading ink at 13 px; the DAO board's capacity-use caption and 365-day anchor
  // line are a sentence and a line of values, in the reading ink at 13 px, their block links in them
  // ---- shared ---- (lane cd-fixes-controls, the ink and size round: 9 → 0)
  // converted: the shortcuts panel's release and clock line and the footer's release and zone line
  // are values, in the reading ink; the allele gel's not-drawn line is a sentence, in the reading
  // ink at 13 px; an unregistered lock's code hash is an id, its link in the reading ink at 13 px
  // inside the lock words' quiet label; decided (DECIDED): the evidence tier's eyebrow, the two
  // gels' keys and the byte map's name
};

// ---- the scan ----

/** The scan gate's directories (`scan-fill-gate.test.ts`): every `.ts` and `.tsx` in them. */
const SCAN_DIRS = ['app', 'components', 'hooks', 'lib'];

/** `text-<x>` classes that are not an ink: the type steps and the alignments and wraps. */
const NOT_INK = new Set<string>([
  ...TYPE_STEPS,
  'left',
  'right',
  'center',
  'justify',
  'start',
  'end',
  'wrap',
  'nowrap',
  'balance',
  'pretty',
  'ellipsis',
  'clip',
]);
/** Variants that hold at rest: the responsive ones. A state variant (hover:, group-hover:) does not. */
const AT_REST = /^(?:sm|md|lg|xl|2xl|max-sm|max-md|max-lg|max-xl|min-\[.+\]|max-\[.+\])$/;

/** A class's variants and its utility, split at the colons outside brackets (`text-[color:…]`). */
function variantsOf(token: string): { variants: string[]; utility: string } {
  const parts: string[] = [];
  let depth = 0;
  let current = '';
  for (const ch of token) {
    if (ch === '[') depth++;
    if (ch === ']') depth--;
    if (ch === ':' && depth === 0) {
      parts.push(current);
      current = '';
    } else current += ch;
  }
  return { variants: parts, utility: current };
}

/** A class's opacity modifier (`/90`, `/[0.9]`), or a size's line height (`text-small/5`). */
const MODIFIER = /\/(?:[\d.]+|\[[^\]]*\])$/;

/**
 * The ink a class sets at rest: `text-text-dim`, another ink, or none (a size, a state variant).
 * Its key is read without the important modifier (`!text-text-dim`) and without an opacity
 * modifier or a line height (`text-text-dim/90`, `text-small/5`): the quiet ink at an alpha is
 * still the quiet ink here (the ink gate refuses the alpha itself).
 */
function inkOfClass(token: string): 'dim' | 'ink' | null {
  const { variants, utility: written } = variantsOf(token);
  if (!variants.every((v) => AT_REST.test(v))) return null;
  const utility = written.replace(/^!/, '').replace(MODIFIER, '');
  if (!utility.startsWith('text-')) return null;
  const key = utility.slice('text-'.length);
  if (key === 'text-dim') return 'dim';
  if (NOT_INK.has(key)) return null;
  // an arbitrary value: the quiet ink's variable, by name or by its channels, is the quiet ink
  if (key.startsWith('['))
    return QUIET_VAR.test(key) ? 'dim' : key.includes('color:') ? 'ink' : null;
  return 'ink';
}
/** The quiet ink's CSS variables (`lib/palette.ts`): its colour and its channels. */
const QUIET_VAR = /--(?:c|rgb)-text-dim\b/;

const LINK = (tag: string) => tag === 'a' || tag === 'Address' || /^(?:[A-Z]\w*)?Link$/.test(tag);

/** The classes a link's look draws (`linkLookClass`, the looks' one owner); none for an unknown look. */
const lookTokens = (look: string): string[] =>
  (LINK_LOOKS as readonly string[]).includes(look)
    ? (linkLookClass(look as LinkLook) ?? '').split(/\s+/).filter(Boolean)
    : [];

/**
 * The components known to ink a prop themselves (BRIEF-roles-rest part D): an element handed to one
 * of them in that prop takes the ink the component gives it, not the ink around the component. A
 * component is named by the module and the export a file imports it from, never by a pattern of
 * names, each with what its source draws; the list is pinned (PROP_INKS_PINNED) and only a reading
 * of the component's source adds to it.
 */
const PROP_INKS: Readonly<
  Record<string, { props: Readonly<Record<string, 'dim' | 'other'>>; reason: string }>
> = {
  '@/components/ui/prompt-heading#PromptHeading': {
    props: { args: 'dim', note: 'dim' },
    reason:
      'its args in text-text-dim (what the command runs on, quiet by rule: R3, B23) and its note, the right prompt, in text-text-dim text-meta',
  },
  '@/components/home/section#SectionHeading': {
    props: { args: 'dim', note: 'dim' },
    reason: "a home section's heading: hands its args and its note to PromptHeading as they are",
  },
  '@/components/ui/detail-layout#DetailSection': {
    props: { args: 'dim' },
    reason:
      "a tier-2 section's heading: its args in text-text-dim beside the title, as PromptHeading's",
  },
  '@/components/scripts/script-hero#HeroMetric': {
    props: { note: 'dim' },
    reason: "a script hero metric: its inline note in text-text-dim after the value (R13's note)",
  },
};
/** The components and props PROP_INKS names: its pinned count, which only a reading changes. */
const PROP_INKS_PINNED = { components: 4, props: 6 };
const AMOUNT_DEFAULT_TONE: Readonly<Record<string, string>> = {
  CkbAmount: 'sign',
  TokenAmount: 'inherit',
};

interface Site {
  kind: Kind;
  line: number;
  what: string;
}

function unwrap(e: ts.Expression): ts.Expression {
  while (
    ts.isParenthesizedExpression(e) ||
    ts.isAsExpression(e) ||
    ts.isSatisfiesExpression(e) ||
    ts.isNonNullExpression(e)
  )
    e = e.expression;
  return e;
}

/** Every site of the four kinds in a TSX source. */
function inkRoleSites(code: string): Site[] {
  return scanInkRoles(code).sites;
}

/** An element that carries the `quiet` class: every word under it is exempt from the gate. */
interface QuietSite {
  line: number;
  what: string;
}

/**
 * Every site of the four kinds in a source (TSX unless `kind` says otherwise), and every element
 * that exempts its subtree.
 */
function scanInkRoles(
  code: string,
  kind: ts.ScriptKind = ts.ScriptKind.TSX
): { sites: Site[]; quiet: QuietSite[] } {
  const source = ts.createSourceFile('scan.tsx', code, ts.ScriptTarget.Latest, true, kind);
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const text = (n: ts.Node) => n.getText(source).replace(/\s+/g, ' ');

  const locals = new Map<string, ts.Expression[]>();
  /** The file's imported names: a local name to `module#export` (`#default` for a default import). */
  const imports = new Map<string, string>();
  const collect = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer)
      locals.set(node.name.text, [...(locals.get(node.name.text) ?? []), node.initializer]);
    if (
      ts.isImportDeclaration(node) &&
      ts.isStringLiteral(node.moduleSpecifier) &&
      node.importClause
    ) {
      const from = node.moduleSpecifier.text;
      const clause = node.importClause;
      if (clause.name) imports.set(clause.name.text, `${from}#default`);
      if (clause.namedBindings && ts.isNamedImports(clause.namedBindings))
        for (const s of clause.namedBindings.elements)
          imports.set(s.name.text, `${from}#${(s.propertyName ?? s.name).text}`);
    }
    ts.forEachChild(node, collect);
  };
  collect(source);

  /** The strings an expression can reach: literals, template parts, branches, call arguments, constants. */
  const leaves = (e0: ts.Expression, depth = 0): string[] => {
    if (depth > 12) return [];
    const e = unwrap(e0);
    const next = (x: ts.Expression) => leaves(x, depth + 1);
    if (ts.isStringLiteralLike(e)) return [e.text];
    if (ts.isTemplateExpression(e))
      return [
        e.head.text,
        ...e.templateSpans.flatMap((s) => [s.literal.text, ...next(s.expression)]),
      ];
    if (ts.isConditionalExpression(e)) return [...next(e.whenTrue), ...next(e.whenFalse)];
    if (ts.isBinaryExpression(e)) return [...next(e.left), ...next(e.right)];
    if (ts.isCallExpression(e)) return e.arguments.flatMap(next);
    if (ts.isArrayLiteralExpression(e))
      return e.elements.flatMap((el) => next(ts.isSpreadElement(el) ? el.expression : el));
    if (ts.isObjectLiteralExpression(e))
      return e.properties.flatMap((p) => (ts.isPropertyAssignment(p) ? next(p.initializer) : []));
    if (ts.isIdentifier(e)) return (locals.get(e.text) ?? []).flatMap(next);
    if (ts.isPropertyAccessExpression(e) || ts.isElementAccessExpression(e)) {
      const key = ts.isPropertyAccessExpression(e)
        ? e.name.text
        : ts.isStringLiteralLike(e.argumentExpression)
          ? e.argumentExpression.text
          : null;
      const base = unwrap(e.expression);
      if (!ts.isIdentifier(base)) return [];
      return (locals.get(base.text) ?? []).flatMap((t0) => {
        const t = unwrap(t0);
        if (!ts.isObjectLiteralExpression(t)) return [];
        return t.properties.flatMap((p) =>
          ts.isPropertyAssignment(p) &&
          (key === null ||
            ((ts.isIdentifier(p.name) || ts.isStringLiteralLike(p.name)) && p.name.text === key))
            ? next(p.initializer)
            : []
        );
      });
    }
    return [];
  };
  const tokensOf = (e: ts.Expression) => leaves(e).flatMap((s) => s.split(/\s+/).filter(Boolean));

  type El = ts.JsxElement | ts.JsxSelfClosingElement;
  const opening = (n: El) => (ts.isJsxElement(n) ? n.openingElement : n);
  const tagOf = (n: El) => text(opening(n).tagName);
  const attr = (n: El, name: string): ts.Expression | undefined => {
    for (const a of opening(n).attributes.properties) {
      if (!ts.isJsxAttribute(a) || text(a.name) !== name || !a.initializer) continue;
      const init = a.initializer;
      return ts.isJsxExpression(init)
        ? init.expression
        : ts.isStringLiteral(init)
          ? init
          : undefined;
    }
    return undefined;
  };
  /**
   * Whether a style's colour can be the quiet ink: `color('text-dim')` (`lib/palette.ts`) or a
   * string naming its variable, through `?:`, `&&`, `||`, `??` and the file's own constants. Any
   * other call (`alpha('text-dim', …)`, `fade(…)`) is another ink.
   */
  const quietColour = (e0: ts.Expression, depth = 0): boolean => {
    if (depth > 12) return false;
    const e = unwrap(e0);
    const next = (x: ts.Expression) => quietColour(x, depth + 1);
    if (ts.isConditionalExpression(e)) return next(e.whenTrue) || next(e.whenFalse);
    if (ts.isBinaryExpression(e)) {
      const op = e.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return next(e.right);
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken)
        return next(e.left) || next(e.right);
      return false;
    }
    if (ts.isCallExpression(e))
      return (
        text(e.expression) === 'color' &&
        e.arguments.length > 0 &&
        leaves(e.arguments[0]).includes('text-dim')
      );
    if (ts.isIdentifier(e)) return (locals.get(e.text) ?? []).some(next);
    if (ts.isStringLiteralLike(e) || ts.isTemplateExpression(e))
      return leaves(e).some((s) => QUIET_VAR.test(s));
    return false;
  };
  /** The colour an element's own `style` object sets, if it sets one. */
  const styleColour = (n: El): ts.Expression | undefined => {
    const style = attr(n, 'style');
    if (style === undefined || !ts.isObjectLiteralExpression(unwrap(style))) return undefined;
    for (const p of (unwrap(style) as ts.ObjectLiteralExpression).properties) {
      if (ts.isPropertyAssignment(p) && ts.isIdentifier(p.name) && p.name.text === 'color')
        return p.initializer;
      if (ts.isShorthandPropertyAssignment(p) && p.name.text === 'color') return p.name;
    }
    return undefined;
  };
  /**
   * The element's own ink: is it quiet (the set-back class), dim (by its class, an arbitrary-value
   * class or its style's colour), inked otherwise, or neither.
   */
  const classes = (n: El) => {
    const value = attr(n, 'className');
    const look = LINK(tagOf(n)) ? attr(n, 'look') : undefined;
    const tokens = [
      // a link's look draws its ink (components/ui/link.tsx: key and onward set one, inline and
      // block keep the line's), read from the one owner of the looks
      ...(look ? leaves(look).flatMap((l) => lookTokens(l)) : []),
      ...(value ? tokensOf(value) : []),
    ];
    const inks = tokens.map(inkOfClass);
    const colour = styleColour(n);
    return {
      quiet: tokens.includes('quiet'),
      dim: inks.includes('dim') || (colour !== undefined && quietColour(colour)),
      inked: colour !== undefined || inks.some((i) => i !== null),
    };
  };

  /** The element a JSX attribute is written on: the component its value is handed to. */
  const ownerOf = (a: ts.JsxAttribute): El => {
    const o = a.parent.parent;
    return ts.isJsxOpeningElement(o) ? o.parent : o;
  };
  /**
   * The ink a component pinned in PROP_INKS gives the prop an attribute hands it, if it is pinned:
   * the component is named by what the file imports it as (`module#export`), never by a pattern.
   */
  const pinnedInk = (a: ts.JsxAttribute): 'dim' | 'other' | undefined => {
    const component = imports.get(tagOf(ownerOf(a)));
    return component === undefined ? undefined : PROP_INKS[component]?.props[text(a.name)];
  };
  /**
   * The ink an element takes from around it, in its file: the nearest JSX ancestor that sets one.
   * `quiet` for a set-back subtree, `dim`, `other`, or `none` (nothing in the file sets one). An
   * element handed to a component as a prop (in an attribute, an array or an object literal there,
   * a function's return) takes the ink the component pins for that prop (PROP_INKS), else the ink
   * around the component: its own class, then its ancestors'.
   */
  const around = (node: ts.Node): 'quiet' | 'dim' | 'other' | 'none' => {
    for (let p: ts.Node | undefined = node.parent; p; p = p.parent) {
      if (ts.isJsxAttribute(p)) {
        const pinned = pinnedInk(p);
        if (pinned !== undefined) return pinned;
        p = ownerOf(p);
      }
      if (ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) {
        const c = classes(p);
        if (c.quiet) return 'quiet';
        if (c.dim) return 'dim';
        if (c.inked) return 'other';
      }
    }
    return 'none';
  };
  const inQuiet = (node: ts.Node): boolean => {
    for (let p: ts.Node | undefined = node.parent; p; p = p.parent) {
      if (ts.isJsxAttribute(p)) p = ownerOf(p);
      if ((ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) && classes(p).quiet) return true;
    }
    return false;
  };

  /** The root elements a render function returns. */
  const roots = (e0: ts.Expression | undefined): El[] => {
    if (!e0) return [];
    const e = unwrap(e0);
    if (ts.isJsxElement(e) || ts.isJsxSelfClosingElement(e)) return [e];
    if (ts.isConditionalExpression(e)) return [...roots(e.whenTrue), ...roots(e.whenFalse)];
    if (ts.isBinaryExpression(e)) {
      const op = e.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return roots(e.right);
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken)
        return [...roots(e.left), ...roots(e.right)];
    }
    return [];
  };
  const returned = (fn: ts.Expression): El[] => {
    const f = unwrap(fn);
    if (!ts.isArrowFunction(f) && !ts.isFunctionExpression(f)) return [];
    if (!ts.isBlock(f.body)) return roots(f.body);
    const out: El[] = [];
    const walk = (n: ts.Node) => {
      if (ts.isFunctionLike(n) && n !== f) return;
      if (ts.isReturnStatement(n)) out.push(...roots(n.expression));
      ts.forEachChild(n, walk);
    };
    walk(f.body);
    return out;
  };

  /**
   * Whether an element's own words are a loading or an empty-state line (`LOADING_OR_EMPTY`): its
   * text children read as one line, or one of the strings its child expressions can reach (a
   * branch's words), never the words of an element inside it, which is read on its own.
   */
  const loadingOrEmpty = (el: ts.JsxElement): boolean => {
    const own = el.children
      .filter(ts.isJsxText)
      .map((t) => t.text)
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim();
    const reached = el.children.flatMap((c) =>
      ts.isJsxExpression(c) && c.expression && !ts.isJsxElement(unwrap(c.expression))
        ? leaves(c.expression).map((l) => l.trim())
        : []
    );
    return [own, ...reached].some((words) => words !== '' && LOADING_OR_EMPTY.test(words));
  };

  const sites: Site[] = [];
  const quiet: QuietSite[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = tagOf(node);
      if (classes(node).quiet) quiet.push({ line: lineOf(node), what: `<${tag}>` });
      const kind: Kind | null = LINK(tag)
        ? 'link'
        : tag in AMOUNT_DEFAULT_TONE
          ? 'amount'
          : tag === 'td'
            ? 'td'
            : tag === 'p'
              ? 'prose'
              : /^[a-z]/.test(tag) && ts.isJsxElement(node) && loadingOrEmpty(node)
                ? 'line'
                : null;
      if (kind !== null) {
        const own = classes(node);
        let dim = false;
        if (own.quiet || inQuiet(node)) dim = false;
        else if (kind === 'amount') {
          const tone = attr(node, 'tone');
          const inherits = tone
            ? leaves(tone).includes('inherit')
            : AMOUNT_DEFAULT_TONE[tag] === 'inherit';
          dim = inherits && around(node) === 'dim';
        } else if (own.dim) dim = true;
        else if (!own.inked) dim = around(node) === 'dim';
        if (dim) sites.push({ kind, line: lineOf(node), what: `<${tag}>` });
      }
    }
    // a ListTable column: an object literal with `render` and `minWidth`
    if (ts.isObjectLiteralExpression(node)) {
      const props = new Map<string, ts.Expression>();
      for (const p of node.properties)
        if (
          ts.isPropertyAssignment(p) &&
          (ts.isIdentifier(p.name) || ts.isStringLiteralLike(p.name))
        )
          props.set(p.name.text, p.initializer);
      if (props.has('render') && props.has('minWidth')) {
        const cls = props.get('className');
        if (cls && tokensOf(cls).some((t) => inkOfClass(t) === 'dim'))
          sites.push({ kind: 'td', line: lineOf(cls), what: 'column className' });
        for (const root of returned(props.get('render')!))
          if (classes(root).dim && !classes(root).quiet)
            sites.push({ kind: 'td', line: lineOf(root), what: `column render <${tagOf(root)}>` });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { sites, quiet };
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

/** The scan's count against the list: every file's count per kind must equal its tolerated one. */
function toleratedFailures(
  found: ReadonlyMap<string, readonly Site[]>,
  tolerated: Readonly<Record<string, Tolerated>>
): string[] {
  const out: string[] = [];
  const paths = new Set([...found.keys(), ...Object.keys(tolerated)]);
  for (const path of [...paths].sort()) {
    const sites = found.get(path) ?? [];
    const entry = tolerated[path];
    for (const kind of KINDS) {
      const of = sites.filter((s) => s.kind === kind);
      const allowed = entry?.sites[kind] ?? 0;
      if (of.length > allowed)
        out.push(
          `${path}: ${of.length} ${kind} site(s) in the quiet ink, ${allowed} tolerated — a new one: ${of
            .map((s) => `${s.line} ${s.what}`)
            .join(
              ', '
            )} (a value, a link, a number or a sentence takes text-text; a label, a unit, a separator or a note stays quiet)`
        );
      if (of.length < allowed)
        out.push(
          `${path}: ${of.length} ${kind} site(s) in the quiet ink, ${allowed} tolerated — lower its count to ${of.length}`
        );
    }
    if (entry && entry.reason.trim().length <= 10)
      out.push(`${path}: a reason too short to say why: "${entry.reason}"`);
  }
  return out;
}

/**
 * The sites decided to stay quiet, file by file (the third read's T NOTE-3: they stood in
 * TOLERATED, the work list whose total only falls, so it could never reach zero): a loading line,
 * an empty line, a note under a figure or a chip row, an empty value's state in a table cell, each
 * named by kind with a count. They are no work left; the scan holds them as it holds TOLERATED.
 */
const DECIDED: Readonly<Record<string, Tolerated>> = {
  // ---- shared (lane cd-fixes-controls, the ink and size round): each a role of the rule ----
  'components/ui/detail-layout.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason:
      'decided (the shared lane): the evidence tier’s “Evidence · for developers”, an eyebrow set as a <p>',
  },
  'components/ui/allele-gel.tsx': {
    family: 'shared',
    sites: { prose: 2 },
    reason:
      'decided (the shared lane): the gel’s two keys, its axis labels (“variants per gene ↑ · one lane per gene…”, “genes ranked by how many variants…”) and a hint of how to use it (“open a gene for every variant”)',
  },
  'components/ui/sequencing-gel.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason:
      'decided (the shared lane): the gel’s key, a hint of how to read it (“read top to bottom: each band is one hex digit…”)',
  },
  'components/ui/script-byte-map.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason:
      'decided (the shared lane): the byte map’s name, the label of the figure (“Script · 33 bytes, then the args each cell sets”)',
  },
  'components/blocks/block-position.tsx': {
    family: 'chain',
    // lane P1, the ink and size roles: the one quiet line of the chain position
    sites: { prose: 1 },
    reason:
      "label 1: an uncle row's 'Uncle parent → included uncle', the label of the two links under it ('Parent 0x…', 'Uncle #… · 0x…')",
  },
  'app/tx/[hash]/client-page.tsx': {
    family: 'chain',
    // lane P1, the ink and size roles: the one quiet line of the transaction page
    sites: { prose: 1 },
    reason:
      "hint 1: 'Select a witness to inspect its bytes.', how to use the witness list above it",
  },
  'components/transaction/tx-structure.tsx': {
    family: 'chain',
    // lane P1, the ink and size roles: the structure's key
    sites: { prose: 1 },
    reason:
      "label 1: the key of the figure's dashed onward bridge ('this cell, onward' beside its swatch), a mark's label as the chain position's legend entries are",
  },
};
/**
 * The pinned sites and files of DECIDED: 15 sites in 6 files, the P3 follow-up's 2d; at the merge
 * of the ink and size round's five lanes, 19 in 14: lane P1's 3 (an uncle row's label, the witness
 * list's hint, the structure's key), the shared lane's 5 (lane cd-fixes-controls: an eyebrow, two
 * gels' keys and a figure's name) and the lists family's loading lines (lane P3: 11 in 7 files, the
 * eight sites of the 15 that were no loading line read in the reading ink). 19 → 8 in 7 files (the
 * lead's follow-up to lane P3: a loading line is a sentence and reads in the reading ink, one rule
 * site-wide, as the chain, assets, topics and shared lanes wrote theirs): the lists family's 11
 * loading lines left; what stays is lane P1's 3 and the shared lane's 5, each a label, a hint, a
 * key or an eyebrow.
 */
const DECIDED_SITES = 8;
const DECIDED_FILES = 7;

/**
 * The elements that carry the `quiet` class (the set-back row, R4: every word under it quiet on
 * purpose, so the gate does not count them), by file, with their count and why. The exemption is
 * a decision: a new one fails, and one that went leaves a count behind, as in TOLERATED.
 */
const QUIET_SITES: Readonly<Record<string, { count: number; reason: string }>> = {
  'components/asset/asset-feed.tsx': {
    count: 1,
    reason: "a neighbour's activity row in an asset's feed, set back (R4): every word dim",
  },
  'components/asset/item-evidence.tsx': {
    count: 1,
    reason: "a neighbour's row in an item's evidence, set back (R4): every word dim",
  },
  'components/ui/cell-tile.tsx': {
    count: 1,
    reason: 'a dimmed cell tile, set back (R4): every word dim, its stones in their own ink',
  },
};

/** The `quiet` exemptions found against QUIET_SITES: every file's count must equal its pin. */
function quietFailures(
  found: ReadonlyMap<string, readonly QuietSite[]>,
  pinned: Readonly<Record<string, { count: number; reason: string }>>
): string[] {
  const out: string[] = [];
  for (const path of [...new Set([...found.keys(), ...Object.keys(pinned)])].sort()) {
    const sites = found.get(path) ?? [];
    const entry = pinned[path];
    const allowed = entry?.count ?? 0;
    if (sites.length > allowed)
      out.push(
        `${path}: ${sites.length} element(s) set back with the quiet class, ${allowed} pinned — a new one: ${sites
          .map((s) => `${s.line} ${s.what}`)
          .join(', ')} (it exempts every word under it from the ink rule: a decision, list it)`
      );
    if (sites.length < allowed)
      out.push(
        `${path}: ${sites.length} element(s) set back with the quiet class, ${allowed} pinned — lower its count to ${sites.length}`
      );
    if (entry && entry.reason.trim().length <= 10)
      out.push(`${path}: a reason too short to say why: "${entry.reason}"`);
  }
  return out;
}

describe('the quiet exemption is counted (review of main..fix/readability, test finding 6)', () => {
  it('finds every element that carries the quiet class, whatever its tag', () => {
    expect(
      scanInkRoles(
        [
          '<tr className="quiet"><td className="text-text-dim">1</td></tr>;',
          "<li className={cn('text-meta', neighbour && 'quiet')}>{row}</li>;",
          "const SET_BACK = 'quiet text-small';",
          '<Row className={SET_BACK} />;',
          '<div className="quieter text-text">not the class</div>;',
        ].join('\n')
      ).quiet.map((s) => `${s.line} ${s.what}`)
    ).toEqual(['1 <tr>', '2 <li>', '4 <Row>']);
  });

  it('fails a new exemption and one that went', () => {
    const pinned = { 'app/a.tsx': { count: 1, reason: 'a neighbour row set back' } };
    expect(quietFailures(new Map([['app/a.tsx', [{ line: 3, what: '<tr>' }]]]), pinned)).toEqual(
      []
    );
    expect(
      quietFailures(
        new Map([
          [
            'app/a.tsx',
            [
              { line: 3, what: '<tr>' },
              { line: 9, what: '<li>' },
            ],
          ],
        ]),
        pinned
      )
    ).toEqual([
      expect.stringMatching(/^app\/a\.tsx: 2 element\(s\) .* 1 pinned — a new one: 3 <tr>, 9 <li>/),
    ]);
    expect(quietFailures(new Map(), pinned)).toEqual([
      'app/a.tsx: 0 element(s) set back with the quiet class, 1 pinned — lower its count to 0',
    ]);
  });
});

describe('the ink-role scan finds what it must, and only that', () => {
  const sitesOf = (lines: string[]) =>
    inkRoleSites(lines.join('\n')).map((s) => `${s.line} ${s.kind} ${s.what}`);

  it('finds a link, a paragraph and a table cell drawn in the quiet ink by their own class', () => {
    expect(
      sitesOf([
        '<Link href="/x" className="text-text-dim hover:text-jade">x</Link>;',
        "<p className={cn('text-small', on ? 'text-text-dim' : 'text-text')}>a sentence</p>;",
        '<td className="text-text-dim">1,234</td>;',
        '<BlockLink number={1} className="md:text-text-dim" />;',
        "const NOTE = 'text-text-dim text-meta';",
        '<p className={NOTE}>read top to bottom</p>;',
      ])
    ).toEqual(['1 link <Link>', '2 prose <p>', '3 td <td>', '4 link <BlockLink>', '6 prose <p>']);
  });

  it('finds a link or an inheriting amount that takes the quiet ink of a line around it', () => {
    expect(
      sitesOf([
        '<div className="text-text-dim text-small">',
        '  proposed in <BlockLink number={n} />',
        '  {rows.map((r) => <AppLink key={r} href={r}>{r}</AppLink>)}',
        '  <CkbAmount form="summary" shannons={x} tone="inherit" />',
        '  <TokenAmount form="summary" amount="1" decimals={8} />',
        '  <CkbAmount form="summary" shannons={x} />',
        '  <Link href="/y" className="text-text">reads</Link>',
        '</div>;',
      ])
    ).toEqual([
      '2 link <BlockLink>',
      '3 link <AppLink>',
      '4 amount <CkbAmount>',
      '5 amount <TokenAmount>',
    ]);
  });

  it('finds a ListTable column whose class or rendered root is quiet', () => {
    expect(
      sitesOf([
        'const columns = [',
        "  { key: 'a', label: 'A', minWidth: 80, className: 'text-text-dim', render: (r) => r.a },",
        "  { key: 'b', label: 'B', minWidth: 80, render: (r) => <span className=\"text-text-dim\">{r.b}</span> },",
        "  { key: 'c', label: 'C', minWidth: 80, render: (r) => { if (!r.c) return <span className=\"text-text-dim\">—</span>; return r.c; } },",
        "  { key: 'd', label: 'D', minWidth: 80, render: (r) => <span className=\"text-text\">{r.d}</span> },",
        '];',
      ])
    ).toEqual(['2 td column className', '3 td column render <span>', '4 td column render <span>']);
  });

  // Review of main..fix/readability (2026-11-18, test finding 6): the quiet ink reached a link, a
  // cell or a paragraph uncounted through an arbitrary-value class and through an inline style
  // colour, both read as "another ink".
  it('finds the quiet ink set by an arbitrary-value class or an inline style colour', () => {
    expect(
      sitesOf([
        '<p className="text-[color:var(--c-text-dim)] text-small">a sentence</p>;',
        '<td className="md:text-[var(--c-text-dim)]">1,234</td>;',
        '<Link href="/x" style={{ color: \'var(--c-text-dim)\' }}>x</Link>;',
        "<p style={{ color: color('text-dim') }}>explains</p>;",
        "<div style={{ color: on ? color('text') : color('text-dim') }}><BlockLink number={1} /></div>;",
        "const ink = color('text-dim');",
        '<p style={{ color: ink }}>held in a name</p>;',
      ])
    ).toEqual([
      '1 prose <p>',
      '2 td <td>',
      '3 link <Link>',
      '4 prose <p>',
      '5 link <BlockLink>',
      '7 prose <p>',
    ]);
  });

  // Review of the fixes (2026-11-19, S3): a class's key was compared whole, so the quiet ink under
  // an opacity modifier (`text-text-dim/90`, read as the key `text-dim/90`) or the important
  // modifier (`!text-text-dim`, read as no ink) passed uncounted; and a size with its line height
  // (`text-small/5`) was read as an ink, so a link in it hid from the quiet line around it.
  it('finds the quiet ink under an opacity or an important modifier', () => {
    expect(
      sitesOf([
        '<p className="text-text-dim/90">a sentence</p>;',
        '<Link href="/x" className="text-text-dim/95">x</Link>;',
        '<td className="text-text-dim/[0.9]">1,234</td>;',
        '<div className="text-text"><p className="!text-text-dim">explains</p></div>;',
        '<BlockLink number={1} className="md:!text-text-dim" />;',
        '<div className="text-text-dim"><Link href="/y" className="text-small/5">y</Link></div>;',
        '<p className="!text-text">reads</p>;',
        '<p className="text-text/90">reads</p>;',
      ])
    ).toEqual([
      '1 prose <p>',
      '2 link <Link>',
      '3 td <td>',
      '4 prose <p>',
      '5 link <BlockLink>',
      '6 link <Link>',
    ]);
  });

  it('passes another arbitrary or inline ink, and a quiet ink at an alpha', () => {
    expect(
      sitesOf([
        '<p className="text-[color:var(--c-text)]">reads</p>;',
        "<p style={{ color: color('text') }}>reads</p>;",
        '<div style={{ color: color(\'text-dim\') }}><Link href="/l" className="text-text">l</Link></div>;',
        "<p style={{ color: alpha('text-dim', 0.6) }}>alpha is the other gate’s</p>;",
      ])
    ).toEqual([]);
  });

  // Re-pinned on purpose (BRIEF-roles-rest part D; renamed, it was "passes a set-back row, a state
  // variant, a prop handed to a component and a label"): its case
  // '<div className="text-text-dim"><Tile end={<Link href="/q">q</Link>} /></div>' is the blind
  // route the brief closes, a link handed to a component inside a quiet line; it is found now, in
  // the test below, and this test keeps its other cases.
  it('passes a set-back row, a state variant and a label', () => {
    expect(
      sitesOf([
        '<tr className="quiet"><td className="text-text-dim">1</td><td><Link href="/z">z</Link></td></tr>;',
        '<div className="quiet text-text-dim"><p>context</p></div>;',
        '<Link href="/x" className="text-text hover:text-text-dim">x</Link>;',
        '<dt className="text-text-dim">Height</dt>;',
        '<span className="text-text-dim">occupied</span>;',
        '<div className="text-text-dim"><p className="text-text">explains</p></div>;',
        '<div className="text-text-dim"><Link href="/i" className="text-[color:var(--ink)]">i</Link></div>;',
      ])
    ).toEqual([]);
  });

  // BRIEF-roles-rest part D: `around()` stopped at a JSX attribute, so an element handed to a
  // component as a prop took no ink, and the newest cells' "Created" line (a quiet span around
  // `Dotted parts={[tx + link, BlockLink, date]}`) passed with its tx and block links quiet.
  it('finds a link or an inheriting amount handed to a component that stands in a quiet line', () => {
    expect(
      sitesOf([
        '<span className="text-text-dim">',
        '  <Dotted parts={[<>tx <Link look="inline" href="/t">t</Link></>, <BlockLink look="inline" number={1} />]} />',
        '</span>;',
        '<div className="text-text-dim"><Tile end={<Link href="/q">q</Link>} /></div>;',
        '<div className="text-text-dim"><Row right={<CkbAmount form="summary" shannons={x} tone="inherit" />} /></div>;',
        '<Dotted className="text-text-dim" parts={[<TokenAmount form="summary" amount="1" decimals={8} />]} />;',
        '<div className="text-text-dim"><Rows lines={[{ key: "a", left: <AppLink href="/a">a</AppLink> }]} /></div>;',
        '<div className="text-text-dim"><List render={(r) => <BlockLink look="inline" number={r} />} /></div>;',
        '<div className="text-text-dim"><Row right={<span className="text-text"><Link href="/r">r</Link></span>} /></div>;',
        '<div className="quiet text-text-dim"><Row right={<Link href="/n">n</Link>} /></div>;',
      ])
    ).toEqual([
      '2 link <Link>',
      '2 link <BlockLink>',
      '4 link <Link>',
      '5 amount <CkbAmount>',
      '6 amount <TokenAmount>',
      '7 link <AppLink>',
      '8 link <BlockLink>',
    ]);
  });

  // BRIEF-roles-rest part D: a component known to ink a prop itself gives the element that ink,
  // whatever stands around the component (PROP_INKS, by the module it is imported from).
  it('gives an element the ink a pinned component gives its prop, by the import, not the name', () => {
    expect(
      sitesOf([
        "import { PromptHeading } from '@/components/ui/prompt-heading';",
        "import { PromptHeading as Heading } from '@/components/ui/prompt-heading';",
        '<PromptHeading command="supply" note={<span>at <BlockLink look="inline" number={1} /></span>} />;',
        '<Heading command="cells" args={<>--tip <BlockLink look="inline" number={2} /></>} />;',
        '<PromptHeading command="x" note={<span className="text-text">at <BlockLink look="inline" number={3} /></span>} />;',
        '<PromptHeading command="class" note={<AppLink look="onward" href="/c">Open the class</AppLink>} />;',
        '<div className="text-text"><PromptHeading command="y" note={<Link look="inline" href="/y">y</Link>} /></div>;',
        '<SectionHeading command="z" note={<Link look="inline" href="/z">z</Link>} />;',
      ])
    ).toEqual(['3 link <BlockLink>', '4 link <BlockLink>', '7 link <Link>']);
  });

  // BRIEF-roles-rest part D: a link's look draws its ink (components/ui/link.tsx): the key and
  // onward looks set one, so they do not take the quiet line's; the inline look keeps the line's.
  it('reads the ink a link’s look draws: key and onward their own, inline the line’s', () => {
    expect(
      sitesOf([
        '<div className="text-text-dim">',
        '  <Link look="key" href="/k">k</Link>',
        '  <AppLink look="onward" href="/o">o</AppLink>',
        '  <BlockLink look="inline" number={1} />',
        '  <Link look="block" href="/b">b</Link>',
        '</div>;',
      ])
    ).toEqual(['4 link <BlockLink>', '5 link <Link>']);
  });

  // BRIEF-roles-rest part C: a loading or an empty-state line drawn in a div or a span was no kind
  // of the gate (only a <p> was prose), so "Resolving script page…" and twenty more stood quiet.
  it('finds a loading or an empty-state line in the quiet ink outside a <p>, and only that', () => {
    expect(
      sitesOf([
        '<div className="text-text-dim text-small">Loading the daily AR history…</div>;',
        '<span className="text-text-dim">{unitText(title)}: reading…</span>;',
        '<div className="text-text-dim">{searching ? "Searching…" : "No results."}</div>;',
        '<div className="text-text-dim"><span>No peers match these evidence filters.</span></div>;',
        '<span className="text-text-dim text-small">no CKB moved for </span>;',
        '<span className="text-text-dim">…</span>;',
        '<span className="text-text-dim">held by</span>;',
        '<span className="text-text-dim">since <Utc8Date timestamp={t} /></span>;',
        '<div className="text-text-dim"><span className="text-text">Loading…</span></div>;',
        '<p className="text-text-dim">Loading the page…</p>;',
        '<tr className="quiet"><td><span>No data yet</span></td></tr>;',
        '<dt className="text-text-dim">Nonce</dt>;',
      ])
    ).toEqual([
      '1 line <div>',
      '2 line <span>',
      '3 line <div>',
      '4 line <span>',
      '5 line <span>',
      '6 line <span>',
      '10 prose <p>',
    ]);
  });

  it('pins the components known to ink a prop: four, six props, each with its reason', () => {
    expect({
      components: Object.keys(PROP_INKS).length,
      props: Object.values(PROP_INKS).reduce((n, c) => n + Object.keys(c.props).length, 0),
    }).toEqual(PROP_INKS_PINNED);
    for (const [component, entry] of Object.entries(PROP_INKS)) {
      expect(component).toMatch(/^@\/[\w/-]+#\w+$/);
      expect(entry.reason.length).toBeGreaterThan(30);
    }
  });

  // Re-pinned on purpose (review of the fixes, S3): '<p className="text-text-dim/60">' passed this
  // gate as "alpha is the other gate's"; the class still sets the quiet ink, at an alpha, so it is
  // a site here as well as an offence of the ink gate's.
  it('counts the quiet ink at an alpha, which the ink gate refuses besides', () => {
    expect(sitesOf(['<p className="text-text-dim/60">alpha is the other gate’s too</p>;'])).toEqual(
      ['1 prose <p>']
    );
  });
});

describe('the tolerated list keys a file by its count per kind, so it only shrinks', () => {
  const site = (kind: Kind, line: number): Site => ({ kind, line, what: '<p>' });
  const list: Record<string, Tolerated> = {
    'app/a.tsx': { family: 'chain', sites: { prose: 2 }, reason: 'notes and a lede to sort' },
  };

  it('passes the same count on other lines', () => {
    expect(
      toleratedFailures(new Map([['app/a.tsx', [site('prose', 3), site('prose', 9)]]]), list)
    ).toEqual([]);
  });

  it('fails a new site in a listed file or an unlisted one', () => {
    expect(
      toleratedFailures(
        new Map([
          ['app/a.tsx', [site('prose', 3), site('prose', 9), site('link', 12)]],
          ['app/b.tsx', [site('td', 4)]],
        ]),
        list
      )
    ).toEqual([
      expect.stringMatching(
        /^app\/a\.tsx: 1 link site\(s\) in the quiet ink, 0 tolerated — a new one: 12 <p>/
      ),
      expect.stringMatching(
        /^app\/b\.tsx: 1 td site\(s\) in the quiet ink, 0 tolerated — a new one: 4 <p>/
      ),
    ]);
  });

  it('fails a site that went until its count is lowered', () => {
    expect(toleratedFailures(new Map([['app/a.tsx', [site('prose', 3)]]]), list)).toEqual([
      'app/a.tsx: 1 prose site(s) in the quiet ink, 2 tolerated — lower its count to 1',
    ]);
    expect(toleratedFailures(new Map(), list)).toEqual([
      'app/a.tsx: 0 prose site(s) in the quiet ink, 2 tolerated — lower its count to 0',
    ]);
  });
});

describe('the ink of a word’s role (R3): links, amounts, table cells and paragraphs', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const found = new Map<string, Site[]>();
  const quiet = new Map<string, QuietSite[]>();
  for (const file of files) {
    const scanned = scanInkRoles(
      readFileSync(file, 'utf8'),
      file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
    );
    if (scanned.sites.length) found.set(relative(root, file), scanned.sites);
    if (scanned.quiet.length) quiet.set(relative(root, file), scanned.quiet);
  }

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(250);
  });

  // Review of the fixes (2026-11-19, N4): the gate read `app` and `components` only, while the
  // scan gate reads `hooks` and `lib` too (a `.tsx` there draws words as well).
  it('reads the directories the scan gate reads, every source in them', () => {
    expect(SCAN_DIRS).toEqual(['app', 'components', 'hooks', 'lib']);
    expect(new Set(files.map((file) => relative(root, file).split(sep)[0]))).toEqual(
      new Set(SCAN_DIRS)
    );
  });

  // Review of the fixes (2026-11-19, N4): the pin was per file, so the list could grow by a file
  // without a second look at the whole; its total is pinned too.
  it('sets back three elements in all: the pinned total of QUIET_SITES', () => {
    expect(Object.values(QUIET_SITES).reduce((total, entry) => total + entry.count, 0)).toBe(3);
  });

  // Re-pinned on purpose (BRIEF-ink-size-roles item 1, the inventory: its figures, states and sentences in the reading ink; renamed, it was "107 sites in 51 files").
  // Re-pinned on purpose (BRIEF-ink-size-roles item 1, the script pages: their cells' states and dashes and their empty lines in the reading ink, their loading lines decided; renamed, it was "99 sites in 49 files").
  // Re-pinned on purpose (lane P2's third round, item 10, at its merge; renamed, it was "… 123 sites
  // in 58 files …" on the base): the script hero's companion line left the list. Re-pinned on
  // purpose (the third read's T NOTE-3; renamed, it was "122 sites in 57 files"): the decided
  // sites left the work list for DECIDED, 15 in 6 files. Re-pinned on purpose (the ink and size
  // round, at the merge of its five lanes; renamed, it was "107 sites in 51 files"): the chain
  // family's 28 sites left the list, 25 in the reading ink and 3 decided (lane P1); the assets
  // family's 33 in 15 files read in the reading ink (lane P2); the lists family's 23 left the list
  // (lane P3); the topics and shared families' 23 in 17 files (lane cd-fixes-controls, 18 converted
  // and 5 decided). No family tolerates a site
  it('tolerates 0 sites in 0 files: the pinned totals by family, which only fall', () => {
    const byFamily: Partial<Record<Family, number>> = {};
    for (const entry of Object.values(TOLERATED))
      byFamily[entry.family] =
        (byFamily[entry.family] ?? 0) +
        Object.values(entry.sites).reduce((sum, n) => sum + (n ?? 0), 0);
    // a family at 0 has no entry left to count (lane P1, the ink and size roles: chain the first),
    // as the size-role gate reads its own
    const pinned = Object.fromEntries(Object.entries(TOLERATED_BY_FAMILY).filter(([, n]) => n > 0));
    expect(byFamily).toEqual(pinned);
    expect(Object.keys(TOLERATED)).toHaveLength(TOLERATED_FILES);
  });

  // Re-pinned on purpose (the ink and size round, at the merge of its five lanes; renamed, it was
  // "15 in 6 files"): the chain family's three decided sites (lane P1: an uncle row's label, the
  // witness list's hint and the structure's key) and the shared lane's five (lane
  // cd-fixes-controls) joined; the lists family's decided sites are its loading lines alone (lane P3).
  // Re-pinned on purpose (the lead's follow-up to lane P3: a loading line is a sentence, the reading
  // ink; renamed, it was "19 in 14 files"): the lists family's 11 loading lines left, none of the
  // lists family's sites is decided
  it('holds the decided sites apart, 8 in 7 files, none of them also on the work list', () => {
    const sites = Object.values(DECIDED).reduce(
      (sum, entry) => sum + Object.values(entry.sites).reduce((n, k) => n + (k ?? 0), 0),
      0
    );
    expect(sites).toBe(DECIDED_SITES);
    expect(Object.keys(DECIDED)).toHaveLength(DECIDED_FILES);
    expect(Object.keys(DECIDED).filter((path) => path in TOLERATED)).toEqual([]);
  });

  it('draws no link, amount, table cell or paragraph in the quiet ink but the tolerated list’s', () => {
    expect(toleratedFailures(found, { ...TOLERATED, ...DECIDED })).toEqual([]);
  });

  it('sets back no element with the quiet class but the pinned list’s', () => {
    expect(quietFailures(quiet, QUIET_SITES)).toEqual([]);
  });
});
