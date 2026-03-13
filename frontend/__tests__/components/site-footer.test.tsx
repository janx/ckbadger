import { render, screen, within } from '../utils/test-utils';
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
    const hardforksLink = within(footer).getByRole('link', { name: 'Hardforks' });
    expect(hardforksLink).toHaveAttribute('href', '/hardforks');
    const githubLink = within(footer).getByRole('link', { name: 'Github' });
    expect(githubLink).toHaveAttribute('href', 'https://github.com/janx/ckbadger');
    const shortcutHint = within(footer).getByText('keys');
    expect(shortcutHint).toBeInTheDocument();
    const profileLink = within(footer).getByRole('link', { name: '@busyforking' });
    expect(profileLink).toHaveAttribute('href', 'https://x.com/busyforking');
    expect(footer).toHaveTextContent('Designed by @busyforking');
    expect(footer).toHaveTextContent('coded by Claude and Codex');
    expect(within(footer).getByText('0.1.0+feature/foo@abcdef123456')).toBeInTheDocument();
  });
});
