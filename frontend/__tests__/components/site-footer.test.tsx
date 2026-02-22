import { render, screen } from '../utils/test-utils';
import { describe, expect, it } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';

describe('SiteFooter', () => {
  it('shows curated quick links, shortcut hint, and attribution', () => {
    render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(footer).not.toHaveClass('border-t');

    expect(screen.queryByText(/ckbadger explorer/i)).toBeNull();
    expect(screen.queryByText(/Local-first CKB observability and protocol context/i)).toBeNull();
    const hardforksLink = screen.getByRole('link', { name: 'Hardforks' });
    expect(hardforksLink).toHaveAttribute('href', '/hardforks');
    expect(screen.queryByRole('link', { name: 'Blocks' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Transactions' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Charts' })).toBeNull();
    const githubLink = screen.getByRole('link', { name: 'Github' });
    expect(githubLink).toHaveAttribute('href', 'https://github.com/janx/ckbadger');
    const footerLinks = Array.from(githubLink.parentElement?.querySelectorAll('a') ?? []).map(
      (link) => link.textContent?.trim()
    );
    expect(footerLinks).toEqual(['Hardforks', 'Github']);
    expect(screen.getByText('Press ? for shortcuts')).toBeInTheDocument();
    expect(screen.getByText(/Built by/i)).toBeInTheDocument();
    const profileLink = screen.getByRole('link', { name: '@busyforking' });
    expect(profileLink).toHaveAttribute('href', 'https://x.com/busyforking');
    expect(profileLink.parentElement).toHaveTextContent('with agents Coco and Dede');
  });
});
