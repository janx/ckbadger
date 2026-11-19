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
  STREAM_COLORS,
} from '@/lib/design-tokens';
import { PALETTE } from '@/lib/palette';

/**
 * Visual finish V11: the bracket-hex inks page code drew became named constants without a value
 * change; the light theme then made each constant a reference into the palette. Each is pinned
 * here to its theme key, and the night value of that key to its rubbing value (re-derived from the
 * design boards in the rubbing × Song print re-theme, frontend/docs/ui-color-guidelines.md), so a
 * drift fails here before it shows on a page.
 *
 * Re-pinned on purpose with the night values of the boards' ruling of 2026-11-18 (readability
 * plan, ruling R2): the quiet slate (Lc 30), the board link and the lock blue (Lc 70) and the slate
 * outline (1.60:1) moved, and the celadon mixes (the row tag's edge, the search tints, the current
 * tile) were recomputed by their rules from the moved celadon and surface. Before: #60574d,
 * #87afc1, #85a9d0; #42514b; #1a1b19, #131210; #313631; #342c25.
 */
describe('inks with no theme token (V11)', () => {
  it('keeps the boards’ shared inks at their keys and rubbing values', () => {
    expect(BOARD_INKS).toEqual({
      quiet: 'var(--c-ink-quiet)',
      link: 'var(--c-board-link)',
      cellLink: 'var(--c-cell-lock)',
    });
    expect(PALETTE.dark['ink-quiet']).toBe('#7f766b');
    expect(PALETTE.dark['board-link']).toBe('#a1cadc');
    expect(PALETTE.dark['cell-lock']).toBe('#a2c7f0');
    // the cell links are the lock blue, one value
    expect(BOARD_INKS.cellLink).toBe(CELL_COLORS.lock);
  });

  it('keeps the chrome inks and the family additions at their keys and rubbing values', () => {
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
    // re-pinned on purpose (lane C2 topics): the channels list's state filter is the controls
    // board's filter chips, whose chosen one is the celadon line, so the pressed edge in the
    // "other" slate went with the page's own chips (it was var(--c-slate-other))
    expect(FIBER_COLORS).not.toHaveProperty('filterOn');
    const { dark } = PALETTE;
    expect(dark['row-tag-accent-edge']).toBe('#46554f');
    expect([dark['search-panel'], dark['search-selected'], dark['search-hover']]).toEqual([
      '#0c0a09',
      '#1a1c1a',
      '#131310',
    ]);
    expect([dark['stream-pending'], dark['stream-pending-hover']]).toEqual(['#0b0908', '#0d0b0a']);
    expect(dark['tile-current']).toBe('#353a36');
    expect(dark['slate-other']).toBe('#3f3730');
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
