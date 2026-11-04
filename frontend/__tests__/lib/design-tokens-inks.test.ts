import { describe, expect, it } from 'vitest';
import {
  BLOCK_POSITION_COLORS,
  BOARD_INKS,
  CELL_COLORS,
  cssVars,
  FIBER_COLORS,
  inkStyle,
  ROW_TAG_COLORS,
  SEARCH_RESULT_COLORS,
  STORAGE_TIER_COLORS,
  STREAM_COLORS,
} from '@/lib/design-tokens';

/**
 * Visual finish V11: the bracket-hex inks page code drew became named constants without a value
 * change. Each constant is pinned here to the exact hex its sites wrote before, so a drift fails
 * here before it shows on a page.
 */
describe('inks with no theme token (V11)', () => {
  it('keeps the boards’ shared inks at the values their sites wrote', () => {
    expect(BOARD_INKS).toEqual({ quiet: '#4b5266', link: '#6aa9c4', cellLink: '#7aa7d6' });
    // the cell links are the lock blue, one value
    expect(BOARD_INKS.cellLink).toBe(CELL_COLORS.lock);
  });

  it('keeps the storage tiers’ inks at the values their sites wrote', () => {
    expect(STORAGE_TIER_COLORS).toEqual({
      btc_ckb: {
        ink: '#a0b880',
        inkLit: '#c0d8a0',
        edge: '#4a6838',
        edgeLit: '#6a8850',
        // `from-[#4a3a12]/50`
        wash: 'rgba(74, 58, 18, 0.5)',
      },
      pure_ckb: {
        ink: '#5abfa0',
        inkLit: '#40e8b0',
        edge: '#1a6050',
        edgeLit: '#2a8068',
        // `from-[#0e3830]/50`
        wash: 'rgba(14, 56, 48, 0.5)',
      },
      centralized_mixture: { edge: '#5a2020' },
    });
  });

  it('keeps the chrome inks and the family additions at the values their sites wrote', () => {
    expect(ROW_TAG_COLORS).toEqual({ accentEdge: '#1f5a46' });
    expect(SEARCH_RESULT_COLORS).toEqual({
      panel: '#06090f',
      selected: '#091217',
      hover: '#070e13',
    });
    expect(STREAM_COLORS).toEqual({ pending: '#04070d', pendingHover: '#060a11' });
    expect(BLOCK_POSITION_COLORS).toEqual({ currentTile: '#123029' });
    expect(FIBER_COLORS.filterOn).toBe('#2c3348');
  });

  it('passes CSS variables to a style prop as they are', () => {
    expect(cssVars({ '--tier-ink': '#a0b880' })).toEqual({ '--tier-ink': '#a0b880' });
    expect(cssVars({})).toEqual({});
  });

  it('sets a text ink as the --ink variable its class reads, never as an inline colour', () => {
    expect(inkStyle(BOARD_INKS.quiet)).toEqual({ '--ink': '#4b5266' });
    expect(inkStyle(BOARD_INKS.link)).not.toHaveProperty('color');
  });
});
