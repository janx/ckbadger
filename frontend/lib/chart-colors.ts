export const CHART_PRIMARY_COLOR = '#4a8c5c'; // 竹青
export const CHART_SECONDARY_COLOR = '#1e7a6a'; // 石绿
export const CHART_TERTIARY_COLOR = '#b88420'; // 琥珀

export const CHART_PALETTE = [
  '#4a8c5c', // 竹青
  '#1e7a6a', // 石绿
  '#3a6ea0', // 靛蓝
  '#b88420', // 琥珀
  '#c04040', // 朱砂
  '#d4a828', // 藤黄
  '#b84060', // 胭脂
  '#7a5090', // 紫檀
  '#68a8b8', // 月白
  '#a06830', // 赭石
  '#4a5868', // 黛色
  '#8ab870', // 豆绿
] as const;

// Centralized chart UI colors
export const CHART_GRID_COLOR = '#e0d8cc';
export const CHART_AXIS_COLOR = '#b0a898';
export const CHART_TOOLTIP_BG = '#f3ede5';
export const CHART_TOOLTIP_BORDER = '#e0d8cc';
export const CHART_HOVER_COLOR = '#d0c8bc';

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
