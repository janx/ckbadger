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
 * The dark theme is the midnight the site was designed in (ink and silk, neon tubes); the light
 * theme is the same ink on paper by day: the accents deepen to text-grade contrast (≥ 4.5:1 on
 * every ground, pinned by __tests__/lib/palette.test.ts), the grounds step from paper (the page)
 * to white (a surface) to a grey tint (elevated, a hover), and the glows go out (`glow` = 0).
 */

export const THEMES = ['dark', 'light'] as const;
export type ThemeName = (typeof THEMES)[number];

const dark = {
  // grounds
  'base-void': '#08090e',
  'base-bg': '#0c0e15',
  'base-surface': '#10131c',
  'base-elevated': '#161a25',
  'base-border': '#222840',
  'base-border-subtle': '#1a1f30',
  // inks
  'text-bright': '#dee2ec',
  text: '#a0a8be',
  'text-dim': '#7c849c',
  'text-ghost': '#343c50',
  /** The quiet slate between ghost and dim: separators, byte offsets, rulers, a note's frame. */
  'ink-quiet': '#4b5266',
  /** The deeper dim of the pipeline and the loading marquee. */
  'ink-muted': '#606880',
  /** The neon white a live figure flares to; the text-selection ink. */
  'neon-white': '#e9fff6',
  // accents (Chinese traditional names: jade 翠玉, aqua 缥碧, gold 琥珀, rouge 胭脂, lavender 紫藤)
  jade: '#2edba3',
  'jade-dim': '#1fb88a',
  'jade-bright': '#3ef0b8',
  aqua: '#68ccf0',
  'aqua-dim': '#4aa8d0',
  'aqua-bright': '#88ddf8',
  gold: '#f2c55c',
  'gold-dim': '#d0a840',
  'gold-bright': '#f8d878',
  rouge: '#e8555a',
  'rouge-dim': '#c04048',
  'rouge-bright': '#f06668',
  lavender: '#b8a9e8',
  'lavender-dim': '#9888c8',
  amber: '#d4883a',
  'amber-dim': '#b07028',
  // asset classes
  token: '#5aa6f5',
  'token-bright': '#8cc3ff',
  object: '#a98cf5',
  identity: '#f08aa8',
  copper: '#dc8a5a',
  /** Plain CKB as a mark: the dim slate. */
  'slate-ckb': '#3a4258',
  /** "Other" as a mark, and a tile's outline: the darkest slate. */
  'slate-other': '#2c3348',
  // the cell's four parts
  'cell-capacity': '#dee2ec',
  'cell-lock': '#7aa7d6',
  'cell-type': '#b5c77a',
  'cell-data': '#6fc3d9',
  /** Framing inside the data (a length, an offset): structure, not content. */
  'data-framing': '#5f9fb2',
  // boards
  'board-link': '#6aa9c4',
  tile: '#141826',
  'tile-current': '#123029',
  'grid-faint': '#121620',
  'flow-ckb': '#8e97ad',
  'flow-fee': '#5a6378',
  // the jade ramp after jade itself (the first stop), strongest first
  'ramp-1': '#24b68a',
  'ramp-2': '#259775',
  'ramp-3': '#287b64',
  'ramp-4': '#2b6054',
  'ramp-5': '#344e48',
  // the transaction pipeline's stage fills (aqua / gold / jade stepped to OKLCH L ≈ 0.66 on dark)
  'pipeline-pending': '#3e9dca',
  'pipeline-proposed': '#bb881a',
  'pipeline-committed': '#00ac7c',
  'pipeline-drop-neutral': '#898989',
  /** The pipeline's pools are wells: recessed below the surface (22 % black on it at night). */
  'pipeline-well': '#0c0f16',
  /** What a stage hue is mixed toward as cycles rise: lighter at night, inkier by day. */
  'pipeline-step-to': '#ffffff',
  // chrome
  'search-panel': '#06090f',
  'search-selected': '#091217',
  'search-hover': '#070e13',
  'stream-pending': '#04070d',
  'stream-pending-hover': '#060a11',
  'row-tag-accent-edge': '#1f5a46',
  'reward-secondary': '#17775c',
  // "who can change the code" swatches
  'code-fixed-fill': '#8a91a6',
  'code-unspendable-fill': '#5c6378',
  // an ELF file's regions
  'elf-header': '#5f9fb2',
  'elf-program-headers': '#7fb0c0',
  'elf-rodata': '#8fd6a8',
  'elf-eh-frame': '#9ab0a0',
  'elf-data': '#d9c38f',
  'elf-meta': '#c9b3d9',
  // the sequencing gel
  'gel-overlay': '#ffffff',
  'gel-band-text': '#17170c',
  'gel-ink-btc-0': '#ffe7d8',
  'gel-ink-btc-1': '#dc8a5a',
  'gel-ink-btc-2': '#9df6dc',
  'gel-ink-pure-0': '#f3fffb',
  'gel-ink-pure-1': '#9df6dc',
  'gel-ink-pure-2': '#3ee0b0',
  // storage tiers
  'tier-btc-ink': '#a0b880',
  'tier-btc-ink-lit': '#c0d8a0',
  'tier-btc-edge': '#4a6838',
  'tier-btc-edge-lit': '#6a8850',
  'tier-btc-wash': '#4a3a12',
  'tier-pure-ink': '#5abfa0',
  'tier-pure-ink-lit': '#40e8b0',
  'tier-pure-edge': '#1a6050',
  'tier-pure-edge-lit': '#2a8068',
  'tier-pure-wash': '#0e3830',
  'tier-centralized-edge': '#5a2020',
  // composition badges
  'badge-inner-pure': '#0b1d1a',
  'badge-inner-btc': '#1a110d',
  'badge-jade-pale': '#f0fffa',
  'badge-jade-light': '#8ff3d6',
  'badge-jade-mid': '#9df6dc',
  'badge-copper-pale': '#ffe7d8',
  'bitcoin-light': '#ffe1cc',
  'bitcoin-dark': '#a95a32',
  // the storage "gem" text
  'gem-gold-0': '#f0d060',
  'gem-gold-1': '#d4b850',
  'gem-jade-0': '#60d0a8',
  'gem-jade-1': '#40e8b0',
  // the Game of Life tile
  'life-bg-ckb': '#0a0f12',
  'life-bg-dual': '#0c0b08',
  // the not-found night
  'night-base': '#080a0f',
  'night-rose': '#ff85b0',
  'night-violet': '#a86ecc',
  // the sky
  'sky-top': '#0b0d17',
  'sky-mid': '#08090e',
  'sky-bottom': '#09090d',
  'well-bg': '#0a0c12',
  'selection-ink': '#e9fff6',
  // artwork, the same by night and by day: the crystal and its copper mount
  'crystal-top': '#a8f6de',
  'crystal-top-light': '#e4fff6',
  'crystal-left': '#3fd3a8',
  'crystal-right': '#86ecce',
  'crystal-edge': '#0b3b30',
  'crystal-glint': '#ffffff',
  'mount-top-light': '#ffeadf',
  'mount-top-mid': '#efa682',
  'mount-top-dark': '#c86f45',
  'mount-front-light': '#df8b5f',
  'mount-front-dark': '#82422a',
  'mount-edge': '#55291a',
} as const;

export type PaletteKey = keyof typeof dark;
export type Palette = Readonly<Record<PaletteKey, string>>;

const light: Palette = {
  'base-void': '#e3e6ea',
  'base-bg': '#f0f2f5',
  'base-surface': '#fafbfc',
  'base-elevated': '#e8ebef',
  'base-border': '#cbd2db',
  'base-border-subtle': '#dde2e8',
  'text-bright': '#141922',
  text: '#3a4254',
  'text-dim': '#5f677a',
  'text-ghost': '#a6adbb',
  'ink-quiet': '#8690a2',
  'ink-muted': '#6b7386',
  'neon-white': '#0b3b30',
  jade: '#0a7254',
  'jade-dim': '#096349',
  'jade-bright': '#0d8a66',
  aqua: '#0d6b96',
  'aqua-dim': '#0a5578',
  'aqua-bright': '#1583b6',
  gold: '#7f5c10',
  'gold-dim': '#6f5010',
  'gold-bright': '#a3781a',
  rouge: '#b5303a',
  'rouge-dim': '#962630',
  'rouge-bright': '#cf3f47',
  lavender: '#6a55b5',
  'lavender-dim': '#564393',
  amber: '#9c520e',
  'amber-dim': '#86450c',
  token: '#2563b5',
  'token-bright': '#3b7bd1',
  object: '#6b4ec2',
  identity: '#ab3466',
  copper: '#9a5024',
  'slate-ckb': '#9aa3b3',
  'slate-other': '#b9c0cc',
  'cell-capacity': '#141922',
  'cell-lock': '#2d5c90',
  'cell-type': '#51691a',
  'cell-data': '#116a7e',
  'data-framing': '#2a6b7f',
  'board-link': '#0a5f86',
  tile: '#e9edf1',
  'tile-current': '#d6efe6',
  'grid-faint': '#e4e8ed',
  'flow-ckb': '#6c7588',
  'flow-fee': '#9099ab',
  'ramp-1': '#2b8c6f',
  'ramp-2': '#4a9d85',
  'ramp-3': '#6bad9c',
  'ramp-4': '#8fbdb3',
  'ramp-5': '#b2cdc8',
  'pipeline-pending': '#1f7aa8',
  'pipeline-proposed': '#9a6f0f',
  'pipeline-committed': '#00855f',
  'pipeline-drop-neutral': '#8a8a8a',
  'pipeline-well': '#e3e7ec',
  'pipeline-step-to': '#141922',
  'search-panel': '#ffffff',
  'search-selected': '#e6f4ef',
  'search-hover': '#f1f5f4',
  'stream-pending': '#e9edf1',
  'stream-pending-hover': '#e2e7ec',
  'row-tag-accent-edge': '#9fd8c4',
  'reward-secondary': '#7fc7ad',
  'code-fixed-fill': '#7b8497',
  'code-unspendable-fill': '#b3bac6',
  'elf-header': '#2a6b7f',
  'elf-program-headers': '#4a90a4',
  'elf-rodata': '#3f9a62',
  'elf-eh-frame': '#6c8a74',
  'elf-data': '#9a7c2a',
  'elf-meta': '#8a6aa8',
  'gel-overlay': '#141922',
  'gel-band-text': '#fafbfc',
  'gel-ink-btc-0': '#c2805a',
  'gel-ink-btc-1': '#9a5024',
  'gel-ink-btc-2': '#0f8f69',
  'gel-ink-pure-0': '#0d9a70',
  'gel-ink-pure-1': '#0a7254',
  'gel-ink-pure-2': '#096349',
  'tier-btc-ink': '#4d6a2a',
  'tier-btc-ink-lit': '#3d5a1f',
  'tier-btc-edge': '#9bb57e',
  'tier-btc-edge-lit': '#7f9c5f',
  'tier-btc-wash': '#dcc88c',
  'tier-pure-ink': '#0a7254',
  'tier-pure-ink-lit': '#096349',
  'tier-pure-edge': '#8fd0bb',
  'tier-pure-edge-lit': '#6dbba2',
  'tier-pure-wash': '#b4e2d2',
  'tier-centralized-edge': '#e0a8a8',
  'badge-inner-pure': '#e6f4ef',
  'badge-inner-btc': '#f6ebe3',
  'badge-jade-pale': '#0d9a70',
  'badge-jade-light': '#2f9f7f',
  'badge-jade-mid': '#2a8d72',
  'badge-copper-pale': '#c2805a',
  'bitcoin-light': '#d9956b',
  'bitcoin-dark': '#7e3f1e',
  'gem-gold-0': '#a3781a',
  'gem-gold-1': '#8a6412',
  'gem-jade-0': '#2a8d72',
  'gem-jade-1': '#0a7254',
  'life-bg-ckb': '#e8f3ee',
  'life-bg-dual': '#f3efe3',
  'night-base': '#f0f2f5',
  'night-rose': '#c8356f',
  'night-violet': '#6b4ec2',
  'sky-top': '#f5f6f8',
  'sky-mid': '#f0f2f5',
  'sky-bottom': '#eef0f3',
  'well-bg': '#eceff3',
  'selection-ink': '#141922',
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
 * The finish that is not a colour: the fixed layers behind the page, the glass and the wells,
 * and `glow`, the factor every halo's alpha is multiplied by (1 at night, 0 by day: nothing
 * glows on paper).
 */
const darkMaterials = {
  glow: '1',
  vignette: 'rgba(0, 0, 0, 0.35)',
  'scanline-ink': 'rgba(10, 14, 28, 0.06)',
  'grain-blend': 'screen',
  'grain-opacity': '0.03',
  'inset-scan': 'rgba(0, 0, 0, 0.06)',
  'glass-fill': 'rgba(16, 19, 28, 0.72)',
  'glass-sheen': 'rgba(255, 255, 255, 0.03)',
  'glass-top-edge': 'rgba(255, 255, 255, 0.1)',
  'glass-bottom-edge': 'rgba(0, 0, 0, 0.55)',
  'glass-ring': 'rgba(0, 0, 0, 0.45)',
  'well-inset': 'rgba(0, 0, 0, 0.65)',
  'well-lip': 'rgba(255, 255, 255, 0.04)',
  'tier2-ground': 'rgba(6, 7, 11, 0.9)',
  'header-strip': 'rgba(10, 12, 18, 0.8)',
  'drop-shadow': '0 14px 32px rgba(0, 0, 0, 0.68)',
  'raised-shadow': '0 10px 24px rgba(0, 0, 0, 0.28)',
  'preview-inset': 'inset 0 2px 8px rgba(0, 0, 0, 0.7), inset 0 0 1px rgba(255, 255, 255, 0.04)',
  'preview-cast': 'rgba(0, 0, 0, 0.5)',
  'preview-sheen': 'rgba(255, 255, 255, 0.08)',
} as const;
export type MaterialKey = keyof typeof darkMaterials;
export type Materials = Readonly<Record<MaterialKey, string>>;

const lightMaterials: Materials = {
  glow: '0',
  vignette: 'rgba(20, 25, 34, 0.06)',
  'scanline-ink': 'rgba(20, 25, 34, 0.03)',
  'grain-blend': 'multiply',
  'grain-opacity': '0.05',
  'inset-scan': 'rgba(20, 25, 34, 0.025)',
  'glass-fill': 'rgba(250, 251, 252, 0.72)',
  'glass-sheen': 'rgba(255, 255, 255, 0.5)',
  'glass-top-edge': 'rgba(255, 255, 255, 0.9)',
  'glass-bottom-edge': 'rgba(20, 25, 34, 0.1)',
  'glass-ring': 'rgba(20, 25, 34, 0.06)',
  'well-inset': 'rgba(20, 25, 34, 0.08)',
  'well-lip': 'rgba(255, 255, 255, 0.9)',
  'tier2-ground': 'rgba(228, 232, 237, 0.9)',
  'header-strip': 'rgba(240, 242, 245, 0.8)',
  'drop-shadow': '0 14px 32px rgba(20, 25, 34, 0.18)',
  'raised-shadow': '0 10px 24px rgba(20, 25, 34, 0.12)',
  'preview-inset': 'inset 0 2px 8px rgba(20, 25, 34, 0.12), inset 0 0 1px rgba(255, 255, 255, 0.6)',
  'preview-cast': 'rgba(20, 25, 34, 0.18)',
  'preview-sheen': 'rgba(255, 255, 255, 0.5)',
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
