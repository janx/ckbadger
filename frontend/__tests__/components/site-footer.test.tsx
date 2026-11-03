import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';
import { OPEN_SHORTCUTS_EVENT } from '@/lib/search-focus';

describe('SiteFooter', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      buildVersion: '0.1.0+feature/foo@abcdef123456',
    };
  });

  // TX-04 (Main.dc.html footer): "CKBadger 0.8.2" as text (the build string in its title), then
  // Hardforks · Fiber dashboard · cknerv · Web5 · Keyboard shortcuts; no always-on pulse.
  // Review 2026-11-03, item 4 (Nav.dc.html): Network and Fiber channels join Hardforks, before the
  // external links — the chain's surroundings, which no header or footer link reached before.
  it('names the release, lists the board links and opens the keyboard shortcuts (TX-04)', () => {
    const opened = vi.fn();
    window.addEventListener(OPEN_SHORTCUTS_EVENT, opened);
    const { container } = render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    const version = within(footer).getByText('CKBadger 0.1.0');
    expect(version.tagName).not.toBe('A');
    expect(version).toHaveAttribute('title', 'CKBadger 0.1.0+feature/foo@abcdef123456');
    expect(container.querySelector('.live-dot')).toBeNull();

    const navigation = within(footer).getByRole('navigation', { name: 'Footer' });
    expect(within(navigation).getByRole('link', { name: 'Hardforks' })).toHaveAttribute(
      'href',
      '/mainnet/hardforks'
    );
    expect(within(navigation).getByRole('link', { name: 'Network' })).toHaveAttribute(
      'href',
      '/mainnet/network'
    );
    const channels = within(navigation).getByRole('link', { name: 'Fiber channels' });
    expect(channels).toHaveAttribute('href', '/mainnet/fiber/channels');
    expect(channels).not.toHaveAttribute('target');
    const fiber = within(navigation).getByRole('link', { name: 'Fiber dashboard' });
    expect(fiber).toHaveAttribute('href', 'https://dashboard.fiber.channel/');
    const cknerv = within(navigation).getByRole('link', { name: 'cknerv' });
    expect(cknerv).toHaveAttribute('href', 'https://cknerv.web5.info');
    expect(cknerv).toHaveAttribute('target', '_blank');
    expect(cknerv).toHaveAttribute('rel', 'noreferrer');
    expect(within(navigation).getByRole('link', { name: 'Web5' })).toHaveAttribute(
      'href',
      'https://web5.info'
    );
    expect(
      Array.from(navigation.querySelectorAll('a, button')).map((el) => el.textContent)
    ).toEqual([
      'Hardforks',
      'Network',
      'Fiber channels',
      'Fiber dashboard',
      'cknerv',
      'Web5',
      'Keyboard shortcuts',
    ]);
    fireEvent.click(within(navigation).getByRole('button', { name: 'Keyboard shortcuts' }));
    expect(opened).toHaveBeenCalledTimes(1);
    window.removeEventListener(OPEN_SHORTCUTS_EVENT, opened);
  });

  it('omits the design and coding attribution', () => {
    render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(footer).not.toHaveTextContent(/designed by|coded by|busyforking|Claude|Codex/i);
  });
});
