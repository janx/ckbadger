import { existsSync, readFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { sourceFiles, unwrap, type Binding } from '@/__tests__/utils/source-scan';

/**
 * Gate for the controls (UI review 2026-11-14, phase C, lane C1; the controls board the owner
 * passed on 2026-11-19, gate D0′): a tab row, a filter, a range, a pager, a text field, a select, a
 * copy button, a help mark, a fold's toggle and a section's disclosure are each one component of
 * `components/ui/` (CONTROL_FILES). Outside
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
 * underline; a tag passed through an `as` prop is read as its default only (the prop's other
 * values are its callers'); an `<input>` of another text type (`email`, `number`, …) and a
 * `<textarea>` (the site has none) are not counted.
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
 * action) is no choice to it, and a range whose words come from a variable
 * (`{label}`) is read as a choice only where it says it is pressed.
 *
 * What it reads since lane cd-fixes-controls (T SHOULD-2, NOTE-3):
 *
 * - a spread into an element, followed to what it sets (`spreadRead`): an object literal's
 *   `aria-pressed` and `role` (and its own spreads), a name of the file holding one, a call of a
 *   function of the file (read as what it returns), and a call of a hook's binder, known by its
 *   name among the binders the files of `hooks/` hand back (`hookBinders`: usePinnedReadout's
 *   `bind`, a pressed button), through `?:`, `&&`, `||` and `??`;
 * - `spread`: a spread it cannot follow (a prop, an import, a call of anything else) on a control,
 *   an element that is one by its tag (`button`, `a`, `input`, `select`, `textarea`, `summary`), a
 *   `Link` / `AppLink`, or an element with a click, a key handler, a role or a tab stop: it is a
 *   site, named with its reason until the scan can follow it;
 * - the role `menuitemradio`, and an `<input type="radio">` or `type="checkbox"`, as a choice;
 * - the pager words `‹ prev`, `next ›`, `next page`, `show more`, `more ↓`, on a button or a link,
 *   on any element that is a control by its attributes, or on a component with a click or a link
 *   (`onClick`, `onGo`, `href`, `to`);
 * - `fold`: a disclosure written by hand (review-p-tests SHOULD-6), an element that says whether it
 *   is open (`aria-expanded`, set or through a spread) outside the library, whose one disclosure
 *   is FoldToggle; the shell's menus and a figure's own selection are named, the rest a work list;
 *   and (the lead's ruling on review-fix-tests SHOULD-4: a native disclosure is a control like any
 *   other) a native one, a `<details>`, or a `<summary>` written apart from its details: one site
 *   per disclosure, whose one component is SectionDisclosure;
 * - the roles `checkbox`, `switch`, `option` and `menuitemcheckbox` as a choice (NOTE-5);
 * - `retry`: an action after a failed read written by hand, its words `Retry`, `try again` or
 *   `Reload` (a `↻` before them) on the same elements: the library's RetryAction draws it (the
 *   chart reviewer's N11: four raw buttons, each with its own ink and underline).
 *
 * Its limits: a binder is known by its name, so a hook's binder of a name another function of a
 * file also bears is read as the binder; a spread it cannot follow on an element that is a control
 * by the spread alone (an SVG mark with no click of its own) walks past; a pager whose words come
 * from a prop (`<Go words="next →">`) or drawn by a component with no click and no link is not
 * seen; a `<details>` drawn by `createElement` or through a tag it cannot read (refused above, by
 * name) and a disclosure drawn by a component other than SectionDisclosure that writes no
 * `<details>` and says nothing of its state are not seen.
 *
 * The sites tolerated today are listed per file with their count per kind, the lane that takes
 * them and a reason: the work list. A new site fails (its file's count is exceeded, or the file is
 * not listed), and so does a site that went (its count must be lowered), so the list only shrinks;
 * its total is pinned too. The sites kept by decision are NAMED apart, each with its reason, and
 * pinned the same way: parts of a figure or a fold, not controls, and the shell's two comboboxes.
 */

type Kind =
  | 'select'
  | 'text-input'
  | 'tab'
  | 'pager'
  | 'choice'
  | 'clipboard'
  | 'range'
  | 'spread'
  | 'retry'
  | 'fold';
const KINDS: readonly Kind[] = [
  'select',
  'text-input',
  'tab',
  'pager',
  'choice',
  'clipboard',
  'range',
  'spread',
  'retry',
  'fold',
];

/**
 * The placement lane a file's sites go to (UI plan, phase C), the shell's own fields (the header's
 * search field and the command palette: no C2 lane names them), or `shared`: the old pager itself.
 */
type Lane =
  | 'c2-lists'
  | 'c2-chain'
  | 'c2-assets'
  | 'c2-topics'
  | 'd1-charts'
  | 'shell'
  | 'shared'
  // the page lanes working in those files now (lane cd-fixes-controls, review-p-tests SHOULD-6)
  | 'p-chain'
  | 'p-assets'
  | 'p-lists'
  | 'p-topics';

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
  'components/ui/retry-action.tsx',
  // the fold's one control (review of phases C and D, SHOULD 5; lane P1): "all 158 cells ↓" and
  // "fewer ↑" on one button that keeps its focus, the short list's words the Pager writes too
  'components/ui/fold-toggle.tsx',
  // a section's disclosure, the one place a `<details>` is written (lane cd-fixes-controls, the
  // lead's ruling on review-fix-tests SHOULD-4): detail-layout's "Raw data" was its first form
  'components/ui/section-disclosure.tsx',
] as const;

/** What the scan leaves to the library: its controls, and the one copy path the copy button uses. */
const LIBRARY_FILES = [...CONTROL_FILES, 'hooks/useCopy.ts'] as const;

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- c2-lists: activities, transactions, blocks, inventory, scripts, depositors ----
  // ---- c2-chain: block, transaction, cell, address, script pages ----
  // components/scripts/script-newest-cells.tsx left the list (lane p-assets): its "All N cells ↓"
  // is a way onward to a place on its page, the onward look's link
  // ---- the disclosures by hand the reader sees since lane cd-fixes-controls (review-p-tests
  // SHOULD-6), for the page lanes working in these files: FoldToggle is the library's one ----
  // 'components/activity/activity-row.tsx' left the list (the lead's final round, items 2 and 3:
  // fold 5 → 0): a protocol list's lines and the participants fold by FoldToggle's box under their
  // lines, the head's names and a participant's objects (and its phone object count) by its inline
  // form inside the line
  // the lead's final round, item 3: the advertisement graph's observations fold by FoldToggle's
  // box (fold 1 → 0); the address holdings' collection rows, what-moved's rest of a side and the
  // nodes table's peer rows and phone filters are another kind of thing, NAMED with their controls
  // ---- the native disclosures the reader sees since lane cd-fixes-controls (the lead's ruling on
  // review-fix-tests SHOULD-4: a native disclosure is a control like any other; SectionDisclosure
  // is the library's one): recorded for the page lanes in these files, converted in one sweep
  // after their rounds merge. Each says whether it is the section disclosure as it stands or needs
  // a form the component lacks (an inner disclosure, no rule of its own; a state its page holds) ----
  'app/tx/[hash]/client-page.tsx': {
    lane: 'p-chain',
    sites: { fold: 7 },
    reason:
      'the raw section’s Cells, Scripts, Witness bytes, Header deps and JSON, inner disclosures with no rule (a form the component lacks); its Cell graph, whose state the page holds (onToggle: a form it lacks); and the page’s own Raw data, the section disclosure as it stands',
  },
  'components/cell/code-cell-board.tsx': {
    lane: 'p-chain',
    sites: { fold: 1 },
    reason:
      'the code cell’s “Type ID lineage of these bytes”, an inner disclosure with no rule (a form the component lacks)',
  },
  // ---- shell ----
  'components/crash-page.tsx': {
    lane: 'shell',
    sites: { retry: 1 },
    reason:
      'the crash page’s “↻ try again”: it reloads the whole page after a crash, not a failed read; the shell draws it with RetryAction or keeps it as its own (it came to light when the reader learned a retry, lane cd-fixes-controls)',
  },
  // ---- c2-assets: token, cluster, class, objects, identities, their feeds ----
  // ---- c2-topics: dao, forks, fiber, network, hardforks, home: replaced at its merge ----
  // ---- d1-charts: the chart pages' range and scale are the library's RangeSelect and FilterChips (lane D1) ----
  // ---- shared: the old pager (components/ui/cursor-pagination.tsx) went with its last use; the
  // click-to-copy of an id (HexCode's, HexDisplay's) went with its last call site (lane c2-finish,
  // rulings round 1: an id inside a link is the link, not a copy target) ----
};

/**
 * The pinned total of TOLERATED's sites: the list may only shrink (1 → 0, lane p-assets). 0 → 1
 * when the reader learned a retry written by hand (lane cd-fixes-controls): the four Retry buttons
 * are the library's RetryAction, and the crash page's "↻ try again" came to light, for the shell.
 */
// → 13 (lane cd-fixes-controls, review-p-tests SHOULD-6: the reader learned a disclosure by hand,
// and twelve came to light for the page lanes: the activity row 5, the nodes table 4, the address
// holdings 1, what-moved 1, the advertisement graph 1) → 12 (the merge with the P3 follow-up: the
// activity row's protocol list folds by FoldToggle, e03f828f) → 11 (the lead's final round: the
// head's names fold by FoldToggle's inline form) → 8 (the activity row's objects, phone object
// count and participants: FoldToggle's inline and box forms) → 1 (the lead's final round, item
// 3: the advertisement graph's fold is FoldToggle's box; the holdings' rows, what-moved's rest of a
// side and the nodes table's rows and filters are NAMED; the crash page's reload stands) → 22 (lane
// cd-fixes-controls, the lead's ruling on review-fix-tests SHOULD-4: the reader learned a native
// disclosure, and 21 came to light for the page lanes: the transaction page 7 and the code cell 1
// for p-chain; the asset components 11 and the script pages 2 for p-assets. Detail-layout's "Raw
// data" became SectionDisclosure, the one place a `<details>` is written)
// → 21 (the disclosure sweep: asset-history's supply changes are SectionDisclosure's section form)
// → 19 (the description and the issuer info as stored are SectionDisclosure's inner form)
// → 17 (the DNA as hex and every gene one row each are SectionDisclosure's inner form)
// → 14 (the decoded fields, all the bytes as hex and a reading's bytes are SectionDisclosure's inner form)
// → 13 (an item's lock and type scripts are SectionDisclosure's inner form)
// → 12 (an m-NFT issuer's raw metadata is SectionDisclosure's inner form)
// → 11 (an object's media dependencies are SectionDisclosure's inner form)
// → 10 (a custody's committed key hashes are SectionDisclosure's inner form)
// → 9 (a family's every reference in full is SectionDisclosure's inner form; the p-assets sites are all the library's, the p-chain ones wait for the exactness lane)
const TOLERATED_TOTAL = 9;

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
    sites: { choice: 3, fold: 1 },
    reason:
      'the theme menu: a preference of the reader, its radio items in the shell’s own menu (menuitemradio, read since lane cd-fixes-controls) and the radio group of its sheet, not a filter of a page; its button opens the shell’s menu (aria-expanded, read since review-p-tests SHOULD-6)',
  },
  'components/layout/nav-menu.tsx': {
    lane: 'shell',
    sites: { fold: 1 },
    reason:
      'the shell’s navigation menu: its button opens the menu (aria-expanded, read since lane cd-fixes-controls, review-p-tests SHOULD-6), the shell’s own, not a fold of a page',
  },
  // ---- what the reader sees since lane cd-fixes-controls (T SHOULD-2): a pressed toggle set
  // through a spread, a menu's radio item, a spread it cannot follow on a control ----
  'components/layout/network-switcher.tsx': {
    lane: 'shell',
    sites: { choice: 1 },
    reason:
      'the network menu: which network the reader reads, its radio items in the shell’s own menu, not a filter of a page',
  },
  'components/cell/cell-microscope.tsx': {
    lane: 'c2-chain',
    sites: { choice: 3 },
    reason:
      'the microscope’s map parts, ELF regions and field pills (its `handlers`): one pressed to pin its reading, a part of the figure (lane c2-finish, item 5, named it)',
  },
  'app/network/network-map.tsx': {
    lane: 'c2-topics',
    sites: { choice: 1 },
    reason:
      'the map’s peer marks (the readout’s binder): one pressed to pin its peer’s reading, a part of the figure',
  },
  'components/fiber/fiber-network-graph.tsx': {
    lane: 'c2-topics',
    sites: { choice: 2 },
    reason:
      'the channel graph’s edges and nodes (the readout’s binder, in its `handlers`): one pressed to pin its channels and partners in the reading, a part of the figure',
  },
  'components/hardforks/script-deployment-timeline.tsx': {
    lane: 'c2-topics',
    sites: { choice: 1 },
    reason:
      'the deployment timeline’s marks (the readout’s binder): one pressed to pin its version’s reading, a part of the figure',
  },
  'components/hardforks/vm-lane-timeline.tsx': {
    lane: 'c2-topics',
    sites: { choice: 1 },
    reason:
      'the VM lanes’ deployment marks (the readout’s binder): one pressed to pin its reading, a part of the figure',
  },
  'components/ui/sequencing-gel.tsx': {
    lane: 'c2-assets',
    sites: { choice: 2 },
    reason:
      'the sequencing gel’s genes, as bands and as rows (the readout’s binder): one pressed to pin its reading, a part of the figure',
  },
  'components/home/ckbytes-section.tsx': {
    lane: 'c2-topics',
    sites: { spread: 1 },
    reason:
      'the CKBytes legend item hands its caller’s hover, focus and tap handlers to its button: a highlight of the figure that says no state, no choice',
  },
  'components/ui/link.tsx': {
    lane: 'shared',
    sites: { spread: 1 },
    reason:
      'the link component hands its callers’ props to its anchor: they are read at each call site, where a pressed state or a role is an attribute of the Link',
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
    sites: { choice: 1, fold: 1 },
    reason:
      'the allele gel’s bands: a band pressed to read it, and a gene’s lane opened to its variants (aria-expanded, read since lane cd-fixes-controls), parts of the figure',
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
  // ---- what the lead's final round (item 3) found to be another kind of thing than a fold of a
  // list: a row that opens its own detail, a node of a figure, a disclosure of a section's
  // controls; each control says it is open (aria-expanded) and stays mounted ----
  'components/address/address-holdings.tsx': {
    lane: 'p-chain',
    sites: { fold: 1 },
    reason:
      'a collection’s row that opens its own objects under it (a paged list of its own): its control is the row itself and its “objects ↓” / “close ↑” button, which stays, not a fold of the holdings list',
  },
  'components/transaction/what-moved.tsx': {
    lane: 'p-chain',
    sites: { fold: 1 },
    reason:
      'the flow figure’s “N more parties” end: a node of the figure with its ribbon and the rest’s summed amount, which draws the side’s parties in its place; its control is the node itself, not a list’s way to go',
  },
  'app/network/nodes-table.tsx': {
    lane: 'p-topics',
    sites: { fold: 4 },
    reason:
      'a peer’s row that opens its own evidence under it, in its three forms (one toggle spread on the row’s 44 px button, “Show / Hide evidence for …”), and the phone’s four filters behind one “filters” button (aria-controls, the section’s controls): rows and a section’s controls, not a fold of a list',
  },
};

/**
 * The pinned total of NAMED's sites: 12, and 14 more since lane cd-fixes-controls's reader learned
 * a spread, a menu's radio item and a spread it cannot follow on a control (T SHOULD-2): the
 * theme menu's radio item, the network menu's, eight pressed toggles of figures through the
 * readout's binder or a function of their file, two of the sequencing gel's, and two spreads of
 * props passed through. 26 → 29 when the reader learned a disclosure by hand (review-p-tests
 * SHOULD-6): the shell's two menus (the theme menu's button, the navigation menu's) and the allele
 * gel's gene lanes, a figure's own selection.
 */
// → 35 (the lead's final round, item 3: the address holdings' collection row 1, what-moved's rest
// of a side 1, the nodes table's peer rows 3 and phone filters 1, another kind of thing than a fold)
const NAMED_TOTAL = 35;

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

/**
 * A pager's words, on an element's text with its spaces collapsed (`…` for what an expression
 * writes). Lane cd-fixes-controls (T NOTE-3) taught it `‹ prev`, `next ›`, `next page`, `show
 * more` and `more ↓`.
 */
const PAGER_WORDS: readonly RegExp[] = [
  /^([←‹] ?)?(prev|previous|newer)$/i,
  /^(next|older)( ?[→›])?$/i,
  /^next page$/i,
  /^(load|show) more$/i,
  /^more ↓$/i,
  /^show all\b/i,
  /^all \S.* ↓$/i,
  /^next (\d[\d,]*|…)( ↓)?$/i,
];

/** The elements a pager's words make a hand-built pager: a button or a link. */
const PAGER_TAGS = new Set(['button', 'a', 'Link', 'AppLink']);

/**
 * A retry's words (lane cd-fixes-controls, the chart reviewer's N11): an action after a failed
 * read, the library's RetryAction (components/ui/retry-action.tsx).
 */
const RETRY_WORDS = /^(↻ ?)?(retry|try again|reload)$/i;

/**
 * The roles of a choice: the filter chips' radio group and radio, a menu's radio item, and (lane
 * cd-fixes-controls, review-p-tests NOTE-5) a checkbox, a switch, a listbox's option and a menu's
 * checkbox item.
 */
const CHOICE_ROLES = new Set([
  'radiogroup',
  'radio',
  'menuitemradio',
  'checkbox',
  'switch',
  'option',
  'menuitemcheckbox',
]);

/** The elements that are controls by their tag: what a spread it cannot follow may make a choice of. */
const INTERACTIVE_TAGS = new Set(['button', 'a', 'input', 'select', 'textarea', 'summary']);
const INTERACTIVE_COMPONENTS = new Set(['Link', 'AppLink']);
/** The attributes that make any element a control: a click, a key, a role, a tab stop. */
const INTERACTIVE_ATTRS = ['onClick', 'onKeyDown', 'role', 'tabIndex'] as const;
/** The props that make a component a pager where its words are a pager's: a click, a link. */
const PAGER_COMPONENT_PROPS = ['onClick', 'onGo', 'href', 'to'] as const;

/**
 * What a spread sets that the scan reads: whether it says pressed, whether it says open or shut
 * (`aria-expanded`, a disclosure: lane cd-fixes-controls, review-p-tests SHOULD-6), the roles it
 * can set.
 */
export interface SpreadRead {
  pressed: boolean;
  expanded?: true;
  roles: string[];
}

/**
 * The binders the hooks hand back (lane cd-fixes-controls, T SHOULD-2): a returned object's
 * property that is a function returning an object literal (`usePinnedReadout`'s `bind`), read for
 * its `aria-pressed` and its literal `role`. A spread of a call of that name (`readout.bind(id)`,
 * `pin.bind(target)`) is read as that binder: the name of a hook's binder is how the scan knows it.
 */
function hookBinders(code: string): Map<string, SpreadRead> {
  const source = ts.createSourceFile('hook.ts', code, ts.ScriptTarget.Latest, true);
  const out = new Map<string, SpreadRead>();
  const visit = (node: ts.Node) => {
    if (ts.isReturnStatement(node) && node.expression) {
      const returned = unwrap(node.expression);
      if (ts.isObjectLiteralExpression(returned))
        for (const p of returned.properties) {
          if (!ts.isPropertyAssignment(p) || !ts.isIdentifier(p.name)) continue;
          const f = unwrap(p.initializer);
          if (!(ts.isArrowFunction(f) || ts.isFunctionExpression(f))) continue;
          const body = ts.isBlock(f.body) ? null : unwrap(f.body as ts.Expression);
          if (!body || !ts.isObjectLiteralExpression(body)) continue;
          const read: SpreadRead = { pressed: false, roles: [] };
          for (const q of body.properties) {
            if (!ts.isPropertyAssignment(q)) continue;
            const key = ts.isIdentifier(q.name) || ts.isStringLiteral(q.name) ? q.name.text : null;
            if (key === 'aria-pressed') read.pressed = true;
            if (key === 'aria-expanded') read.expanded = true;
            if (key === 'role' && ts.isStringLiteralLike(q.initializer))
              read.roles.push(q.initializer.text);
          }
          out.set(p.name.text, read);
        }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

/** A span of time as a range's choice writes it (`24 h`, `14 d`, `1y`, `90 days`): a button's words. */
const SPAN_WORDS = /^\d+ ?(h|d|w|m|y|mo|hours?|days?|weeks?|months?|years?)$/i;

/** At most this many readings of one element's text (a `?:` in each of several children). */
const TEXT_READINGS = 32;

/** The controls a source writes by hand, and what it could not read. */
function scanControls(
  code: string,
  scriptKind: ts.ScriptKind = ts.ScriptKind.TSX,
  binders: ReadonlyMap<string, SpreadRead> = new Map()
): Scan {
  const source = ts.createSourceFile('scan.tsx', code, ts.ScriptTarget.Latest, true, scriptKind);
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const sites: Site[] = [];
  const refused: string[] = [];
  const refuse = (n: ts.Node, why: string) => refused.push(`${lineOf(n)}: ${why}`);

  // the file's names: what each may hold
  const bindings = new Map<string, Binding[]>();
  const bind = (name: string, b: Binding) => bindings.set(name, [...(bindings.get(name) ?? []), b]);
  // the file's functions: what each returns (a spread of a call of one is read as its returns)
  const functions = new Map<string, ts.Expression[]>();
  const returnsOf = (f: ts.FunctionLikeDeclaration): ts.Expression[] => {
    if (!f.body) return [];
    if (!ts.isBlock(f.body)) return [f.body as ts.Expression];
    const out: ts.Expression[] = [];
    const walk = (n: ts.Node) => {
      if (ts.isFunctionLike(n)) return;
      if (ts.isReturnStatement(n) && n.expression) out.push(n.expression);
      ts.forEachChild(n, walk);
    };
    ts.forEachChild(f.body, walk);
    return out;
  };
  const collect = (node: ts.Node) => {
    if (ts.isFunctionDeclaration(node) && node.name) functions.set(node.name.text, returnsOf(node));
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.initializer &&
      (ts.isArrowFunction(unwrap(node.initializer)) ||
        ts.isFunctionExpression(unwrap(node.initializer)))
    )
      functions.set(
        node.name.text,
        returnsOf(unwrap(node.initializer) as ts.ArrowFunction | ts.FunctionExpression)
      );
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

  /**
   * What a spread sets, where the scan can follow it (lane cd-fixes-controls, T SHOULD-2): an
   * object literal's `aria-pressed` and `role` (and its own spreads), a name holding one, a call of
   * a function of the file (read as what it returns), a call of a hook's binder by its name
   * (`hookBinders`), through `?:`, `&&`, `||` and `??`. Null where it cannot: a prop, an import, a
   * call of anything else, a computed key.
   */
  const spreadRead = (e: ts.Expression, seen: ReadonlySet<string>): SpreadRead | null => {
    const x = unwrap(e);
    const merge = (reads: (SpreadRead | null)[]): SpreadRead | null =>
      reads.some((r) => r === null)
        ? null
        : {
            pressed: reads.some((r) => r!.pressed),
            ...(reads.some((r) => r!.expanded) ? { expanded: true as const } : {}),
            roles: reads.flatMap((r) => r!.roles),
          };
    if (
      x.kind === ts.SyntaxKind.NullKeyword ||
      x.kind === ts.SyntaxKind.FalseKeyword ||
      (ts.isIdentifier(x) && x.text === 'undefined')
    )
      return { pressed: false, roles: [] };
    if (ts.isObjectLiteralExpression(x)) {
      const parts: (SpreadRead | null)[] = [{ pressed: false, roles: [] }];
      for (const p of x.properties) {
        if (ts.isSpreadAssignment(p)) {
          parts.push(spreadRead(p.expression, seen));
          continue;
        }
        if (p.name && ts.isComputedPropertyName(p.name)) {
          parts.push(null);
          continue;
        }
        const key =
          p.name && (ts.isIdentifier(p.name) || ts.isStringLiteral(p.name)) ? p.name.text : null;
        if (key === 'aria-pressed') parts.push({ pressed: true, roles: [] });
        if (key === 'aria-expanded') parts.push({ pressed: false, expanded: true, roles: [] });
        if (key === 'role') {
          const roles = ts.isPropertyAssignment(p)
            ? strings(p.initializer)
            : ts.isShorthandPropertyAssignment(p)
              ? strings(p.name)
              : null;
          parts.push(roles ? { pressed: false, roles } : null);
        }
      }
      return merge(parts);
    }
    if (ts.isConditionalExpression(x))
      return merge([spreadRead(x.whenTrue, seen), spreadRead(x.whenFalse, seen)]);
    if (ts.isBinaryExpression(x)) {
      const op = x.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return spreadRead(x.right, seen);
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken)
        return merge([spreadRead(x.left, seen), spreadRead(x.right, seen)]);
      return null;
    }
    if (ts.isIdentifier(x)) {
      if (seen.has(x.text)) return null;
      const found = bindings.get(x.text) ?? [];
      if (found.length === 0 || found.some((b) => b.kind !== 'value')) return null;
      return merge(
        found.flatMap((b) =>
          b.kind === 'value' ? b.values.map((v) => spreadRead(v, new Set([...seen, x.text]))) : []
        )
      );
    }
    if (ts.isCallExpression(x)) {
      const callee = unwrap(x.expression);
      if (ts.isIdentifier(callee) && functions.has(callee.text) && !seen.has(callee.text))
        return merge(
          functions.get(callee.text)!.map((r) => spreadRead(r, new Set([...seen, callee.text])))
        );
      const name = ts.isIdentifier(callee)
        ? callee.text
        : ts.isPropertyAccessExpression(callee)
          ? callee.name.text
          : null;
      const binder = name === null ? undefined : binders.get(name);
      return binder ?? null;
    }
    return null;
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

  /**
   * Whether a `<summary>` stands in a `<details>` of the same JSX: up through elements, fragments,
   * expressions and the `?:`, `&&` and parentheses they hold, to an element whose tag is `details`.
   */
  const insideDetails = (node: ts.Node): boolean => {
    for (let at = node.parent; at; at = at.parent) {
      if (ts.isJsxElement(at) && at.openingElement !== node) {
        if (tagsOf(at.openingElement.tagName)?.includes('details')) return true;
        continue;
      }
      if (
        ts.isJsxFragment(at) ||
        ts.isJsxExpression(at) ||
        ts.isParenthesizedExpression(at) ||
        ts.isConditionalExpression(at) ||
        ts.isBinaryExpression(at) ||
        (ts.isJsxElement(at) && at.openingElement === node)
      )
        continue;
      return false;
    }
    return false;
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
    // a disclosure by hand (lane cd-fixes-controls, review-p-tests SHOULD-6): an element that says
    // whether it is open; FoldToggle is the library's one
    if (attr('aria-expanded'))
      sites.push({ kind: 'fold', line: lineOf(node), what: `<${shown} aria-expanded>` });
    // a native disclosure (lane cd-fixes-controls, the lead's ruling on review-fix-tests SHOULD-4):
    // a `<details>`, or a `<summary>` written apart from its details; SectionDisclosure is the
    // library's one
    if (tags.includes('details') || (tags.includes('summary') && !insideDetails(node)))
      sites.push({ kind: 'fold', line: lineOf(node), what: `<${shown}>` });

    // a control by its tag, its component or its attributes
    const handled = INTERACTIVE_ATTRS.some((key) => attr(key));
    const interactive =
      tags.some((t) => INTERACTIVE_TAGS.has(t)) || INTERACTIVE_COMPONENTS.has(name) || handled;

    // what a spread sets (lane cd-fixes-controls, T SHOULD-2): its aria-pressed and its role where
    // the scan can follow it; a spread it cannot follow on a control is a site to name
    let tab = false;
    for (const a of attrs) {
      if (!ts.isJsxSpreadAttribute(a)) continue;
      const read = spreadRead(a.expression, new Set());
      if (read === null) {
        // an <input>'s spread with no literal type after it is refused below
        if (interactive && !tags.includes('input'))
          sites.push({
            kind: 'spread',
            line: lineOf(node),
            what: `<${shown} {...${a.expression.getText(source).slice(0, 60)}}>`,
          });
        continue;
      }
      if (read.pressed)
        sites.push({ kind: 'choice', line: lineOf(node), what: `<${shown} {…} aria-pressed>` });
      if (read.expanded)
        sites.push({ kind: 'fold', line: lineOf(node), what: `<${shown} {…} aria-expanded>` });
      const tabRole = read.roles.find((r) => r === 'tab' || r === 'tablist');
      if (tabRole) {
        tab = true;
        sites.push({ kind: 'tab', line: lineOf(node), what: `<${shown} {…} role="${tabRole}">` });
      }
      const choiceRole = read.roles.find((r) => CHOICE_ROLES.has(r));
      if (choiceRole)
        sites.push({
          kind: 'choice',
          line: lineOf(node),
          what: `<${shown} {…} role="${choiceRole}">`,
        });
    }

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
        // a choice by hand (lane cd-fixes-controls, T SHOULD-2): a radio or a checkbox
        if (types?.some((t) => t === 'radio' || t === 'checkbox'))
          sites.push({
            kind: 'choice',
            line: lineOf(node),
            what: `<${shown} type="${types.join('|')}">`,
          });
      }
    }

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
        const radio = roles.find((r) => CHOICE_ROLES.has(r));
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

    // a pager by hand: its words on a button or a link, on any element that is a control by its
    // attributes, or on a component with a click or a link (lane cd-fixes-controls, T NOTE-3)
    const pagerDrawer =
      tags.some((t) => PAGER_TAGS.has(t)) ||
      PAGER_TAGS.has(name) ||
      (tags.length > 0 && handled) ||
      (tags.length === 0 && PAGER_COMPONENT_PROPS.some((key) => attr(key)));
    // a retry by hand (lane cd-fixes-controls): its words on the elements that draw a pager
    if (children && pagerDrawer) {
      const again = textsOf(children).find((t) => RETRY_WORDS.test(t));
      if (again !== undefined)
        sites.push({ kind: 'retry', line: lineOf(node), what: `<${shown}>${again}</${shown}>` });
    }
    if (name === 'CursorPagination')
      sites.push({ kind: 'pager', line: lineOf(node), what: '<CursorPagination>' });
    else if (children && pagerDrawer) {
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
      // as an input of another type); the hidden and the number still pass. Re-pinned again (lane
      // cd-fixes-controls, T SHOULD-2): a checkbox is a choice by hand (it passed)
    ).toEqual([
      'choice <input type="checkbox">',
      'choice <input type="checkbox">',
      'range <input type="range">',
    ]);
    expect(
      kinds(`
        const KIND = 'search';
        export const A = ({ plain }: { plain: boolean }) => (
          <div>
            <input type={KIND} />
            <input type={plain ? 'text' : 'checkbox'} />
          </div>
        );`)
      // re-pinned (lane cd-fixes-controls, T SHOULD-2): a type that may be a checkbox is a choice too
    ).toEqual([
      'text-input <input type="search">',
      'text-input <input type="text|checkbox">',
      'choice <input type="text|checkbox">',
    ]);
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
      // re-pinned on purpose (lane cd-fixes-controls, review-p-tests SHOULD-6): a button that
      // says it is open is a disclosure by hand, a fold (it passed)
    ).toEqual([
      'choice <button aria-pressed>',
      'choice <span role="radiogroup">',
      'choice <button role="radio">',
      'fold <button aria-expanded>',
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

  // Lane cd-fixes-controls (T SHOULD-2): a choice set through a spread, which the scan walked past
  // (eight pressed toggles in six files at c2808282), and two more forms of a choice.
  it('follows a spread to its aria-pressed: an object, a name, a function of the file, a hook’s binder', () => {
    const binders = new Map([['bind', { pressed: true, roles: ['button'] }]]);
    expect(
      scanControls(
        `
        const PRESSED = { 'aria-pressed': true };
        export function A({ on, pin }: { on: boolean; pin: Pin }) {
          const handlers = (t: string) => ({ 'aria-pressed': on, onClick: () => t });
          const wrapped = (t: string) => ({ ...pin.bind(t), className: 'x' });
          return (
            <div>
              <button {...{ 'aria-pressed': on }}>a</button>
              <button {...PRESSED}>b</button>
              <button {...handlers('c')}>c</button>
              <g {...pin.bind('d')} />
              <circle {...wrapped('e')} />
              <button {...(on ? handlers('f') : {})}>f</button>
              <button {...{ onClick: () => {} }}>g</button>
            </div>
          );
        }`,
        ts.ScriptKind.TSX,
        binders
      ).sites.map((s) => `${s.kind} ${s.what}`)
    ).toEqual([
      'choice <button {…} aria-pressed>',
      'choice <button {…} aria-pressed>',
      'choice <button {…} aria-pressed>',
      'choice <g {…} aria-pressed>',
      'choice <circle {…} aria-pressed>',
      'choice <button {…} aria-pressed>',
    ]);
  });

  it('reads a hook’s binder from the object it returns: its aria-pressed, its literal role', () => {
    expect(
      hookBinders(`
        export function useReadout() {
          const isPinned = (t: string) => t === 'a';
          return {
            active: null,
            toggle: () => {},
            bind: (t: string) => ({ role: 'button', 'aria-pressed': isPinned(t), onClick: () => t }),
            hoverOnly: (t: string) => ({ onMouseEnter: () => t }),
          };
        }`)
    ).toEqual(
      new Map([
        ['bind', { pressed: true, roles: ['button'] }],
        ['hoverOnly', { pressed: false, roles: [] }],
      ])
    );
  });

  it('reads a role set in a spread: a tab, a radio', () => {
    expect(
      kinds(`
        export const A = () => (
          <div>
            <button {...{ role: 'tab' }}>a</button>
            <span {...{ role: 'radio', 'aria-checked': true }}>b</span>
            <span {...{ role: 'status' }}>c</span>
          </div>
        );`)
    ).toEqual(['tab <button {…} role="tab">', 'choice <span {…} role="radio">']);
  });

  it('counts a menu’s radio item and an input of type radio or checkbox as a choice', () => {
    expect(
      kinds(`
        export const A = ({ on }: { on: boolean }) => (
          <div role="menu">
            <button role="menuitemradio" aria-checked={on}>a</button>
            <input type="radio" name="x" />
            <input type={on ? 'checkbox' : 'radio'} />
            <input type="hidden" />
          </div>
        );`)
    ).toEqual([
      'choice <button role="menuitemradio">',
      'choice <input type="radio">',
      'choice <input type="checkbox|radio">',
    ]);
  });

  it('refuses a spread it cannot follow on an interactive element, until the site is named', () => {
    expect(
      kinds(`
        export function A(props: { rest: object }) {
          return (
            <div {...props.rest}>
              <button {...props.rest}>a</button>
              <a href="#" {...props.rest}>b</a>
              <Link href="/x" {...props.rest}>c</Link>
              <span onClick={() => {}} {...props.rest}>d</span>
              <svg {...props.rest} />
              <Card {...props.rest} />
            </div>
          );
        }`)
    ).toEqual([
      'spread <button {...props.rest}>',
      'spread <a {...props.rest}>',
      'spread <Link {...props.rest}>',
      'spread <span {...props.rest}>',
    ]);
  });

  // Lane cd-fixes-controls (T NOTE-3): the pager words it did not know, and a pager drawn by an
  // element other than a button or a link
  it('finds a pager by the words it did not know, and drawn by another element', () => {
    expect(
      kinds(`
        export const A = ({ go }: { go: () => void }) => (
          <div>
            <button>show more</button>
            <button>Next page</button>
            <button>‹ prev</button>
            <button>next ›</button>
            <button>more ↓</button>
            <span role="button" onClick={go}>older →</span>
            <TextButton onClick={go}>← newer</TextButton>
            <div>next</div>
            <Mono>newer</Mono>
          </div>
        );`)
    ).toEqual([
      'pager <button>show more</button>',
      'pager <button>Next page</button>',
      'pager <button>‹ prev</button>',
      'pager <button>next ›</button>',
      'pager <button>more ↓</button>',
      'pager <span>older →</span>',
      'pager <TextButton>← newer</TextButton>',
    ]);
  });

  // Lane cd-fixes-controls (the chart reviewer's N11): a retry written by hand, which the gate
  // passed as an action ("Retry" on a raw button with its own ink and underline, in four places)
  it('finds a retry written by hand: an action after a failed read, by its words', () => {
    expect(
      kinds(`
        export const A = ({ again }: { again: () => void }) => (
          <div>
            <button type="button" onClick={again} className="text-jade ml-2 underline">Retry</button>
            <button onClick={again}><span>↻ </span>try again</button>
            <span role="button" onClick={again}>Reload</span>
            <TextButton onClick={again}>retry</TextButton>
            <p>Retrying automatically.</p>
            <RetryAction words="Retry" onRetry={again} />
          </div>
        );`)
    ).toEqual([
      'retry <button>Retry</button>',
      // each child's words are read trimmed: the glyph's span "↻ " and its words "try again"
      'retry <button>↻try again</button>',
      'retry <span>Reload</span>',
      'retry <TextButton>retry</TextButton>',
    ]);
  });

  // Lane cd-fixes-controls (review-p-tests SHOULD-6): a disclosure written by hand, which the gate
  // did not read: FoldToggle is the one disclosure control, and a button that says whether it is
  // open (`aria-expanded`) outside the library is a fold by hand, its own words or not
  it('finds a disclosure by hand: aria-expanded, set or through a spread', () => {
    expect(
      kinds(`
        const TOGGLE = { onClick: () => {}, 'aria-expanded': false };
        export const A = ({ open }: { open: boolean }) => (
          <div>
            <button type="button" aria-expanded={open}>{open ? 'fewer ↑' : '3 more ↓'}</button>
            <button type="button" aria-expanded>fewer ↑</button>
            <tr {...TOGGLE} />
            <button type="button" aria-controls="x">plain</button>
          </div>
        );`)
    ).toEqual([
      'fold <button aria-expanded>',
      'fold <button aria-expanded>',
      'fold <tr {…} aria-expanded>',
    ]);
  });

  // Lane cd-fixes-controls (review-fix-tests SHOULD-4, the lead's ruling): a native disclosure is
  // a control like any other. A `<details>` takes its state from the browser and says nothing of
  // it, so `aria-expanded` never saw one; one disclosure is one site, its `<details>`, and a
  // `<summary>` written apart from its details (a part of one drawn elsewhere) is one too
  it('finds a native disclosure: a details, and a summary written apart from its details', () => {
    expect(
      kinds(`
        export const A = ({ open, more }: { open: boolean; more: boolean }) => (
          <div>
            <details open={open}>
              <summary className="text-text-dim">Raw data</summary>
              <pre />
            </details>
            <details>
              {more ? <summary>Cells · 3</summary> : <summary>Cells</summary>}
            </details>
          </div>
        );
        export const Part = () => <summary>Witness bytes</summary>;`)
    ).toEqual(['fold <details>', 'fold <details>', 'fold <summary>']);
  });

  // Lane cd-fixes-controls (review-p-tests NOTE-5): the choice roles it did not read
  it('counts a checkbox, a switch, an option and a menu’s checkbox item as a choice', () => {
    expect(
      kinds(`
        export const A = ({ on }: { on: boolean }) => (
          <div>
            <button role="checkbox" aria-checked={on}>a</button>
            <button role="switch" aria-checked={on}>b</button>
            <li role="option" aria-selected={on}>c</li>
            <button role="menuitemcheckbox" aria-checked={on}>d</button>
          </div>
        );`)
    ).toEqual([
      'choice <button role="checkbox">',
      'choice <button role="switch">',
      'choice <li role="option">',
      'choice <button role="menuitemcheckbox">',
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
  // the binders the hooks hand back (lane cd-fixes-controls, T SHOULD-2): usePinnedReadout's `bind`
  const binders = new Map<string, SpreadRead>();
  for (const file of files)
    if (rel(file).startsWith('hooks/'))
      for (const [name, read] of hookBinders(readFileSync(file, 'utf8'))) {
        const known = binders.get(name);
        binders.set(
          name,
          known
            ? { pressed: known.pressed || read.pressed, roles: [...known.roles, ...read.roles] }
            : read
        );
      }
  const found = new Map<string, Site[]>();
  const refused: string[] = [];
  for (const file of files) {
    const path = rel(file);
    if ((LIBRARY_FILES as readonly string[]).includes(path)) continue;
    const scan = scanControls(
      readFileSync(file, 'utf8'),
      file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
      binders
    );
    if (scan.sites.length) found.set(path, scan.sites);
    refused.push(...scan.refused.map((r) => `${path}:${r}`));
  }

  it('scans the source, and every control file is there', () => {
    expect(files.length).toBeGreaterThan(300);
    // the readout hook's binder, read from its source: a button that says it is pressed
    expect(binders.get('bind')).toEqual({ pressed: true, roles: ['button'] });
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
