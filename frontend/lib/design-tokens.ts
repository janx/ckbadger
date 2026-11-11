import type { CSSProperties } from 'react';
import { alpha, color } from '@/lib/palette';

/**
 * The named colours the boards and graphics draw with. Every value is a CSS reference into the
 * theme (`var(--c-<key>)`, lib/palette.ts), so a page switches theme without re-rendering; the
 * dark theme's values are the ones the site was designed in, and the light theme's sit at the
 * same roles on paper. A drawer that needs a concrete hex (a canvas) uses `useThemePalette()`.
 */

/** Canonical semantic encodings from the approved site review. */
export const CLASS_COLORS = {
  dao: color('gold'),
  tokens: color('token'),
  objects: color('object'),
  identities: color('identity'),
  ckb: color('slate-ckb'),
  other: color('slate-other'),
} as const;
export type AssetClass = keyof typeof CLASS_COLORS;
export const CELL_COLORS = {
  capacity: color('cell-capacity'),
  lock: color('cell-lock'),
  type: color('cell-type'),
  data: color('cell-data'),
} as const;
export const BITCOIN_COLORS = {
  copper: color('copper'),
  light: color('bitcoin-light'),
  dark: color('bitcoin-dark'),
  inner: color('badge-inner-btc'),
} as const;
export const DATA_COLOR = color('jade');
/**
 * The ribbons of the tx page's "What moved" flows (review 2026-11-03 v2): the class colours for
 * items; CKB a neutral grey bright enough for a stroke on the night sky (CLASS_COLORS.ckb is the
 * dim slate of a mark); the fee to the miner dimmer still; DAO compensation the one gold.
 */
export const FLOW_COLORS = {
  tokens: CLASS_COLORS.tokens,
  objects: CLASS_COLORS.objects,
  identities: CLASS_COLORS.identities,
  ckb: color('flow-ckb'),
  fee: color('flow-fee'),
  compensation: CLASS_COLORS.dao,
} as const;
/** Ended states (an expired identity term): neutral grey, never gold or the class colour. */
export const EXPIRED_COLOR = color('text-dim');
export const HUE_RAMP = [
  DATA_COLOR,
  color('ramp-1'),
  color('ramp-2'),
  color('ramp-3'),
  color('ramp-4'),
  color('ramp-5'),
] as const;

/**
 * `n` distinct shades of the jade ramp, brightest first, for `n` bands of one split: the designed
 * stops while they last; more bands (the HODL wave's eight) take `n` stops spread evenly along the
 * same ramp, so no two bands share a colour and the order stays bright to dark (review C#4). A
 * stop between two designed ones is a `color-mix` of them, so it follows the theme too.
 */
export function jadeRamp(n: number): string[] {
  if (!Number.isSafeInteger(n) || n < 1) throw new Error(`jade ramp of ${n} shades`);
  if (n <= HUE_RAMP.length) return HUE_RAMP.slice(0, n);
  return Array.from({ length: n }, (_, k) => {
    const t = (k / (n - 1)) * (HUE_RAMP.length - 1);
    const i = Math.min(Math.floor(t), HUE_RAMP.length - 2);
    const share = Math.round((t - i) * 1000) / 10;
    if (share === 0) return HUE_RAMP[i];
    if (share === 100) return HUE_RAMP[i + 1];
    return `color-mix(in srgb, ${HUE_RAMP[i + 1]} ${share}%, ${HUE_RAMP[i]})`;
  });
}

/**
 * Object composition (FINAL_REVIEW decision 3, AssetClasses board): Pure CKB is a crystal,
 * BTC+CKB the crystal set in Bitcoin's copper; the off-chain tiers stay plain grey outlines.
 */
export const COMPOSITION_COLORS = {
  crystal: {
    top: color('crystal-top'),
    topLight: color('crystal-top-light'),
    left: color('crystal-left'),
    right: color('crystal-right'),
    edge: color('crystal-edge'),
    glint: color('crystal-glint'),
  },
  copperMount: {
    topLight: color('mount-top-light'),
    topMid: color('mount-top-mid'),
    topDark: color('mount-top-dark'),
    frontLight: color('mount-front-light'),
    frontMid: BITCOIN_COLORS.dark,
    frontDark: color('mount-front-dark'),
    edge: color('mount-edge'),
  },
  /** Off-chain tier icons: the dim text ink, on the page background. */
  offchain: color('text-dim'),
  offchainFill: color('base-bg'),
} as const;
/** Badge frames and inks for the two fully on-chain tiers (one drawing everywhere, IV-24). */
export const COMPOSITION_BADGES = {
  pure_ckb: {
    /** Two layers: the inner fill (clipped to the padding box) over the gradient frame. */
    frame: `linear-gradient(${color('badge-inner-pure')}, ${color('badge-inner-pure')}), linear-gradient(135deg, ${color('badge-jade-pale')}, ${color('badge-jade-light')} 40%, ${color('jade-dim')} 85%)`,
    ink: `linear-gradient(90deg, ${color('gel-ink-pure-0')}, ${color('gel-ink-pure-1')} 50%, ${color('gel-ink-pure-2')})`,
    bar: `linear-gradient(90deg, ${color('gel-ink-pure-2')}, ${color('badge-jade-pale')}, ${color('badge-jade-light')})`,
  },
  btc_ckb: {
    frame: `linear-gradient(${BITCOIN_COLORS.inner}, ${BITCOIN_COLORS.inner}), linear-gradient(135deg, ${BITCOIN_COLORS.light}, ${BITCOIN_COLORS.copper} 35%, ${color('badge-jade-light')} 72%, ${color('jade-dim')})`,
    ink: `linear-gradient(90deg, ${color('badge-copper-pale')}, ${BITCOIN_COLORS.copper} 45%, ${color('badge-jade-mid')} 92%)`,
    bar: `linear-gradient(90deg, ${BITCOIN_COLORS.dark}, ${BITCOIN_COLORS.light}, ${BITCOIN_COLORS.copper}, ${color('badge-jade-light')})`,
  },
} as const;

// ---- F2 blocks: the Block board's reward shares (primary jade, secondary deep jade, committed
// fees ink, proposal fees gold — gold only for what was proposed). ----
export const REWARD_COLORS = {
  primary: DATA_COLOR,
  secondary: color('reward-secondary'),
  committed: color('text'),
  proposal: CLASS_COLORS.dao,
} as const;
/** The Block board's position diagram: the current block's tile (the SVG and the phone list). */
export const BLOCK_POSITION_COLORS = { currentTile: color('tile-current') } as const;

// ---- F2 group dao (round-2 fix) ----
/**
 * Nervos DAO secondary-issuance partition (board V63): the whole issuance a quiet slate, miners
 * the light text grey, compensation the DAO gold (the only gold), burnt the darkest "other" slate.
 */
export const DAO_ISSUANCE_COLORS = {
  total: color('ink-quiet'),
  miners: color('text'),
  compensation: CLASS_COLORS.dao,
  burnt: CLASS_COLORS.other,
} as const;

// ---- F2 group `fiber` (Fiber network, Fiber channel, Forks, Fork detail) ----
/** Jade = open now / CKB locked now; grey ink = history; a dashed light line = forced. */
export const FIBER_COLORS = {
  open: DATA_COLOR,
  openFill: alpha('jade', 0.28),
  history: color('text'),
  historyFill: alpha('text', 0.3),
  forced: color('text-bright'),
  nodeFill: color('tile'),
  nodeStroke: color('text'),
  label: color('text'),
  count: color('text-bright'),
  halo: color('base-bg'),
  note: color('text-dim'),
  /** The edge of the pressed state filter on the channels list (the "other" slate). */
  filterOn: color('slate-other'),
} as const;
/** Uncles on chain are grey ink; this node's view and an uncle inclusion are jade. */
export const FORK_COLORS = {
  uncles: color('text'),
  nodeSwitches: DATA_COLOR,
  uncleLink: DATA_COLOR,
  dropped: color('text-bright'),
  spine: color('text-ghost'),
  edge: color('ink-quiet'),
  tileFill: color('tile'),
  tileStroke: color('slate-other'),
  tileStrokeLit: color('text-bright'),
  /** The row-rule ink: every chart's grid is 1 px solid border-subtle (visual finish V13 b). */
  grid: color('base-border-subtle'),
  axis: color('base-border-subtle'),
  tick: color('text-dim'),
  ink: color('text-bright'),
  dim: color('text'),
  halo: color('base-bg'),
} as const;

// ---- group scripts (F2-4 / F2-5) -------------------------------------------------------------
/**
 * "Who can change the code" (Scripts board): one fixed swatch per API code class. Immutable code
 * is filled grey, replaceable code is an outline (lighter edge = fewer signers), never by index.
 */
export const CODE_CLASS_COLORS = {
  fixed: { fill: color('code-fixed-fill'), edge: color('text') },
  unspendable: { fill: color('code-unspendable-fill'), edge: color('text-dim') },
  multisig: { fill: color('base-bg'), edge: color('text') },
  one_key: { fill: color('base-bg'), edge: color('text-bright') },
  unsupported: { fill: color('base-bg'), edge: color('ink-quiet') },
  mixed: { fill: color('base-bg'), edge: color('board-link') },
  ambiguous: { fill: color('base-bg'), edge: color('identity') },
  retired: { fill: color('base-border-subtle'), edge: color('text-ghost') },
} as const;
/** Free capacity in a cell anatomy bar (the Script board's "free" swatch). */
export const FREE_CAPACITY_COLOR = color('base-border-subtle');
/** The neutral of plain CKB held: the area under occupied in a script's history. */
export const HELD_CAPACITY_COLOR = CLASS_COLORS.ckb;
/** A code cell's ELF file to scale (CellCode board): one tone per kind of region. */
export const ELF_REGION_COLORS = {
  header: color('elf-header'),
  programHeaders: color('elf-program-headers'),
  rodata: color('elf-rodata'),
  ehFrame: color('elf-eh-frame'),
  text: color('cell-data'),
  data: color('elf-data'),
  meta: color('elf-meta'),
  sectionHeaders: color('elf-header'),
  padding: color('ink-quiet'),
} as const;

// ---- F2 group `objects` (Object sequencing gel, collection allele gel, Inventory bars) ----
/**
 * Gene colours of the Object board's sequencing gel: a Bitcoin-pointer gene (`btcfs` rule) is
 * copper, like Bitcoin everywhere; the other genes that own DNA bytes take cyan, violet and slate
 * in DNA order; a gene that only reads bytes another gene owns (a one-option gene) is light ink.
 */
export const GENE_COLORS = {
  bitcoin: BITCOIN_COLORS.copper,
  cycle: [color('aqua'), color('object'), color('text-dim')],
  shared: color('text-bright'),
} as const;
/** The gel plate: faint copper lanes, light wells, the byte ruler and the genotype overlay. */
export const GEL_COLORS = {
  lane: BITCOIN_COLORS.copper,
  well: color('text-bright'),
  ruler: color('text-ghost'),
  rulerText: color('ink-quiet'),
  label: color('text-dim'),
  heading: color('text'),
  overlay: color('gel-overlay'),
  bandText: color('gel-band-text'),
} as const;
/** Allele ink by the collection's composition (board `url(#ink)`, copper since FINAL_REVIEW 3). */
export const GEL_INK = {
  btc_ckb: [color('gel-ink-btc-0'), color('gel-ink-btc-1'), color('gel-ink-btc-2')],
  pure_ckb: [color('gel-ink-pure-0'), color('gel-ink-pure-1'), color('gel-ink-pure-2')],
  other: [CLASS_COLORS.objects, CLASS_COLORS.objects, CLASS_COLORS.objects],
} as const;

// ---- Inks with no theme token (visual finish V11) ---------------------------------------------
// Values the pages drew as bracket-hex classes (`text-[#4b5266]`), named here so page code carries
// no hex; no new Tailwind colour tokens (ruling V11) and no value changed. A text ink stays a class
// (R11: an inline `color` is a data hue): `inkStyle` sets the value and the element's
// `text-[color:var(--ink)]` reads it, so the rules that win over a class still win over it — the
// global `a:hover` jade on a link, a `hover:` state. Borders, backgrounds and data hues go through
// `style`, the pattern above; a `hover:` state on them reads a CSS variable set with `cssVars`.

/** CSS custom properties for a `style` prop (React's `CSSProperties` has no `--*` keys). */
export function cssVars(vars: Record<`--${string}`, string>): CSSProperties {
  return vars as CSSProperties;
}

/** `style` for a text ink with no theme token, read by the element's `text-[color:var(--ink)]`. */
export function inkStyle(color: string): CSSProperties {
  return cssVars({ '--ink': color });
}

/** Inks the boards share. */
export const BOARD_INKS = {
  /**
   * The quiet slate between text.ghost and text.dim: separators (↔, vs, —, →), byte offsets, the
   * version ruler's dates and day labels, the dashed frame of a note.
   */
  quiet: color('ink-quiet'),
  /** A link on the cell, fiber and fork boards (hashes, outputs, a script): cyan under aqua. */
  link: color('board-link'),
  /** A cell or script link on the cell boards: the lock blue. */
  cellLink: CELL_COLORS.lock,
} as const;

// ---- Chrome inks (visual finish V11) ----
/** The accent row tag's deep jade edge under its jade ink (table-rows T5). */
export const ROW_TAG_COLORS = { accentEdge: color('row-tag-accent-edge') } as const;
/** The header search's results: a well deeper than the page, its selected row and a hovered one. */
export const SEARCH_RESULT_COLORS = {
  panel: color('search-panel'),
  selected: color('search-selected'),
  hover: color('search-hover'),
} as const;
/** The activity stream's "n new activities" bar and its hover. */
export const STREAM_COLORS = {
  pending: color('stream-pending'),
  pendingHover: color('stream-pending-hover'),
} as const;

// ---- Storage tiers (composition card, content preview; visual finish V11) ----
/**
 * The storage tiers' own inks: BTC+CKB a moss green over a dark amber wash, Pure CKB a deep jade
 * over a dark jade wash, centralised a dark rouge edge. `inkLit` / `edgeLit` are the tooltip
 * button's hover; a `wash` is the preview header gradient's start, at 50 % (it fades to nothing).
 */
export const STORAGE_TIER_COLORS = {
  btc_ckb: {
    ink: color('tier-btc-ink'),
    inkLit: color('tier-btc-ink-lit'),
    edge: color('tier-btc-edge'),
    edgeLit: color('tier-btc-edge-lit'),
    wash: alpha('tier-btc-wash', 0.5),
  },
  pure_ckb: {
    ink: color('tier-pure-ink'),
    inkLit: color('tier-pure-ink-lit'),
    edge: color('tier-pure-edge'),
    edgeLit: color('tier-pure-edge-lit'),
    wash: alpha('tier-pure-wash', 0.5),
  },
  centralized_mixture: { edge: color('tier-centralized-edge') },
} as const;
