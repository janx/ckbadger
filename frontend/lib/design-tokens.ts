/** Canonical semantic encodings from the approved site review. */
export const CLASS_COLORS = {
  dao: '#f2c55c',
  tokens: '#5aa6f5',
  objects: '#a98cf5',
  identities: '#f08aa8',
  ckb: '#3a4258',
  other: '#2c3348',
} as const;
export type AssetClass = keyof typeof CLASS_COLORS;
export const CELL_COLORS = {
  capacity: '#dee2ec',
  lock: '#7aa7d6',
  type: '#b5c77a',
  data: '#6fc3d9',
} as const;
export const BITCOIN_COLORS = {
  copper: '#dc8a5a',
  light: '#ffe1cc',
  dark: '#a95a32',
  inner: '#1a110d',
} as const;
export const DATA_COLOR = '#2edba3';
/** Ended states (an expired identity term): neutral grey, never gold or the class colour. */
export const EXPIRED_COLOR = '#7c849c';
export const HUE_RAMP = ['#2edba3', '#24b68a', '#259775', '#287b64', '#2b6054', '#344e48'] as const;

/**
 * Object composition (FINAL_REVIEW decision 3, AssetClasses board): Pure CKB is a crystal,
 * BTC+CKB the crystal set in Bitcoin's copper; the off-chain tiers stay plain grey outlines.
 */
export const COMPOSITION_COLORS = {
  crystal: {
    top: '#a8f6de',
    topLight: '#e4fff6',
    left: '#3fd3a8',
    right: '#86ecce',
    edge: '#0b3b30',
    glint: '#ffffff',
  },
  copperMount: {
    topLight: '#ffeadf',
    topMid: '#efa682',
    topDark: '#c86f45',
    frontLight: '#df8b5f',
    frontMid: BITCOIN_COLORS.dark,
    frontDark: '#82422a',
    edge: '#55291a',
  },
  /** Off-chain tier icons: the dim text ink, on the page background. */
  offchain: '#7c849c',
  offchainFill: '#0c0e15',
} as const;
/** Badge frames and inks for the two fully on-chain tiers (one drawing everywhere, IV-24). */
export const COMPOSITION_BADGES = {
  pure_ckb: {
    /** Two layers: the inner fill (clipped to the padding box) over the gradient frame. */
    frame:
      'linear-gradient(#0b1d1a, #0b1d1a), linear-gradient(135deg, #f0fffa, #8ff3d6 40%, #1fb88a 85%)',
    ink: 'linear-gradient(90deg, #f3fffb, #9df6dc 50%, #3ee0b0)',
    bar: 'linear-gradient(90deg, #3ee0b0, #f0fffa, #8ff3d6)',
  },
  btc_ckb: {
    frame: `linear-gradient(${BITCOIN_COLORS.inner}, ${BITCOIN_COLORS.inner}), linear-gradient(135deg, ${BITCOIN_COLORS.light}, ${BITCOIN_COLORS.copper} 35%, #8ff3d6 72%, #1fb88a)`,
    ink: `linear-gradient(90deg, #ffe7d8, ${BITCOIN_COLORS.copper} 45%, #9df6dc 92%)`,
    bar: `linear-gradient(90deg, ${BITCOIN_COLORS.dark}, ${BITCOIN_COLORS.light}, ${BITCOIN_COLORS.copper}, #8ff3d6)`,
  },
} as const;

// ---- group scripts (F2-4 / F2-5) -------------------------------------------------------------
/**
 * "Who can change the code" (Scripts board): one fixed swatch per API code class. Immutable code
 * is filled grey, replaceable code is an outline (lighter edge = fewer signers), never by index.
 */
export const CODE_CLASS_COLORS = {
  fixed: { fill: '#8a91a6', edge: '#a0a8be' },
  unspendable: { fill: '#5c6378', edge: '#7c849c' },
  multisig: { fill: '#0c0e15', edge: '#a0a8be' },
  one_key: { fill: '#0c0e15', edge: '#dee2ec' },
  unsupported: { fill: '#0c0e15', edge: '#4b5266' },
  mixed: { fill: '#0c0e15', edge: '#6aa9c4' },
  ambiguous: { fill: '#0c0e15', edge: '#f08aa8' },
  retired: { fill: '#1a1f30', edge: '#343c50' },
} as const;
/** Free capacity in a cell anatomy bar (the Script board's "free" swatch). */
export const FREE_CAPACITY_COLOR = '#1a1f30';
/** The neutral of plain CKB held: the area under occupied in a script's history. */
export const HELD_CAPACITY_COLOR = '#3a4258';
