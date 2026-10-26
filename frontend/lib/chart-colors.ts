import { CLASS_COLORS, DATA_COLOR, HUE_RAMP } from '@/lib/design-tokens';

export const CHART_PRIMARY_COLOR = DATA_COLOR;
export const CHART_SECONDARY_COLOR = HUE_RAMP[1];
export const CHART_TERTIARY_COLOR = HUE_RAMP[2];
export const CHART_PALETTE = HUE_RAMP;

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
  Transfer: CLASS_COLORS.ckb,
  'DAO Deposit': CLASS_COLORS.dao,
  'DAO Withdraw': CLASS_COLORS.dao,
  Token: CLASS_COLORS.tokens,
  Object: CLASS_COLORS.objects,
  Identity: CLASS_COLORS.identities,
  'Script Call': CLASS_COLORS.other,
};
export const ASSET_CATEGORY_COLORS: Record<string, string> = CLASS_COLORS;
export const SCRIPT_CHART_COLORS = HUE_RAMP;
