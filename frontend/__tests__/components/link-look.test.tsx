import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import Link, { LINK_LOOKS } from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { GLOBALS_CSS } from '@/__tests__/utils/css-paint';

/**
 * The looks of a link (the controls board, gate D0′, item "links"; lane c2-links): a link says its
 * job, never its classes. Three looks a page's content draws — a row's key (the bright ink), a link
 * inside a line (the line's ink), a way onward (celadon, never broken, with its arrow, which the
 * component writes) — and two that draw no link ink of their own: a block (a tile, a card, a mark)
 * and the shell's navigation. Hover and focus turn any of them celadon: one rule, globals.css.
 */
const ink = (el: Element) =>
  [...el.classList].filter(
    (c) => /^text-(?!meta|small|body|section)/.test(c) || /underline|decoration/.test(c)
  );

describe('Link: the look a caller names', () => {
  it('knows five looks, three of them a page content’s', () => {
    expect(LINK_LOOKS).toEqual(['key', 'inline', 'onward', 'block', 'nav']);
  });

  it('draws a row’s key in the bright ink, no underline, no arrow', () => {
    render(
      <Link href="/blocks/7" look="key">
        #7
      </Link>
    );
    const link = screen.getByRole('link', { name: '#7' });
    expect(ink(link)).toEqual(['text-text-bright']);
    expect(link.textContent).toBe('#7');
  });

  it('draws a link inside a line in the line’s ink: no ink of its own', () => {
    render(
      <p className="text-text">
        paid by{' '}
        <Link href="/address/ckb1" look="inline">
          ckb1…dcratvu9
        </Link>
      </p>
    );
    expect(ink(screen.getByRole('link'))).toEqual([]);
  });

  it('draws a way onward celadon and unbroken, and writes its arrow held to the last word', () => {
    render(
      <Link href="/tokens/0xab/holders" look="onward">
        all 522 holders
      </Link>
    );
    // the arrow is drawn, not read: the link's name is its words
    const link = screen.getByRole('link', { name: 'all 522 holders' });
    expect(ink(link)).toEqual(['text-jade']);
    expect(link).toHaveClass('whitespace-nowrap');
    expect(link.textContent).toBe('all 522 holders →');
    const arrow = link.lastElementChild!;
    expect(arrow.textContent).toBe(' →');
    expect(arrow).toHaveAttribute('aria-hidden', 'true');
  });

  it('points a way onward to a place further down this page downward', () => {
    render(
      <Link href="#its-cells" look="onward">
        all 1,204 cells
      </Link>
    );
    expect(screen.getByRole('link').lastElementChild!.textContent).toBe(' ↓');
  });

  it('draws no link ink for a block or the shell’s navigation, and keeps the caller’s layout', () => {
    render(
      <>
        <Link href="/blocks/7" look="block" className="flex gap-2 border p-3">
          a tile
        </Link>
        <Link href="/blocks" look="nav" className="text-text lowercase">
          blocks
        </Link>
      </>
    );
    const [block, nav] = screen.getAllByRole('link');
    expect(ink(block)).toEqual([]);
    expect(block).toHaveClass('flex', 'gap-2', 'border', 'p-3');
    // the shell's navigation writes its own design
    expect(nav).toHaveClass('text-text', 'lowercase');
  });

  it('leaves a link within this page to the browser, which goes to the place', () => {
    function Here() {
      return <output data-testid="at">{useLocation().pathname + useLocation().hash}</output>;
    }
    render(
      <MemoryRouter initialEntries={['/mainnet/scripts/x']}>
        <Here />
        <Link href="#its-cells" look="onward">
          all cells
        </Link>
      </MemoryRouter>
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', '#its-cells');
    const followed = fireEvent.click(link);
    // not prevented: the browser follows the fragment; the router is not asked
    expect(followed).toBe(true);
    expect(screen.getByTestId('at')).toHaveTextContent('/mainnet/scripts/x');
  });
});

describe('ExternalLink: the same looks, its ↗ its own', () => {
  it('draws a way to another site celadon with ↗, and no →', () => {
    render(
      <ExternalLink href="https://ckb2021.example" look="onward">
        ckb2021 overview
      </ExternalLink>
    );
    const link = screen.getByRole('link', { name: 'ckb2021 overview ↗' });
    expect(ink(link)).toEqual(['text-jade']);
    expect(link).toHaveClass('whitespace-nowrap');
    expect(link.textContent).not.toContain('→');
  });

  it('draws a key or a link inside a line as Link does', () => {
    render(
      <>
        <ExternalLink href="https://a.example" look="key">
          a
        </ExternalLink>
        <ExternalLink href="https://b.example" look="inline">
          b
        </ExternalLink>
      </>
    );
    const [key, inline] = screen.getAllByRole('link');
    expect(ink(key)).toEqual(['text-text-bright']);
    expect(ink(inline)).toEqual([]);
  });
});

describe('hover and focus: one rule for every look, by night and by day', () => {
  it('turns a link celadon under the pointer and under the keyboard’s focus', () => {
    const rule = GLOBALS_CSS.match(/\na:focus-visible,\na:hover\s*\{([^}]*)\}/);
    expect(rule, 'globals.css: one rule for a:focus-visible and a:hover').not.toBeNull();
    expect(rule![1]).toMatch(/color: var\(--c-jade\);/);
    // the glow is the theme's: none by day (--m-glow is 0)
    expect(rule![1]).toMatch(/calc\(0\.45 \* var\(--m-glow\)\)/);
  });

  it('leaves an SVG link its own hover and focus', () => {
    const rule = GLOBALS_CSS.match(/\nsvg a:focus-visible,\nsvg a:hover\s*\{([^}]*)\}/);
    expect(rule, 'globals.css: an SVG link keeps its own').not.toBeNull();
    expect(rule![1]).toMatch(/color: inherit;/);
    expect(rule![1]).toMatch(/text-shadow: none;/);
  });
});
