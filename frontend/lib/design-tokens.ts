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

// ---- F2 blocks: the Block board's reward shares (primary jade, secondary deep jade, committed
// fees ink, proposal fees gold — gold only for what was proposed). ----
export const REWARD_COLORS = {
  primary: DATA_COLOR,
  secondary: '#17775c',
  committed: '#a0a8be',
  proposal: CLASS_COLORS.dao,
} as const;

// ---- F2 group dao (round-2 fix) ----
/**
 * Nervos DAO secondary-issuance partition (board V63): the whole issuance a quiet slate, miners
 * the light text grey, compensation the DAO gold (the only gold), burnt the darkest "other" slate.
 */
export const DAO_ISSUANCE_COLORS = {
  total: '#4b5266',
  miners: '#a0a8be',
  compensation: CLASS_COLORS.dao,
  burnt: CLASS_COLORS.other,
} as const;

// ---- F2 group `fiber` (Fiber network, Fiber channel, Forks, Fork detail) ----
/** Jade = open now / CKB locked now; grey ink = history; a dashed light line = forced. */
export const FIBER_COLORS = {
  open: '#2edba3',
  openFill: 'rgba(46,219,163,.28)',
  history: '#a0a8be',
  historyFill: 'rgba(160,168,190,.3)',
  forced: '#dee2ec',
  nodeFill: '#141826',
  nodeStroke: '#a0a8be',
  label: '#a0a8be',
  count: '#dee2ec',
  halo: '#0c0e15',
  note: '#7c849c',
} as const;
/** Uncles on chain are grey ink; this node's view and an uncle inclusion are jade. */
export const FORK_COLORS = {
  uncles: '#a0a8be',
  nodeSwitches: '#2edba3',
  uncleLink: '#2edba3',
  dropped: '#dee2ec',
  spine: '#343c50',
  edge: '#4b5266',
  tileFill: '#141826',
  tileStroke: '#2c3348',
  tileStrokeLit: '#dee2ec',
  grid: '#161a25',
  axis: '#1a1f30',
  tick: '#7c849c',
  ink: '#dee2ec',
  dim: '#a0a8be',
  halo: '#0c0e15',
} as const;
