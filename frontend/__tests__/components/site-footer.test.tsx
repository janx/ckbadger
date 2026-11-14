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

  // TX-04 (Main.dc.html footer): Hardforks · Fiber dashboard · cknerv · Web5 · Keyboard
  // shortcuts; no always-on pulse. Review 2026-11-03, item 4 (Nav.dc.html): Network and Fiber
  // channels join Hardforks, before the external links — the chain's surroundings, which no header
  // or footer link reached before. Rubbing × Song print, phase 2: the release ("CKBadger 0.8.2")
  // moved to the status line at the foot of the window; the footer is its links alone.
  it('lists the board links, opens the keyboard shortcuts, and names no release (TX-04)', () => {
    const opened = vi.fn();
    window.addEventListener(OPEN_SHORTCUTS_EVENT, opened);
    const { container } = render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(footer).not.toHaveTextContent(/CKBadger|0\.1\.0/);
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
    // U-20 (UI review 2026-11-14): a link that leaves the site says so, ' ↗' after its words, and
    // opens in a new tab without a referrer
    const fiber = within(navigation).getByRole('link', { name: 'Fiber dashboard ↗' });
    expect(fiber).toHaveAttribute('href', 'https://dashboard.fiber.channel/');
    const cknerv = within(navigation).getByRole('link', { name: 'cknerv ↗' });
    expect(cknerv).toHaveAttribute('href', 'https://cknerv.web5.info');
    const web5 = within(navigation).getByRole('link', { name: 'Web5 ↗' });
    expect(web5).toHaveAttribute('href', 'https://web5.info');
    for (const external of [fiber, cknerv, web5]) {
      expect(external).toHaveAttribute('target', '_blank');
      expect(external).toHaveAttribute('rel', 'noreferrer');
    }
    expect(
      Array.from(navigation.querySelectorAll('a, button')).map((el) => el.textContent)
    ).toEqual([
      'Hardforks',
      'Network',
      'Fiber channels',
      'Fiber dashboard ↗',
      'cknerv ↗',
      'Web5 ↗',
      'Keyboard shortcuts',
    ]);
    // the links within the site carry no mark
    for (const name of ['Hardforks', 'Network', 'Fiber channels'])
      expect(within(navigation).getByRole('link', { name })).not.toHaveTextContent('↗');
    fireEvent.click(within(navigation).getByRole('button', { name: 'Keyboard shortcuts' }));
    expect(opened).toHaveBeenCalledTimes(1);
    window.removeEventListener(OPEN_SHORTCUTS_EVENT, opened);
  });

  // U-20 (UI review 2026-11-14): the footer reads in the nav's case (U-01: lowercase by CSS, the
  // names as written), and "Keyboard shortcuts" is drawn from md only, where the site has keys to
  // hint at (PA-29: a touch screen has none; the status line's key hints start at md too).
  it('sets its links in lowercase and draws "Keyboard shortcuts" from md only (U-20, PA-29)', () => {
    render(<SiteFooter />);
    const navigation = within(screen.getByRole('contentinfo')).getByRole('navigation', {
      name: 'Footer',
    });
    const entries = Array.from(navigation.querySelectorAll('a, button'));
    expect(entries).toHaveLength(7);
    for (const entry of entries) expect(entry, entry.textContent!).toHaveClass('lowercase');
    const keys = within(navigation).getByRole('button', { name: 'Keyboard shortcuts' });
    expect(keys).toHaveClass('hidden', 'md:inline');
    expect(keys).not.toHaveClass('inline');
  });

  it('omits the design and coding attribution', () => {
    render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(footer).not.toHaveTextContent(/designed by|coded by|busyforking|Claude|Codex/i);
  });
});
