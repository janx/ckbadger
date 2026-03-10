export const CHART_PRIMARY_COLOR = '#44ee77'; // phosphor green
export const CHART_SECONDARY_COLOR = '#ff66aa'; // rose
export const CHART_TERTIARY_COLOR = '#44bbff'; // sky

export const CHART_PALETTE = [
  '#44ee77', // phosphor green
  '#ff66aa', // rose
  '#44bbff', // sky
  '#00ffaa', // mint
  '#bb88ff', // violet
  '#66ff99', // green-bright
  '#ff6699', // rose-bright
  '#66ddff', // sky-bright
  '#44ffcc', // mint-bright
  '#ccaaff', // violet-bright
  '#2daa55', // green-dim
  '#dd4488', // rose-dim
] as const;

// Centralized chart UI colors (dark theme)
export const CHART_GRID_COLOR = '#1c2236';
export const CHART_AXIS_COLOR = '#3a4260';
export const CHART_TOOLTIP_BG = '#0e1119';
export const CHART_TOOLTIP_BORDER = '#222840';
export const CHART_HOVER_COLOR = '#141820';

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
