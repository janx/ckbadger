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

const dark = {
  // grounds (lib.js: well, bg, surface, elevated; line, hair)
  'base-void': '#0f0d0b',
  'base-bg': '#15120f',
  'base-surface': '#1b1714',
  'base-elevated': '#241f1a',
  'base-border': '#2a231d',
  'base-border-subtle': '#221c17',
  // inks (lib.js: moon, text, dim — lightened from #8a8073 to 4.5:1 — ghost)
  'text-bright': '#e8e2d6',
  text: '#b9b0a2',
  'text-dim': '#908679',
  'text-ghost': '#4a4139',
  /** The quiet ink between ghost and dim: separators, byte offsets, rulers, a note's frame. */
  'ink-quiet': '#60574d',
  /** The deeper dim of the pipeline and the loading marquee. */
  'ink-muted': '#756c61',
  /** The phosphor white a live figure flares to (moon lit by celadon); the text-selection ink. */
  'neon-white': '#f2f8f6',
  // accents, by the key names the pages use: jade = celadon 青瓷, aqua = indigo 靛青, gold = old
  // gold 古金, rouge = cinnabar text 朱砂, lavender = lotus 藕荷, amber = moss 苔绿
  jade: '#93c1b6',
  'jade-dim': '#6f9a90',
  'jade-bright': '#b7d9d0',
  aqua: '#7fa3c9',
  'aqua-dim': '#6e8aa7',
  'aqua-bright': '#97bce3',
  gold: '#c9a45c',
  'gold-dim': '#a38750',
  'gold-bright': '#e3bd75',
  rouge: '#d9634a',
  'rouge-dim': '#aa503c',
  'rouge-bright': '#f57c62',
  lavender: '#a797c4',
  'lavender-dim': '#8e82a5',
  amber: '#a3bf7a',
  'amber-dim': '#889d69',
  /** Cinnabar, the mark: prompt squares, seals, the search square (UI grade, not text). */
  mark: '#c2452d',
  // asset classes: token azurite 石青, object a lotus violet, identity the boards' rose, copper
  // ochre 赭石 (bitcoin)
  token: '#6f9ad6',
  'token-bright': '#87b3f1',
  object: '#a58ad6',
  identity: '#c97f93',
  copper: '#cc8a5e',
  /** Plain CKB as a mark: the dim ink, a step above ghost. */
  'slate-ckb': '#524941',
  /** "Other" as a mark, and a tile's outline: the boards' second line (lib.js line2). */
  'slate-other': '#342c25',
  // the cell's four parts
  'cell-capacity': '#e8e2d6',
  'cell-lock': '#85a9d0',
  'cell-type': '#aac583',
  'cell-data': '#89b2c0',
  /** Framing inside the data (a length, an offset): structure, not content. */
  'data-framing': '#7898a3',
  // boards
  'board-link': '#87afc1',
  tile: '#181512',
  'tile-current': '#313631',
  'grid-faint': '#1c1713',
  'flow-ckb': '#a09789',
  'flow-fee': '#71675c',
  // the jade ramp after jade itself (the first stop), strongest first: celadon mixed into the
  // page ground at 80 / 64 / 50 / 38 / 27 %
  'ramp-1': '#7a9e95',
  'ramp-2': '#66827a',
  'ramp-3': '#546a63',
  'ramp-4': '#45554e',
  'ramp-5': '#37413c',
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
  // chrome
  'search-panel': '#0c0a09',
  'search-selected': '#1a1b19',
  'search-hover': '#131210',
  'stream-pending': '#0b0908',
  'stream-pending-hover': '#0d0b0a',
  'row-tag-accent-edge': '#42514b',
  'reward-secondary': '#5e7870',
  // "who can change the code" swatches
  'code-fixed-fill': '#9c9385',
  'code-unspendable-fill': '#665d53',
  // an ELF file's regions
  'elf-header': '#7898a3',
  'elf-program-headers': '#8daeb9',
  'elf-rodata': '#9ac09b',
  'elf-eh-frame': '#aab9aa',
  'elf-data': '#d5b985',
  'elf-meta': '#b8acd0',
  // the sequencing gel
  'gel-overlay': '#f6f2ea',
  'gel-band-text': '#0e0c0a',
  'gel-ink-btc-0': '#f1dece',
  'gel-ink-btc-1': '#cc8a5e',
  'gel-ink-btc-2': '#b7d9d0',
  'gel-ink-pure-0': '#f2f8f6',
  'gel-ink-pure-1': '#b7d9d0',
  'gel-ink-pure-2': '#93c1b6',
  // storage tiers
  'tier-btc-ink': '#a7bc8a',
  'tier-btc-ink-lit': '#bdd3a0',
  'tier-btc-edge': '#485036',
  'tier-btc-edge-lit': '#5f6c47',
  'tier-btc-wash': '#473b25',
  'tier-pure-ink': '#87b4a9',
  'tier-pure-ink-lit': '#b7d9d0',
  'tier-pure-edge': '#404e48',
  'tier-pure-edge-lit': '#546a63',
  'tier-pure-wash': '#292e2a',
  'tier-centralized-edge': '#57251a',
  // composition badges
  'badge-inner-pure': '#181a17',
  'badge-inner-btc': '#1a1510',
  'badge-jade-pale': '#f2f8f6',
  'badge-jade-light': '#b7d9d0',
  'badge-jade-mid': '#a5cdc3',
  'badge-copper-pale': '#f1dece',
  'bitcoin-light': '#edd6c3',
  'bitcoin-dark': '#945d36',
  // the storage "gem" text
  'gem-gold-0': '#e3bd75',
  'gem-gold-1': '#c9a45c',
  'gem-jade-0': '#93c1b6',
  'gem-jade-1': '#b7d9d0',
  // the Game of Life tile
  'life-bg-ckb': '#141412',
  'life-bg-dual': '#15120d',
  // the not-found night
  'night-base': '#0d0b09',
  'night-rose': '#dd91a5',
  'night-violet': '#a797c4',
  // the sky: the page ground is the boards' bg, a hair lighter at the top, a hair deeper at the foot
  'sky-top': '#171411',
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
  'rouge-dim': '#a6594c',
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
  'search-panel': '#fffdf8',
  'search-selected': '#e6e9e2',
  'search-hover': '#f2f1ea',
  'stream-pending': '#eee8dc',
  'stream-pending-hover': '#e4dccd',
  'row-tag-accent-edge': '#acbfb6',
  'reward-secondary': '#91ada4',
  'code-fixed-fill': '#7a7163',
  'code-unspendable-fill': '#bbb1a0',
  'elf-header': '#416a7a',
  'elf-program-headers': '#588192',
  'elf-rodata': '#3d7249',
  'elf-eh-frame': '#5f7362',
  'elf-data': '#82672c',
  'elf-meta': '#7468a7',
  'gel-overlay': '#1f1b17',
  'gel-band-text': '#faf7f0',
  'gel-ink-btc-0': '#b78c70',
  'gel-ink-btc-1': '#93522b',
  'gel-ink-btc-2': '#346e65',
  'gel-ink-pure-0': '#346e65',
  'gel-ink-pure-1': '#2f6b63',
  'gel-ink-pure-2': '#185750',
  'tier-btc-ink': '#4b6e2e',
  'tier-btc-ink-lit': '#3b5d1c',
  'tier-btc-edge': '#a5b58b',
  'tier-btc-edge-lit': '#88a06a',
  'tier-btc-wash': '#d1c3a4',
  'tier-pure-ink': '#2f6b63',
  'tier-pure-ink-lit': '#185750',
  'tier-pure-edge': '#a9bcb3',
  'tier-pure-edge-lit': '#87a69d',
  'tier-pure-wash': '#ccd4ca',
  'tier-centralized-edge': '#dba799',
  'badge-inner-pure': '#e6e9e2',
  'badge-inner-btc': '#f2eae0',
  'badge-jade-pale': '#346e65',
  'badge-jade-light': '#407a70',
  'badge-jade-mid': '#326d64',
  'badge-copper-pale': '#b78c70',
  'bitcoin-light': '#c19c84',
  'bitcoin-dark': '#7a3b11',
  'gem-gold-0': '#80600f',
  'gem-gold-1': '#684e0d',
  'gem-jade-0': '#346e65',
  'gem-jade-1': '#2f6b63',
  'life-bg-ckb': '#e7e6dc',
  'life-bg-dual': '#ede7d9',
  'night-base': '#f3eee4',
  'night-rose': '#9c3b5f',
  'night-violet': '#5e4d9a',
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
 * The rubbing / Song print finish (the boards' `scan` and `inset`): CRT scanlines by night, 1 px
 * of black at 22 % in every 3; ruled paper by day, the same lines in the ink at 4.5 %. The vignette
 * and the grain sit at about half their midnight strength. A frame (`.glass`, the rule name kept)
 * is a flat tile: the surface, opaque, under its own 1 px border — no sheen, no lit edges, no
 * outer ring. A well is the boards' inset (`inset 0 1px 3px` at 60 % black by night, the ink at
 * 8 % by day) with no lip. The tints are the palette's own grounds and inks.
 */
const darkMaterials = {
  glow: '1',
  vignette: 'rgba(0, 0, 0, 0.18)',
  'scanline-ink': 'rgba(0, 0, 0, 0.22)',
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
  'header-strip': hexAlpha(dark['base-void'], 0.8),
  'drop-shadow': '0 14px 32px rgba(0, 0, 0, 0.68)',
  'raised-shadow': '0 10px 24px rgba(0, 0, 0, 0.28)',
  'preview-inset': `inset 0 2px 8px rgba(0, 0, 0, 0.7), inset 0 0 1px ${hexAlpha(dark['text-bright'], 0.04)}`,
  'preview-cast': 'rgba(0, 0, 0, 0.5)',
  'preview-sheen': hexAlpha(dark['text-bright'], 0.08),
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
  'header-strip': hexAlpha(light['base-bg'], 0.8),
  'drop-shadow': `0 14px 32px ${hexAlpha(DAY_INK, 0.18)}`,
  'raised-shadow': `0 10px 24px ${hexAlpha(DAY_INK, 0.12)}`,
  'preview-inset': `inset 0 2px 8px ${hexAlpha(DAY_INK, 0.12)}, inset 0 0 1px ${hexAlpha(DAY_PAPER, 0.6)}`,
  'preview-cast': hexAlpha(DAY_INK, 0.18),
  'preview-sheen': hexAlpha(DAY_PAPER, 0.5),
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
