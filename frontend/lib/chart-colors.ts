export const CHART_PRIMARY_COLOR = '#8ce00a'; // Emphasis lime
export const CHART_SECONDARY_COLOR = '#00d7eb'; // Interactive cyan
export const CHART_TERTIARY_COLOR = '#ffb900'; // Warning amber

export const CHART_PALETTE = [
  '#8ce00a', // Emphasis lime
  '#00d7eb', // Interactive cyan
  '#ffb900', // Warning amber
  '#008df8', // Info blue
  '#ff000f', // Negative red
  '#9a5feb', // Argonaut bright magenta
  '#67ffef', // Bright cyan
  '#abe05a', // Bright lime
  '#ffd141', // Bright amber
  '#0092ff', // Bright blue
  '#6c43a5', // Argonaut magenta
  '#6b6860', // Muted
] as const;

// Centralized chart UI colors
export const CHART_GRID_COLOR = '#1f2430';
export const CHART_AXIS_COLOR = '#4a4740';
export const CHART_TOOLTIP_BG = '#12151e';
export const CHART_TOOLTIP_BORDER = '#1f2430';
export const CHART_HOVER_COLOR = '#4a4740';

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
