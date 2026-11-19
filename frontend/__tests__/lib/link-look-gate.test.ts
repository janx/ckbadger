import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
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
 *   (`text-body`, `text-right`, `text-nowrap`) is no ink. The shell's navigation (`look="nav"` in
 *   `components/layout/`) writes its own design and is not counted;
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
 * its count is lowered; the list's total is pinned and only falls. A raw `<a>` kept by decision is
 * NAMED apart, with its reason.
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
  'app/address/[addr]/client-page.tsx': {
    lane: 'p-chain',
    sites: { look: 5, ink: 2, raw: 1 },
    reason: '5 without a look; inks by hand (text-text-bright); 1 raw <a>',
  },
  'app/blocks/[id]/client-page.tsx': {
    lane: 'p-chain',
    sites: { look: 22, ink: 7, raw: 1 },
    reason:
      '22 without a look; inks by hand (text-text-dim, text-text-bright, text-interactive); 1 raw <a>',
  },
  'app/blocks/[id]/uncles/[index]/client-page.tsx': {
    lane: 'p-chain',
    sites: { look: 6 },
    reason: '6 without a look',
  },
  'app/cell/[outpoint]/client-page.tsx': {
    lane: 'p-chain',
    sites: { look: 15, ink: 8 },
    reason: '15 without a look; inks by hand (text-text-bright, text-text, hover:text-text-bright)',
  },
  'app/tx/[hash]/client-page.tsx': {
    lane: 'p-chain',
    sites: { look: 17, ink: 16, raw: 2 },
    reason:
      '17 without a look; inks by hand (hover:text-jade, text-text-bright, text-text); 2 raw <a>',
  },
  'components/address/address-history.tsx': {
    lane: 'p-chain',
    sites: { look: 7, ink: 1 },
    reason: '7 without a look; inks by hand (text-text-dim, hover:text-jade)',
  },
  'components/address/address-holdings.tsx': {
    lane: 'p-chain',
    sites: { look: 11, ink: 6, raw: 1 },
    reason: '11 without a look; inks by hand (text-text-bright, hover:text-jade); 1 raw <a>',
  },
  'components/blocks/block-position.tsx': {
    lane: 'p-chain',
    sites: { look: 12, ink: 1 },
    reason: '12 without a look; inks by hand (text-text-bright)',
  },
  'components/blocks/block-transactions-table.tsx': {
    lane: 'p-chain',
    sites: { look: 3, ink: 2 },
    reason: '3 without a look; inks by hand (hover:underline)',
  },
  'components/blocks/reward-breakdown.tsx': {
    lane: 'p-chain',
    sites: { look: 3 },
    reason: '3 without a look',
  },
  'components/cell/cell-transactions.tsx': {
    lane: 'p-chain',
    sites: { look: 5, ink: 4 },
    reason: '5 without a look; inks by hand (hover:text-jade, text-[color:var(--ink)], style)',
  },
  'components/cell/code-cell-board.tsx': {
    lane: 'p-chain',
    sites: { look: 5, ink: 4 },
    reason: '5 without a look; inks by hand (text-[color:var(--ink)], style, color)',
  },
  'components/cell/inventory-line.tsx': {
    lane: 'p-chain',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text-bright, hover:text-jade)',
  },
  'components/participant-ref.tsx': {
    lane: 'p-chain',
    sites: { look: 3, ink: 2 },
    reason: '3 without a look; inks by hand (hover:text-jade, text-text, text-jade-dim)',
  },
  'components/transaction/script-group-matrix.tsx': {
    lane: 'p-chain',
    sites: { look: 3, ink: 3 },
    reason: '3 without a look; inks by hand (hover:text-jade, text-text-bright, text-text)',
  },
  'components/transaction/transaction-cell-content.tsx': {
    lane: 'p-chain',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (hover:text-jade)',
  },
  'components/transaction/transaction-cell-tiles.tsx': {
    lane: 'p-chain',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text, hover:text-jade)',
  },
  'components/transaction/transaction-list-table.tsx': {
    lane: 'p-chain',
    sites: { look: 3, ink: 3 },
    reason: '3 without a look; inks by hand (hover:text-jade, hover:underline)',
  },
  'components/transaction/tx-structure.tsx': {
    lane: 'p-chain',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text-bright, hover:text-jade)',
  },
  'components/transaction/what-moved.tsx': {
    lane: 'p-chain',
    sites: { look: 2, ink: 2 },
    reason: '2 without a look; inks by hand (hover:text-jade, text-text-dim)',
  },
  // ---- p-assets ----
  'app/scripts/[name]/client-page.tsx': {
    lane: 'p-assets',
    sites: { look: 10, ink: 11, raw: 1 },
    reason: '10 without a look; inks by hand (text-emphasis, hover:underline); 1 raw <a>',
  },
  'app/tokens/[typeHash]/client-page.tsx': {
    lane: 'p-assets',
    sites: { look: 4, ink: 2, raw: 1 },
    reason: '4 without a look; inks by hand (text-jade, hover:text-jade); 1 raw <a>',
  },
  'components/asset/asset-feed.tsx': {
    lane: 'p-assets',
    sites: { look: 4, ink: 1 },
    reason: '4 without a look; inks by hand (text-text-bright)',
  },
  'components/asset/asset-history.tsx': {
    lane: 'p-assets',
    sites: { look: 6, ink: 1 },
    reason: '6 without a look; inks by hand (text-text-dim)',
  },
  'components/asset/cell-versions.tsx': {
    lane: 'p-assets',
    sites: { look: 5, ink: 3 },
    reason: '5 without a look; inks by hand (text-text-dim, text-text-bright)',
  },
  'components/asset/collection-detail.tsx': {
    lane: 'p-assets',
    sites: { look: 2 },
    reason: '2 without a look',
  },
  'components/asset/collection-evidence.tsx': {
    lane: 'p-assets',
    sites: { look: 4 },
    reason: '4 without a look',
  },
  'components/asset/collection-gallery.tsx': {
    lane: 'p-assets',
    sites: { look: 3, ink: 1 },
    reason: '3 without a look; inks by hand (text-text-bright)',
  },
  'components/asset/collection-objects.tsx': {
    lane: 'p-assets',
    sites: { look: 3, ink: 1 },
    reason: '3 without a look; inks by hand (text-text-bright)',
  },
  'components/asset/detail-shell.tsx': {
    lane: 'p-assets',
    sites: { look: 1 },
    reason: '1 without a look',
  },
  'components/asset/genetics.tsx': {
    lane: 'p-assets',
    sites: { look: 4, ink: 2, raw: 2 },
    reason: '4 without a look; inks by hand (text-aqua); 2 raw <a>',
  },
  'components/asset/identity-document.tsx': {
    lane: 'p-assets',
    sites: { look: 1 },
    reason: '1 without a look',
  },
  'components/asset/inventory-summary.tsx': {
    lane: 'p-assets',
    sites: { look: 1 },
    reason: '1 without a look',
  },
  'components/asset/inventory-table.tsx': {
    lane: 'p-assets',
    sites: { look: 5, ink: 5 },
    reason: '5 without a look; inks by hand (text-text-bright)',
  },
  'components/asset/item-evidence.tsx': {
    lane: 'p-assets',
    sites: { look: 3 },
    reason: '3 without a look',
  },
  'components/asset/mnft-detail.tsx': {
    lane: 'p-assets',
    sites: { look: 8, ink: 1, raw: 1 },
    reason: '8 without a look; inks by hand (text-aqua); 1 raw <a>',
  },
  'components/asset/object-media.tsx': {
    lane: 'p-assets',
    sites: { ink: 2, raw: 2 },
    reason: 'inks by hand (text-base-bg, text-copper); 2 raw <a>',
  },
  'components/asset/spore-detail.tsx': {
    lane: 'p-assets',
    sites: { look: 9 },
    reason: '9 without a look',
  },
  'components/asset/token-holders.tsx': {
    lane: 'p-assets',
    sites: { look: 2, ink: 1 },
    reason: '2 without a look; inks by hand (text-text-bright)',
  },
  'components/identity/did-readout.tsx': {
    lane: 'p-assets',
    sites: { look: 1, ink: 1, raw: 1 },
    reason: '1 without a look; inks by hand (text-text-bright, hover:underline); 1 raw <a>',
  },
  'components/identity/dotcell-item-detail.tsx': {
    lane: 'p-assets',
    sites: { look: 4, ink: 1 },
    reason: '4 without a look; inks by hand (text-emphasis, hover:underline)',
  },
  'components/identity/dotcell-party.tsx': {
    lane: 'p-assets',
    sites: { look: 3, ink: 1 },
    reason: '3 without a look; inks by hand (hover:underline)',
  },
  'components/identity/dotcell-records-table.tsx': {
    lane: 'p-assets',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text-bright, decoration-aqua/35, underline)',
  },
  'components/identity/dotcell-roles.tsx': {
    lane: 'p-assets',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (hover:text-jade)',
  },
  'components/identity/identity-evidence.tsx': {
    lane: 'p-assets',
    sites: { look: 6 },
    reason: '6 without a look',
  },
  'components/identity/identity-overview.tsx': {
    lane: 'p-assets',
    sites: { look: 6 },
    reason: '6 without a look',
  },
  'components/identity/identity-records.tsx': {
    lane: 'p-assets',
    sites: { look: 1 },
    reason: '1 without a look',
  },
  'components/scripts/script-code-evidence.tsx': {
    lane: 'p-assets',
    sites: { look: 3, ink: 3 },
    reason:
      '3 without a look; inks by hand (hover:underline, text-emphasis, hover:text-text-bright)',
  },
  'components/scripts/script-family-board.tsx': {
    lane: 'p-assets',
    sites: { look: 3, ink: 4, raw: 1 },
    reason:
      '3 without a look; inks by hand (hover:text-jade, hover:text-text-bright, hover:underline); 1 raw <a>',
  },
  'components/scripts/script-family-page.tsx': {
    lane: 'p-assets',
    sites: { look: 11, ink: 12, raw: 1 },
    reason:
      '11 without a look; inks by hand (hover:text-jade, text-text, hover:underline); 1 raw <a>',
  },
  'components/scripts/script-newest-cells.tsx': {
    lane: 'p-assets',
    sites: { look: 6, ink: 6, raw: 1 },
    reason: '6 without a look; inks by hand (hover:text-jade, text-text, text-text-dim); 1 raw <a>',
  },
  'components/ui/address.tsx': {
    lane: 'p-assets',
    sites: { ink: 1 },
    reason:
      'the look an Address had before (bright, a faint indigo underline), drawn while a caller states none: it goes with the last such call site (most are p-assets)',
  },
  // ---- p-lists ----
  'app/activities/page.tsx': {
    lane: 'p-lists',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text-dim, hover:text-jade)',
  },
  'app/blocks/page.tsx': {
    lane: 'p-lists',
    sites: { look: 9, ink: 5 },
    reason: '9 without a look; inks by hand (text-text-bright, hover:text-jade, text-text-dim)',
  },
  'app/dao/depositors/page.tsx': {
    lane: 'p-lists',
    sites: { look: 5, ink: 2 },
    reason: '5 without a look; inks by hand (hover:underline, text-text-bright, text-text)',
  },
  'app/scripts/page.tsx': {
    lane: 'p-lists',
    sites: { look: 2, ink: 3, raw: 1 },
    reason:
      '2 without a look; inks by hand (hover:underline, text-text-bright, hover:text-emphasis); 1 raw <a>',
  },
  'app/transactions/page.tsx': {
    lane: 'p-lists',
    sites: { look: 3, ink: 1 },
    reason: '3 without a look; inks by hand (text-text-dim, hover:text-jade)',
  },
  'components/activity-event-row.tsx': {
    lane: 'p-lists',
    sites: { look: 5, ink: 5 },
    reason: '5 without a look; inks by hand (hover:text-jade, text-text-bright, text-positive)',
  },
  'components/activity/activity-row.tsx': {
    lane: 'p-lists',
    sites: { look: 4, ink: 4 },
    reason: '4 without a look; inks by hand (hover:text-jade, text-text-dim, hover:text-text)',
  },
  'components/crash-page.tsx': {
    lane: 'p-lists',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text, hover:text-jade)',
  },
  'components/error-page-shell.tsx': {
    lane: 'p-lists',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text)',
  },
  'components/home/activity-feed.tsx': {
    lane: 'p-lists',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-interactive, hover:text-jade)',
  },
  'components/home/activity-stats.tsx': {
    lane: 'p-lists',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-interactive, hover:text-jade)',
  },
  'components/home/ckbytes-section.tsx': {
    lane: 'p-lists',
    sites: { look: 3, ink: 2 },
    reason: '3 without a look; inks by hand (hover:text-jade, text-interactive)',
  },
  'components/home/mini-chart.tsx': {
    lane: 'p-lists',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text-bright, hover:text-jade)',
  },
  'components/home/ranked-bars.tsx': {
    lane: 'p-lists',
    sites: { look: 2, ink: 1 },
    reason: '2 without a look; inks by hand (text-text-dim, hover:text-jade)',
  },
  'components/not-found-page.tsx': {
    lane: 'p-lists',
    sites: { look: 5, ink: 5 },
    reason: '5 without a look; inks by hand (text-text, hover:text-jade, text-jade)',
  },
  'components/pipeline/pipeline-panel.tsx': {
    lane: 'p-lists',
    sites: { look: 3, ink: 2 },
    reason: '3 without a look; inks by hand (text-aqua, no-underline, text-interactive)',
  },
  // ---- p-topics ----
  'app/fiber/channels/[id]/client-page.tsx': {
    lane: 'p-topics',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (hover:text-text)',
  },
  'app/fiber/channels/client-page.tsx': {
    lane: 'p-topics',
    sites: { look: 4, ink: 5, raw: 1 },
    reason:
      '4 without a look; inks by hand (hover:underline, hover:text-jade, text-text-bright); 1 raw <a>',
  },
  'app/forks/[height]/[orphan]/client-page.tsx': {
    lane: 'p-topics',
    sites: { look: 10, ink: 7 },
    reason: '10 without a look; inks by hand (hover:text-jade, hover:underline, text-emphasis)',
  },
  'app/forks/page.tsx': {
    lane: 'p-topics',
    sites: { look: 3, ink: 4, raw: 1 },
    reason:
      '3 without a look; inks by hand (text-text, hover:text-jade, hover:underline); 1 raw <a>',
  },
  'app/hardforks/page.tsx': {
    lane: 'p-topics',
    sites: { look: 3, ink: 4, raw: 1 },
    reason:
      '3 without a look; inks by hand (hover:text-text-bright, hover:underline, text-text); 1 raw <a>',
  },
  'app/network/client-page.tsx': {
    lane: 'p-topics',
    sites: { ink: 1, raw: 1 },
    reason: 'inks by hand (hover:text-text-bright, hover:underline); 1 raw <a>',
  },
  'components/dao/dao-board.tsx': {
    lane: 'p-topics',
    sites: { look: 13, ink: 8, raw: 1 },
    reason:
      '13 without a look; inks by hand (hover:underline, hover:text-text-bright, text-text); 1 raw <a>',
  },
  'components/dao/dao-grows.tsx': {
    lane: 'p-topics',
    sites: { look: 1 },
    reason: '1 without a look',
  },
  'components/dao/dao-sources.tsx': {
    lane: 'p-topics',
    sites: { look: 2, ink: 2 },
    reason: '2 without a look; inks by hand (text-text, hover:text-text-bright, hover:underline)',
  },
  'components/deep-fork-alert.tsx': {
    lane: 'p-topics',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (style, color)',
  },
  'components/fiber/fiber-channel-board.tsx': {
    lane: 'p-topics',
    sites: { look: 5, ink: 5 },
    reason: '5 without a look; inks by hand (text-[color:var(--ink)], style, color)',
  },
  'components/fiber/fiber-channel-step.tsx': {
    lane: 'p-topics',
    sites: { look: 2, ink: 2 },
    reason: '2 without a look; inks by hand (text-[color:var(--ink)], hover:underline, style)',
  },
  'components/fiber/fiber-network-graph.tsx': {
    lane: 'p-topics',
    sites: { look: 4, ink: 4 },
    reason: '4 without a look; inks by hand (hover:text-jade, text-text-bright)',
  },
  'components/forks/miner-link.tsx': {
    lane: 'p-topics',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text-bright, hover:text-jade)',
  },
  'components/hardforks/script-deployment-timeline.tsx': {
    lane: 'p-topics',
    sites: { look: 3, ink: 3 },
    reason: '3 without a look; inks by hand (text-text, hover:text-text-bright, hover:underline)',
  },
  // ---- d1-charts ----
  'app/charts/page.tsx': {
    lane: 'd1-charts',
    sites: { ink: 1, raw: 1 },
    reason: 'inks by hand (text-text-dim, hover:text-jade); 1 raw <a>',
  },
  'components/charts/blocks-of-day.tsx': {
    lane: 'd1-charts',
    sites: { look: 2, ink: 2 },
    reason: '2 without a look; inks by hand (text-jade, hover:underline)',
  },
  'components/charts/chart-detail.tsx': {
    lane: 'd1-charts',
    sites: { look: 4, ink: 5, raw: 1 },
    reason: '4 without a look; inks by hand (text-jade, hover:underline, text-text-dim); 1 raw <a>',
  },
  'components/charts/chart-small-multiple.tsx': {
    lane: 'd1-charts',
    sites: { look: 2, ink: 2 },
    reason: '2 without a look; inks by hand (hover:text-jade, text-text-bright)',
  },
  'components/charts/topic-charts.tsx': {
    lane: 'd1-charts',
    sites: { look: 1, ink: 1 },
    reason: '1 without a look; inks by hand (text-text, hover:text-jade)',
  },
  // ---- shell ----
  'components/stats-bar.tsx': {
    lane: 'shell',
    sites: { look: 2 },
    reason: '2 without a look',
  },
};

/** The pinned total of TOLERATED's sites: the list only shrinks. */
const TOLERATED_TOTAL = 604;

/** Raw anchors kept by decision, by file: their count and why. */
const NAMED_RAW: Readonly<Record<string, { count: number; reason: string }>> = {
  'components/layout/wordmark.tsx': {
    count: 1,
    reason:
      'the error page’s bare home link (`plain`): it renders without the router, and its href stays `/` where Link would prefix the network',
  },
};

const SCAN_DIRS = ['app', 'components', 'hooks', 'lib'] as const;

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
const LINK_COMPONENTS = new Set(['Link', 'AppLink', 'ExternalLink', 'BlockLink', 'Address']);

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

/** A name of the file: a value it may hold, a component, or a prop (with its default, if any). */
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
  };

  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node)) visitElement(node.openingElement, node.children);
    else if (ts.isJsxSelfClosingElement(node)) visitElement(node, null);
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
  tolerated: Readonly<Record<string, Tolerated>>,
  named: Readonly<Record<string, { count: number; reason: string }>> = {}
): string[] {
  const out: string[] = [];
  for (const path of [...new Set([...found.keys(), ...Object.keys(tolerated)])].sort()) {
    const sites = found.get(path) ?? [];
    const entry = tolerated[path];
    for (const kind of KINDS) {
      const of = sites.filter((s) => s.kind === kind);
      const allowed = (entry?.sites[kind] ?? 0) + (kind === 'raw' ? (named[path]?.count ?? 0) : 0);
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
    if (entry && entry.reason.trim().length <= 10)
      out.push(`${path}: a reason too short to say why: "${entry.reason}"`);
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

  it('counts a raw anchor named by decision apart from the list', () => {
    const named = { 'app/b.tsx': { count: 1, reason: 'a bare home link outside the router' } };
    expect(toleratedFailures(new Map([['app/b.tsx', [site('raw', 4)]]]), {}, named)).toEqual([]);
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
    expect(toleratedFailures(found, TOLERATED, NAMED_RAW)).toEqual([]);
  });

  it(`tolerates ${TOLERATED_TOTAL} sites in all: the pinned total, which only falls`, () => {
    const total = Object.values(TOLERATED).reduce(
      (sum, entry) => sum + Object.values(entry.sites).reduce((a, n) => a + (n ?? 0), 0),
      0
    );
    expect(total).toBe(TOLERATED_TOTAL);
    expect(Object.values(NAMED_RAW).reduce((sum, e) => sum + e.count, 0)).toBe(1);
  });
});
