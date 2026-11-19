import { existsSync, readFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { sourceFiles, unwrap, type Binding } from '@/__tests__/utils/source-scan';

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
 * Three more kinds it reads since lane c2-finish (item 5), so "one component per control" is held
 * here, not in a brief:
 *
 * - `choice`: an element that says it is pressed (`aria-pressed`: a toggle; a group of them is a
 *   filter or a range), or the role `radiogroup` or `radio` (the filter chips' own form);
 * - `clipboard`: a clipboard write by hand, `navigator.clipboard` or a call of the one copy path's
 *   `useCopy` (the copy button is the library's; hooks/useCopy.ts is its own path);
 * - `range`: an `<input type="range">`, or a button whose words are a span of time (`24 h`, `14 d`,
 *   `1 y`, `90 days`: the RangeSelect's choices).
 *
 * Their limits: a toggle that says nothing of its state (no `aria-pressed`: a fold, an expander, an
 * action such as "Retry") is no choice to it, and a range whose words come from a variable
 * (`{label}`) is read as a choice only where it says it is pressed.
 *
 * The sites tolerated today are listed per file with their count per kind, the lane that takes
 * them and a reason: the work list. A new site fails (its file's count is exceeded, or the file is
 * not listed), and so does a site that went (its count must be lowered), so the list only shrinks;
 * its total is pinned too. The sites kept by decision are NAMED apart, each with its reason, and
 * pinned the same way: parts of a figure or a fold, not controls, and the shell's two comboboxes.
 */

type Kind = 'select' | 'text-input' | 'tab' | 'pager' | 'choice' | 'clipboard' | 'range';
const KINDS: readonly Kind[] = [
  'select',
  'text-input',
  'tab',
  'pager',
  'choice',
  'clipboard',
  'range',
];

/**
 * The placement lane a file's sites go to (UI plan, phase C), the shell's own fields (the header's
 * search field and the command palette: no C2 lane names them), or `shared`: the old pager itself.
 */
type Lane = 'c2-lists' | 'c2-chain' | 'c2-assets' | 'c2-topics' | 'd1-charts' | 'shell' | 'shared';

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

/** What the scan leaves to the library: its controls, and the one copy path the copy button uses. */
const LIBRARY_FILES = [...CONTROL_FILES, 'hooks/useCopy.ts'] as const;

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- c2-lists: activities, transactions, blocks, inventory, scripts, depositors ----
  // ---- c2-chain: block, transaction, cell, address, script pages ----
  // components/scripts/script-newest-cells.tsx left the list (lane p-assets): its "All N cells ↓"
  // is a way onward to a place on its page, the onward look's link
  // ---- c2-assets: token, cluster, class, objects, identities, their feeds ----
  // ---- c2-topics: dao, forks, fiber, network, hardforks, home: replaced at its merge ----
  // ---- d1-charts: the chart pages' range and scale are the library's RangeSelect and FilterChips (lane D1) ----
  // ---- shared: the old pager (components/ui/cursor-pagination.tsx) went with its last use; the
  // click-to-copy of an id (HexCode's, HexDisplay's) went with its last call site (lane c2-finish,
  // rulings round 1: an id inside a link is the link, not a copy target) ----
};

/** The pinned total of TOLERATED's sites: the list may only shrink (1 → 0, lane p-assets). */
const TOLERATED_TOTAL = 0;

/**
 * The sites kept by decision (lane c2-finish, item 5): parts of a figure, a fold or a preference
 * menu, not a filter, a range or a field of the page; and the shell's two comboboxes. Each says
 * why; the list and its total are pinned, so a new one is a decision written here.
 */
const NAMED: Readonly<Record<string, Tolerated>> = {
  'components/search-bar.tsx': {
    lane: 'shell',
    sites: { 'text-input': 1 },
    reason:
      'the header’s search field: a combobox of its own (its cursor and its list), not a TextField; it stands in WELL_MATERIAL, the well every field reads',
  },
  'components/command-palette.tsx': {
    lane: 'shell',
    sites: { 'text-input': 1 },
    reason:
      'the palette’s query field: a combobox of its own (its commands and its keys), not a TextField; it stands in WELL_MATERIAL, the well every field reads',
  },
  'components/layout/theme-switcher.tsx': {
    lane: 'shell',
    sites: { choice: 2 },
    reason:
      'the theme menu: a preference of the reader, its radio items in the shell’s own menu, not a filter of a page',
  },
  'app/tx/[hash]/client-page.tsx': {
    lane: 'c2-chain',
    sites: { choice: 1 },
    reason:
      'the witness picker: a witness of the figure pressed to light its bytes, a part of the figure',
  },
  'components/scripts/script-code-evidence.tsx': {
    lane: 'c2-chain',
    sites: { choice: 1 },
    reason: 'the code evidence’s event chips: each lights its events in the figure beside it',
  },
  'components/ui/allele-gel.tsx': {
    lane: 'c2-assets',
    sites: { choice: 1 },
    reason: 'the allele gel’s bands: a band pressed to read it, a part of the figure',
  },
  'components/ui/hex-map.tsx': {
    lane: 'c2-chain',
    sites: { choice: 3 },
    reason:
      'the hex map’s fields and bytes: one pressed to light it in the map, a part of the figure',
  },
  'components/ui/composition-set.tsx': {
    lane: 'c2-lists',
    sites: { choice: 2 },
    reason:
      'the composition legend’s tiers: the inventory’s tier filter keeps its own form, the owner’s first-class rule for composition',
  },
};

/** The pinned total of NAMED's sites. */
const NAMED_TOTAL = 12;

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

/** A span of time as a range's choice writes it (`24 h`, `14 d`, `1y`, `90 days`): a button's words. */
const SPAN_WORDS = /^\d+ ?(h|d|w|m|y|mo|hours?|days?|weeks?|months?|years?)$/i;

/** At most this many readings of one element's text (a `?:` in each of several children). */
const TEXT_READINGS = 32;

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

    // a choice of one of several by hand: a toggle that says it is pressed (a group of them is a
    // filter or a range), and the radio group the filter chips draw
    if (attr('aria-pressed'))
      sites.push({ kind: 'choice', line: lineOf(node), what: `<${shown} aria-pressed>` });

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
        else if (types.includes('range'))
          sites.push({ kind: 'range', line: lineOf(node), what: `<${shown} type="range">` });
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
        const radio = roles.find((r) => r === 'radiogroup' || r === 'radio');
        if (radio)
          sites.push({ kind: 'choice', line: lineOf(node), what: `<${shown} role="${radio}">` });
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

    // a range by hand: a button whose words are a span of time (the RangeSelect writes them)
    if (children && tags.includes('button')) {
      const span = textsOf(children).find((t) => SPAN_WORDS.test(t));
      if (span !== undefined)
        sites.push({ kind: 'range', line: lineOf(node), what: `<${shown}>${span}</${shown}>` });
    }

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
    // a clipboard write by hand: the clipboard itself, or the one copy path's hook
    else if (
      ts.isPropertyAccessExpression(node) &&
      node.name.text === 'clipboard' &&
      ((ts.isIdentifier(node.expression) && node.expression.text === 'navigator') ||
        (ts.isPropertyAccessExpression(node.expression) &&
          node.expression.name.text === 'navigator'))
    )
      sites.push({ kind: 'clipboard', line: lineOf(node), what: 'navigator.clipboard' });
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === 'useCopy'
    )
      sites.push({ kind: 'clipboard', line: lineOf(node), what: 'useCopy(…)' });
    if (ts.isCallExpression(node)) {
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

/**
 * The scan's count against the lists: every file's count per kind must equal its tolerated one
 * and its named one together.
 */
function toleratedFailures(
  found: ReadonlyMap<string, readonly Site[]>,
  tolerated: Readonly<Record<string, Tolerated>>,
  named: Readonly<Record<string, Tolerated>> = {}
): string[] {
  const out: string[] = [];
  const paths = new Set([...found.keys(), ...Object.keys(tolerated), ...Object.keys(named)]);
  for (const path of [...paths].sort()) {
    const sites = found.get(path) ?? [];
    const entry = tolerated[path];
    const kept = named[path];
    for (const kind of KINDS) {
      const of = sites.filter((s) => s.kind === kind);
      const allowed = (entry?.sites[kind] ?? 0) + (kept?.sites[kind] ?? 0);
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
    for (const listed of [entry, kept])
      if (listed && listed.reason.trim().length <= 10)
        out.push(`${path}: a reason too short to say why: "${listed.reason}"`);
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
      // re-pinned (lane c2-finish, item 5): a range input is a range choice by hand now (it passed
      // as an input of another type); the checkbox, the hidden and the number still pass
    ).toEqual(['range <input type="range">']);
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

  // Lane c2-finish, item 5: what the scan could not read before, so a brief had to hold it.
  it('finds a choice by hand: an aria-pressed button, a radiogroup, a radio', () => {
    expect(
      kinds(`
        export const A = ({ on }: { on: boolean }) => (
          <div>
            <button aria-pressed={on}>tokens</button>
            <span role="radiogroup" aria-label="status">
              <button role="radio" aria-checked={on}>all</button>
            </span>
            <button aria-expanded={on}>filters</button>
            <button>plain</button>
          </div>
        );`)
    ).toEqual([
      'choice <button aria-pressed>',
      'choice <span role="radiogroup">',
      'choice <button role="radio">',
    ]);
  });

  it('finds a clipboard write: navigator.clipboard, and a use of useCopy', () => {
    expect(
      kinds(`
        export function A({ v }: { v: string }) {
          const { copy } = useCopy(v);
          const write = () => navigator.clipboard.writeText(v);
          const other = () => window.navigator.clipboard?.writeText(v);
          return <button onClick={copy}>x</button>;
        }`)
    ).toEqual([
      'clipboard useCopy(…)',
      'clipboard navigator.clipboard',
      'clipboard navigator.clipboard',
    ]);
  });

  it('finds a range choice by hand: a range input, a button whose words are a span of time', () => {
    expect(
      kinds(`
        export const A = ({ d }: { d: number }) => (
          <div>
            <input type="range" min={0} max={9} />
            <button>24 h</button>
            <button>{'14 d'}</button>
            <button>1y</button>
            <button>90 days</button>
            <button>24 h activity</button>
            <button>{d} d</button>
          </div>
        );`)
    ).toEqual([
      'range <input type="range">',
      'range <button>24 h</button>',
      'range <button>14 d</button>',
      'range <button>1y</button>',
      'range <button>90 days</button>',
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

  // Lane c2-finish, item 5: a site kept by decision is named with its reason, counted the same way
  it('passes a named site, fails one more, and fails a named site that went', () => {
    const named: Record<string, Tolerated> = {
      'app/b.tsx': { lane: 'c2-chain', sites: { choice: 1 }, reason: 'a part of the figure' },
    };
    const choice = (line: number): Site => ({
      kind: 'choice',
      line,
      what: '<button aria-pressed>',
    });
    expect(toleratedFailures(new Map([['app/b.tsx', [choice(4)]]]), {}, named)).toEqual([]);
    expect(toleratedFailures(new Map([['app/b.tsx', [choice(4), choice(9)]]]), {}, named)).toEqual([
      expect.stringMatching(/^app\/b\.tsx: 2 raw choice site\(s\), 1 tolerated — a new one: /),
    ]);
    expect(toleratedFailures(new Map(), {}, named)).toEqual([
      'app/b.tsx: 0 raw choice site(s), 1 tolerated — lower its count to 0',
    ]);
    expect(
      toleratedFailures(
        new Map([['app/b.tsx', [choice(4)]]]),
        {},
        {
          'app/b.tsx': { ...named['app/b.tsx'], reason: 'kept' },
        }
      )
    ).toEqual(['app/b.tsx: a reason too short to say why: "kept"']);
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
    if ((LIBRARY_FILES as readonly string[]).includes(path)) continue;
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

  it('writes no raw control but the tolerated list’s and the named list’s', () => {
    expect(toleratedFailures(found, TOLERATED, NAMED)).toEqual([]);
  });

  const totalOf = (list: Readonly<Record<string, Tolerated>>) =>
    Object.values(list).reduce(
      (sum, entry) => sum + Object.values(entry.sites).reduce((a, n) => a + (n ?? 0), 0),
      0
    );

  it(`tolerates ${TOLERATED_TOTAL} sites in all: the pinned total, which only falls`, () => {
    expect(totalOf(TOLERATED)).toBe(TOLERATED_TOTAL);
  });

  it(`names ${NAMED_TOTAL} sites kept by decision, each with its reason: the pinned total`, () => {
    expect(totalOf(NAMED)).toBe(NAMED_TOTAL);
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
