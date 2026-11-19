import { readFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { sourceFiles, unwrap, type Binding } from '@/__tests__/utils/source-scan';
import { LINK_LOOKS } from '@/components/ui/link';

/**
 * Gate for the look of a link (the controls board, gate D0′, item "links"; lane c2-links): a link
 * says its job and the one owner of the looks draws it (components/ui/link.tsx: `key`, `inline`,
 * `onward`, `block`, `nav`). In `app/`, `components/`, `hooks/` and `lib/` it reads every link
 * element: `Link` and `AppLink`, `ExternalLink`, the shared renderers that draw one and pass their
 * caller's look through (`BlockLink`, `Address`), and a raw `<a>`. Three kinds of site:
 *
 * - `look`: a link component that states no `look`;
 * - `ink`: a link whose `className` or `style` writes a link ink or an underline by hand — a colour
 *   class (`text-<ink>` at any variant, `hover:text-jade` included: hover and focus are one rule in
 *   app/globals.css; an arbitrary `text-[color:…]` or `text-[var(…)]`), a decoration class
 *   (`underline`, `no-underline`, `overline`, `line-through`, `decoration-*`, `underline-offset-*`),
 *   or a `color` / `textDecoration…` key of a `style` object. A size step, an alignment or a wrap
 *   (`text-body`, `text-right`, `text-nowrap`) is no ink. Since lane cd-fixes-controls (T NOTE-2)
 *   the same is read on every element inside a link (`<Link><span className="text-jade
 *   hover:underline">`), one site a child, up to a link inside it, which is read on its own. The
 *   shell's navigation (`look="nav"` in `components/layout/`) writes its own design and is not
 *   counted;
 * - `raw`: an `<a>` outside the two link components (components/ui/link.tsx, external-link.tsx).
 *
 * A look is read through string literals, `?:`, `&&` and the file's own constants; a look handed
 * through from the component's own prop (`look={look}`, a prop with no default) is its caller's to
 * state. What it cannot read it refuses: a look that is none of the looks, a look it cannot resolve,
 * `nav` outside the shell (components/layout/), a way onward whose text types its own arrow (the
 * link writes it), and a link whose props are spread with no `look` after them.
 *
 * Limits, what a source gate cannot see: a link drawn by a component it does not name (a page's own
 * `MinerLink`, `PartyLink`: their inner `Link` is read in the file that defines them, their call
 * sites are not), and the callers of such a component when it hands a look through; an `<a>` made by
 * `createElement`, or held in a tag variable (the controls gate refuses a tag it cannot read); a
 * class held in a constant imported from another module; and the ink a link takes from its line
 * (the ink-role gate, `__tests__/lib/ink-role-gate.test.ts`, reads a link in the quiet ink: this
 * gate does not repeat it).
 *
 * The sites tolerated today are listed per file with their count per kind, the lane that takes them
 * (by page family: `p-chain`, `p-assets`, `p-lists`, `p-topics`, `d1-charts`, `shell`) and a
 * reason: the work list of the lanes that follow. A new site fails, and so does one that went until
 * its count is lowered; the list's total is pinned and only falls. A site kept by decision is NAMED
 * apart, per kind, with its reason, and held by the scan as the list is (lane cd-fixes-controls, T
 * NOTE-1: a named site that went fails until its count is lowered).
 */

type Kind = 'look' | 'ink' | 'raw';
const KINDS: readonly Kind[] = ['look', 'ink', 'raw'];

type Lane = 'p-chain' | 'p-assets' | 'p-lists' | 'p-topics' | 'd1-charts' | 'shell';

interface Tolerated {
  lane: Lane;
  sites: Partial<Record<Kind, number>>;
  reason: string;
}

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- p-chain ----
  // app/address/[addr]/client-page.tsx left the list (lane P1 follow-up): the address's JSON is a
  // way out of the site, an ExternalLink to the API's absolute URL (raw 1 → 0); look 5 → 4, ink
  // 2 → 1 in lane D2, look 4 → 0, ink 1 → 0 in lane P1
  // app/blocks/[id]/client-page.tsx left the list (lane P1 follow-up): the hardfork resources are
  // ExternalLinks in the onward look (raw 1 → 0, ink 1 → 0), a step arrow's mark takes its block
  // link's ink (ink 1 → 0); look 22 → 0, ink 7 → 1 in lanes D2 and P1, ink 1 → 2 at the merge with
  // cd-fixes-controls (the reader of an ink inside a link)
  'components/participant-ref.tsx': {
    // look 3 → 0 (lane P1 follow-up): its three links hand their caller's look through (`look`,
    // as an Address's), and the chain pages state theirs; the inks by hand stay for a caller that
    // states none, the feeds' call sites, which are the lists lane's
    lane: 'p-lists',
    sites: { ink: 2 },
    reason:
      "inks by hand where a caller states no look (hover:text-jade, text-text, text-jade-dim): the feeds' call sites",
  },
  // components/transaction/transaction-cell-content.tsx left the list (lane P1 follow-up): a token
  // amount's "no readable amount of" takes its link's ink (ink 1 → 0; lane P1 took the file's own
  // inks, the reader of an ink inside a link found this one)
  // 'components/transaction/transaction-list-table.tsx' left the list (lane P3: look 2, ink 3 →
  // 0; the hash a key's look, the Hash inheriting its ink; the blocks inline)
  // ---- p-assets ----
  // app/scripts/[name]/client-page.tsx left the list (lane p-assets: each link states its look: look 10 → 0, ink 11 → 0, raw 1 → 0)
  // app/tokens/[typeHash]/client-page.tsx left the list (lane p-assets: each link states its look: look 4 → 0, ink 2 → 0, raw 1 → 0)
  // components/asset/asset-feed.tsx left the list (lane p-assets: each link states its look: look 2 → 0)
  // components/asset/asset-history.tsx left the list (lane p-assets: each link states its look: look 6 → 0, ink 1 → 0)
  // components/asset/cell-versions.tsx left the list (lane p-assets: each link states its look: look 5 → 0, ink 3 → 0)
  // components/asset/collection-detail.tsx left the list (lane p-assets: each link states its look: look 2 → 0)
  // components/asset/collection-evidence.tsx left the list (lane p-assets: each link states its look: look 4 → 0)
  // components/asset/collection-objects.tsx left the list (lane p-assets: each link states its look: look 3 → 0, ink 1 → 0)
  // components/asset/detail-shell.tsx left the list (lane p-assets: each link states its look: look 1 → 0)
  'components/asset/genetics.tsx': {
    // look 4 → 0 (lane p-assets: each link states its look)
    lane: 'p-assets',
    sites: { ink: 2, raw: 2 },
    // the reason corrected (lane cd-fixes-controls, review-p-tests SHOULD-4): it said "4 without a look"
    reason: 'inks by hand (text-aqua); 2 raw <a> to a file of any scheme',
  },
  // components/asset/identity-document.tsx left the list (lane p-assets: each link states its look: look 1 → 0)
  // components/asset/inventory-summary.tsx left the list (lane p-assets: each link states its look: look 1 → 0)
  // components/asset/inventory-table.tsx left the list (lane p-assets: each link states its look: look 3 → 0, ink 3 → 0)
  // components/asset/item-evidence.tsx left the list (lane p-assets: each link states its look: look 3 → 0)
  'components/asset/mnft-detail.tsx': {
    // look 6 → 0 (lane p-assets: each link states its look)
    lane: 'p-assets',
    // look 8 → 6 (lane p-assets, R8): the issue sentence's two links, moved to the lede, are inline
    sites: { ink: 1, raw: 1 },
    // the reason corrected (lane cd-fixes-controls, review-p-tests SHOULD-4): it said "6 without a look"
    reason: 'inks by hand (text-aqua); 1 raw <a> to a file of any scheme',
  },
  'components/asset/object-media.tsx': {
    lane: 'p-assets',
    sites: { ink: 2, raw: 2 },
    reason: 'inks by hand (text-base-bg, text-copper); 2 raw <a>',
  },
  // components/asset/spore-detail.tsx left the list (lane p-assets: each link states its look: look 9 → 0)
  // components/asset/token-holders.tsx left the list (lane p-assets: each link states its look: look 2 → 0, ink 1 → 0)
  'components/identity/did-readout.tsx': {
    // look 1 → 0 (lane p-assets: each link states its look)
    lane: 'p-assets',
    sites: { ink: 1, raw: 1 },
    // the reason corrected (lane cd-fixes-controls, review-p-tests SHOULD-4): it said "1 without a look"
    reason: 'inks by hand (text-text-bright, hover:underline); 1 raw <a> to a file of any scheme',
  },
  // components/identity/dotcell-item-detail.tsx left the list (lane p-assets: each link states its look: look 4 → 0, ink 1 → 0)
  // components/identity/dotcell-party.tsx left the list (lane p-assets: each link states its look: look 2 → 0)
  // components/identity/dotcell-records-table.tsx left the list (lane p-assets: each link states its look: look 1 → 0, ink 1 → 0)
  // components/identity/dotcell-roles.tsx left the list (lane p-assets: each link states its look: look 1 → 0, ink 1 → 0)
  // components/identity/identity-evidence.tsx left the list (lane p-assets: each link states its look: look 6 → 0)
  // components/identity/identity-overview.tsx left the list (lane p-assets: each link states its look: look 5 → 0)
  // components/identity/identity-records.tsx left the list (lane p-assets: each link states its look: look 1 → 0)
  // components/scripts/script-code-evidence.tsx left the list (lane p-assets: each link states its look: look 3 → 0, ink 3 → 0)
  // components/scripts/script-family-board.tsx left the list (lane p-assets: each link states its look: look 3 → 0, ink 4 → 0, raw 1 → 0)
  // components/scripts/script-family-page.tsx left the list (lane p-assets: each link states its look: look 10 → 0, ink 11 → 0, raw 1 → 0)
  // components/scripts/script-newest-cells.tsx left the list (lane p-assets: each link states its look: look 5 → 0, ink 4 → 0)
  // 'components/ui/address.tsx' left the list (lane cd-fixes-controls, review-p-tests SHOULD-4: every
  // caller states its look, which is now required; the look it had before went, ink 1 → 0)
  // ---- p-lists ----
  // 'app/activities/page.tsx' left the list (lane P3: look 1, ink 1 → 0; the transactions link a way
  // onward)
  // 'app/blocks/page.tsx' left the list (lane P3: look 9, ink 5 → 0; a block number its row's key,
  // the miner, the paid block and the hardfork inline)
  // 'app/dao/depositors/page.tsx' left the list (lane P3: look 5, ink 2 → 0; a depositor its row's
  // key, the way to the DAO page onward, the blocks inline)
  // 'app/scripts/page.tsx' left the list (lane P3: look 1, ink 2, raw 1 → 0; a script's name its
  // row's key, "How it works" a way onward)
  // 'app/transactions/page.tsx' left the list (lane P3: look 3, ink 2 → 0; the activities link
  // a way onward, the pool's squares blocks, the pool's hash a key whose Hash inherits its ink)
  // 'components/activity-event-row.tsx' left the list (the P3 follow-up's 2c: look 5, ink 6 → 0;
  // every link of an item fact and a protocol note inline in its line, a token amount in its own
  // ink by the amount's rule, a dim symbol the line's ink)
  // 'components/activity/activity-row.tsx' left the list (lane P3: look 4, ink 4 → 0; the headline
  // its row's key, the evidence's block and transaction and a call's name inline in their lines)
  // 'components/crash-page.tsx' left the list (lane P3: look 1, ink 1 → 0; the way on a way onward)
  // 'components/error-page-shell.tsx' left the list (lane P3: look 1, ink 1 → 0; the seal caption's block inline)
  // 'components/home/activity-feed.tsx' left the list (lane P3: look 1, ink 1 → 0; "view all" a way onward)
  // 'components/home/activity-stats.tsx' left the list (lane P3: look 1, ink 1 → 0; "charts" a way onward)
  // 'components/home/ckbytes-section.tsx' left the list (lane P3: look 3, ink 2 → 0; "supply" the heading's key, its block inline, "DAO details" a way onward)
  // 'components/home/mini-chart.tsx' left the list (lane P3: look 1, ink 1 → 0; a card's title its key)
  // 'components/home/ranked-bars.tsx' left the list (lane P3: look 2, ink 1 → 0; the title its key, a row one block)
  // 'components/not-found-page.tsx' left the list (lane P3: look 5, ink 5 → 0; the ways on and the alternative ways onward)
  // 'components/pipeline/pipeline-panel.tsx' left the list for NAMED (the P3 follow-up's 2c: look
  // 3, ink 2, the owner's accepted pipeline design)
  // ---- p-topics ----
  // the Fiber channel and channels pages left the list (lane P4: look 1, ink 1 and look 3, ink
  // 4, raw 1 → 0)
  // 'app/forks/[height]/[orphan]/client-page.tsx' left the list (lane P4: look 10, ink 7 → 0)
  // 'app/forks/page.tsx' left the list (lane P4: look 3, ink 4, raw 1 → 0)
  // 'app/hardforks/page.tsx' left the list (lane P4: look 3, ink 4, raw 1 → 0)
  // 'app/network/client-page.tsx' left the list (lane P4: ink 1, raw 1 → 0)
  // the DAO page's board, its growth section and its sources left the list (lane P4: look 13,
  // ink 8, raw 1; look 1; look 2, ink 2 → 0)
  // 'components/deep-fork-alert.tsx' left the list (lane P4: look 1, ink 1 → 0, the button the
  // block look, its words' ink on a span inside it), and came back when the reader learned an ink
  // written inside a link (lane cd-fixes-controls, T NOTE-2): ink 1
  'components/deep-fork-alert.tsx': {
    lane: 'p-topics',
    sites: { ink: 1 },
    reason:
      'a link drawn as a filled button: its words’ ink, the one its fill calls for, written on a span inside it (style color), which no look carries',
  },
  // the Fiber board, step and graph left the list (lane P4: look 11, ink 11 → 0)
  // 'components/forks/miner-link.tsx' left the list (lane cd-fixes-controls, review-p-tests
  // SHOULD-4: its five callers state their look, which is now required; its former ink went, 1 → 0)
  // ---- shell ----
  'components/stats-bar.tsx': {
    lane: 'shell',
    sites: { look: 2, ink: 5 },
    reason:
      '2 without a look; five inks on children: the epoch link’s label, value and position spans (text-text-dim, text-text-bright with group-hover:text-jade, text-text)',
  },
};

/** The pinned total of TOLERATED's sites: the list only shrinks. */
// 610 → 588 (lane D1: the chart pages' 22 sites state their look) → 577 (lane D2, 11 more): the
// transaction page's facts rail and its block link went (1), the block page's facts aside and its
// two links (2), the cell page's Created block link (2: look, ink), the address page's kind-line
// lock link and its Lock metric's (4: two looks, two inks), the uncle page's canonical-hash link
// (1), the identity owner line's address (1) → 559 (lane D4, 18 more: the phone rows' keys, block
// links and names state their look — transactions 4, pool 2, block transactions 2, scripts 2, a
// script's cells 2, inventory 4, Fiber channels 2) → 557 (lane c2-finish, rulings round 1: the .cell
// party's lock-hash link states `block` and its hand-written underline went, 2) → 554 (lane D4
// follow-up: the asset feed's phone row, its key and its block link, 3: two looks, one ink) → 551
// (the links the phone rows share with the tables, paysNote and proposedIn, inline: 3, two looks
// and one ink) → 434: page lane P4, 103 (the fork page's 17 and MinerLink's look 1, the forks
// page's 8, the deep-fork alert's 2; the hardforks page, 14: its edition block link and the
// overview links inline, "How it works" a way onward and no raw <a>, 8; the deployment
// evidence's three links inline, 6; the Fiber channel page, 16: the breadcrumb's link inline,
// 2, the board's party, lock-script, commitment-cell and output links, 10, each `inline` or
// `key`, the step's transaction `key` and block `inline`, 4; the Fiber channels page, 16: the
// list's channel `key`, its parties `inline` in the table and the phone row, the lede's way
// down `onward` (the raw anchor and its typed arrow gone), 8, the graph's party and block
// links, 8; the network page, 2: its lede's "How it works" a Link of the onward look, one
// ink and one raw <a>; the DAO page, 27: its blocks inline, a depositor its row's key, the
// ways onward (the lede's, the charts', every depositor's, the activity lists', the source
// block's and the anchor's) onward with no typed arrow and no raw <a>, the DAO script's code
// hash a key), and lane D3, 14 (the transaction page's script-group code links moved to
// script-group-rows.tsx stating their look, 8: two looks, four inks, two raw; the
// structure's group link went, 2; the cell page's transaction links, 4: two looks, two inks)
// → 265: page lane P2 (p-assets), 169: the m-NFT issue sentence's two links inline (2); a
// collection's items list, its name a key, its holder and its block inline (4); a script's
// newest cells' footer, two ways onward (4); the script pages, 44 (app/scripts/[name] and
// script-family-page, each link its look); the asset, identity, token and script components,
// 115 (23 entries left the list; genetics, mnft-detail, object-media and did-readout keep their
// raw anchors to a file of any scheme and their inks) → 110: page lane P1 (p-chain), 155: the
// transaction page's lede (its item names, the cellbase's paid block, its own block, which the
// strip states, 10), the block page's lede and cellbase row (2), the cell page's origin line and
// its code-cell link (8), and every link of the chain pages' files states its look, inks by hand
// gone (135 in seventeen files, fifteen leaving the list; the address's API JSON and the block's
// hardfork resources keep their raw anchors) → 121 (lane cd-fixes-controls, T NOTE-2: the reader
// learned an ink written on an element inside a link; with the lanes' merges, eleven stand — the
// transactions page's and the transaction list's Hash in the bright ink, the activity row's span
// in the quiet ink, the status line's epoch link's five label, value and position spans, the
// deep-fork alert's words on its filled button, a transaction cell's "no readable amount of", the
// block page's step arrow mark; TabLinks' name span is NAMED) → 111 (lane P3: the transactions
// page, 5, and its list, 5, every link its look, the Hash of a row's key inheriting its link's ink)
// → 109 (lane P3: the activities page's way onward, 2) → 105 (lane P3: the scripts list, 4) → 91
// (lane P3: the home page's five files, 14) → 77 (lane P3: the error pages' three files, 14) → 63
// (lane P3: the blocks list, 14) → 55 (lane P3: the activity row, 8) → 48 (lane P3: the DAO
// depositors, 7) → 40 (the merge with lane P1's follow-up, 8: participant-ref.tsx's three links
// hand their caller's look through, 3; the address's API JSON and the block's hardfork resources
// are ExternalLinks, 2, the resources' hand ink with them, 1; the block's step arrow mark and a
// token cell's "no readable amount of" take their link's ink, 2) → 24 (the P3 follow-up, 16: the
// item facts and protocol notes of activity-event-row.tsx, 11, its 2c; the pipeline panel's 5,
// NAMED: the owner's accepted design) → 22 (the merge with lane cd-fixes-controls,
// review-p-tests SHOULD-4: Address's and MinerLink's fallback inks, 2, for callers that state no
// look; every caller states one, and the look is required)
const TOLERATED_TOTAL = 22;

/** Sites kept by decision, by file: their count per kind and why. */
interface Named {
  sites: Partial<Record<Kind, number>>;
  reason: string;
}

const NAMED: Readonly<Record<string, Named>> = {
  'components/layout/wordmark.tsx': {
    sites: { raw: 1 },
    reason:
      'the error page’s bare home link (`plain`): it renders without the router, and its href stays `/` where Link would prefix the network',
  },
  'components/pipeline/pipeline-panel.tsx': {
    sites: { look: 3, ink: 2 },
    reason:
      'the owner’s accepted pipeline design (homepage pipeline finish, F and G, merged 2026-11-18): the tip’s flaring block link, the pinned tx or block’s aqua "open tx ↗" with its own rule, and the heading’s "all blocks →" in the interactive ink are drawn as the approved board drew them',
  },
  'components/ui/tabs.tsx': {
    sites: { ink: 1 },
    reason:
      'TabLinks’ stated design: a tab’s name span carries its state (bright where current, a zero quiet, else the reading ink) and its hover and focus jade from the whole 32 px target (group-hover, group-focus-visible); its link is `block`, which draws no ink',
  },
};

/**
 * The pinned total of NAMED's sites: 1, and TabLinks' name since the reader learned a child's ink,
 * and the pipeline panel's 5 (the P3 follow-up's 2c).
 */
const NAMED_TOTAL = 7;

/** The files that draw the one `<a>` of a link (they own it). */
const LINK_FILES = ['components/ui/link.tsx', 'components/ui/external-link.tsx'];

interface Site {
  kind: Kind;
  line: number;
  what: string;
}

interface Scan {
  sites: Site[];
  refused: string[];
}

/** The link components: each states its look (the last two hand their caller's look through). */
const LINK_COMPONENTS = new Set([
  'Link',
  'AppLink',
  'ExternalLink',
  'BlockLink',
  'Address',
  // a miner's link, its caller's look handed through (lane cd-fixes-controls, review-p-tests
  // SHOULD-4: a MinerLink with no look drew a hand ink the gate counted once, in its own file)
  'MinerLink',
]);

/** A type step, an alignment or a wrap: a `text-` class that is no ink. */
const NOT_INK_TEXT =
  /^text-(meta|small|body|section|hero|index|entity|display|left|center|right|justify|start|end|wrap|nowrap|balance|pretty|ellipsis|clip)$/;

/** Whether a class (its variants and `!` aside) writes a link ink or an underline. */
function isLinkInk(cls: string): boolean {
  let depth = 0;
  let start = 0;
  for (let i = 0; i < cls.length; i++) {
    if (cls[i] === '[') depth++;
    else if (cls[i] === ']') depth--;
    else if (cls[i] === ':' && depth === 0) start = i + 1;
  }
  const u = cls.slice(start).replace(/^!/, '');
  if (/^(underline|no-underline|overline|line-through)$/.test(u)) return true;
  if (/^(decoration|underline-offset)-/.test(u)) return true;
  if (!u.startsWith('text-')) return false;
  if (NOT_INK_TEXT.test(u)) return false;
  // an arbitrary size (`text-[13px]`, `text-[length:…]`) is no ink; a colour or a variable is
  const arbitrary = /^text-\[(.+)\]$/.exec(u);
  if (arbitrary) return !/^(\d|length:)/.test(arbitrary[1]);
  return true;
}

/** The link elements a source writes, and what it could not read; `path` places the shell. */
function scanLinks(
  code: string,
  path: string,
  scriptKind: ts.ScriptKind = ts.ScriptKind.TSX
): Scan {
  const source = ts.createSourceFile('scan.tsx', code, ts.ScriptTarget.Latest, true, scriptKind);
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const sites: Site[] = [];
  const refused: string[] = [];
  const refuse = (n: ts.Node, why: string) => refused.push(`${lineOf(n)}: ${why}`);
  const shell = path.startsWith('components/layout/');

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
        ts.isCallExpression(init)
      )
        bind(node.name.text, { kind: 'component' });
      else bind(node.name.text, { kind: 'value', values: [init] });
      if (init) initializers.set(node.name.text, init);
    }
    if ((ts.isBindingElement(node) || ts.isParameter(node)) && ts.isIdentifier(node.name))
      bind(node.name.text, { kind: 'prop', defaults: node.initializer ? [node.initializer] : [] });
    ts.forEachChild(node, collect);
  };
  const initializers = new Map<string, ts.Expression>();
  collect(source);

  /**
   * Whether a `style` sets a link's ink: a `color`, a `textDecoration…` or the boards' `--ink` key of
   * an object, or the boards' `inkStyle(…)`, read through `?:` and the file's constants; null where
   * it cannot be read.
   */
  const styleInks = (e: ts.Expression, seen: ReadonlySet<string> = new Set()): boolean | null => {
    const x = unwrap(e);
    if (ts.isObjectLiteralExpression(x))
      return x.properties.some(
        (p) =>
          ts.isPropertyAssignment(p) &&
          /^['"]?(color|textDecoration\w*|--ink)['"]?$/.test(p.name.getText(source))
      );
    if (ts.isCallExpression(x) && ts.isIdentifier(x.expression) && x.expression.text === 'inkStyle')
      return true;
    if (ts.isConditionalExpression(x)) {
      const a = styleInks(x.whenTrue, seen);
      const b = styleInks(x.whenFalse, seen);
      return a === null || b === null ? null : a || b;
    }
    if (ts.isIdentifier(x) && x.text === 'undefined') return false;
    if (ts.isIdentifier(x) && !seen.has(x.text) && initializers.has(x.text))
      return styleInks(initializers.get(x.text)!, new Set([...seen, x.text]));
    return null;
  };

  /** The strings an expression can be: null where it cannot be read, 'through' for a prop's own. */
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
    if (ts.isIdentifier(x) && !seen.has(x.text)) {
      const found = bindings.get(x.text);
      if (!found) return null;
      const out: string[] = [];
      for (const b of found) {
        if (b.kind === 'component') return null;
        if (b.kind === 'prop' && b.defaults.length === 0) return ['through'];
        for (const v of b.kind === 'value' ? b.values : b.defaults) {
          const s = strings(v, new Set([...seen, x.text]));
          if (s === null) return null;
          out.push(...s);
        }
      }
      return out;
    }
    return null;
  };

  /** Every word of a class expression: its string pieces, through `cn(…)`, `?:`, templates, names. */
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

  /** The readings of an element's own text, `…` for what an expression writes. */
  const textsOf = (children: ts.NodeArray<ts.JsxChild>): string[] => {
    let readings = [''];
    const append = (alternatives: string[]) => {
      readings = readings.flatMap((r) => alternatives.map((a) => r + a)).slice(0, 32);
    };
    for (const child of children) {
      if (ts.isJsxText(child)) append([child.text]);
      else if (ts.isJsxExpression(child) && child.expression)
        append(strings(child.expression)?.filter((s) => s !== 'through') ?? ['…']);
      else if (ts.isJsxElement(child) || ts.isJsxFragment(child)) append(textsOf(child.children));
    }
    return readings.map((r) => r.trim());
  };

  const visitElement = (
    node: ts.JsxOpeningElement | ts.JsxSelfClosingElement,
    children: ts.NodeArray<ts.JsxChild> | null
  ) => {
    if (!ts.isIdentifier(node.tagName)) return;
    const name = node.tagName.text;
    const raw =
      name === 'a' ||
      (/^[A-Z]/.test(name) &&
        !LINK_COMPONENTS.has(name) &&
        (bindings.get(name) ?? []).some(
          (b) => b.kind === 'value' && b.values.some((v) => (strings(v) ?? []).includes('a'))
        ));
    if (!raw && !LINK_COMPONENTS.has(name)) return;
    const tag = raw ? 'a' : name;
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

    let looks: string[] = [];
    if (raw) sites.push({ kind: 'raw', line: lineOf(node), what: '<a>' });
    else {
      const look = attr('look');
      const lastSpread = attrs.reduce((at, a, i) => (ts.isJsxSpreadAttribute(a) ? i : at), -1);
      if (lastSpread >= 0 && (!look || attrs.indexOf(look) < lastSpread)) {
        refuse(node, 'a link whose props are spread with no look after them');
        return;
      }
      if (!look) sites.push({ kind: 'look', line: lineOf(node), what: `<${tag}>` });
      else {
        const v = valueOf(look);
        const read = v && strings(v);
        if (!read) {
          refuse(node, `a look the scan cannot read: ${v ? v.getText(source) : '(none)'}`);
          return;
        }
        looks = read.filter((s) => s !== 'through');
        const wrong = looks.find((s) => !(LINK_LOOKS as readonly string[]).includes(s));
        if (wrong !== undefined) {
          refuse(node, `"${wrong}" is not a look (${LINK_LOOKS.join(', ')})`);
          return;
        }
        if (looks.includes('nav') && !shell) {
          refuse(node, `the nav look is the shell’s (components/layout/), not ${path}'s`);
          return;
        }
        if (
          looks.includes('onward') &&
          children &&
          textsOf(children).some((t) => /[→↗↓↑←]$/.test(t))
        )
          refuse(node, 'a way onward types its own arrow (the link writes it)');
      }
    }

    // the shell's navigation writes its own design (a raw anchor of the shell is counted as raw)
    if (shell && (raw || (looks.length > 0 && looks.every((l) => l === 'nav')))) return;
    const cls = attr('className');
    const clsValue = cls && valueOf(cls);
    const inked = clsValue ? classTokens(clsValue).filter(isLinkInk) : [];
    const style = attr('style');
    const styleValue = style && valueOf(style);
    const styleInk = styleValue ? styleInks(styleValue) : false;
    if (styleInk === null)
      refuse(node, `a link's style the scan cannot read: ${styleValue!.getText(source)}`);
    if (inked.length || styleInk)
      sites.push({
        kind: 'ink',
        line: lineOf(node),
        what: `<${tag}> ${[...inked, ...(styleInk ? ['style color'] : [])].join(' ')}`,
      });

    // an ink or an underline written on an element inside the link (lane cd-fixes-controls, T
    // NOTE-2), one site a child, up to a link inside it, which is read on its own
    const isLink = (n: ts.JsxOpeningElement | ts.JsxSelfClosingElement) =>
      ts.isIdentifier(n.tagName) && (n.tagName.text === 'a' || LINK_COMPONENTS.has(n.tagName.text));
    const childInks = (n: ts.Node) => {
      if (ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n)) {
        const open = ts.isJsxElement(n) ? n.openingElement : n;
        if (isLink(open)) return;
        const own = (key: string) =>
          open.attributes.properties.find(
            (a): a is ts.JsxAttribute =>
              ts.isJsxAttribute(a) && ts.isIdentifier(a.name) && a.name.text === key
          );
        const childCls = own('className');
        const childClsValue = childCls && valueOf(childCls);
        const childInked = childClsValue ? classTokens(childClsValue).filter(isLinkInk) : [];
        const childStyle = own('style');
        const childStyleValue = childStyle && valueOf(childStyle);
        const childStyleInk = childStyleValue ? styleInks(childStyleValue) : false;
        if (childStyleInk === null)
          refuse(
            open,
            `a style inside a link the scan cannot read: ${childStyleValue!.getText(source)}`
          );
        if (childInked.length || childStyleInk)
          sites.push({
            kind: 'ink',
            line: lineOf(open),
            what: `<${tag}> > <${open.tagName.getText(source)}> ${[
              ...childInked,
              ...(childStyleInk ? ['style color'] : []),
            ].join(' ')}`,
          });
        if (ts.isJsxElement(n)) n.children.forEach(childInks);
        return;
      }
      ts.forEachChild(n, childInks);
    };
    if (children) children.forEach(childInks);
  };

  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node)) visitElement(node.openingElement, node.children);
    else if (ts.isJsxSelfClosingElement(node)) visitElement(node, null);
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { sites, refused };
}

/** The scan's count against the list: every file's count per kind must equal its tolerated one. */
function toleratedFailures(
  found: ReadonlyMap<string, readonly Site[]>,
  tolerated: Readonly<Record<string, Tolerated>>,
  named: Readonly<Record<string, Named>> = {}
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
          `${path}: ${of.length} ${kind} site(s), ${allowed} tolerated — a new one: ${of
            .map((s) => `${s.line} ${s.what}`)
            .join(', ')} (state the link's look: components/ui/link.tsx)`
        );
      if (of.length < allowed)
        out.push(
          `${path}: ${of.length} ${kind} site(s), ${allowed} tolerated — lower its count to ${of.length}`
        );
    }
    for (const listed of [entry, kept])
      if (listed && listed.reason.trim().length <= 10)
        out.push(`${path}: a reason too short to say why: "${listed.reason}"`);
  }
  return out;
}

const read = (code: string, path = 'components/x.tsx') =>
  scanLinks(code, path).sites.map((s) => `${s.kind} ${s.what}`);

describe('the link scan finds what it must, and only that', () => {
  it('finds a link component that states no look, and passes one that states one', () => {
    expect(
      read(`
        export const A = ({ on, look }: { on: boolean; look: 'key' | 'inline' }) => (
          <p>
            <Link href="/a">a</Link>
            <AppLink href="/b">b</AppLink>
            <ExternalLink href="https://c.example">c</ExternalLink>
            <BlockLink number={7} />
            <Address address="ckb1q" />
            <Link href="/d" look="key">d</Link>
            <Link href="/e" look={on ? 'onward' : 'inline'}>e</Link>
            <BlockLink number={8} look={look} />
          </p>
        );`)
    ).toEqual([
      'look <Link>',
      'look <AppLink>',
      'look <ExternalLink>',
      'look <BlockLink>',
      'look <Address>',
    ]);
  });

  it('finds a link ink or an underline written by hand, at any variant, by class or by style', () => {
    expect(
      read(`
        const INK = 'text-text-bright font-mono';
        export const A = ({ on }: { on: boolean }) => (
          <p>
            <Link href="/a" look="key" className="text-text hover:text-jade">a</Link>
            <Link href="/b" look="inline" className={cn('font-mono', on && 'hover:underline')}>b</Link>
            <Link href="/c" look="key" className={INK}>c</Link>
            <Link href="/d" look="inline" className="text-[color:var(--ink)]">d</Link>
            <Link href="/e" look="inline" className="decoration-aqua/35 underline-offset-[3px]">e</Link>
            <Link href="/f" look="inline" style={{ color: 'var(--c-text)' }}>f</Link>
            <Link href="/g" look="key" className="md:!text-text-dim">g</Link>
            <Link href="/h" look="inline" style={inkStyle(BOARD_INKS.link)}>h</Link>
            <Link href="/i" look="inline" style={CELL_LINK}>i</Link>
            <Link href="/j" look="block" style={{ '--ink': 'var(--c-jade)' }}>j</Link>
          </p>
        );
        const CELL_LINK = inkStyle(BOARD_INKS.cellLink);`)
    ).toEqual([
      'ink <Link> text-text hover:text-jade',
      'ink <Link> hover:underline',
      'ink <Link> text-text-bright',
      'ink <Link> text-[color:var(--ink)]',
      'ink <Link> decoration-aqua/35 underline-offset-[3px]',
      'ink <Link> style color',
      'ink <Link> md:!text-text-dim',
      'ink <Link> style color',
      'ink <Link> style color',
      'ink <Link> style color',
    ]);
  });

  it('passes a size, an alignment, a wrap, a face and a layout', () => {
    expect(
      read(`
        export const A = () => (
          <Link href="/a" look="key" className="text-body sm:text-small text-right text-nowrap font-mono block whitespace-nowrap p-3 transition-colors text-[13px]">
            a
          </Link>
        );`)
    ).toEqual([]);
  });

  it('finds a raw anchor, and its ink', () => {
    expect(
      read(`
        export const A = () => (
          <p>
            <a href="#how">how it works</a>
            <a href="/x" className="hover:text-text-bright hover:underline">x</a>
          </p>
        );`)
    ).toEqual(['raw <a>', 'raw <a>', 'ink <a> hover:text-text-bright hover:underline']);
  });

  // Lane cd-fixes-controls (T NOTE-2): a link's ink or underline written on a child walked past
  it('finds a link ink or an underline written on a child, not past a link inside it', () => {
    expect(
      read(`
        export const A = ({ on }: { on: boolean }) => (
          <p>
            <Link href="/a" look="inline"><span className="text-jade hover:underline">a</span></Link>
            <Link href="/b" look="key"><Hash className="text-text-bright" value="0x1" /></Link>
            <a href="#c"><span style={{ color: 'var(--c-text)' }}>c</span></a>
            <Link href="/d" look="key"><span className="font-mono text-body">d</span></Link>
            <Link href="/e" look="key">{on && <span className="text-text-dim">e</span>}</Link>
            <Link href="/f" look="key">
              <Link href="/g" look="key" className="text-text">g</Link>
            </Link>
          </p>
        );`)
    ).toEqual([
      'ink <Link> > <span> text-jade hover:underline',
      'ink <Link> > <Hash> text-text-bright',
      'raw <a>',
      'ink <a> > <span> style color',
      'ink <Link> > <span> text-text-dim',
      'ink <Link> text-text',
    ]);
  });

  it('lets the shell’s navigation write its own design, in the shell alone', () => {
    const nav = `
      export const N = () => (
        <Link href="/blocks" look="nav" className="text-text hover:text-jade lowercase">blocks</Link>
      );`;
    expect(read(nav, 'components/layout/header.tsx')).toEqual([]);
    expect(scanLinks(nav, 'components/asset/x.tsx').refused).toEqual([
      expect.stringMatching(/^3: the nav look is the shell’s \(components\/layout\/\), not /),
    ]);
  });

  it('refuses what it cannot read', () => {
    const scan = scanLinks(
      `
      import { pick } from './x';
      export function A(props: { rest: object }) {
        return (
          <p>
            <Link href="/a" look="bright">a</Link>
            <Link href="/b" look={pick()}>b</Link>
            <Link href="/c" look="onward">all 522 holders →</Link>
            <Link {...props.rest} href="/d" />
            <ExternalLink href="https://e.example" look="onward">{'e ↗'}</ExternalLink>
            <Link href="/f" look="key" style={pick()}>f</Link>
          </p>
        );
      }`,
      'components/x.tsx'
    );
    expect(scan.refused).toEqual([
      expect.stringMatching(/^6: "bright" is not a look/),
      expect.stringMatching(/^7: a look the scan cannot read: pick\(\)/),
      expect.stringMatching(/^8: a way onward types its own arrow/),
      expect.stringMatching(/^9: a link whose props are spread with no look after them/),
      expect.stringMatching(/^10: a way onward types its own arrow/),
      expect.stringMatching(/^11: a link's style the scan cannot read: pick\(\)/),
    ]);
  });
});

describe('the tolerated list keys a file by its count per kind, so it only shrinks', () => {
  const site = (kind: Kind, line: number): Site => ({ kind, line, what: '<Link>' });
  const list: Record<string, Tolerated> = {
    'app/a.tsx': { lane: 'p-lists', sites: { look: 2 }, reason: 'two links to give a look' },
  };

  it('passes the same count on other lines', () => {
    expect(
      toleratedFailures(new Map([['app/a.tsx', [site('look', 3), site('look', 9)]]]), list)
    ).toEqual([]);
  });

  it('fails a new site in a listed file or an unlisted one, and one that went until lowered', () => {
    expect(
      toleratedFailures(
        new Map([
          ['app/a.tsx', [site('look', 3), site('look', 9), site('ink', 12)]],
          ['app/b.tsx', [site('raw', 4)]],
        ]),
        list
      )
    ).toEqual([
      expect.stringMatching(/^app\/a\.tsx: 1 ink site\(s\), 0 tolerated — a new one: 12 /),
      expect.stringMatching(/^app\/b\.tsx: 1 raw site\(s\), 0 tolerated — a new one: 4 /),
    ]);
    expect(toleratedFailures(new Map([['app/a.tsx', [site('look', 3)]]]), list)).toEqual([
      'app/a.tsx: 1 look site(s), 2 tolerated — lower its count to 1',
    ]);
  });

  // Re-pinned (lane cd-fixes-controls, T NOTE-1): a site named by decision is counted per kind,
  // and held by the scan as the tolerated list is: one that went fails until its count is lowered
  it('counts a site named by decision apart from the list, and holds it as the list', () => {
    const named = {
      'app/b.tsx': { sites: { raw: 1 }, reason: 'a bare home link outside the router' },
    };
    expect(toleratedFailures(new Map([['app/b.tsx', [site('raw', 4)]]]), {}, named)).toEqual([]);
    expect(toleratedFailures(new Map(), {}, named)).toEqual([
      'app/b.tsx: 0 raw site(s), 1 tolerated — lower its count to 0',
    ]);
    expect(
      toleratedFailures(
        new Map([['app/b.tsx', [site('raw', 4)]]]),
        {},
        {
          'app/b.tsx': { sites: { raw: 1 }, reason: 'kept' },
        }
      )
    ).toEqual(['app/b.tsx: a reason too short to say why: "kept"']);
  });
});

describe('every link states its look (lane c2-links)', () => {
  const root = join(__dirname, '..', '..');
  const rel = (file: string) => relative(root, file).split(sep).join('/');
  const files = sourceFiles(root);
  const found = new Map<string, Site[]>();
  const refused: string[] = [];
  for (const file of files) {
    const path = rel(file);
    if (LINK_FILES.includes(path)) continue;
    const scan = scanLinks(
      readFileSync(file, 'utf8'),
      path,
      file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
    );
    if (scan.sites.length) found.set(path, scan.sites);
    refused.push(...scan.refused.map((r) => `${path}:${r}`));
  }

  it('scans the source, and knows the owner’s looks', () => {
    expect(files.length).toBeGreaterThan(300);
    expect(LINK_LOOKS).toEqual(['key', 'inline', 'onward', 'block', 'nav']);
  });

  it('reads every link it meets', () => {
    expect(refused).toEqual([]);
  });

  it('writes no link without its look, with a hand-written ink or raw, but the list’s', () => {
    expect(toleratedFailures(found, TOLERATED, NAMED)).toEqual([]);
  });

  it(`tolerates ${TOLERATED_TOTAL} sites in all: the pinned total, which only falls`, () => {
    const total = Object.values(TOLERATED).reduce(
      (sum, entry) => sum + Object.values(entry.sites).reduce((a, n) => a + (n ?? 0), 0),
      0
    );
    expect(total).toBe(TOLERATED_TOTAL);
  });

  it(`names ${NAMED_TOTAL} sites kept by decision, each with its reason: the pinned total`, () => {
    expect(
      Object.values(NAMED).reduce(
        (sum, entry) => sum + Object.values(entry.sites).reduce((a, n) => a + (n ?? 0), 0),
        0
      )
    ).toBe(NAMED_TOTAL);
  });
});
