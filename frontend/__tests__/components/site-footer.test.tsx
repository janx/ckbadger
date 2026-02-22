import { render, screen } from '../utils/test-utils';
import { describe, expect, it } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';

describe('SiteFooter', () => {
  it('shows curated quick links, shortcut hint, and attribution', () => {
    render(<SiteFooter />);

    expect(screen.queryByText(/ckbadger explorer/i)).toBeNull();
    expect(screen.queryByText(/Local-first CKB observability and protocol context/i)).toBeNull();
    expect(screen.getByRole('link', { name: 'Hardforks' })).toHaveAttribute('href', '/hardforks');
    expect(screen.queryByRole('link', { name: 'Blocks' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Transactions' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Charts' })).toBeNull();
    expect(screen.getByRole('link', { name: 'Github' })).toHaveAttribute(
      'href',
      'https://github.com/janx/ckbadger'
    );
    expect(screen.getByText('Press ? for shortcuts')).toBeInTheDocument();
    expect(screen.getByText(/Built by/i)).toBeInTheDocument();
    const profileLink = screen.getByRole('link', { name: '@busyforking' });
    expect(profileLink).toHaveAttribute('href', 'https://x.com/busyforking');
    expect(profileLink.parentElement).toHaveTextContent('with agents Coco and Dede');
  });
});
