import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
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
 * - `prose`: a paragraph, `<p>`.
 *
 * An element carries the quiet ink by its own class (`text-text-dim` at rest: with no variant, or a
 * responsive one; `hover:text-text-dim` is a pointer's state; an arbitrary value naming the quiet
 * ink's variable, `text-[color:var(--c-text-dim)]`), by its own `style` colour (`color('text-dim')`
 * or a string naming that variable; `alpha(…)` and `fade(…)` are other inks) or, when it sets no
 * ink of its own, by the nearest ink its JSX ancestors in the same file set around it (a link in a
 * dim note). A class or a colour is read through `?:`, `&&`, `cn(…)`, template literals and the
 * file's own constants, so one that is quiet in one branch counts. An element handed to a
 * component as a prop is the component's to ink, and is not followed into it; a `quiet` class on
 * the element or an ancestor exempts it (the set-back row draws every word quiet on purpose), and
 * every element that carries that class is pinned by file in QUIET_SITES, so an exemption is a
 * decision too.
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
 * them are not followed); a component that inks its children (the ancestor walk sees only the
 * elements of the file that carry a `className` or a `style`); ink set in CSS (`.prose-tier2 p`,
 * `.comment` in app/globals.css; lane F3 step 2 takes them, and a CSS check should then hold
 * them); a `style` that is not an object literal; and, because the list counts sites per kind and
 * not by name, a new site that replaces a fixed one of the same kind in the same file.
 */

type Kind = 'link' | 'amount' | 'td' | 'prose';
const KINDS: readonly Kind[] = ['link', 'amount', 'td', 'prose'];

/** The lane whose pages a file belongs to (the hand-back groups the list by them). */
type Family = 'chain' | 'assets' | 'lists' | 'topics' | 'shared';

interface Tolerated {
  family: Family;
  /** The sites of each kind in the file, today. */
  sites: Partial<Record<Kind, number>>;
  /** Why they stand, in one line: what they are and who takes them. */
  reason: string;
}

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- chain ----
  'app/address/[addr]/client-page.tsx': {
    family: 'chain',
    sites: { prose: 4 },
    reason: 'section sentences and an empty state in the quiet ink: explaining sentences read',
  },
  'app/blocks/[id]/client-page.tsx': {
    family: 'chain',
    // link 13 → 11 (lane D2): the facts aside's "Paid in" and "Paying cellbase" links went with it
    sites: { link: 11, prose: 7 },
    reason:
      'block heights, hashes and the hardfork link in quiet facts and proposal rows; empty and loading lines; the proposal notes',
  },
  'app/blocks/[id]/uncles/[index]/client-page.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason: 'the empty proposals sentence',
  },
  'app/blocks/page.tsx': {
    family: 'chain',
    // td 5 → 4 (readability F3-e): the 608 px table's age is the first line of its "Age · txs" cell
    sites: { link: 3, td: 4, prose: 2 },
    reason:
      'ages, reward-finality heights, cycles states and HF links quiet in table cells and rows; the loading and error lines',
  },
  'app/cell/[outpoint]/client-page.tsx': {
    family: 'chain',
    // link 4 → 1, prose 2 → 1 (B7): the out-point sentence reads in the reading ink
    sites: { link: 1, prose: 1 },
    reason: 'the DAO line and its deposit block link in the quiet ink',
  },
  'app/tx/[hash]/client-page.tsx': {
    family: 'chain',
    sites: { link: 6, prose: 12 },
    reason:
      'cell and dep links, the waiting and empty lines, the witness and dep notes in the quiet ink',
  },
  'components/address/address-history.tsx': {
    family: 'chain',
    sites: { link: 1, td: 1 },
    reason: "a cell's cycles state in a table cell and its script links in a quiet line",
  },
  'components/address/address-holdings.tsx': {
    family: 'chain',
    // prose 7 → 6 (B7): the balance's parts are labelled facts, no longer a quiet <p>
    sites: { link: 4, prose: 6 },
    reason: 'deposit block and cell links, empty holdings and the collection note in the quiet ink',
  },
  'components/blocks/block-position.tsx': {
    family: 'chain',
    sites: { link: 1, prose: 7 },
    reason:
      'the position sentence with its block links, the proposal-origin lines and the uncle notes in the quiet ink',
  },
  'components/blocks/block-transactions-table.tsx': {
    family: 'chain',
    sites: { td: 2 },
    reason: "a transaction's index and its cycles state quiet in table cells",
  },
  'components/blocks/block-visual.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason:
      'the size-and-cycles sentence under the blocks figure (a note under a figure, or a reading: P1 decides)',
  },
  'components/blocks/reward-breakdown.tsx': {
    family: 'chain',
    sites: { link: 3, prose: 1 },
    reason: 'the reward sentence and its finality, payment and cellbase links in the quiet ink',
  },
  'components/cell-graph-renderer.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason:
      "the graph's empty state (an empty state reads; a placeholder is the rule's open point)",
  },
  'components/cell-graph.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason: "the graph's loading placeholder (the rule's open point: loading lines)",
  },
  'components/cell/cell-microscope.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason: 'the empty-data sentence',
  },
  'components/cell/cell-transactions.tsx': {
    family: 'chain',
    sites: { link: 1, prose: 2 },
    reason: 'the loading line, the as-of block link and the code-cell note in the quiet ink',
  },
  'components/cell/code-cell-board.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason: 'the empty version-history sentence',
  },
  'components/home/ckbytes-section.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason:
      'the CKBytes formula under the figure (a note under a figure, or a reading: P1 decides)',
  },
  'components/home/ranked-bars.tsx': {
    family: 'chain',
    sites: { link: 1 },
    reason: "the ranked list's title link in the quiet ink",
  },
  'components/transaction/script-group-matrix.tsx': {
    family: 'chain',
    sites: { link: 1, prose: 1 },
    reason: 'a group link and the runs line in the quiet ink',
  },
  'components/transaction/tx-structure.tsx': {
    family: 'chain',
    sites: { prose: 1 },
    reason: "the structure's legend line (a note under a figure: P1 decides)",
  },
  'components/transaction/what-moved.tsx': {
    family: 'chain',
    sites: { link: 1 },
    reason: "an object's id link in the quiet ink",
  },
  // ---- assets ----
  'app/tokens/[typeHash]/client-page.tsx': {
    family: 'assets',
    sites: { prose: 4 },
    reason:
      "the token's description, label provenance and empty and unavailable sentences in the quiet ink",
  },
  'components/asset/asset-feed.tsx': {
    family: 'assets',
    sites: { td: 1, prose: 2 },
    reason: "a row's cycles state in a table cell; the empty feed sentences",
  },
  'components/asset/asset-history.tsx': {
    family: 'assets',
    sites: { link: 1, prose: 4 },
    reason:
      'a supply event tx link, the empty and peak lines and the history notes in the quiet ink',
  },
  'components/asset/cell-versions.tsx': {
    family: 'assets',
    sites: { link: 4, prose: 1 },
    reason:
      "a version's block, owner, lock and spending-tx links and the empty sentence in the quiet ink",
  },
  'components/asset/collection-detail.tsx': {
    family: 'assets',
    sites: { prose: 5 },
    reason:
      'the collection sentence, empty and unrecorded states and the identity count line in the quiet ink',
  },
  'components/asset/collection-evidence.tsx': {
    family: 'assets',
    sites: { prose: 1 },
    reason: 'the no-description sentence',
  },
  'components/asset/collection-gallery.tsx': {
    family: 'assets',
    sites: { prose: 1 },
    reason: 'the empty gallery sentence',
  },
  'components/asset/collection-holders.tsx': {
    family: 'assets',
    sites: { prose: 2 },
    reason: "the full collection's counts and the holder-list note in the quiet ink",
  },
  'components/asset/collection-objects.tsx': {
    family: 'assets',
    sites: { link: 1, prose: 4 },
    reason:
      "an object's block link, the decoding state lines and the empty sentence in the quiet ink",
  },
  'components/asset/detail-shell.tsx': {
    family: 'assets',
    sites: { link: 1, prose: 1 },
    reason: 'a breadcrumb link and the read-state sentence in the quiet ink',
  },
  'components/asset/dropped-genes-caption.tsx': {
    family: 'assets',
    sites: { prose: 1 },
    reason: 'the dropped-genes caption (a note under a figure: P2 decides)',
  },
  'components/asset/genetics.tsx': {
    family: 'assets',
    sites: { link: 3, td: 5, prose: 14 },
    reason:
      'gene offsets, rules and found values quiet in table cells; decoder links; the DNA state lines and notes',
  },
  'components/asset/identity-detail.tsx': {
    family: 'assets',
    sites: { prose: 1 },
    reason: 'the no-expiry sentence',
  },
  'components/asset/identity-document.tsx': {
    family: 'assets',
    sites: { prose: 1 },
    reason: 'the configure bits sentence',
  },
  'components/asset/mnft-detail.tsx': {
    family: 'assets',
    sites: { prose: 4 },
    reason:
      'the renderer URL, the class and set counts and the composition and lock notes in the quiet ink',
  },
  'components/asset/object-hero.tsx': {
    family: 'assets',
    sites: { prose: 1 },
    reason: "the traits line under the object's figure (a note, or a sentence: P2 decides)",
  },
  'components/asset/object-media.tsx': {
    family: 'assets',
    sites: { prose: 2 },
    reason: 'the inscription and decode-state sentences',
  },
  'components/asset/protocol-evidence.tsx': {
    family: 'assets',
    sites: { prose: 5 },
    reason:
      "the empty-data, interpretation and more-bytes lines, a reading's words and the hint line in the quiet ink",
  },
  'components/asset/spore-detail.tsx': {
    family: 'assets',
    sites: { link: 2, prose: 2 },
    reason:
      'the cluster holder sentence with its address and block links; the burned-object sentence',
  },
  'components/identity/did-readout.tsx': {
    family: 'assets',
    sites: { prose: 2 },
    reason: 'the unread-fields and servers sentences in the quiet ink',
  },
  'components/identity/dotcell-item-detail.tsx': {
    family: 'assets',
    sites: { prose: 2 },
    reason: 'the sub-names and top-level-name lines',
  },
  'components/identity/dotcell-records-table.tsx': {
    family: 'assets',
    sites: { td: 2 },
    reason:
      "a record's label and TTL quiet in raw <td>s (the label stays quiet; the TTL is a number)",
  },
  'components/identity/identity-evidence.tsx': {
    family: 'assets',
    sites: { link: 1, prose: 1 },
    reason: 'the decoded-from sentence with its tx link in the quiet ink',
  },
  'components/identity/identity-records.tsx': {
    family: 'assets',
    sites: { prose: 1 },
    reason: 'the records and TTL count line in the quiet ink',
  },
  // ---- lists ----
  'app/activities/page.tsx': {
    family: 'lists',
    sites: { link: 1, prose: 2 },
    reason: 'the transactions link and the statistics error and empty sentences in the quiet ink',
  },
  'app/inventory/inventory-page-client.tsx': {
    family: 'lists',
    // link 1 → 0 (controls lane C2): the class links are the controls board's tab row
    sites: { prose: 1 },
    reason: 'the loading line in the quiet ink',
  },
  'app/scripts/[name]/client-page.tsx': {
    family: 'lists',
    sites: { td: 3, prose: 6 },
    reason:
      'custody, version and evidence states in table cells; the resolution, empty, loading and capacity sentences (script pages: P3 with the scripts list, or P2 with R7)',
  },
  'app/scripts/page.tsx': {
    family: 'lists',
    // link 1 → 0, prose 2 → 1 (B7): the lede and its link read in the reading ink
    sites: { prose: 1 },
    reason: "the code-class bar's caption in the quiet ink",
  },
  'app/transactions/page.tsx': {
    family: 'lists',
    sites: { link: 1, td: 2, prose: 11 },
    reason:
      'interpretation states in table cells; the activities link; the statistics, pool and list sentences and notes',
  },
  'components/activities-stream-explorer.tsx': {
    family: 'lists',
    sites: { prose: 5 },
    reason: 'the period note, the reading, error and empty lines in the quiet ink',
  },
  'components/asset/inventory-summary.tsx': {
    family: 'lists',
    sites: { link: 1, prose: 2 },
    reason: "the inventory's totals sentences and the anchor block link in the quiet ink",
  },
  'components/asset/inventory-table.tsx': {
    family: 'lists',
    sites: { td: 5 },
    reason: 'supply, H-multiplier, occupied and unavailable values quiet in table cells',
  },
  'components/scripts/script-code-evidence.tsx': {
    family: 'lists',
    sites: { link: 2, prose: 1 },
    reason: "the unspendable note and the code-cell links (the code-class bar's words are F1's)",
  },
  'components/scripts/script-family-board.tsx': {
    family: 'lists',
    sites: { prose: 1 },
    reason: 'the handover line under the holder figure (a note under a figure: P3 decides)',
  },
  'components/scripts/script-family-page.tsx': {
    family: 'lists',
    // link 4 → 3, prose 6 → 5 (B7): the lede and its link read in the reading ink
    sites: { link: 3, td: 9, prose: 5 },
    reason:
      'deployment blocks, cells, dashes and states quiet in table cells and links; the reference, loading, empty and cells sentences',
  },
  'components/scripts/script-hero.tsx': {
    family: 'lists',
    sites: { prose: 1 },
    reason: "the script hero's companion line in the quiet ink",
  },
  'components/scripts/script-newest-cells.tsx': {
    family: 'lists',
    sites: { link: 1, prose: 2 },
    reason: "a cell's tx link, the loading line and the caption in the quiet ink",
  },
  'components/transaction/transaction-list-table.tsx': {
    family: 'lists',
    sites: { td: 3 },
    reason: 'the cellbase reward, the cells count and the cycles state quiet in table cells',
  },
  // ---- topics ----
  'app/dao/depositors/page.tsx': {
    family: 'topics',
    sites: { link: 2, td: 1, prose: 1 },
    reason: "a depositor's rank in a table cell; the ranking-moved sentence and its block links",
  },
  'app/fiber/channels/[id]/client-page.tsx': {
    family: 'topics',
    sites: { link: 1, prose: 2 },
    reason: 'the breadcrumb link, the channel id line and the loading line in the quiet ink',
  },
  'app/fiber/channels/client-page.tsx': {
    family: 'topics',
    sites: { td: 1, prose: 2 },
    reason: "a channel row's time in a table cell; the loading lines",
  },
  // 'app/forks/[height]/[orphan]/client-page.tsx' left the list (lane P4): the breadcrumb link
  // takes its look, a row's clock and the uncle sentence read in the reading ink
  'app/forks/page.tsx': {
    family: 'topics',
    // prose 2 → 1 (lane P4): the empty fork-history line reads in the reading ink
    sites: { prose: 1 },
    reason: 'the loading fork-history line, a transient note',
  },
  'app/hardforks/page.tsx': {
    family: 'topics',
    // prose 10 → 5 (B7): the lede, the two editions paragraphs, the hash-type paragraph and the
    // RFC note explain, in the reading ink; link 2 → 0, prose 5 → 1 (page lane P4): an edition's
    // epoch, block and day line reads in the reading ink, its block a link inside the line; "How
    // it works" is a way onward; the no-header and not-reached states read; the consensus line
    // (the RPC's name) went to tier 2
    sites: { prose: 1 },
    reason: 'the loading line, a transient note while the timeline is read',
  },
  'app/network/advertisement-graph.tsx': {
    family: 'topics',
    // prose 4 → 3 (B7): the advertisement note explains, in the reading ink
    sites: { prose: 3 },
    reason: 'the loading, refresh and empty lines in the quiet ink',
  },
  'app/network/client-page.tsx': {
    family: 'topics',
    // link 1 → 0, prose 8 → 3 (B7): the lede, the crawler explanations and the note explain, in
    // the reading ink
    sites: { prose: 3 },
    reason: 'the round lines and the MaxMind credit in the quiet ink',
  },
  'app/network/distributions.tsx': {
    family: 'topics',
    sites: { prose: 2 },
    reason: 'the empty line and the exact counts line under a distribution',
  },
  'app/network/network-map.tsx': {
    family: 'topics',
    sites: { prose: 2 },
    reason: 'the loading line and the one-node countries line under the map',
  },
  'app/network/nodes-table.tsx': {
    family: 'topics',
    sites: { prose: 9 },
    reason: "a node's observation sentences and empty states in its detail",
  },
  'app/network/trends.tsx': {
    family: 'topics',
    // prose 5 → 4 (B7): the top-label note explains, in the reading ink
    sites: { td: 1, prose: 4 },
    reason: 'a bucket count in a table cell; the loading, empty and round lines',
  },
  'components/dao/dao-board.tsx': {
    family: 'topics',
    // link 6 → 3, prose 3 → 1 (B7): the lede and the AR explanation, with their links, read in
    // the reading ink; link 3 → 2 (B4 second fix round): a depositor's lock is written by the one
    // decision (LockWordsView, components/ui/lock-kind.tsx), its link no longer this file's
    sites: { link: 2, td: 2, prose: 1 },
    reason:
      "the exact amounts line, the block links in the readout and anchor lines, a holder's rank and share in table cells",
  },
  'components/dao/dao-grows.tsx': {
    family: 'topics',
    sites: { link: 1 },
    reason: "the block link in the cycle readout's idle line",
  },
  'components/dao/dao-history.tsx': {
    family: 'topics',
    sites: { prose: 1 },
    reason: 'the not-enough-days sentence',
  },
  'components/fiber/fiber-channel-board.tsx': {
    family: 'topics',
    // prose 5 → 2 (B7): the lede and the lock and output notes explain, in the reading ink
    sites: { link: 1, prose: 2 },
    reason: 'the channel id, the empty commitment sentence, a party link',
  },
  'components/fiber/fiber-channel-step.tsx': {
    family: 'topics',
    sites: { link: 1, prose: 1 },
    reason: "a step's block link and its loading line",
  },
  'components/fiber/fiber-network-graph.tsx': {
    family: 'topics',
    sites: { link: 2, prose: 1 },
    reason: "a pair's party links in its readout and the empty-graph sentence",
  },
  // ---- shared ----
  'components/activity/activity-row.tsx': {
    family: 'shared',
    sites: { link: 3 },
    reason: "B4 rows: the row's block and tx links and a type-call link in the quiet ink",
  },
  'components/command-palette.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason:
      "the shortcuts panel's release and clock line: chrome, the rule's open point (B1 hand-back)",
  },
  // components/error-page-shell.tsx left the list (readability R6): the caption under the seal,
  // a note under a figure, quiet by rule, is the seal's <figcaption> now that the seal stands in a
  // figure of its own beside or below the text, no longer a <p> (this gate's other route for it).
  'components/layout/site-footer.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason: "the footer's release and zone line: chrome, the rule's open point (B1 hand-back)",
  },
  'components/ui/allele-gel.tsx': {
    family: 'shared',
    sites: { prose: 3 },
    reason:
      "the gel's two reading notes and its not-drawn note: notes under a figure, quiet by rule",
  },
  'components/ui/detail-layout.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason: "the evidence tier's eyebrow, a label set as a <p>: quiet by rule",
  },
  'components/ui/lock-kind.tsx': {
    family: 'shared',
    sites: { link: 1 },
    reason: "B4 rows: an unregistered lock's code-hash link inside the quiet lock-kind label",
  },
  'components/ui/script-byte-map.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason: "the byte map's caption: a note under a figure, quiet by rule",
  },
  'components/ui/sequencing-gel.tsx': {
    family: 'shared',
    sites: { prose: 1 },
    reason: "the gel's reading note: a note under a figure, quiet by rule",
  },
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
  const collect = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer)
      locals.set(node.name.text, [...(locals.get(node.name.text) ?? []), node.initializer]);
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
    const tokens = value ? tokensOf(value) : [];
    const inks = tokens.map(inkOfClass);
    const colour = styleColour(n);
    return {
      quiet: tokens.includes('quiet'),
      dim: inks.includes('dim') || (colour !== undefined && quietColour(colour)),
      inked: colour !== undefined || inks.some((i) => i !== null),
    };
  };

  /**
   * The ink an element takes from around it, in its file: the nearest JSX ancestor that sets one.
   * `quiet` for a set-back subtree, `dim`, `other`, or `none` (nothing in the file sets one, or the
   * element is a prop handed to a component).
   */
  const around = (node: ts.Node): 'quiet' | 'dim' | 'other' | 'none' => {
    for (let p: ts.Node | undefined = node.parent; p; p = p.parent) {
      if (ts.isJsxAttribute(p)) return 'none';
      if (ts.isJsxElement(p)) {
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
      if (ts.isJsxAttribute(p)) return false;
      if (ts.isJsxElement(p) && classes(p).quiet) return true;
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

  it('passes a set-back row, a state variant, a prop handed to a component and a label', () => {
    expect(
      sitesOf([
        '<tr className="quiet"><td className="text-text-dim">1</td><td><Link href="/z">z</Link></td></tr>;',
        '<div className="quiet text-text-dim"><p>context</p></div>;',
        '<Link href="/x" className="text-text hover:text-text-dim">x</Link>;',
        '<div className="text-text-dim"><Tile end={<Link href="/q">q</Link>} /></div>;',
        '<dt className="text-text-dim">Height</dt>;',
        '<span className="text-text-dim">occupied</span>;',
        '<div className="text-text-dim"><p className="text-text">explains</p></div>;',
        '<div className="text-text-dim"><Link href="/i" className="text-[color:var(--ink)]">i</Link></div>;',
      ])
    ).toEqual([]);
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

  it('draws no link, amount, table cell or paragraph in the quiet ink but the tolerated list’s', () => {
    expect(toleratedFailures(found, TOLERATED)).toEqual([]);
  });

  it('sets back no element with the quiet class but the pinned list’s', () => {
    expect(quietFailures(quiet, QUIET_SITES)).toEqual([]);
  });
});
