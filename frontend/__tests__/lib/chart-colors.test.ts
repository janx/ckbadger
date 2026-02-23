import { describe, expect, it } from 'vitest';
import {
  CHART_PALETTE,
  CHART_PRIMARY_COLOR,
  CHART_SECONDARY_COLOR,
  getChartPaletteColor,
} from '@/lib/chart-colors';

describe('chart-colors', () => {
  it('starts palette with primary and secondary semantic colors', () => {
    expect(CHART_PALETTE[0]).toBe(CHART_PRIMARY_COLOR);
    expect(CHART_PALETTE[1]).toBe(CHART_SECONDARY_COLOR);
  });

  it('wraps indexes for fallback palette lookup', () => {
    expect(getChartPaletteColor(CHART_PALETTE.length)).toBe(CHART_PALETTE[0]);
    expect(getChartPaletteColor(-1)).toBe(CHART_PALETTE[CHART_PALETTE.length - 1]);
  });
});
