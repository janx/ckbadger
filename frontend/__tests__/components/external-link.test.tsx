import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ExternalLink } from '@/components/ui/external-link';

// U-20 (UI review 2026-11-14): one form for a link that leaves the site.
describe('ExternalLink', () => {
  it('says it leaves the site: its words, then " ↗" held to the last word, a new tab, no referrer', () => {
    render(
      <ExternalLink href="https://web5.info" className="text-text">
        Web5
      </ExternalLink>
    );
    const link = screen.getByRole('link', { name: 'Web5 ↗' });
    expect(link).toHaveAttribute('href', 'https://web5.info');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noreferrer');
    expect(link).toHaveClass('text-text');
    // the mark never stands alone at the start of a line: re-pinned (lane c2-finish, rulings
    // round 2), a no-break space glues it to the last word (it was a space, held by the span's
    // nowrap alone), the span still nowrap
    const mark = link.lastElementChild!;
    expect(mark).toHaveTextContent('↗');
    expect(mark.textContent).toBe('\u00a0↗');
    expect(mark).toHaveClass('whitespace-nowrap');
  });

  it('refuses an address that does not leave the site', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<ExternalLink href="/hardforks">Hardforks</ExternalLink>)).toThrow(
      'ExternalLink: "/hardforks" is not an http(s) address'
    );
    vi.restoreAllMocks();
  });
});
