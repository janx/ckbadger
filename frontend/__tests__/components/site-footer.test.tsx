import { render, screen } from '../utils/test-utils';
import { beforeEach, describe, expect, it } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';

describe('SiteFooter', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      buildVersion: '0.1.0+feature/foo@abcdef123456',
    };
  });

  it('shows curated quick links, shortcut hint, and attribution', () => {
    render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(footer).toHaveClass('border-t');
    const footerPanel = footer.querySelector('.rounded-xl');
    expect(footerPanel).not.toBeNull();

    expect(screen.queryByText(/ckbadger explorer/i)).toBeNull();
    expect(screen.queryByText(/Local-first CKB observability and protocol context/i)).toBeNull();
    const hardforksLink = screen.getByRole('link', { name: 'Hardforks' });
    expect(hardforksLink).toHaveAttribute('href', '/hardforks');
    expect(screen.queryByRole('link', { name: 'Blocks' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Transactions' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Charts' })).toBeNull();
    const githubLink = screen.getByRole('link', { name: 'Github' });
    expect(githubLink).toHaveAttribute('href', 'https://github.com/janx/ckbadger');
    expect(githubLink).not.toHaveClass('text-emphasis');
    const shortcutHint = screen.getByText('Press ? for shortcuts');
    expect(shortcutHint).toBeInTheDocument();
    expect(hardforksLink.className).not.toEqual(shortcutHint.className);
    expect(githubLink.className).not.toEqual(shortcutHint.className);
    const profileLink = screen.getByRole('link', { name: '@busyforking' });
    expect(profileLink).toHaveAttribute('href', 'https://x.com/busyforking');
    expect(footer).toHaveTextContent('Built by @busyforking with agents Coco and Dede');
    expect(screen.getByText('0.1.0+feature/foo@abcdef123456')).toBeInTheDocument();
  });
});
