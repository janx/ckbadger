export const CHART_PRIMARY_COLOR = '#00ff41';
export const CHART_SECONDARY_COLOR = '#ffb000';
export const CHART_TERTIARY_COLOR = '#00c389';

export const CHART_PALETTE = [
  CHART_PRIMARY_COLOR,
  CHART_SECONDARY_COLOR,
  CHART_TERTIARY_COLOR,
  '#3b82f6',
  '#ef4444',
  '#14b8a6',
  '#f97316',
  '#84cc16',
  '#22d3ee',
  '#ec4899',
  '#6366f1',
  '#5a6a7f',
] as const;

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
