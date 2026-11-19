import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import Link, { INLINE_UNDERLINE, LINK_LOOKS } from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { GLOBALS_CSS } from '@/__tests__/utils/css-paint';
import { wrapsAt } from '@/__tests__/utils/tailwind-box';

/** The underline of a link inside a line, as its classes write it: 1 px, 3 px under, muted. */
const UNDERLINE = [
  'underline',
  'decoration-1',
  'underline-offset-[3px]',
  'decoration-[color:var(--c-ink-muted)]',
];

/**
 * The looks of a link (the controls board, gate D0′, item "links"; lane c2-links): a link says its
 * job, never its classes. Three looks a page's content draws — a row's key (the bright ink), a link
 * inside a line (the line's ink, underlined in the muted ink), a way onward (celadon, wrapping
 * between its words, with its arrow glued to its last word, which the component writes) — and two that draw no link ink of their own: a block (a tile, a card, a mark)
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

  // Re-pinned on purpose (the coordinator's ruling): it was "no ink of its own", the underline
  // waiting for its colour; it is now underlined in the muted ink, 1 px, 3 px under the text.
  it('draws a link inside a line in the line’s ink, underlined in the muted ink', () => {
    render(
      <p className="text-text">
        paid by{' '}
        <Link href="/address/ckb1" look="inline">
          ckb1…dcratvu9
        </Link>
      </p>
    );
    expect(ink(screen.getByRole('link'))).toEqual(UNDERLINE);
    expect(UNDERLINE).toContain(`decoration-[color:var(--c-${INLINE_UNDERLINE})]`);
  });

  it('underlines a link inside a line alone: no other look draws the line', () => {
    render(
      <>
        {LINK_LOOKS.filter((look) => look !== 'inline').map((look) => (
          <Link key={look} href={`/${look}`} look={look}>
            {look}
          </Link>
        ))}
      </>
    );
    for (const link of screen.getAllByRole('link'))
      expect(
        [...link.classList].filter((c) => /underline|decoration/.test(c)),
        link.textContent!
      ).toEqual([]);
  });

  // Re-pinned (lane c2-finish, rulings round 2): a way onward wraps between its words (it was
  // never broken, whitespace-nowrap, and a long one ran out of the page: the chart pages' hardfork
  // note, 413 px on one line, scrolled them sideways 77 px at 360); its arrow stays held to its
  // last word: a no-break space and the arrow (it was a space and the arrow), in a span that does
  // not wrap, glued to the word before it.
  it('draws a way onward celadon, wrapping between its words, its arrow held to the last word', () => {
    render(
      <Link href="/tokens/0xab/holders" look="onward">
        all 522 holders
      </Link>
    );
    // the arrow is drawn, not read: the link's name is its words
    const link = screen.getByRole('link', { name: 'all 522 holders' });
    expect(ink(link)).toEqual(['text-jade']);
    expect(link).not.toHaveClass('whitespace-nowrap');
    expect(link.textContent).toBe('all 522 holders\u00a0→');
    const arrow = link.lastElementChild!;
    expect(arrow.textContent).toBe('\u00a0→');
    expect(arrow).toHaveClass('whitespace-nowrap');
    expect(arrow).toHaveAttribute('aria-hidden', 'true');
  });

  // The hold, exactly: a line may break at a space (after it); the arrow's span begins with a
  // no-break space, which offers no break before or after it (Unicode line breaking, LB12 and
  // LB12a: glue), and the word before it ends in a letter or a digit, no space and no hyphen, so
  // there is no break between the last word and the arrow; inside the span, white-space: nowrap
  // keeps the two characters together however the line's own white-space reads. So where the words
  // wrap, the arrow ends the line its last word ends, never a line of its own.
  it('lets a long way onward wrap at its spaces while its arrow is glued to its last word', () => {
    const words = 'Hardforks: Mirana, epoch 5,414 · Meepo, epoch 12,293';
    render(
      <p>
        <Link href="/hardforks" look="onward">
          {words}
        </Link>
      </p>
    );
    const link = screen.getByRole('link', { name: words });
    for (const w of [1280, 390, 360]) expect(wrapsAt(link, w), `at ${w}`).toBe(true);
    const arrow = link.lastElementChild!;
    for (const w of [1280, 360]) expect(wrapsAt(arrow, w)).toBe(false);
    expect(arrow.textContent).toMatch(/^\u00a0→$/);
    const before = arrow.previousSibling!;
    expect(before.nodeType).toBe(Node.TEXT_NODE);
    expect(before.textContent).toBe(words);
    expect(before.textContent).toMatch(/[^\s-]$/);
  });

  it('points a way onward to a place further down this page downward', () => {
    render(
      <Link href="#its-cells" look="onward">
        all 1,204 cells
      </Link>
    );
    // re-pinned (lane c2-finish, rulings round 2): glued by a no-break space (was a space)
    expect(screen.getByRole('link').lastElementChild!.textContent).toBe(' ↓');
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
    // re-pinned (lane c2-finish, rulings round 2): it wraps between its words, its ↗ glued to the
    // last word by a no-break space (it was nowrap)
    expect(link).not.toHaveClass('whitespace-nowrap');
    expect(link.lastElementChild!.textContent).toBe('\u00a0↗');
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
    // re-pinned with the ruling: the muted underline
    expect(ink(inline)).toEqual(UNDERLINE);
  });
});

describe('hover and focus: one rule for every look, by night and by day', () => {
  it('turns a link celadon under the pointer and under the keyboard’s focus', () => {
    const rule = GLOBALS_CSS.match(/\na:focus-visible,\na:hover\s*\{([^}]*)\}/);
    expect(rule, 'globals.css: one rule for a:focus-visible and a:hover').not.toBeNull();
    expect(rule![1]).toMatch(/(^|\s)color: var\(--c-jade\);/);
    // the glow is the theme's: none by day (--m-glow is 0)
    expect(rule![1]).toMatch(/calc\(0\.45 \* var\(--m-glow\)\)/);
    // an underline follows its words into the celadon; it does not stay muted (the rule is
    // `a` and a state, over the one class that colours the line)
    expect(rule![1]).toMatch(/text-decoration-color: currentColor;/);
  });

  it('leaves an SVG link its own hover and focus', () => {
    const rule = GLOBALS_CSS.match(/\nsvg a:focus-visible,\nsvg a:hover\s*\{([^}]*)\}/);
    expect(rule, 'globals.css: an SVG link keeps its own').not.toBeNull();
    expect(rule![1]).toMatch(/color: inherit;/);
    expect(rule![1]).toMatch(/text-shadow: none;/);
  });
});
