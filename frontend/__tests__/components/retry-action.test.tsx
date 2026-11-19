import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { RetryAction } from '@/components/ui/retry-action';
import { targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The one form of an action that follows a failed read (lane cd-fixes-controls, the chart
 * reviewer's N11 handed to the library): "Retry" was written by hand in four places, a raw button
 * with its own ink and underline (`text-jade ml-2 underline`). The library's action: a real
 * button, its words the caller's, its ink the link rule's way onward (celadon, the look's class,
 * none written here), no underline, 28 px high from 640 and 32 below, like the other parts.
 */
describe('RetryAction', () => {
  it('is a real button with the caller’s words, and acts on a click', () => {
    const onRetry = vi.fn();
    render(<RetryAction words="Retry" onRetry={onRetry} />);
    const button = screen.getByRole('button', { name: 'Retry' });
    expect(button.tagName).toBe('BUTTON');
    expect(button).toHaveAttribute('type', 'button');
    fireEvent.click(button);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('takes the way onward’s ink from the link rule, and writes no underline', () => {
    render(<RetryAction words="Retry" onRetry={() => {}} />);
    const classes = screen.getByRole('button').className.split(/\s+/);
    expect(classes).toContain('text-jade');
    expect(classes).toContain('look-action');
    expect(classes.filter((c) => /underline|decoration/.test(c))).toEqual([]);
  });

  it('follows the link rule under the pointer and the keyboard’s focus (app/globals.css)', () => {
    const css = readFileSync(join(__dirname, '..', '..', 'app', 'globals.css'), 'utf8');
    expect(css).toMatch(
      /a:focus-visible,\s*a:hover,\s*\.look-action:focus-visible,\s*\.look-action:hover\s*\{\s*color: var\(--c-jade\);/
    );
  });

  it('stands 28 px high from 640 and 32 below it', () => {
    render(<RetryAction words="Retry" onRetry={() => {}} />);
    const button = screen.getByRole('button');
    expect([1280, 640, 639, 390, 360].map((w) => targetHeightPx(button, w))).toEqual([
      28, 28, 32, 32, 32,
    ]);
  });

  it('refuses an action with no words', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<RetryAction words=" " onRetry={() => {}} />)).toThrow(
      'RetryAction: an action has its words'
    );
  });
});
