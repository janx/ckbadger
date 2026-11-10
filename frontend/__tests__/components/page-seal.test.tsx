import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  PAGE_SEAL_SIZES,
  PAGE_SEAL_TILT,
  PageSeal,
  SealedHead,
  type PageSealScale,
} from '@/components/ui/page-seal';
import { compound } from '@/lib/talisman/compound';
import { PAGE } from '@/lib/talisman/recipes';
import { expectBefore, expectOnePageSeal } from '@/__tests__/utils/seals';

// block #20,914,858's hash (the night's test hash)
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';
const PAINT = { ink: 'var(--c-mark)', ground: 'var(--c-base-bg)' };
const SCALES = Object.keys(PAGE_SEAL_SIZES) as PageSealScale[];

describe('PageSeal (rubbing × Song print, phase 3)', () => {
  it('sizes a record (block, transaction) at 150 / 96 px and an object (address, cell) at 112 / 80', () => {
    expect(PAGE_SEAL_SIZES).toEqual({
      record: { wide: 150, narrow: 96 },
      object: { wide: 112, narrow: 80 },
    });
    expect(PAGE_SEAL_TILT).toBe(-4);
  });

  it('draws one page seal of the hash at the wide size, tilted, and the narrow size below md', () => {
    for (const scale of SCALES) {
      const { container, unmount } = render(
        <SealedHead hash={HASH} scale={scale}>
          <h1>#20,914,858</h1>
        </SealedHead>
      );
      const seal = expectOnePageSeal(container, HASH, scale);
      // The svg is a block, so no descender space opens under the stamp.
      expect(seal).toHaveClass('[&>svg]:block');
      unmount();
    }
  });

  // The narrow size is a CSS height on the one drawn seal. That is the seal drawn at that size
  // only because the page talisman is drawn in its own units and the size writes nothing but the
  // svg's width and height: pinned here for every size the pages use.
  it('draws the same talisman at every size: only the svg’s width and height differ', () => {
    const sized = (px: number) =>
      compound(HASH, PAGE, px, { ...PAINT, tilt: PAGE_SEAL_TILT })
        .svg.replace(/^<svg width="\d+" height="\d+"/, '<svg')
        .trim();
    for (const { wide, narrow } of Object.values(PAGE_SEAL_SIZES)) {
      expect(sized(narrow)).toBe(sized(wide));
      expect(compound(HASH, PAGE, narrow, PAINT).svg).toMatch(
        new RegExp(`^<svg width="\\d+" height="${narrow}"`)
      );
    }
  });

  it('presses the seal first, the identity beside it in a column that may shrink', () => {
    render(
      <SealedHead hash={HASH} scale="record">
        <h1>#20,914,858</h1>
        <p>Block {HASH}</p>
      </SealedHead>
    );
    const title = screen.getByRole('heading', { level: 1 });
    const seal = screen.getByRole('img', { name: 'Hash seal 0xbbb655ae' });
    expectBefore(seal, title);
    const head = title.closest('[data-sealed-head]')!;
    expect(head).toHaveClass('flex', 'items-start', 'min-w-0');
    const column = title.parentElement!;
    expect(column).toHaveClass('min-w-0', 'flex-1');
    expect(column.parentElement).toBe(head);
    expect(column).toHaveTextContent(`Block ${HASH}`);
  });

  it('refuses a hash that is not 32 bytes, with no fallback', () => {
    expect(() => render(<PageSeal hash={HASH.slice(0, 65)} scale="object" />)).toThrow(
      'not a lowercase 32-byte hash'
    );
  });
});
