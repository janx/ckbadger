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

  // Lane P2's third round (item 9): a list the route sends no count of folds too, asked for by
  // name ("all activity ↓"), no number where there is none; the same one button, the same way back
  it('folds an uncounted list asked for by name, "all activity ↓" / "fewer ↑", no number', () => {
    function Uncounted() {
      const [open, setOpen] = useState(false);
      return (
        <div>
          <ul id="feed">{open ? <li>the whole feed</li> : <li>the newest five</li>}</ul>
          <FoldToggle open={open} uncounted="activity" controls="feed" onToggle={setOpen} />
        </div>
      );
    }
    render(<Uncounted />);
    const toggle = screen.getByRole('button', { name: 'all activity ↓' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(toggle).toHaveAttribute('aria-controls', 'feed');
    expect(toggle.textContent).not.toMatch(/\d/);
    toggle.focus();
    fireEvent.click(toggle);
    expect(screen.getByRole('button', { name: 'fewer ↑' })).toBe(toggle);
    expect(document.activeElement).toBe(toggle);
    expect(screen.getByText('the whole feed')).toBeInTheDocument();
  });
});
