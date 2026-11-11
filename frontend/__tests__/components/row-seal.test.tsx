import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import {
  ROW_SEAL_FOOTPRINT_PX,
  ROW_SEAL_GAP_PX,
  ROW_SEAL_PX,
  ROW_SEAL_WIDTH_PX,
  RowSeal,
} from '@/components/ui/row-seal';
import { compound } from '@/lib/talisman/compound';
import { ROW } from '@/lib/talisman/recipes';
import { expectOneRowSeal } from '@/__tests__/utils/seals';

const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';
const PAINT = { ink: 'var(--c-mark)', ground: 'var(--c-base-bg)' };

/** 32 bytes from a seed, without randomness: a different stone for every seed. */
function hashOf(seed: number): string {
  let hex = '';
  let x = seed * 2654435761;
  for (let i = 0; i < 64; i++) {
    x = (x * 1103515245 + 12345) % 2147483648;
    hex += (x % 16).toString(16);
  }
  return `0x${hex}`;
}

describe('RowSeal (rubbing × Song print, phase 3)', () => {
  it('draws the row seal of the hash at 28 px, an image that is not a link', () => {
    const { container } = render(<RowSeal hash={HASH} />);
    expect(ROW_SEAL_PX).toBe(28);
    const seal = expectOneRowSeal(container, HASH);
    expect(seal).toHaveClass('[&>svg]:block');
  });

  // A list column's minimum width adds the footprint; the footprint is only true while the drawn
  // stone is 19 px wide (the stone's field is fixed, 7 × 11, so every hash gives the same width).
  it('is 19 px wide for every hash, so a column budget built on its footprint holds', () => {
    for (let seed = 1; seed <= 24; seed++) {
      const svg = compound(hashOf(seed), ROW, ROW_SEAL_PX, PAINT).svg;
      expect(svg).toMatch(new RegExp(`^<svg width="${ROW_SEAL_WIDTH_PX}" height="28"`));
    }
    expect(ROW_SEAL_WIDTH_PX).toBe(19);
    expect(ROW_SEAL_GAP_PX).toBe(8);
    expect(ROW_SEAL_FOOTPRINT_PX).toBe(27);
    // the gap is the seal's own 8 px right margin
    const { container } = render(<RowSeal hash={HASH} />);
    expect(container.firstElementChild).toHaveClass('mr-2');
  });

  it('overhangs a one-line row by 5 px above and below only when asked (28 = 18 + 2 × 5)', () => {
    const { container, rerender } = render(<RowSeal hash={HASH} />);
    expect(container.firstElementChild).not.toHaveClass('-my-[5px]');
    rerender(<RowSeal hash={HASH} overhang />);
    expect(container.firstElementChild).toHaveClass('-my-[5px]');
  });
});
