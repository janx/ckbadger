export const CHART_PRIMARY_COLOR = '#f0b866'; // amber
export const CHART_SECONDARY_COLOR = '#e87ea0'; // rose
export const CHART_TERTIARY_COLOR = '#6ab0e8'; // sky

export const CHART_PALETTE = [
  '#f0b866', // amber
  '#e87ea0', // rose
  '#6ab0e8', // sky
  '#5ce0b8', // mint
  '#b08af0', // violet
  '#f8ca80', // amber-bright
  '#f07898', // rose-bright
  '#82c0f0', // sky-bright
  '#78f0d0', // mint-bright
  '#c8a0f8', // violet-bright
  '#c89440', // amber-dim
  '#c0608a', // rose-dim
] as const;

// Centralized chart UI colors (dark theme)
export const CHART_GRID_COLOR = '#1a1f30';
export const CHART_AXIS_COLOR = '#343c50';
export const CHART_TOOLTIP_BG = '#10131c';
export const CHART_TOOLTIP_BORDER = '#222840';
export const CHART_HOVER_COLOR = '#161a25';

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
