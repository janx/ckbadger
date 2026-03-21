// Chinese Traditional palette for charts
export const CHART_PRIMARY_COLOR = '#2edba3'; // 翠玉 jade
export const CHART_SECONDARY_COLOR = '#e8555a'; // 胭脂 rouge
export const CHART_TERTIARY_COLOR = '#68ccf0'; // 缥碧 aqua

export const CHART_PALETTE = [
  '#2edba3', // jade
  '#e8555a', // rouge
  '#68ccf0', // aqua
  '#f2c55c', // gold
  '#b8a9e8', // lavender
  '#d4883a', // amber
  '#1fb88a', // jade-dim
  '#c04048', // rouge-dim
  '#4aa8d0', // aqua-dim
  '#d0a840', // gold-dim
  '#9888c8', // lavender-dim
  '#b07028', // amber-dim
] as const;

export const CHART_GRID_COLOR = '#1a1f30';
export const CHART_AXIS_COLOR = '#343c50';
export const CHART_TOOLTIP_BG = '#10131c';
export const CHART_TOOLTIP_BORDER = CHART_TOOLTIP_BG;
export const CHART_HOVER_BG = '#161a25';
export const CHART_HOVER_COLOR = CHART_HOVER_BG;

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}

// Activity type colors — shared by activity card, activity breakdown, and pie charts
export const ACTIVITY_TYPE_COLORS: Record<string, string> = {
  Transfer: '#00ffaa',
  'DAO Deposit': '#44ee77',
  'DAO Withdraw': '#2daa55',
  Token: '#ff66aa',
  Object: '#bb88ff',
  Identity: '#44bbff',
  'Script Call': '#ff8800',
};

// Asset category colors — keyed by category slug
export const ASSET_CATEGORY_COLORS: Record<string, string> = {
  dao: ACTIVITY_TYPE_COLORS['DAO Deposit'],
  tokens: ACTIVITY_TYPE_COLORS.Token,
  objects: ACTIVITY_TYPE_COLORS.Object,
  other: '#666677',
};

// Rotating palette for per-script charts
export const SCRIPT_CHART_COLORS = [
  '#44ee77',
  '#ff66aa',
  '#44bbff',
  '#00ffaa',
  '#bb88ff',
  '#66ff99',
  '#ff6699',
  '#66ddff',
  '#44ffcc',
  '#ccaaff',
] as const;
