import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { HelpMark } from '@/components/ui/help-mark';
import { HelpPopover } from '@/components/ui/help-popover';
import { inkOf, QUIET } from '@/__tests__/utils/ink-role';
import { boxPx, targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The help mark of the controls board (gate D0′): one mark, an 18 px box on the quieter frame line
 * with a 12 px `?` in the quiet ink, square, its target 32 px at every width. It is the trigger of
 * the one help popover (components/ui/help-popover.tsx), which keeps what it opens.
 */
describe('HelpMark', () => {
  it('is an 18 px square box with a 12 px “?” in the quiet ink', () => {
    render(<HelpMark label="Explain HMul" expanded={false} controls="panel" onClick={() => {}} />);
    const mark = screen.getByRole('button', { name: 'Explain HMul' });
    expect(mark).toHaveTextContent('?');
    expect(mark).toHaveClass('text-meta', 'border');
    expect(inkOf(mark)).toBe(QUIET);
    expect(mark.style.borderColor).toBe('var(--c-slate-other)');
    expect(mark.className).not.toMatch(/rounded|\bbg-/);
    expect(boxPx(mark, 1280)).toBe(18);
  });

  it('takes a 32 px target at every width, its box staying 18 px', () => {
    render(<HelpMark label="Explain HMul" expanded={false} controls="panel" onClick={() => {}} />);
    const mark = screen.getByRole('button');
    expect([1280, 640, 390, 360].map((w) => targetHeightPx(mark, w))).toEqual([32, 32, 32, 32]);
    expect(boxPx(mark, 390)).toBe(18);
  });

  it('says whether what it opens is open, and which it is, and answers a click', () => {
    const onClick = vi.fn();
    render(<HelpMark label="Explain HMul" expanded controls="panel" onClick={onClick} />);
    const mark = screen.getByRole('button');
    expect(mark).toHaveAttribute('aria-expanded', 'true');
    expect(mark).toHaveAttribute('aria-controls', 'panel');
    fireEvent.click(mark);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('is the help popover’s one trigger: the popover draws the mark and opens its own panel', () => {
    render(
      <HelpPopover label="Explain HMul" title="HMul">
        <p>CKB held ÷ occupied</p>
      </HelpPopover>
    );
    const trigger = screen.getByRole('button', { name: 'Explain HMul' });
    expect(boxPx(trigger, 1280)).toBe(18);
    expect(trigger.style.borderColor).toBe('var(--c-slate-other)');
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('dialog', { name: 'HMul' })).toHaveTextContent('CKB held ÷ occupied');
    expect(trigger.getAttribute('aria-controls')).toBe(screen.getByRole('dialog').id);
  });
});
