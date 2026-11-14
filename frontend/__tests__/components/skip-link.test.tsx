import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { PAGE_CONTENT_ID, SkipLink } from '@/components/layout/skip-link';

// U-20 (UI review 2026-11-14): the skip link, in the shell's voice, drawn only on focus.
describe('SkipLink', () => {
  it('is a lowercase command line, hidden until it has the focus', () => {
    render(<SkipLink />);
    const link = screen.getByRole('link', { name: 'skip to content' });
    expect(link).toHaveAttribute('href', `#${PAGE_CONTENT_ID}`);
    expect(link).toHaveClass('sr-only', 'focus:not-sr-only', 'focus:fixed', 'font-mono');
    const square = link.firstElementChild!;
    expect(square).toHaveClass('prompt-square');
    expect(square).toHaveAttribute('aria-hidden', 'true');
  });

  it('moves the focus to the page and leaves the URL as it was', () => {
    const before = window.location.href;
    render(
      <>
        <SkipLink />
        <div id={PAGE_CONTENT_ID} tabIndex={-1}>
          the page
        </div>
      </>
    );
    fireEvent.click(screen.getByRole('link', { name: 'skip to content' }));
    expect(screen.getByText('the page')).toHaveFocus();
    expect(window.location.href).toBe(before);
  });
});
