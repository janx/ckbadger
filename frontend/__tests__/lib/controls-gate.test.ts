import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the controls (UI review 2026-11-14, phase C, lane C1; the controls board the owner
 * passed on 2026-11-19, gate D0′): a tab row, a filter, a range, a pager, a text field, a select, a
 * copy button and a help mark are each one component of `components/ui/` (CONTROL_FILES). Outside
 * them no page or component writes the raw form of one. Four kinds can be told from the source:
 *
 * - `select`: a `<select>`;
 * - `text-input`: an `<input>` whose type is `text` or `search`, or that has no type (a text field
 *   by default);
 * - `tab`: an element with the role `tab` or `tablist`, or one drawn as a tab: a class of the
 *   tab's 2 px underline (`border-b-2`, at any variant), the look every hand-built tab row of the
 *   site shares;
 * - `pager`: a use of `CursorPagination` (the pager `components/ui/pager.tsx` replaces), or a
 *   hand-built one: a `<button>`, an `<a>` or a `Link` / `AppLink` whose words are a pager's —
 *   `← prev`, `previous`, `newer`, `next →`, `older →`, `load more`, `show all …`, `next 50 ↓` —
 *   read from its text, its string literals, both branches of a `?:` and the fixed parts of a
 *   template literal.
 *
 * A tag is read where the file writes it: an intrinsic tag, or one held in a variable of the file
 * (`const Tag = 'select'`, the default of an `as` prop), read as every tag it can be. The type of
 * an `<input>` and a `role` are read through string literals, `?:`, `&&` and the file's own
 * constants. What it cannot read it refuses by name, so a control it does not understand fails
 * the gate instead of passing it: an `<input>` whose props are spread with no literal type after
 * them, a type or a role it cannot resolve, a tag variable with no value it can read (`TAGS[k]`),
 * an `<input>` made by `createElement` without a literal type. A tag variable holding a component
 * (an import, a function, `memo(…)`, a prop with no default, an object's property such as
 * `options.loading`) is a component, not a raw control.
 *
 * Limits, the routes a source gate cannot see: a pager whose words come from a variable or a prop
 * of a component other than `CursorPagination` (`{label}`), a pager drawn by an element other than
 * a button or a link (a `div` with `onClick`), a list that loads more on scroll with no control,
 * numbered page buttons with no word; a tab row drawn without a role and without the 2 px
 * underline; a `role` set in an object spread into an element (the source's object `role:` keys
 * are domain words: `'holdings-nav'`, `'Fiber funding'`); a tag passed through an `as` prop is
 * read as its default only (the prop's other values are its callers'); an `<input>` of another
 * text type (`email`, `number`, …) and a `<textarea>` (the site has none) are not counted.
 *
 * The sites tolerated today are listed per file with their count per kind, the C2 lane that takes
 * them and a reason: this is the work list of the four placement lanes (c2-lists, c2-chain,
 * c2-assets, c2-topics) and of the shell's two fields. A new site fails (its file's count is
 * exceeded, or the file is not listed), and so does a site that went (its count must be lowered),
 * so the list only shrinks; its total is pinned too.
 */

type Kind = 'select' | 'text-input' | 'tab' | 'pager';
const KINDS: readonly Kind[] = ['select', 'text-input', 'tab', 'pager'];

/**
 * The placement lane a file's sites go to (UI plan, phase C), the shell's own fields (the header's
 * search field and the command palette: no C2 lane names them), or `shared`: the old pager itself.
 */
type Lane = 'c2-lists' | 'c2-chain' | 'c2-assets' | 'c2-topics' | 'shell' | 'shared';

interface Tolerated {
  lane: Lane;
  /** The sites of each kind in the file, today. */
  sites: Partial<Record<Kind, number>>;
  /** What they are, in one line. */
  reason: string;
}

/** The control components (lane C1): the only files that may write a raw control. */
const CONTROL_FILES = [
  'components/ui/tabs.tsx',
  'components/ui/filter-chips.tsx',
  'components/ui/range-select.tsx',
  'components/ui/pager.tsx',
  'components/ui/text-field.tsx',
  'components/ui/select.tsx',
  'components/ui/copy-button.tsx',
  'components/ui/help-mark.tsx',
] as const;

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- c2-lists: activities, transactions, blocks, inventory, scripts, depositors ----
  'app/blocks/page.tsx': {
    lane: 'c2-lists',
    sites: { pager: 1 },
    reason: 'the blocks list’s cursor pager (newer / older)',
  },
  'app/dao/depositors/page.tsx': {
    lane: 'c2-lists',
    sites: { pager: 1 },
    reason: 'the depositors list’s cursor pager',
  },
  'app/inventory/inventory-page-client.tsx': {
    lane: 'c2-lists',
    sites: { select: 2, 'text-input': 1, tab: 1, pager: 1 },
    reason:
      'the standard filter’s phone select and the sort select, the name search, the class tabs (links), the pager',
  },
  'app/scripts/page.tsx': {
    lane: 'c2-lists',
    sites: { select: 1, 'text-input': 1, tab: 1, pager: 1 },
    reason: 'the sort select, the name search, the role tabs, the pager',
  },
  'app/transactions/page.tsx': {
    lane: 'c2-lists',
    sites: { select: 1, tab: 1, pager: 2 },
    reason: 'the kind select, the committed / pool tabs, the two lists’ pagers',
  },
  'components/activities-stream-explorer.tsx': {
    lane: 'c2-lists',
    sites: { pager: 1 },
    reason: 'the activities stream’s boxed “Older” button',
  },
  // ---- c2-chain: block, transaction, cell, address, script pages ----
  'app/address/[addr]/client-page.tsx': {
    lane: 'c2-chain',
    sites: { tab: 1 },
    reason: 'the activities / transactions / live cells tabs (buttons, aria-pressed)',
  },
  'app/blocks/[id]/client-page.tsx': {
    lane: 'c2-chain',
    sites: { tab: 1, pager: 2 },
    reason: 'the transactions / activities tabs and their two pagers',
  },
  'app/scripts/[name]/client-page.tsx': {
    lane: 'c2-chain',
    sites: { pager: 1 },
    reason: 'the script version’s cells pager',
  },
  'components/address/address-holdings.tsx': {
    lane: 'c2-chain',
    sites: { pager: 2 },
    reason: 'the holdings lists’ pager and the objects list’s',
  },
  'components/scripts/script-family-page.tsx': {
    lane: 'c2-chain',
    sites: { pager: 1 },
    reason: 'a script family’s cells pager',
  },
  'components/scripts/script-newest-cells.tsx': {
    lane: 'c2-chain',
    sites: { pager: 1 },
    reason: 'the newest cells’ “All N cells ↓”: the short list’s form, or a way onward',
  },
  // ---- c2-assets: token, cluster, class, objects, identities, their feeds ----
  'app/tokens/[typeHash]/client-page.tsx': {
    lane: 'c2-assets',
    sites: { pager: 1 },
    reason: 'the supply events’ pager',
  },
  'components/asset/asset-feed.tsx': {
    lane: 'c2-assets',
    sites: { tab: 2, pager: 1 },
    reason: 'the feed’s tablist and its tabs, the feed’s pager',
  },
  'components/asset/collection-gallery.tsx': {
    lane: 'c2-assets',
    sites: { select: 1, 'text-input': 1, pager: 1 },
    reason: 'the gallery’s name search, its sort select and its pager',
  },
  'components/asset/collection-holders.tsx': {
    lane: 'c2-assets',
    sites: { pager: 1 },
    reason: 'a collection’s holders pager',
  },
  'components/asset/collection-objects.tsx': {
    lane: 'c2-assets',
    sites: { pager: 1 },
    reason: 'a collection’s objects pager',
  },
  'components/asset/item-evidence.tsx': {
    lane: 'c2-assets',
    sites: { select: 1 },
    reason: 'the evidence’s version select',
  },
  'components/asset/token-holders.tsx': {
    lane: 'c2-assets',
    sites: { pager: 1 },
    reason: 'a token’s holders pager',
  },
  // ---- c2-topics: dao, forks, fiber, network, hardforks, home, charts ----
  'app/forks/page.tsx': {
    lane: 'c2-topics',
    sites: { pager: 2 },
    reason: 'the branch switches’ “Show all … ↓” and their pager',
  },
  'app/network/nodes-table.tsx': {
    lane: 'c2-topics',
    sites: { select: 2, 'text-input': 2, pager: 1 },
    reason: 'the nodes table’s two filter selects, country and version fields, and “Load more”',
  },
  'components/home/activity-feed.tsx': {
    lane: 'c2-topics',
    sites: { tab: 1 },
    reason: 'the home feed’s tabs',
  },
  // ---- shell: the header's search field and the command palette ----
  'components/command-palette.tsx': {
    lane: 'shell',
    sites: { 'text-input': 1 },
    reason: 'the palette’s query field (a combobox of its own)',
  },
  'components/search-bar.tsx': {
    lane: 'shell',
    sites: { 'text-input': 1 },
    reason: 'the header’s search field (a combobox with its own cursor): the well TextField copies',
  },
  // ---- shared: the old pager ----
  'components/ui/cursor-pagination.tsx': {
    lane: 'shared',
    sites: { pager: 2 },
    reason: 'the old pager’s previous / next buttons: it goes with its last use (the orphan gate)',
  },
};

/** The pinned total of TOLERATED's sites: the list may only shrink. */
const TOLERATED_TOTAL = 48;

const SCAN_DIRS = ['app', 'components', 'hooks', 'lib'] as const;

interface Site {
  kind: Kind;
  line: number;
  what: string;
}

interface Scan {
  sites: Site[];
  /** What the scan could not read, as `line: why`. */
  refused: string[];
}

/** A pager's words, on an element's text with its spaces collapsed (`…` for what an expression writes). */
const PAGER_WORDS: readonly RegExp[] = [
  /^(← ?)?(prev|previous|newer)$/i,
  /^(next|older)( ?→)?$/i,
  /^load more$/i,
  /^show all\b/i,
  /^all \S.* ↓$/i,
  /^next (\d[\d,]*|…)( ↓)?$/i,
];

/** The elements a pager's words make a hand-built pager: a button or a link. */
const PAGER_TAGS = new Set(['button', 'a', 'Link', 'AppLink']);

/** At most this many readings of one element's text (a `?:` in each of several children). */
const TEXT_READINGS = 32;

/** What a name of the file is bound to: a value it may hold, a component, or a prop. */
type Binding =
  | { kind: 'value'; values: ts.Expression[] }
  | { kind: 'component' }
  | { kind: 'prop'; defaults: ts.Expression[] };

const unwrap = (e: ts.Expression): ts.Expression => {
  let x = e;
  while (
    ts.isParenthesizedExpression(x) ||
    ts.isAsExpression(x) ||
    ts.isSatisfiesExpression(x) ||
    ts.isTypeAssertionExpression(x) ||
    ts.isNonNullExpression(x)
  )
    x = x.expression;
  return x;
};

/** The controls a source writes by hand, and what it could not read. */
function scanControls(code: string, scriptKind: ts.ScriptKind = ts.ScriptKind.TSX): Scan {
  const source = ts.createSourceFile('scan.tsx', code, ts.ScriptTarget.Latest, true, scriptKind);
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const sites: Site[] = [];
  const refused: string[] = [];
  const refuse = (n: ts.Node, why: string) => refused.push(`${lineOf(n)}: ${why}`);

  // the file's names: what each may hold
  const bindings = new Map<string, Binding[]>();
  const bind = (name: string, b: Binding) => bindings.set(name, [...(bindings.get(name) ?? []), b]);
  const collect = (node: ts.Node) => {
    if (ts.isImportClause(node) && node.name) bind(node.name.text, { kind: 'component' });
    if (ts.isImportSpecifier(node) || ts.isNamespaceImport(node))
      bind(node.name.text, { kind: 'component' });
    if ((ts.isFunctionDeclaration(node) || ts.isClassDeclaration(node)) && node.name)
      bind(node.name.text, { kind: 'component' });
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)) {
      const init = node.initializer && unwrap(node.initializer);
      if (
        !init ||
        ts.isArrowFunction(init) ||
        ts.isFunctionExpression(init) ||
        ts.isClassExpression(init) ||
        ts.isCallExpression(init) ||
        ts.isNewExpression(init) ||
        ts.isJsxElement(init) ||
        ts.isJsxSelfClosingElement(init)
      )
        bind(node.name.text, { kind: 'component' });
      else bind(node.name.text, { kind: 'value', values: [init] });
    }
    if ((ts.isBindingElement(node) || ts.isParameter(node)) && ts.isIdentifier(node.name))
      bind(node.name.text, {
        kind: 'prop',
        defaults: node.initializer ? [node.initializer] : [],
      });
    ts.forEachChild(node, collect);
  };
  collect(source);

  /** The strings an expression can be (null where it cannot be read), through the file's names. */
  const strings = (e: ts.Expression, seen: ReadonlySet<string> = new Set()): string[] | null => {
    const x = unwrap(e);
    if (ts.isStringLiteral(x) || ts.isNoSubstitutionTemplateLiteral(x)) return [x.text];
    if (x.kind === ts.SyntaxKind.NullKeyword || x.kind === ts.SyntaxKind.FalseKeyword) return [];
    if (ts.isIdentifier(x) && x.text === 'undefined') return [];
    if (ts.isConditionalExpression(x)) {
      const a = strings(x.whenTrue, seen);
      const b = strings(x.whenFalse, seen);
      return a && b ? [...a, ...b] : null;
    }
    if (ts.isBinaryExpression(x)) {
      const op = x.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return strings(x.right, seen);
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken) {
        const a = strings(x.left, seen);
        const b = strings(x.right, seen);
        return a && b ? [...a, ...b] : null;
      }
      return null;
    }
    if (ts.isIdentifier(x)) {
      if (seen.has(x.text)) return null;
      const found = bindings.get(x.text);
      if (!found) return null;
      const out: string[] = [];
      for (const b of found) {
        const values = b.kind === 'value' ? b.values : b.kind === 'prop' ? b.defaults : null;
        if (!values || values.length === 0) return null;
        for (const v of values) {
          const s = strings(v, new Set([...seen, x.text]));
          if (s === null) return null;
          out.push(...s);
        }
      }
      return out;
    }
    return null;
  };

  /** Every word of a class expression: its string pieces, through `cn(…)`, `?:`, templates and names. */
  const classTokens = (e: ts.Expression, seen: ReadonlySet<string> = new Set()): string[] => {
    const out: string[] = [];
    const visit = (n: ts.Node) => {
      if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) out.push(n.text);
      else if (ts.isTemplateHead(n) || ts.isTemplateMiddle(n) || ts.isTemplateTail(n))
        out.push(n.text);
      else if (ts.isIdentifier(n) && !seen.has(n.text))
        for (const b of bindings.get(n.text) ?? [])
          if (b.kind === 'value')
            for (const v of b.values) out.push(...classTokens(v, new Set([...seen, n.text])));
      ts.forEachChild(n, visit);
    };
    visit(e);
    return out.flatMap((s) => s.split(/\s+/)).filter(Boolean);
  };

  /**
   * The intrinsic tags a name used as a tag can hold ([] where it holds only components), null
   * where it cannot be read. A component is a name the file imports or declares as a function, a
   * class, a call's result (`memo`, `lazy`, `dynamic`), a prop with no default, a name it does not
   * declare, or an object's property (`options.loading`).
   */
  const tagValues = (e: ts.Expression, seen: ReadonlySet<string>): string[] | null => {
    const x = unwrap(e);
    if (ts.isStringLiteral(x) || ts.isNoSubstitutionTemplateLiteral(x)) return [x.text];
    if (ts.isConditionalExpression(x)) {
      const a = tagValues(x.whenTrue, seen);
      const b = tagValues(x.whenFalse, seen);
      return a && b ? [...a, ...b] : null;
    }
    if (ts.isPropertyAccessExpression(x)) return [];
    if (!ts.isIdentifier(x) || seen.has(x.text)) return null;
    const found = bindings.get(x.text);
    if (!found) return [];
    const out: string[] = [];
    for (const b of found) {
      if (b.kind === 'component' || (b.kind === 'prop' && b.defaults.length === 0)) continue;
      for (const v of b.kind === 'value' ? b.values : b.defaults) {
        const s = tagValues(v, new Set([...seen, x.text]));
        if (s === null) return null;
        out.push(...s);
      }
    }
    return out;
  };

  /** The intrinsic tags a JSX tag can be: [] for a component, null where it cannot be read. */
  const tagsOf = (tag: ts.JsxTagNameExpression): string[] | null => {
    if (!ts.isIdentifier(tag)) return [];
    if (/^[a-z]/.test(tag.text)) return [tag.text];
    return tagValues(tag, new Set());
  };

  /** The readings of an element's text: its words, `…` for what an expression writes. */
  const textsOf = (children: ts.NodeArray<ts.JsxChild>): string[] => {
    let readings = [''];
    const append = (alternatives: string[]) => {
      readings = readings.flatMap((r) => alternatives.map((a) => r + a)).slice(0, TEXT_READINGS);
    };
    const pieces = (e: ts.Expression): string[] => {
      const x = unwrap(e);
      if (ts.isTemplateExpression(x))
        return [x.head.text + x.templateSpans.map((s) => `…${s.literal.text}`).join('')];
      if (ts.isConditionalExpression(x)) return [...pieces(x.whenTrue), ...pieces(x.whenFalse)];
      if (
        ts.isBinaryExpression(x) &&
        x.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken
      )
        return [...pieces(x.right), ''];
      return strings(x) ?? ['…'];
    };
    for (const child of children) {
      if (ts.isJsxText(child)) append([child.text]);
      else if (ts.isJsxExpression(child)) {
        if (child.expression) append(pieces(child.expression));
      } else if (ts.isJsxElement(child)) append(textsOf(child.children));
      else if (ts.isJsxFragment(child)) append(textsOf(child.children));
    }
    return readings.map((r) => r.replace(/\s+/g, ' ').trim());
  };

  const visitElement = (
    node: ts.JsxOpeningElement | ts.JsxSelfClosingElement,
    children: ts.NodeArray<ts.JsxChild> | null
  ) => {
    const tags = tagsOf(node.tagName);
    const name = node.tagName.getText(source);
    if (tags === null) {
      refuse(node, `a tag held in ${name}, with no value the scan can read`);
      return;
    }
    const shown = tags.length ? tags.join('|') : name;
    const attrs = node.attributes.properties;
    const attr = (key: string) =>
      attrs.find(
        (a): a is ts.JsxAttribute =>
          ts.isJsxAttribute(a) && ts.isIdentifier(a.name) && a.name.text === key
      );
    const valueOf = (a: ts.JsxAttribute): ts.Expression | null =>
      !a.initializer
        ? null
        : ts.isStringLiteral(a.initializer)
          ? a.initializer
          : ts.isJsxExpression(a.initializer) && a.initializer.expression
            ? a.initializer.expression
            : null;

    if (tags.includes('select'))
      sites.push({ kind: 'select', line: lineOf(node), what: `<${shown}>` });

    if (tags.includes('input')) {
      const type = attr('type');
      const lastSpread = attrs.reduce((at, a, i) => (ts.isJsxSpreadAttribute(a) ? i : at), -1);
      if (lastSpread >= 0 && (!type || attrs.indexOf(type) < lastSpread))
        refuse(node, 'an <input> with spread props and no literal type after them');
      else if (!type) sites.push({ kind: 'text-input', line: lineOf(node), what: `<${shown}>` });
      else {
        const v = valueOf(type);
        const types = v && strings(v);
        if (!types) refuse(node, `the type of an <input>: ${v ? v.getText(source) : '(none)'}`);
        else if (types.some((t) => t === 'text' || t === 'search'))
          sites.push({
            kind: 'text-input',
            line: lineOf(node),
            what: `<${shown} type="${types.join('|')}">`,
          });
      }
    }

    let tab = false;
    const role = attr('role');
    if (role) {
      const v = valueOf(role);
      const roles = v && strings(v);
      if (!roles) refuse(node, `a role: ${v ? v.getText(source) : '(none)'}`);
      else {
        const named = roles.find((r) => r === 'tab' || r === 'tablist');
        if (named) {
          tab = true;
          sites.push({ kind: 'tab', line: lineOf(node), what: `<${shown} role="${named}">` });
        }
      }
    }
    const cls = attr('className');
    const clsValue = cls && valueOf(cls);
    if (
      !tab &&
      clsValue &&
      classTokens(clsValue).some((t) => t.split(':').pop()!.replace(/^!/, '') === 'border-b-2')
    )
      sites.push({ kind: 'tab', line: lineOf(node), what: `<${shown} class="border-b-2">` });

    if (name === 'CursorPagination')
      sites.push({ kind: 'pager', line: lineOf(node), what: '<CursorPagination>' });
    else if (children && (tags.some((t) => PAGER_TAGS.has(t)) || PAGER_TAGS.has(name))) {
      const word = textsOf(children).find((t) => PAGER_WORDS.some((re) => re.test(t)));
      if (word !== undefined)
        sites.push({ kind: 'pager', line: lineOf(node), what: `<${shown}>${word}</${shown}>` });
    }
  };

  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node)) visitElement(node.openingElement, node.children);
    else if (ts.isJsxSelfClosingElement(node)) visitElement(node, null);
    else if (ts.isCallExpression(node)) {
      const callee = node.expression;
      const named =
        (ts.isIdentifier(callee) && callee.text === 'createElement') ||
        (ts.isPropertyAccessExpression(callee) && callee.name.text === 'createElement');
      const first = node.arguments[0];
      if (named && first && ts.isStringLiteralLike(first)) {
        if (first.text === 'select')
          sites.push({ kind: 'select', line: lineOf(node), what: 'createElement(select)' });
        if (first.text === 'input') {
          const props = node.arguments[1] && unwrap(node.arguments[1]);
          const type =
            props && ts.isObjectLiteralExpression(props)
              ? props.properties.find(
                  (p): p is ts.PropertyAssignment =>
                    ts.isPropertyAssignment(p) && p.name.getText(source) === 'type'
                )
              : undefined;
          const types = type && strings(type.initializer);
          if (!types) refuse(node, 'an <input> made by createElement with no literal type');
          else if (types.some((t) => t === 'text' || t === 'search'))
            sites.push({
              kind: 'text-input',
              line: lineOf(node),
              what: `createElement(input type="${types.join('|')}")`,
            });
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { sites, refused };
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
  for (const path of [...new Set([...found.keys(), ...Object.keys(tolerated)])].sort()) {
    const sites = found.get(path) ?? [];
    const entry = tolerated[path];
    for (const kind of KINDS) {
      const of = sites.filter((s) => s.kind === kind);
      const allowed = entry?.sites[kind] ?? 0;
      if (of.length > allowed)
        out.push(
          `${path}: ${of.length} raw ${kind} site(s), ${allowed} tolerated — a new one: ${of
            .map((s) => `${s.line} ${s.what}`)
            .join(', ')} (draw it with its component in components/ui/)`
        );
      if (of.length < allowed)
        out.push(
          `${path}: ${of.length} raw ${kind} site(s), ${allowed} tolerated — lower its count to ${of.length}`
        );
    }
    if (entry && entry.reason.trim().length <= 10)
      out.push(`${path}: a reason too short to say why: "${entry.reason}"`);
  }
  return out;
}

const kinds = (code: string) => scanControls(code).sites.map((s) => `${s.kind} ${s.what}`);

describe('the controls scan finds what it must, and only that', () => {
  it('finds a raw select, and a text field typed text, search or not at all', () => {
    expect(
      kinds(`
        export const A = () => (
          <form>
            <select value="a" onChange={() => {}}><option>a</option></select>
            <input type="text" />
            <input type={'search'} />
            <input value="x" readOnly />
          </form>
        );`)
    ).toEqual([
      'select <select>',
      'text-input <input type="text">',
      'text-input <input type="search">',
      'text-input <input>',
    ]);
  });

  it('passes an input of another type, and one a constant or a branch types as one', () => {
    expect(
      kinds(`
        const KIND = 'checkbox';
        export const A = ({ range }: { range: boolean }) => (
          <div>
            <input type="checkbox" />
            <input type={KIND} />
            <input type={range ? 'range' : 'number'} />
            <input {...{ id: 'a' }} type="hidden" />
          </div>
        );`)
    ).toEqual([]);
    expect(
      kinds(`
        const KIND = 'search';
        export const A = ({ plain }: { plain: boolean }) => (
          <div>
            <input type={KIND} />
            <input type={plain ? 'text' : 'checkbox'} />
          </div>
        );`)
    ).toEqual(['text-input <input type="search">', 'text-input <input type="text|checkbox">']);
  });

  it('finds a role of tab or tablist, and the tab underline drawn by hand', () => {
    expect(
      kinds(`
        const TAB = 'text-section border-b-2 pb-2';
        export const A = ({ on }: { on: boolean }) => (
          <div>
            <div role="tablist"><button role={on ? 'tab' : undefined}>a</button></div>
            <button className={TAB}>b</button>
            <a className={\`text-body max-sm:border-b-2 \${on ? 'border-jade' : ''}\`}>c</a>
            <span role="status" className="border-b">d</span>
          </div>
        );`)
    ).toEqual([
      'tab <div role="tablist">',
      'tab <button role="tab">',
      'tab <button class="border-b-2">',
      'tab <a class="border-b-2">',
    ]);
  });

  it('finds CursorPagination and a pager written by hand, and passes other words', () => {
    expect(
      kinds(`
        export const A = ({ busy, n }: { busy: boolean; n: number }) => (
          <div>
            <CursorPagination hasMore hasPrevious={false} onNext={() => {}} onPrevious={() => {}} />
            <button>← Prev</button>
            <button>Next →</button>
            <AppLink href="/b">older →</AppLink>
            <button>{busy ? 'Loading…' : 'Load more'}</button>
            <button>{\`Show all \${n} branch switches ↓\`}</button>
            <a href="#">next 50 ↓</a>
            <button>How it works<span> ↓</span></button>
            <button>{n} transactions ↓</button>
            <button>Previous block</button>
            <Link href="/x">fewer ↑</Link>
          </div>
        );`)
    ).toEqual([
      'pager <CursorPagination>',
      'pager <button>← Prev</button>',
      'pager <button>Next →</button>',
      'pager <AppLink>older →</AppLink>',
      'pager <button>Load more</button>',
      'pager <button>Show all … branch switches ↓</button>',
      'pager <a>next 50 ↓</a>',
    ]);
  });

  it('reads a tag held in a variable as every tag it can be', () => {
    expect(
      kinds(`
        const Field = 'select';
        export function A({ as: Tag = 'input', wide }: { as?: 'input' | 'div'; wide: boolean }) {
          const Box = wide ? 'div' : 'select';
          return <div><Field /><Tag type="search" /><Box /></div>;
        }`)
    ).toEqual(['select <select>', 'text-input <input type="search">', 'select <div|select>']);
  });

  it('refuses what it cannot read', () => {
    const scan = scanControls(`
      import { kindOf, roleOf, Tag as Imported } from './x';
      export function A(props: { kind: string; role: string; rest: object }) {
        const Unknown = TAGS[props.kind];
        return (
          <div>
            <input {...props.rest} />
            <input type="text" {...props.rest} />
            <input type={props.kind} />
            <input type={kindOf()} />
            <div role={props.role} />
            <div role={roleOf()} />
            <Unknown />
            <Imported />
            {createElement('input', props.rest)}
          </div>
        );
      }`);
    expect(scan.refused).toEqual([
      expect.stringMatching(/^7: an <input> with spread props and no literal type after them/),
      expect.stringMatching(/^8: an <input> with spread props and no literal type after them/),
      expect.stringMatching(/^9: the type of an <input>: props\.kind/),
      expect.stringMatching(/^10: the type of an <input>: kindOf\(\)/),
      expect.stringMatching(/^11: a role: props\.role/),
      expect.stringMatching(/^12: a role: roleOf\(\)/),
      expect.stringMatching(/^13: a tag held in Unknown, with no value the scan can read/),
      expect.stringMatching(/^15: an <input> made by createElement with no literal type/),
    ]);
  });

  it('counts createElement of a select or a typed input', () => {
    expect(
      kinds(`
        export const A = () => [
          createElement('select', null),
          React.createElement('input', { type: 'search' }),
          createElement('input', { type: 'checkbox' }),
          document.createElement('style'),
        ];`)
    ).toEqual(['select createElement(select)', 'text-input createElement(input type="search")']);
  });
});

describe('the tolerated list keys a file by its count per kind, so it only shrinks', () => {
  const site = (kind: Kind, line: number): Site => ({ kind, line, what: '<select>' });
  const list: Record<string, Tolerated> = {
    'app/a.tsx': { lane: 'c2-lists', sites: { select: 2 }, reason: 'the sort and the filter' },
  };

  it('passes the same count on other lines', () => {
    expect(
      toleratedFailures(new Map([['app/a.tsx', [site('select', 3), site('select', 9)]]]), list)
    ).toEqual([]);
  });

  it('fails a new site in a listed file or an unlisted one', () => {
    expect(
      toleratedFailures(
        new Map([
          ['app/a.tsx', [site('select', 3), site('select', 9), site('pager', 12)]],
          ['app/b.tsx', [site('tab', 4)]],
        ]),
        list
      )
    ).toEqual([
      expect.stringMatching(/^app\/a\.tsx: 1 raw pager site\(s\), 0 tolerated — a new one: 12 /),
      expect.stringMatching(/^app\/b\.tsx: 1 raw tab site\(s\), 0 tolerated — a new one: 4 /),
    ]);
  });

  it('fails a site that went until its count is lowered', () => {
    expect(toleratedFailures(new Map([['app/a.tsx', [site('select', 3)]]]), list)).toEqual([
      'app/a.tsx: 1 raw select site(s), 2 tolerated — lower its count to 1',
    ]);
  });
});

describe('no raw control outside the control components (lane C1)', () => {
  const root = join(__dirname, '..', '..');
  const rel = (file: string) => relative(root, file).split(sep).join('/');
  const files = sourceFiles(root);
  const found = new Map<string, Site[]>();
  const refused: string[] = [];
  for (const file of files) {
    const path = rel(file);
    if ((CONTROL_FILES as readonly string[]).includes(path)) continue;
    const scan = scanControls(
      readFileSync(file, 'utf8'),
      file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
    );
    if (scan.sites.length) found.set(path, scan.sites);
    refused.push(...scan.refused.map((r) => `${path}:${r}`));
  }

  it('scans the source, and every control file is there', () => {
    expect(files.length).toBeGreaterThan(300);
    for (const path of CONTROL_FILES) expect(existsSync(join(root, path)), path).toBe(true);
  });

  it('reads every control it meets', () => {
    expect(refused).toEqual([]);
  });

  it('writes no raw control but the tolerated list’s', () => {
    expect(toleratedFailures(found, TOLERATED)).toEqual([]);
  });

  it(`tolerates ${TOLERATED_TOTAL} sites in all: the pinned total, which only falls`, () => {
    const total = Object.values(TOLERATED).reduce(
      (sum, entry) => sum + Object.values(entry.sites).reduce((a, n) => a + (n ?? 0), 0),
      0
    );
    expect(total).toBe(TOLERATED_TOTAL);
  });
});

// The proof page (lane C1): the controls mounted in every state of the board, for a real browser,
// served by the dev server and never built (proof/controls.html). Its entry is no input of the
// build, it is linted with the app, and it mounts every control but the help mark, which
// HelpPopover draws.
describe('the controls’ proof page is the dev server’s alone', () => {
  const root = join(__dirname, '..', '..');

  it('loads its own module from its own page', () => {
    const html = readFileSync(join(root, 'proof/controls.html'), 'utf8');
    expect([...html.matchAll(/<script type="module" src="([^"]+)">/g)].map((m) => m[1])).toEqual([
      '/proof/controls.tsx',
    ]);
  });

  it('is no input of the build, which builds index.html alone, and is linted with the app', () => {
    // Vite's build takes index.html when its config names no other input (build.rollupOptions)
    expect(readFileSync(join(root, 'vite.config.ts'), 'utf8')).not.toMatch(
      /rollupOptions|\binput\s*:|proof/
    );
    expect(readFileSync(join(root, 'package.json'), 'utf8')).toContain(
      '"lint": "eslint app components hooks lib src proof"'
    );
  });

  it('mounts every control of the library', () => {
    const code = readFileSync(join(root, 'proof/controls.tsx'), 'utf8');
    const imported = [...code.matchAll(/from '@\/(components\/ui\/[a-z-]+)'/g)].map(
      (m) => `${m[1]}.tsx`
    );
    expect(CONTROL_FILES.filter((path) => !imported.includes(path))).toEqual([
      'components/ui/help-mark.tsx',
    ]);
    expect(imported).toContain('components/ui/help-popover.tsx');
  });
});
