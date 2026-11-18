/**
 * The colour system's one source of truth: every colour of both themes, by name.
 *
 * `PALETTE` holds the hex of each colour key per theme and `MATERIALS` the non-colour finish
 * values (glass fills, the sky, the glow scale). `themeBaseStyles()` turns both into CSS custom
 * properties — `--c-<key>` (the colour), `--rgb-<key>` (its channels, for Tailwind's opacity
 * modifiers) and `--m-<key>` (a material) — declared on `:root` for the dark theme and redeclared
 * under `:root[data-theme='light']` (tailwind.config.ts emits them as base styles). Page code
 * never writes a hex: Tailwind classes resolve through the channels, `lib/design-tokens.ts`
 * constants are `color()` references, and the few canvas and WebGL drawers that need a concrete
 * value read `PALETTE[theme]` through `useThemePalette()`.
 *
 * By night (`dark`, the default) the theme is "rubbing": a dark warm-ink ground, moon-white text,
 * celadon phosphor, cinnabar marks, old gold. By day (`light`) it is "Song print": the same inks on
 * paper — the accents deepen to text-grade contrast (≥ 4.5:1 on every ground, pinned by
 * __tests__/lib/palette.test.ts), the grounds step from paper (the page) to a whiter sheet (a
 * surface) to a darker tint (elevated, a hover), and the glows go out (`glow` = 0). The values come
 * from the design boards (docs/superpowers/design/2026-11-10-rubbing-song/src/lib.js `THEMES`); how
 * each derived key is made from them is tabled in frontend/docs/ui-color-guidelines.md. No key is a
 * cold blue-black grey: every neutral is a warm ink.
 */

export const THEMES = ['dark', 'light'] as const;
export type ThemeName = (typeof THEMES)[number];

/**
 * By night the text grade is perceptual (readability plan 2026-11-18, ruling R2; the boards'
 * ruling of that day, Gate F0, replaces the rubbing's night values below): every ink that carries
 * a word is graded by `apcaLc` on the ground it is read on, the reading ink at Lc 75 or more and
 * every other word ink at 60 or more on the page, the surface and the well (72 and 58 on the
 * elevated tint), the marks under 60; WCAG's 4.5:1 stays the second floor. The values are the
 * prototype's ladder (OKLCH lightness moved, hue kept, chroma kept unless the colour left sRGB),
 * moved further in lightness only where a pin asked for it; the derivations are tabled in
 * frontend/docs/ui-color-guidelines.md and the floors pinned in palette.test.ts.
 */
const dark = {
  // grounds (lib.js: well, bg; the surface, the elevated tint, the line and the hair at 1.08,
  // 1.17, 1.40 and 1.25:1 to the page ground by the 2026-11-18 ruling)
  'base-void': '#0f0d0b',
  'base-bg': '#15120f',
  'base-surface': '#1e1a17',
  'base-elevated': '#26211c',
  'base-border': '#362e28',
  'base-border-subtle': '#2d2621',
  /**
   * The 1 px rule between the rows of a list or a table (and under its head): the hair by night
   * (1.25:1 to the page ground); by day the hair stepped down until it clears the paper and a
   * ruled line by 1.25:1 (R5).
   */
  'row-rule': '#2d2621',
  /**
   * A bar's empty track: a step of its own by night (1.30:1 to the page ground); by day stepped
   * down as the row rule (R5).
   */
  'bar-track': '#2f2924',
  /**
   * The row rule and the bar track on the tier-2 ground (ruling 4b), where `.tier2-ground`
   * re-points the two keys to these: the same by night (they clear that ground by 1.29 / 1.34);
   * by day L −0.030 until they clear the tier-2 ground and a ruled line on it by 1.25:1.
   */
  'row-rule-deep': '#2d2621',
  'bar-track-deep': '#2f2924',
  // inks (lib.js moon, text, dim, ghost), by Lc on the page ground: bright 92, reading 76, quiet
  // 61, then the marks: muted 45, quiet slate 30, ghost 20
  'text-bright': '#ede7db',
  text: '#d6cdbe',
  'text-dim': '#bdb3a4',
  'text-ghost': '#685f56',
  /** The quiet ink between ghost and dim: separators, byte offsets, rulers, a note's frame. */
  'ink-quiet': '#7f766b',
  /** The deeper dim of the pipeline and the loading marquee. */
  'ink-muted': '#9e9589',
  /** The phosphor white a live figure flares to (moon lit by celadon); the text-selection ink. */
  'neon-white': '#f3f9f8',
  // accents, by the key names the pages use: jade = celadon 青瓷, aqua = indigo 靛青, gold = old
  // gold 古金, rouge = cinnabar text 朱砂, lavender = lotus 藕荷, amber = moss 苔绿. By night each
  // reads at Lc 70 on the page ground, its quiet step at 60 to 61 and its bright step at 80; the
  // cinnabar text, a signal, at 63 over its quiet step's 61 and under its bright step's 68
  jade: '#9ecdc2',
  'jade-dim': '#90bdb2',
  'jade-bright': '#b9dcd2',
  aqua: '#a2c8ef',
  'aqua-dim': '#99b7d6',
  'aqua-bright': '#b4d8fe',
  gold: '#e4bf76',
  'gold-dim': '#ceb077',
  'gold-bright': '#f6cf87',
  rouge: '#ff9e88',
  /**
   * The cinnabar text's quiet step at text grade, like every other `*-dim` (UI review 2026-11-14):
   * the board's dim moved in lightness until it reads at the text grade (by night Lc 60 on every
   * word ground, under the cinnabar text; by day 4.5:1). It was the fraction digits of a negative
   * amount until ruling R5 gave an amount one ink.
   */
  'rouge-dim': '#fc9982',
  'rouge-bright': '#feac99',
  lavender: '#cdbceb',
  'lavender-dim': '#baaed2',
  amber: '#b1ce88',
  'amber-dim': '#a4ba84',
  /** Cinnabar, the mark: prompt squares, seals, the search square (UI grade, not text). */
  mark: '#c2452d',
  // asset classes: token azurite 石青, object a lotus violet, identity the boards' rose, copper
  // ochre 赭石 (bitcoin); by night each at Lc 70 on the page ground, as the accents
  token: '#9ec6fe',
  'token-bright': '#bad7ff',
  object: '#d0b8fe',
  identity: '#fcaec2',
  copper: '#fab487',
  /**
   * Plain CKB as a mark, a hair above the ghost; also the ground of words (an address balance
   * bar's "free" part, the current code-cell holder), set by them by night: the neutral of the mix
   * dim ghost 0.12 moved in OKLCH lightness until text-bright reads on it at Lc 70 (4.93:1). The
   * mix drifted with the inks to #72695f, where neither ink held a word (4.37:1 and 3.5:1).
   */
  'slate-ckb': '#6a6157',
  /** "Other" as a mark, and a tile's outline: the boards' second line (lib.js line2), at 1.60:1. */
  'slate-other': '#3f3730',
  // the cell's four parts
  'cell-capacity': '#ede7db',
  'cell-lock': '#a2c7f0',
  'cell-type': '#b1cd8a',
  'cell-data': '#a1cad9',
  /** Framing inside the data (a length, an offset): structure, not content. */
  'data-framing': '#97b9c4',
  // boards
  'board-link': '#a1cadc',
  tile: '#1a1613',
  'tile-current': '#353a36',
  'grid-faint': '#28231f',
  'flow-ckb': '#bab1a2',
  'flow-fee': '#978d81',
  // the jade ramp after jade itself (the first stop), strongest first: celadon mixed into the
  // page ground at 80 / 64 / 50 / 38 / 27 %
  'ramp-1': '#83a89e',
  'ramp-2': '#6d8a82',
  'ramp-3': '#5a7068',
  'ramp-4': '#495953',
  'ramp-5': '#3a443f',
  // the transaction pipeline's stage fills (indigo / gold / celadon, hue and chroma kept, at OKLCH
  // L 0.66 by night; the drop neutral is the text ink at half chroma at the same L)
  'pipeline-pending': '#7296bb',
  'pipeline-proposed': '#b08c44',
  'pipeline-committed': '#709d92',
  'pipeline-drop-neutral': '#95928b',
  /** The pipeline's pools are wells: the boards' well ground by night, recessed below the surface. */
  'pipeline-well': '#0f0d0b',
  /** What a stage hue is mixed toward as cycles rise: lighter at night, inkier by day. */
  'pipeline-step-to': '#f6f2ea',
  /**
   * A block in the pipeline is a solid plate of its own tone (1.29 : 1 on the page ground by night,
   * 1.27 by day), never faded; the number and the counts on it are text grade (palette.test).
   */
  'pipeline-plate': '#2f2821',
  /**
   * The proposal window (direction G): the strap through the heads of the plates is gilt across
   * every plate inside the window and the joints between them, a ground the head's words keep
   * 4.5 : 1 on (by night text-bright 8.0, text 6.24 : 1; text-dim is not used on it), never faded.
   */
  'pipeline-gilt': '#564013',
  /**
   * The cut a transaction is set in: 2 px of this tone round every square and the cellbase on a
   * plate, and only there (no cut where there is no transaction).
   */
  'pipeline-cut': '#13100d',
  /**
   * The proposal dots on a plate, derived against it (≥ 3 : 1 for waiting and committed in both
   * themes): a waiting proposal in the proposed gold, a committed one in the committed celadon,
   * an expired one the quiet dot.
   */
  'pipeline-dot-waiting': '#ba9a5a',
  'pipeline-dot-committed': '#63897f',
  'pipeline-dot-expired': '#48413a',
  // chrome
  'search-panel': '#0c0a09',
  'search-selected': '#1a1c1a',
  'search-hover': '#131310',
  'stream-pending': '#0b0908',
  'stream-pending-hover': '#0d0b0a',
  'row-tag-accent-edge': '#46554f',
  'reward-secondary': '#647e77',
  // "who can change the code" swatches
  'code-fixed-fill': '#c5bbac',
  /**
   * The darker of the two greys, set by what it carries: the bar on /scripts writes its class's
   * name and share in it, and the fill-ink rule gives those words the bright ink, which reads on
   * it at Lc 70 (4.94:1). By night it is the neutral of the mix it was (dim and ghost) moved in
   * OKLCH lightness only to that grade; a mix of two inks drifts with them (#8a8175 held a word
   * in neither ink: the bright one 3.11:1 and Lc 56, the dark one Lc 38).
   */
  'code-unspendable-fill': '#6a6155',
  // an ELF file's regions
  'elf-header': '#97b9c4',
  'elf-program-headers': '#abd0dc',
  'elf-rodata': '#9ac09b',
  'elf-eh-frame': '#aab9aa',
  'elf-data': '#d5b985',
  'elf-meta': '#baaed2',
  // the sequencing gel
  'gel-overlay': '#f6f2ea',
  'gel-ink-btc-0': '#f1dece',
  'gel-ink-btc-1': '#cc8a5e',
  'gel-ink-btc-2': '#b7d9d0',
  'gel-ink-pure-0': '#f2f8f6',
  'gel-ink-pure-1': '#b7d9d0',
  'gel-ink-pure-2': '#93c1b6',
  // storage tiers
  'tier-pure-ink-lit': '#b9dcd2',
  // composition badges
  'badge-inner-pure': '#181a17',
  'badge-inner-btc': '#1a1510',
  'badge-jade-pale': '#f2f8f6',
  'badge-jade-light': '#b7d9d0',
  'badge-jade-mid': '#a5cdc3',
  'badge-copper-pale': '#f1dece',
  'bitcoin-light': '#edd6c3',
  'bitcoin-dark': '#945d36',
  // the Game of Life tile
  'life-bg-ckb': '#151512',
  'life-bg-dual': '#15120e',
  // the sky: the page ground is the boards' bg, a hair lighter at the top, a hair deeper at the foot
  'sky-top': '#181512',
  'sky-mid': '#15120f',
  'sky-bottom': '#13100d',
  'well-bg': '#0f0d0b',
  'selection-ink': '#f6f2ea',
  // artwork, the same by night and by day: the celadon crystal and its ochre-copper mount
  'crystal-top': '#c9e3dc',
  'crystal-top-light': '#edf6f3',
  'crystal-left': '#7bb3a6',
  'crystal-right': '#b7d9d0',
  'crystal-edge': '#112d28',
  'crystal-glint': '#fbfaf6',
  'mount-top-light': '#f3e3d4',
  'mount-top-mid': '#e7a376',
  'mount-top-dark': '#b8784c',
  'mount-front-light': '#cc8a5e',
  'mount-front-dark': '#8c4f22',
  'mount-edge': '#663816',
} as const;

export type PaletteKey = keyof typeof dark;
export type Palette = Readonly<Record<PaletteKey, string>>;

const light: Palette = {
  'base-void': '#fbf9f4',
  'base-bg': '#f3eee4',
  'base-surface': '#faf7f0',
  'base-elevated': '#e9e2d4',
  'base-border': '#d4cab8',
  'base-border-subtle': '#e3dbcc',
  'row-rule': '#d5cdbf',
  'bar-track': '#d4cdc0',
  'row-rule-deep': '#cbc3b5',
  'bar-track-deep': '#cac3b6',
  'text-bright': '#1f1b17',
  text: '#4a433b',
  'text-dim': '#6c6356',
  'text-ghost': '#b3a898',
  'ink-quiet': '#93897a',
  'ink-muted': '#786f61',
  'neon-white': '#143f39',
  jade: '#2f6b63',
  'jade-dim': '#346e65',
  'jade-bright': '#185750',
  aqua: '#2e5f8f',
  'aqua-dim': '#466789',
  'aqua-bright': '#194b7a',
  gold: '#80600f',
  'gold-dim': '#786234',
  'gold-bright': '#684e0d',
  rouge: '#a8392a',
  'rouge-dim': '#9b4f43',
  'rouge-bright': '#902214',
  lavender: '#5e4d9a',
  'lavender-dim': '#675d93',
  amber: '#46701f',
  'amber-dim': '#5d7b47',
  mark: '#b5331f',
  token: '#2a5a9e',
  'token-bright': '#154688',
  object: '#5e45a8',
  identity: '#9c3b5f',
  copper: '#93522b',
  'slate-ckb': '#a69c8c',
  'slate-other': '#c4b9a5',
  'cell-capacity': '#1f1b17',
  'cell-lock': '#2b5c8c',
  'cell-type': '#466f23',
  'cell-data': '#2f6579',
  'data-framing': '#416a7a',
  'board-link': '#2e647d',
  tile: '#eee8dc',
  'tile-current': '#dce2db',
  'grid-faint': '#ebe5d8',
  'flow-ckb': '#62594e',
  'flow-fee': '#93897a',
  'ramp-1': '#56857d',
  'ramp-2': '#769a91',
  'ramp-3': '#91ada4',
  'ramp-4': '#a9bcb3',
  'ramp-5': '#becbc1',
  'pipeline-pending': '#3e6fa0',
  'pipeline-proposed': '#866619',
  'pipeline-committed': '#3d7870',
  'pipeline-drop-neutral': '#6e6b67',
  'pipeline-well': '#eee8dc',
  'pipeline-step-to': '#1f1b17',
  'pipeline-plate': '#ddd4c3',
  'pipeline-gilt': '#c8b47f',
  'pipeline-cut': '#f8f4ec',
  'pipeline-dot-waiting': '#866619',
  'pipeline-dot-committed': '#3d7870',
  'pipeline-dot-expired': '#b9b3a8',
  'search-panel': '#fffdf8',
  'search-selected': '#e6e9e2',
  'search-hover': '#f2f1ea',
  'stream-pending': '#eee8dc',
  'stream-pending-hover': '#e4dccd',
  'row-tag-accent-edge': '#acbfb6',
  'reward-secondary': '#91ada4',
  'code-fixed-fill': '#7a7163',
  'code-unspendable-fill': '#bbb1a0',
  // An ELF region's hue paints its hex digits on the cell page: text grade on the four grounds and
  // the tier-2 ground, moved in OKLCH lightness only where short (lane E2 round 3). The program
  // headers are the header's hue and chroma a step darker by day (a step lighter by night), as far
  // from it as the night pair: a lighter step at 4.5:1 would land on the header (round 4).
  'elf-header': '#416a7a',
  'elf-program-headers': '#2d5665',
  'elf-rodata': '#3b7047',
  'elf-eh-frame': '#576a5a',
  'elf-data': '#7c6126',
  'elf-meta': '#695c9a',
  'gel-overlay': '#1f1b17',
  'gel-ink-btc-0': '#b78c70',
  'gel-ink-btc-1': '#93522b',
  'gel-ink-btc-2': '#346e65',
  'gel-ink-pure-0': '#346e65',
  'gel-ink-pure-1': '#2f6b63',
  'gel-ink-pure-2': '#185750',
  'tier-pure-ink-lit': '#185750',
  'badge-inner-pure': '#e6e9e2',
  'badge-inner-btc': '#f2eae0',
  'badge-jade-pale': '#346e65',
  'badge-jade-light': '#407a70',
  'badge-jade-mid': '#326d64',
  'badge-copper-pale': '#b78c70',
  'bitcoin-light': '#c19c84',
  'bitcoin-dark': '#7a3b11',
  'life-bg-ckb': '#e7e6dc',
  'life-bg-dual': '#ede7d9',
  'sky-top': '#f5f1e8',
  'sky-mid': '#f3eee4',
  'sky-bottom': '#f0eadf',
  'well-bg': '#fbf9f4',
  'selection-ink': '#1f1b17',
  'crystal-top': dark['crystal-top'],
  'crystal-top-light': dark['crystal-top-light'],
  'crystal-left': dark['crystal-left'],
  'crystal-right': dark['crystal-right'],
  'crystal-edge': dark['crystal-edge'],
  'crystal-glint': dark['crystal-glint'],
  'mount-top-light': dark['mount-top-light'],
  'mount-top-mid': dark['mount-top-mid'],
  'mount-top-dark': dark['mount-top-dark'],
  'mount-front-light': dark['mount-front-light'],
  'mount-front-dark': dark['mount-front-dark'],
  'mount-edge': dark['mount-edge'],
};

export const PALETTE: Readonly<Record<ThemeName, Palette>> = { dark, light };

/**
 * The finish that is not a colour: the fixed layers behind the page, the frames and the wells,
 * and `glow`, the factor every halo's alpha is multiplied by (1 at night, 0 by day: nothing
 * glows on paper).
 *
 * The rubbing / Song print finish (the boards' `scan` and `inset`): ruled paper by day, 1 px of
 * the ink at 4.5 % in every 3 over the page (`scanline-ink`). By night no scan lies over the page
 * (readability R1: on the night ground its lines showed only where they crossed the text, and cut
 * its strokes): `scanline-ink` is transparent, and the layer, mounted in both themes, paints
 * nothing; the night's scan is cut into a figure's fills instead (`fill-scan`, `FILL_SCAN`). The
 * vignette and the grain sit at about half their midnight strength. A frame
 * (`.glass`, the rule name kept) is a flat tile: the surface, opaque, under its own 1 px border —
 * no sheen, no lit edges, no outer ring. A well is the boards' inset (`inset 0 1px 3px` at 60 % black by night, the ink at
 * 8 % by day) with no lip. The tints are the palette's own grounds and inks. `seal-press` is how
 * faint the newest block's seal is pressed on the 404 page: the cinnabar at 16 % on the night,
 * 8 % on paper (where the same red reads stronger): the most each allowed, while it stood behind
 * the verse, with every ink of the verse and its facts still text-grade across a stroke
 * (palette.test); since readability R6 it stands clear of the text, beside or below it.
 */

/**
 * The scan of a fill (readability R1, Gate F0): by night the scan is a property of a figure's
 * fill — a bar, an area — and never lies over a word. Of every `period` CSS px, `row` px is hit:
 * the ground behind the fill shows through it at `strength`, the strength the window-wide layer
 * had on a fill. An HTML fill takes the class `.scan-fill` (app/globals.css), whose mask image is
 * the material `fill-scan`; an SVG fill takes the mask components/ui/scan-fill.tsx draws from these
 * numbers, drawn while the material is not `none`. By day there is none: the ruled paper is the
 * window's (`scanline-ink`).
 */
export const FILL_SCAN = { period: 3, row: 1, strength: 0.22 } as const;

/** `fill-scan` by night: an alpha mask from the fill's foot up, `period − row` px whole, `row` px hit. */
function fillScanMask(): string {
  const { period, row, strength } = FILL_SCAN;
  const whole = 'rgba(0, 0, 0, 1)';
  const hit = `rgba(0, 0, 0, ${1 - strength})`;
  const at = period - row;
  return `repeating-linear-gradient(0deg, ${whole} 0px, ${whole} ${at}px, ${hit} ${at}px, ${hit} ${period}px)`;
}

const darkMaterials = {
  glow: '1',
  vignette: 'rgba(0, 0, 0, 0.18)',
  'scanline-ink': 'transparent',
  'fill-scan': fillScanMask(),
  'grain-blend': 'screen',
  'grain-opacity': '0.02',
  'inset-scan': 'rgba(0, 0, 0, 0.06)',
  'glass-fill': hexAlpha(dark['base-surface'], 1),
  'glass-sheen': 'transparent',
  'glass-top-edge': 'transparent',
  'glass-bottom-edge': 'transparent',
  'glass-ring': 'transparent',
  'well-inset': 'rgba(0, 0, 0, 0.6)',
  'well-lip': 'transparent',
  'tier2-ground': hexAlpha(dark['base-void'], 0.9),
  // the sticky vitals strip is opaque: the page scrolls under it, never through it
  'header-strip': dark['base-surface'],
  'drop-shadow': '0 14px 32px rgba(0, 0, 0, 0.68)',
  'raised-shadow': '0 10px 24px rgba(0, 0, 0, 0.28)',
  'preview-inset': `inset 0 2px 8px rgba(0, 0, 0, 0.7), inset 0 0 1px ${hexAlpha(dark['text-bright'], 0.04)}`,
  'preview-cast': 'rgba(0, 0, 0, 0.5)',
  'preview-sheen': hexAlpha(dark['text-bright'], 0.08),
  'seal-press': '0.16',
} as const;
export type MaterialKey = keyof typeof darkMaterials;
export type Materials = Readonly<Record<MaterialKey, string>>;

/** The ink a shadow or a ruled line is drawn in by day (the day text-bright, 31 27 23). */
const DAY_INK = light['text-bright'];
/** The paper a highlight is by day (the day well, 251 249 244). */
const DAY_PAPER = light['base-void'];

const lightMaterials: Materials = {
  glow: '0',
  vignette: hexAlpha(DAY_INK, 0.03),
  'scanline-ink': hexAlpha(DAY_INK, 0.045),
  'fill-scan': 'none',
  'grain-blend': 'multiply',
  'grain-opacity': '0.03',
  'inset-scan': hexAlpha(DAY_INK, 0.025),
  'glass-fill': hexAlpha(light['base-surface'], 1),
  'glass-sheen': 'transparent',
  'glass-top-edge': 'transparent',
  'glass-bottom-edge': 'transparent',
  'glass-ring': 'transparent',
  'well-inset': hexAlpha(DAY_INK, 0.08),
  'well-lip': 'transparent',
  'tier2-ground': hexAlpha(light['base-elevated'], 0.9),
  'header-strip': light['base-surface'],
  'drop-shadow': `0 14px 32px ${hexAlpha(DAY_INK, 0.18)}`,
  'raised-shadow': `0 10px 24px ${hexAlpha(DAY_INK, 0.12)}`,
  'preview-inset': `inset 0 2px 8px ${hexAlpha(DAY_INK, 0.12)}, inset 0 0 1px ${hexAlpha(DAY_PAPER, 0.6)}`,
  'preview-cast': hexAlpha(DAY_INK, 0.18),
  'preview-sheen': hexAlpha(DAY_PAPER, 0.5),
  'seal-press': '0.08',
};

export const MATERIALS: Readonly<Record<ThemeName, Materials>> = {
  dark: darkMaterials,
  light: lightMaterials,
};

export const PALETTE_KEYS = Object.keys(dark) as PaletteKey[];
export const MATERIAL_KEYS = Object.keys(darkMaterials) as MaterialKey[];

/** The CSS reference of a colour key, for a style, an SVG attribute or a CSS string. */
export function color(key: PaletteKey): string {
  return `var(--c-${key})`;
}

/** A colour key at `alpha` (0–1). */
export function alpha(key: PaletteKey, a: number): string {
  if (!(a >= 0 && a <= 1)) throw new Error(`palette: alpha ${a} of ${key} is not in 0..1`);
  return `rgb(var(--rgb-${key}) / ${a})`;
}

/** A colour key at `alpha`, scaled by the theme's glow factor: a halo, out by day. */
export function glowAlpha(key: PaletteKey, a: number): string {
  if (!(a >= 0 && a <= 1)) throw new Error(`palette: alpha ${a} of ${key} is not in 0..1`);
  return `rgb(var(--rgb-${key}) / calc(${a} * var(--m-glow)))`;
}

/**
 * Any CSS colour (a `color()` reference, a hex) at `alpha`, as a `color-mix` toward transparent —
 * for a tint drawn from a colour the caller only holds as a reference.
 */
export function fade(cssColor: string, a: number): string {
  if (typeof cssColor !== 'string' || cssColor.trim() === '')
    throw new Error(`palette: fade of an empty colour`);
  if (!(a >= 0 && a <= 1)) throw new Error(`palette: alpha ${a} of ${cssColor} is not in 0..1`);
  return `color-mix(in srgb, ${cssColor} ${Math.round(a * 1000) / 10}%, transparent)`;
}

/** The CSS reference of a material. */
export function material(key: MaterialKey): string {
  return `var(--m-${key})`;
}

/** Tailwind's colour value for a key: the channels under the class's opacity modifier. */
export function twColor(key: PaletteKey): string {
  return `rgb(var(--rgb-${key}) / <alpha-value>)`;
}

/** A concrete hex (from `PALETTE[theme]`) at `alpha`, for a canvas that cannot read a variable. */
export function hexAlpha(hex: string, a: number): string {
  if (!(a >= 0 && a <= 1)) throw new Error(`palette: alpha ${a} of ${hex} is not in 0..1`);
  const { r, g, b } = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

export function hexToRgb(hex: string): { r: number; g: number; b: number } {
  if (!/^#[0-9a-f]{6}$/i.test(hex)) throw new Error(`palette: ${hex} is not #rrggbb`);
  return {
    r: Number.parseInt(hex.slice(1, 3), 16),
    g: Number.parseInt(hex.slice(3, 5), 16),
    b: Number.parseInt(hex.slice(5, 7), 16),
  };
}

/** The declarations of one theme: `--c-*`, `--rgb-*` and `--m-*`. */
export function themeDeclarations(theme: ThemeName): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of PALETTE_KEYS) {
    const hex = PALETTE[theme][key];
    const { r, g, b } = hexToRgb(hex);
    out[`--c-${key}`] = hex;
    out[`--rgb-${key}`] = `${r} ${g} ${b}`;
  }
  for (const key of MATERIAL_KEYS) out[`--m-${key}`] = MATERIALS[theme][key];
  return out;
}

/** The base styles tailwind.config.ts adds: the dark theme on :root, the light one by attribute. */
export function themeBaseStyles(): Record<string, Record<string, string>> {
  return {
    ":root, :root[data-theme='dark']": themeDeclarations('dark'),
    ":root[data-theme='light']": themeDeclarations('light'),
  };
}

// ---- contrast (WCAG 2.x), for the palette's own tests and any check of a pairing ----

function channelLinear(v: number): number {
  const s = v / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

export function relativeLuminance(hex: string): number {
  const { r, g, b } = hexToRgb(hex);
  return 0.2126 * channelLinear(r) + 0.7152 * channelLinear(g) + 0.0722 * channelLinear(b);
}

export function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

// ---- perceptual contrast (APCA-W3 0.0.98G-4g): the night text grade (readability ruling R2) ----

/** APCA's screen luminance: each sRGB channel to the 2.4 power (no linear toe), weighted. */
function apcaLuminance(hex: string): number {
  const { r, g, b } = hexToRgb(hex);
  return 0.2126729 * (r / 255) ** 2.4 + 0.7151522 * (g / 255) ** 2.4 + 0.072175 * (b / 255) ** 2.4;
}

/** APCA's soft clamp of a near-black luminance (flare). */
function apcaClampBlack(y: number): number {
  return y > 0.022 ? y : y + (0.022 - y) ** 1.414;
}

/**
 * The lightness contrast (APCA Lc) of `ink` read on `ground`, APCA-W3 0.0.98G-4g: positive for a
 * dark ink on a light ground, negative for a light ink on a dark one; the grade is its absolute
 * value. Unlike `contrastRatio` it is not symmetric, and it weighs a light ink on a dark ground as
 * the eye does, weaker than the same ratio the other way round, which is why the night theme's
 * text is graded by it (palette.test pins the floors) and WCAG's 4.5:1 stays the second floor.
 */
export function apcaLc(ink: string, ground: string): number {
  const text = apcaClampBlack(apcaLuminance(ink));
  const back = apcaClampBlack(apcaLuminance(ground));
  if (Math.abs(back - text) < 0.0005) return 0;
  if (back > text) {
    const s = (back ** 0.56 - text ** 0.57) * 1.14;
    return s < 0.1 ? 0 : (s - 0.027) * 100;
  }
  const s = (back ** 0.65 - text ** 0.62) * 1.14;
  return s > -0.1 ? 0 : (s + 0.027) * 100;
}

// ---- the ink of words inside a filled segment (DY-03, UI review 2026-11-14) ----

/**
 * The two inks a word inside a filled bar segment can take, in either theme: the darkest ground
 * (the well) and the brightest text. By night the well is the dark one, by day the light one.
 */
export const FILL_INKS = ['base-void', 'text-bright'] as const satisfies readonly PaletteKey[];
export type FillInk = (typeof FILL_INKS)[number];

/** A palette colour at `alpha` laid over a ground key, as the eye sees it (sRGB "over"). */
export function overGround(
  theme: ThemeName,
  key: PaletteKey,
  a: number,
  ground: PaletteKey = 'base-bg'
): string {
  if (!(a >= 0 && a <= 1)) throw new Error(`palette: alpha ${a} of ${key} is not in 0..1`);
  const fill = hexToRgb(PALETTE[theme][key]);
  const under = hexToRgb(PALETTE[theme][ground]);
  const channel = (f: number, g: number) =>
    Math.round(a * f + (1 - a) * g)
      .toString(16)
      .padStart(2, '0');
  return `#${channel(fill.r, under.r)}${channel(fill.g, under.g)}${channel(fill.b, under.b)}`;
}

/**
 * The ink for words on a fill of `key` at `alpha` over `ground` in `theme`: of `FILL_INKS`, the one
 * with the higher contrast on that fill — chosen from the fill's own luminance, so a light fill
 * takes the dark ink and a dark fill the light one, whatever the segment's rank.
 */
export function inkOnFill(
  theme: ThemeName,
  key: PaletteKey,
  a: number,
  ground: PaletteKey = 'base-bg'
): { ink: FillInk; contrast: number } {
  return inkOnColours(theme, [overGround(theme, key, a, ground)]);
}

/**
 * The ink for words that stand on several colours at once (a gradient fill under one label, as
 * the eye sees each over its ground): of `FILL_INKS`, the one whose lowest contrast over them is
 * the highest. `inkOnFill` is the one-colour case.
 */
export function inkOnColours(
  theme: ThemeName,
  colours: readonly string[]
): { ink: FillInk; contrast: number } {
  if (colours.length === 0) throw new Error('palette: an ink on no colour');
  return FILL_INKS.map((ink) => ({
    ink,
    contrast: Math.min(...colours.map((c) => contrastRatio(PALETTE[theme][ink], c))),
  })).reduce((best, next) => (next.contrast > best.contrast ? next : best));
}
