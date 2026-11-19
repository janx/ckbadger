import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { Dotted, dottedGroup } from '@/components/ui/phone-row';

/**
 * PA-25, PB-23 (UI review 2026-11-14): a "·" was left at a line's end ("Script call · .bit Time
 * Index State ·" / ".bit Time Info"; "644,078.845914 · USDC ·") or opened a line ("· 3.82%
 * occupied"; readouts whose wrapped items each start with "·"). In the one separator-joined list a
 * separator belongs to the item after it, so a line breaks before a separator and never after it,
 * and a separator is not drawn when its item starts a line. jsdom lays nothing out, so the geometry
 * is pinned by its classes:
 *
 * - the list is a wrapping flex row that clips what lies LEFT of its box only (a clip-path inset
 *   of 0 at the left and far out on the other sides): never its right, so an unbreakable unit
 *   wider than the list shows past its edge, where the page-overflow survey sees it, instead of
 *   being cut with no scroll and no error (the lead's ruling b; `overflow-hidden` cut it);
 * - every item after the first hangs its separator 2ch left of its own text (`-ml-[2ch]` with
 *   `pl-[2ch] -indent-[2ch]`): an item that starts a line has its separator in those 2ch left of
 *   the list's box, clipped; mid-line, the 3ch gap less those 2ch leaves 1ch before the dot;
 * - a wrapped item's further lines start at its padding edge, the list's left edge, unclipped.
 */
const items = (list: Element) => [...list.querySelectorAll(':scope > [data-dotted-item]')];

describe('Dotted: the one separator-joined list', () => {
  it('joins its parts as "a · b · c", each separator inside the item after it', () => {
    const { container } = render(<Dotted parts={['a', 'b', 'c']} />);
    const list = container.firstElementChild as HTMLElement;
    expect(list).toHaveAttribute('data-dotted');
    expect(list.textContent).toBe('a · b · c');
    const [first, ...rest] = items(list);
    expect(first.querySelector('[data-dotted-separator]')).toBeNull();
    for (const item of rest) {
      const separator = item.firstElementChild as HTMLElement;
      expect(separator).toHaveAttribute('data-dotted-separator');
      expect(separator.textContent).toBe(' · ');
      // the dot and the space after it never part from the item's first word
      expect(separator).toHaveClass('whitespace-nowrap', 'text-text-dim');
      expect(separator).toHaveAttribute('aria-hidden', 'true');
    }
  });

  it('is a wrapping row that clips a separator hung left of its box when its item starts a line', () => {
    const { container } = render(<Dotted parts={['a', 'b']} />);
    const list = container.firstElementChild as HTMLElement;
    expect(list).toHaveClass(
      'flex',
      'flex-wrap',
      'items-baseline',
      'gap-x-[3ch]',
      'min-w-0',
      // re-pinned (A4 review 4): the left edge 0.75ch out, so the first item's focus ring (1 px
      // at a 2 px offset: 3 px; 0.75ch is 4.95 px at 11 px, 5.85 px at 13 px) shows, while a
      // hung "· " keeps its dot hidden: the dot's cell ends 1ch left of the box
      '[clip-path:inset(-1em_-100vw_-1em_-0.75ch)]'
    );
    // re-pinned (ruling b): the list never hides text on its right
    expect(list).not.toHaveClass('overflow-hidden');
    const [first, second] = items(list);
    expect(first).not.toHaveClass('-ml-[2ch]');
    expect(second).toHaveClass('-ml-[2ch]', 'pl-[2ch]', '-indent-[2ch]', 'min-w-0');
    // what the item holds does not inherit the hanging indent
    expect(second.lastElementChild).toHaveClass('indent-0');
  });

  // The 404 seal's caption: two figures after words. Inline, the list is one box on its line, so it
  // moves to the next line whole and the words before it break before it; inside it the rule above
  // holds where a line is narrower than the list.
  it('stands inline after words as one box when asked, with the same separators and clipping', () => {
    const { container } = render(
      <p>
        words before <Dotted inline parts={['a', 'b']} />
      </p>
    );
    const list = container.querySelector('[data-dotted]') as HTMLElement;
    expect(list).toHaveClass(
      'inline-flex',
      'flex-wrap',
      'items-baseline',
      'gap-x-[3ch]',
      'min-w-0',
      '[clip-path:inset(-1em_-100vw_-1em_-0.75ch)]'
    );
    expect(list).not.toHaveClass('flex');
    expect(list.textContent).toBe('a · b');
    const [first, second] = items(list);
    expect(first.querySelector('[data-dotted-separator]')).toBeNull();
    expect(second.firstElementChild).toHaveAttribute('data-dotted-separator');
    expect(second).toHaveClass('-ml-[2ch]', 'pl-[2ch]', '-indent-[2ch]');
  });

  it('draws nothing for an empty part, so no separator is left with nothing after it', () => {
    const { container } = render(<Dotted parts={['a', null, false, undefined, '', 'b', null]} />);
    const list = container.firstElementChild as HTMLElement;
    expect(items(list)).toHaveLength(2);
    expect(list.textContent).toBe('a · b');
    expect(list.querySelectorAll('[data-dotted-separator]')).toHaveLength(1);
  });

  it('draws nothing at all for no part', () => {
    const { container } = render(<Dotted parts={[null, false]} />);
    expect(container.firstElementChild).toBeNull();
  });

  it('refuses a list inside another: its parts belong to the outer one', () => {
    expect(() => render(<Dotted parts={['a', <Dotted key="b" parts={['b', 'c']} />]} />)).toThrow(
      'a separated list inside another'
    );
  });

  // The lead's final round, items 2 and 3: a head's names fold behind one control that stays the
  // same element while the names before it grow ("+3 more locks" → "fewer"). A group's items are
  // its own (keyed by their place in the group, not in the list), and a group may be joined to the
  // item before it: no separator, one space's room (the 3ch gap less 2ch).
  it('keeps a group’s items while the groups before it grow, and joins a group to the item before', () => {
    function Names() {
      const [open, setOpen] = useState(false);
      const names = open ? ['a', 'b', 'c', 'd'] : ['a', 'b'];
      return (
        <Dotted
          parts={[
            'head',
            dottedGroup(names, { role: 'names' }),
            dottedGroup(
              [
                <button key="fold" type="button" onClick={() => setOpen(!open)}>
                  {open ? 'fewer' : '+2 more'}
                </button>,
              ],
              { role: 'fold', joined: true }
            ),
          ]}
        />
      );
    }
    const { container } = render(<Names />);
    const list = container.firstElementChild as HTMLElement;
    expect(list.textContent).toBe('head · a · b +2 more');
    const button = screen.getByRole('button');
    const joined = button.closest('[data-dotted-item]') as HTMLElement;
    expect(joined.querySelector('[data-dotted-separator]')).toBeNull();
    expect(joined).toHaveClass('-ml-[2ch]');
    expect(joined).not.toHaveClass('pl-[2ch]');
    fireEvent.click(button);
    expect(list.textContent).toBe('head · a · b · c · d fewer');
    // the same element: nothing was unmounted under the reader
    expect(screen.getByRole('button')).toBe(button);
  });
});
