import { render, screen, within } from '@/__tests__/utils/test-utils';
import { beforeEach, describe, expect, it } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';

describe('SiteFooter', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      buildVersion: '0.1.0+feature/foo@abcdef123456',
    };
  });

  it('groups the quick links in footer navigation and keeps build and shortcut information', () => {
    render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    const navigation = within(footer).getByRole('navigation', { name: 'Footer' });
    const hardforksLink = within(navigation).getByRole('link', { name: 'Hardforks' });
    expect(hardforksLink).toHaveAttribute('href', '/mainnet/hardforks');
    const versionLink = within(footer).getByRole('link', {
      name: 'CKBadger 0.1.0+feature/foo@abcdef123456',
    });
    expect(versionLink).toHaveAttribute('href', 'https://github.com/janx/ckbadger');
    expect(versionLink).toHaveAttribute('title', 'CKBadger 0.1.0+feature/foo@abcdef123456');
    const fiberLink = within(navigation).getByRole('link', { name: 'Fiber Dashboard' });
    expect(fiberLink).toHaveAttribute('href', 'https://dashboard.fiber.channel/');
    const cknervLink = within(navigation).getByRole('link', { name: 'cknerv' });
    expect(cknervLink).toHaveAttribute('href', 'https://cknerv.web5.info');
    expect(cknervLink).toHaveAttribute('target', '_blank');
    expect(cknervLink).toHaveAttribute('rel', 'noreferrer');
    const web5Link = within(navigation).getByRole('link', { name: 'Web5' });
    expect(web5Link).toHaveAttribute('href', 'https://web5.info');
    expect(within(navigation).getAllByRole('link')).toEqual([
      hardforksLink,
      fiberLink,
      cknervLink,
      web5Link,
    ]);
    expect(navigation).not.toContainElement(versionLink);
    const shortcutHint = within(footer).getByText('keys');
    expect(shortcutHint).toBeInTheDocument();
    expect(navigation).not.toContainElement(shortcutHint);
    expect(within(footer).getByText('?').tagName).toBe('KBD');
  });

  it('omits the design and coding attribution', () => {
    render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(footer).not.toHaveTextContent(/designed by|coded by|busyforking|Claude|Codex/i);
  });
});
