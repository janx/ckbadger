import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { PromptHeading } from '@/components/ui/prompt-heading';

// Rubbing × Song print, phase 2 (boards home.js, tx.js): every section is a command line — the
// 8 px cinnabar prompt square, the command in lowercase moon, dim args, a right-prompt note.

const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');

describe('PromptHeading', () => {
  it('is a tier-1 h2 whose square is its own ::before: command lowercase, args dim and regular', () => {
    render(<PromptHeading id="pipeline-heading" command="Pipeline" args="--tip 20,969,560" />);
    const h2 = screen.getByRole('heading', { level: 2, name: 'Pipeline --tip 20,969,560' });
    expect(h2).toHaveAttribute('id', 'pipeline-heading');
    expect(h2).toHaveClass('prompt', 'text-section', 'text-text-bright', 'flex', 'flex-wrap');
    // the h2's square is CSS (h2.text-section::before), so no element draws a second one
    expect(h2.querySelector('.section-mark, .prompt-square')).toBeNull();
    const [command, args] = Array.from(h2.children) as HTMLElement[];
    expect(command).toHaveTextContent(/^Pipeline$/);
    expect(command).toHaveClass('lowercase');
    expect(args).toHaveTextContent(/^--tip 20,969,560$/);
    expect(args).toHaveClass('text-text-dim', 'font-normal', 'normal-case');
  });

  it('puts the right-prompt note beside the heading, quiet, outside its name', () => {
    render(<PromptHeading command="network" args="--days 14" note="hover a point for the day" />);
    const h2 = screen.getByRole('heading', { name: 'network --days 14' });
    const row = h2.parentElement!;
    expect(row).toHaveClass('flex', 'flex-wrap', 'items-baseline');
    const note = row.lastElementChild as HTMLElement;
    expect(note).not.toBe(h2);
    expect(note).toHaveTextContent('hover a point for the day');
    // R4 (re-pinned on purpose): the note is information, in the dim ink
    expect(note).toHaveClass('text-text-dim', 'text-meta', 'ml-auto');
    expect(h2).not.toContainElement(note);
  });

  // UI review 2026-11-14 (U-04, U-08, ruling R4): the note carries information ("at #21,009,130",
  // "newest first", "Open the collection →"), so it is the dim ink, not the ghost (1.9:1 by night);
  // a link in it keeps the note's ink and takes the page's link hover (`a:hover`, globals.css).
  it('writes the note in the dim ink; a link in it keeps that ink and the page’s link hover', () => {
    render(
      <PromptHeading command="cluster" note={<a href="/clusters/0xab">Open the collection →</a>} />
    );
    const link = screen.getByRole('link', { name: 'Open the collection →' });
    const note = link.closest('[data-prompt-note]') as HTMLElement;
    expect(note).toHaveClass('text-text-dim', 'text-meta', 'ml-auto');
    expect(note).not.toHaveClass('text-text-ghost');
    // the link names no ink of its own: it is the note's dim, and jade under the pointer
    expect([...link.classList].filter((c) => /^text-/.test(c))).toEqual([]);
    // re-pinned on purpose (lane cd-fixes-controls): the rule's selector ends with the library's
    // action after a failed read (`.look-action`), after `a:hover`
    expect(css).toMatch(
      /\ba:hover,\n\.look-action:focus-visible,\n\.look-action:hover\s*\{\s*color: var\(--c-jade\);/
    );
  });

  it('renders the bare heading when there is no args or note', () => {
    const { container } = render(<PromptHeading command="supply" />);
    const h2 = screen.getByRole('heading', { name: 'supply' });
    expect(container.firstElementChild).toBe(h2);
    expect(h2.children).toHaveLength(1);
  });

  it('draws the square as an element on any other tag, or the mark it is given', () => {
    const { rerender } = render(<PromptHeading as="div" command="Cells" />);
    const head = screen.getByText('Cells').parentElement!;
    expect(head.tagName).toBe('DIV');
    expect(head).not.toHaveClass('text-section');
    const square = head.firstElementChild as HTMLElement;
    expect(square).toHaveClass('section-mark');
    expect(square).toHaveAttribute('aria-hidden', 'true');
    rerender(
      <PromptHeading
        as="div"
        command="Cells"
        mark={<span data-testid="own-mark" className="section-mark section-mark-live" />}
      />
    );
    expect(screen.getByText('Cells').parentElement!.firstElementChild).toBe(
      screen.getByTestId('own-mark')
    );
  });

  it('refuses a mark on an h2, whose square is CSS', () => {
    expect(() => render(<PromptHeading command="cells" mark={<span />} />)).toThrow(
      'PromptHeading: an h2 draws its square in CSS (h2.text-section::before)'
    );
  });

  it('paints the square: 8 px of the cinnabar mark, square-cornered, unlit, before every tier-1 h2', () => {
    const rule = css.match(
      /\.prompt-square,\s*\.section-mark,\s*h2\.text-section::before\s*\{([^}]*)\}/
    );
    expect(rule).not.toBeNull();
    expect(rule![1]).toContain('width: 8px;');
    expect(rule![1]).toContain('height: 8px;');
    expect(rule![1]).toContain('background: var(--c-mark);');
    expect(rule![1]).not.toMatch(/border-radius|box-shadow|--c-jade/);
    // a command is typed lowercase: every tier-1 h2, hand-written ones included
    expect(css).toMatch(/h2\.text-section\s*\{\s*text-transform: lowercase;\s*\}/);
    // the warning mark is gold, still unlit
    expect(css).toMatch(/\.section-mark-gold\s*\{\s*background: var\(--c-gold\);\s*\}/);
  });
});
