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

    const hardforksLink = screen.getByRole('link', { name: 'Hardforks' });
    expect(hardforksLink).toHaveAttribute('href', '/hardforks');
    const githubLink = screen.getByRole('link', { name: 'Github' });
    expect(githubLink).toHaveAttribute('href', 'https://github.com/janx/ckbadger');
    const shortcutHint = screen.getByText('keys');
    expect(shortcutHint).toBeInTheDocument();
    const profileLink = screen.getByRole('link', { name: '@busyforking' });
    expect(profileLink).toHaveAttribute('href', 'https://x.com/busyforking');
    expect(footer).toHaveTextContent('Designed by @busyforking');
    expect(footer).toHaveTextContent('coded by Claude and Codex');
    expect(screen.getByText('0.1.0+feature/foo@abcdef123456')).toBeInTheDocument();
  });
});
