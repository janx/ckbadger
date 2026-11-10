import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { HashSeal, ROW_ONLY_BELOW_PX, type HashSealVariant } from '@/components/ui/hash-seal';
import { compound } from '@/lib/talisman/compound';
import { RECIPES } from '@/lib/talisman/recipes';

// count the generator's calls without changing what it draws
vi.mock('@/lib/talisman/compound', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/talisman/compound')>();
  return { ...actual, compound: vi.fn(actual.compound) };
});

// block #20,914,858's hash (the night's test hash)
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';

const VARIANTS: [HashSealVariant, number][] = [
  ['page', 150],
  ['row', 28],
  ['brush', 150],
];

/** The markup as the DOM serialises it once parsed. */
function parsed(markup: string): string {
  const holder = document.createElement('span');
  holder.innerHTML = markup;
  return holder.innerHTML;
}

describe('HashSeal', () => {
  it('renders one svg image per variant, named by the hash', () => {
    for (const [variant, size] of VARIANTS) {
      const { container, unmount } = render(<HashSeal hash={HASH} variant={variant} size={size} />);
      const svgs = container.querySelectorAll('svg');
      expect(svgs).toHaveLength(1);
      expect(svgs[0]).toHaveAttribute('role', 'img');
      expect(svgs[0]).toHaveAttribute('aria-label', 'Hash seal 0xbbb655ae');
      expect(container.querySelector(`[data-hash-seal="${variant}"]`)).toContainElement(svgs[0]);
      unmount();
    }
  });

  it('draws at the size it is asked, on the longer side', () => {
    for (const [variant, size] of [
      ...VARIANTS,
      ['row', 16],
      ['page', 112],
      ['brush', 320],
    ] as const) {
      const { container, unmount } = render(<HashSeal hash={HASH} variant={variant} size={size} />);
      const svg = container.querySelector('svg');
      expect(svg).toHaveAttribute('height', String(size));
      expect(Number(svg?.getAttribute('width'))).toBeLessThan(size);
      unmount();
    }
  });

  it('paints with the palette: the seal red, the ground', () => {
    const { container } = render(<HashSeal hash={HASH} variant="row" size={28} />);
    expect(container.querySelector('svg > rect')).toHaveAttribute('fill', 'var(--c-rouge)');
    expect(container.querySelector('svg > g')).toHaveAttribute('fill', 'var(--c-base-bg)');
    const page = render(<HashSeal hash={HASH} variant="page" size={150} ink="var(--c-gold)" />);
    expect(page.container.querySelector('svg > rect')).toHaveAttribute('stroke', 'var(--c-gold)');
  });

  it('tilts when asked and carries the caller’s classes', () => {
    const { container } = render(
      <HashSeal hash={HASH} variant="page" size={150} tilt={-4} className="mt-2" />
    );
    expect(container.querySelector('svg')?.getAttribute('style')).toBe('transform: rotate(-4deg)');
    expect(container.firstElementChild).toHaveClass('mt-2', 'inline-block');
  });

  it('draws once per hash, variant and size: a re-render keeps the same element', () => {
    const draw = vi.mocked(compound);
    draw.mockClear();
    const { container, rerender } = render(<HashSeal hash={HASH} variant="row" size={28} />);
    const first = container.querySelector('svg');
    expect(draw).toHaveBeenCalledTimes(1);
    rerender(<HashSeal hash={HASH} variant="row" size={28} />);
    expect(container.querySelector('svg')).toBe(first);
    expect(draw).toHaveBeenCalledTimes(1);
    rerender(<HashSeal hash={HASH} variant="row" size={32} />);
    expect(draw).toHaveBeenCalledTimes(2);
    expect(container.querySelector('svg')).toHaveAttribute('height', '32');
  });

  it('refuses anything but a lowercase 32-byte hash, with no fallback', () => {
    for (const bad of ['0x12', HASH.toUpperCase().replace('0X', '0x'), HASH.slice(2), `${HASH}<b>`])
      expect(() => render(<HashSeal hash={bad} variant="row" size={28} />)).toThrow(
        'not a lowercase 32-byte hash'
      );
  });

  it('draws only the row seal below 40 px', () => {
    expect(ROW_ONLY_BELOW_PX).toBe(40);
    expect(() => render(<HashSeal hash={HASH} variant="page" size={28} />)).toThrow(
      'use the row seal'
    );
    expect(() => render(<HashSeal hash={HASH} variant="brush" size={39} />)).toThrow(
      'use the row seal'
    );
    expect(() => render(<HashSeal hash={HASH} variant="row" size={16} />)).not.toThrow();
  });

  it('refuses a paint that is not a palette reference or a hex colour', () => {
    expect(() =>
      render(<HashSeal hash={HASH} variant="row" size={28} ink={'red"/><script>'} />)
    ).toThrow('ink must be');
  });

  // The pins are the design reference: one fixed hash per variant, the generator's markup as the
  // talisman was approved (rubbing × Song, design 3 / 8 folded / 6). A change to them is a change
  // to the design and is reviewed as one.
  for (const [variant, size] of VARIANTS)
    it(`pins the ${variant} seal of one hash to the design reference`, async () => {
      const markup = compound(HASH, RECIPES[variant], size, {
        ink: 'var(--c-rouge)',
        ground: 'var(--c-base-bg)',
      }).svg;
      await expect(markup).toMatchFileSnapshot(`../fixtures/hash-seal/${variant}.svg`);
      const { container } = render(<HashSeal hash={HASH} variant={variant} size={size} />);
      expect(container.firstElementChild?.innerHTML).toBe(parsed(markup));
    });
});
