export const CHART_PRIMARY_COLOR = '#ff6b9d'; // Citypop pink
export const CHART_SECONDARY_COLOR = '#4dd0c8'; // Teal
export const CHART_TERTIARY_COLOR = '#ff8c42'; // Citypop orange

export const CHART_PALETTE = [
  '#ff6b9d', // Citypop pink
  '#ff8c42', // Orange
  '#b07cff', // Violet
  '#4dd0c8', // Teal
  '#64b5f6', // Sky blue
  '#ffe066', // Warm yellow
  '#ff4081', // Hot pink
  '#78edd8', // Bright teal
  '#d4a0ff', // Light violet
  '#ffb070', // Light coral
  '#80cbc4', // Muted teal
  '#706068', // Muted
] as const;

// Centralized chart UI colors
export const CHART_GRID_COLOR = '#1e1a2a';
export const CHART_AXIS_COLOR = '#453d42';
export const CHART_TOOLTIP_BG = '#110e1a';
export const CHART_TOOLTIP_BORDER = '#1e1a2a';
export const CHART_HOVER_COLOR = '#453d42';

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
