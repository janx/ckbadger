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
import { PALETTE } from '@/lib/palette';

/**
 * Visual finish V11: the bracket-hex inks page code drew became named constants without a value
 * change; the light theme then made each constant a reference into the palette. Each is pinned
 * here to its theme key, and the midnight value of that key to the exact hex the sites wrote, so a
 * drift fails here before it shows on a page.
 */
describe('inks with no theme token (V11)', () => {
  it('keeps the boards’ shared inks at the keys whose midnight values their sites wrote', () => {
    expect(BOARD_INKS).toEqual({
      quiet: 'var(--c-ink-quiet)',
      link: 'var(--c-board-link)',
      cellLink: 'var(--c-cell-lock)',
    });
    expect(PALETTE.dark['ink-quiet']).toBe('#4b5266');
    expect(PALETTE.dark['board-link']).toBe('#6aa9c4');
    expect(PALETTE.dark['cell-lock']).toBe('#7aa7d6');
    // the cell links are the lock blue, one value
    expect(BOARD_INKS.cellLink).toBe(CELL_COLORS.lock);
  });

  it('keeps the storage tiers’ inks at the keys whose midnight values their sites wrote', () => {
    expect(STORAGE_TIER_COLORS).toEqual({
      btc_ckb: {
        ink: 'var(--c-tier-btc-ink)',
        inkLit: 'var(--c-tier-btc-ink-lit)',
        edge: 'var(--c-tier-btc-edge)',
        edgeLit: 'var(--c-tier-btc-edge-lit)',
        // `from-[#4a3a12]/50`
        wash: 'rgb(var(--rgb-tier-btc-wash) / 0.5)',
      },
      pure_ckb: {
        ink: 'var(--c-tier-pure-ink)',
        inkLit: 'var(--c-tier-pure-ink-lit)',
        edge: 'var(--c-tier-pure-edge)',
        edgeLit: 'var(--c-tier-pure-edge-lit)',
        // `from-[#0e3830]/50`
        wash: 'rgb(var(--rgb-tier-pure-wash) / 0.5)',
      },
      centralized_mixture: { edge: 'var(--c-tier-centralized-edge)' },
    });
    const { dark } = PALETTE;
    expect([dark['tier-btc-ink'], dark['tier-btc-ink-lit'], dark['tier-btc-edge']]).toEqual([
      '#a0b880',
      '#c0d8a0',
      '#4a6838',
    ]);
    expect([dark['tier-btc-edge-lit'], dark['tier-btc-wash']]).toEqual(['#6a8850', '#4a3a12']);
    expect([dark['tier-pure-ink'], dark['tier-pure-ink-lit'], dark['tier-pure-edge']]).toEqual([
      '#5abfa0',
      '#40e8b0',
      '#1a6050',
    ]);
    expect([dark['tier-pure-edge-lit'], dark['tier-pure-wash']]).toEqual(['#2a8068', '#0e3830']);
    expect(dark['tier-centralized-edge']).toBe('#5a2020');
  });

  it('keeps the chrome inks and the family additions at their keys and midnight values', () => {
    expect(ROW_TAG_COLORS).toEqual({ accentEdge: 'var(--c-row-tag-accent-edge)' });
    expect(SEARCH_RESULT_COLORS).toEqual({
      panel: 'var(--c-search-panel)',
      selected: 'var(--c-search-selected)',
      hover: 'var(--c-search-hover)',
    });
    expect(STREAM_COLORS).toEqual({
      pending: 'var(--c-stream-pending)',
      pendingHover: 'var(--c-stream-pending-hover)',
    });
    expect(BLOCK_POSITION_COLORS).toEqual({ currentTile: 'var(--c-tile-current)' });
    expect(FIBER_COLORS.filterOn).toBe('var(--c-slate-other)');
    const { dark } = PALETTE;
    expect(dark['row-tag-accent-edge']).toBe('#1f5a46');
    expect([dark['search-panel'], dark['search-selected'], dark['search-hover']]).toEqual([
      '#06090f',
      '#091217',
      '#070e13',
    ]);
    expect([dark['stream-pending'], dark['stream-pending-hover']]).toEqual(['#04070d', '#060a11']);
    expect(dark['tile-current']).toBe('#123029');
    expect(dark['slate-other']).toBe('#2c3348');
  });

  it('passes CSS variables to a style prop as they are', () => {
    expect(cssVars({ '--tier-ink': 'var(--c-tier-btc-ink)' })).toEqual({
      '--tier-ink': 'var(--c-tier-btc-ink)',
    });
    expect(cssVars({})).toEqual({});
  });

  it('sets a text ink as the --ink variable its class reads, never as an inline colour', () => {
    expect(inkStyle(BOARD_INKS.quiet)).toEqual({ '--ink': 'var(--c-ink-quiet)' });
    expect(inkStyle(BOARD_INKS.link)).not.toHaveProperty('color');
  });
});
