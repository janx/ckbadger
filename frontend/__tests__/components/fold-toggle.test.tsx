/**
 * The fold's one control (review of phases C and D, SHOULD 5; lane P1): one button that stays
 * mounted, says which way it goes in both states and keeps the keyboard's focus while its list
 * opens and folds.
 */
import { useState } from 'react';
import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { FoldToggle } from '@/components/ui/fold-toggle';

function Fold() {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <ul id="the-list">
        {Array.from({ length: open ? 158 : 6 }, (_, i) => (
          <li key={i}>cell {i}</li>
        ))}
      </ul>
      <FoldToggle open={open} count={158} singular="cell" controls="the-list" onToggle={setOpen} />
    </div>
  );
}

describe('FoldToggle', () => {
  it('says the whole list folded and the way back opened, aria-expanded and aria-controls in both', () => {
    render(<Fold />);
    const toggle = screen.getByRole('button', { name: /^all 158\scells ↓$/ });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(toggle).toHaveAttribute('aria-controls', 'the-list');
    fireEvent.click(toggle);
    expect(toggle).toHaveTextContent(/^fewer ↑$/);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(toggle).toHaveAttribute('aria-controls', 'the-list');
    expect(screen.getAllByRole('listitem')).toHaveLength(158);
  });

  it('keeps the keyboard’s focus on the one button while the list opens and folds', () => {
    render(<Fold />);
    const toggle = screen.getByRole('button', { name: /^all 158\scells ↓$/ });
    toggle.focus();
    expect(document.activeElement).toBe(toggle);
    fireEvent.click(toggle);
    // the same element, still focused: nothing was unmounted under the reader
    expect(screen.getByRole('button', { name: 'fewer ↑' })).toBe(toggle);
    expect(document.activeElement).toBe(toggle);
    fireEvent.click(toggle);
    expect(screen.getByRole('button', { name: /^all 158\scells ↓$/ })).toBe(toggle);
    expect(document.activeElement).toBe(toggle);
  });

  it('writes a noun’s own plural, and draws the Pager’s way to go', () => {
    render(
      <FoldToggle
        open={false}
        count={158}
        singular="witness"
        plural="witnesses"
        controls="w"
        onToggle={() => {}}
      />
    );
    const toggle = screen.getByRole('button', { name: /^all 158\switnesses ↓$/ });
    expect(toggle).toHaveClass('text-jade', 'text-small', 'h-8', 'sm:h-7');
  });

  // The lead's final round, item 3: the rest of a list inside a line of names is one control too,
  // its inline form: a word of its line, how many it holds, the way back, and the same button.
  describe('its inline form, inside a line of names', () => {
    function Line({ words }: { words?: string }) {
      const [open, setOpen] = useState(false);
      const names = ['JoyID lock', 'Omnilock', 'lock 0x9b9b…9b9b', 'PW lock', 'Sale lock', 'RGB++'];
      return (
        <p className="text-small leading-row">
          <span id="the-names">{(open ? names : names.slice(0, 2)).join(' · ')}</span>{' '}
          <FoldToggle
            form="inline"
            open={open}
            count={4}
            singular="lock"
            controls="the-names"
            onToggle={setOpen}
            words={words}
          />
        </p>
      );
    }

    it('says how many it holds folded and the way back opened, aria-expanded and aria-controls in both, on one button that keeps its focus', () => {
      render(<Line />);
      const toggle = screen.getByRole('button', { name: /^\+4\smore\slocks$/ });
      expect(toggle).toHaveAttribute('aria-expanded', 'false');
      expect(toggle).toHaveAttribute('aria-controls', 'the-names');
      toggle.focus();
      fireEvent.click(toggle);
      expect(screen.getByRole('button', { name: 'fewer' })).toBe(toggle);
      expect(toggle).toHaveAttribute('aria-expanded', 'true');
      expect(toggle).toHaveAttribute('aria-controls', 'the-names');
      expect(document.activeElement).toBe(toggle);
      expect(document.getElementById('the-names')).toHaveTextContent(/Sale lock · RGB\+\+$/);
      fireEvent.click(toggle);
      expect(screen.getByRole('button', { name: /^\+4\smore\slocks$/ })).toBe(toggle);
      expect(document.activeElement).toBe(toggle);
    });

    it('is a word of its line: its size, its line height, no box; its hit area its own, 32 px below 640 and 24 from it', () => {
      render(<Line />);
      const toggle = screen.getByRole('button', { name: /more\slocks$/ });
      expect(toggle).toHaveAttribute('data-fold-toggle', 'inline');
      // the fold's ink; no size, line height, height, padding or border of its own: the line's
      expect(toggle).toHaveClass('text-jade', 'p-0', 'bg-transparent');
      for (const c of toggle.className.split(/\s+/))
        expect(c).not.toMatch(/^(sm:)?(text-(meta|small|body)|leading-|h-|py?-[1-9]|border)/);
      // the hit area: a box centred on the words, laid over the line (absolute), not the line's
      expect(toggle).toHaveClass(
        'relative',
        'before:absolute',
        'before:top-1/2',
        'before:left-1/2',
        'before:h-8',
        'before:min-w-8',
        'sm:before:h-6',
        'sm:before:min-w-6',
        'before:w-full'
      );
    });

    it('writes the words of a figure that stands for the folded list, and refuses them on a box', () => {
      render(<Line words="+1 −1 objects" />);
      const toggle = screen.getByRole('button', { name: '+1 −1 objects' });
      fireEvent.click(toggle);
      expect(toggle).toHaveTextContent(/^fewer$/);
      expect(() =>
        render(
          <FoldToggle
            open={false}
            count={2}
            singular="object"
            controls="x"
            onToggle={() => {}}
            words="+2"
          />
        )
      ).toThrow(/a box fold writes its own words/);
    });
  });
});
