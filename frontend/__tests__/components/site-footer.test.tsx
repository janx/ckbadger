import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';
import { OPEN_SHORTCUTS_EVENT } from '@/lib/search-focus';
import { localZone } from '@/lib/clock';

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
  // moved to the status line at the foot of the window; the footer's links name none. (Below md
  // its last row says the release once more, for the phone: see the test after the next.)
  it('lists the board links, opens the keyboard shortcuts, and names no release (TX-04)', () => {
    const opened = vi.fn();
    window.addEventListener(OPEN_SHORTCUTS_EVENT, opened);
    const { container } = render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(container.querySelector('.live-dot')).toBeNull();

    const navigation = within(footer).getByRole('navigation', { name: 'Footer' });
    expect(navigation).not.toHaveTextContent(/CKBadger|0\.1\.0/i);
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

  // Ruling on B4 (2026-11-14): the phone must not lose the release. With "Keyboard shortcuts"
  // drawn from md only, nothing on a phone opened the help, whose last line was a phone's one
  // place for the release and the clock's zone. Below md the footer ends on one quiet row that
  // says both, in its lowercase: the release from the status line's source (resolveBuildVersion,
  // releaseOf; the whole build string in its title), the zone from the clock's (localZone). From
  // md it is not drawn: the status line shows the release, the help the zone. Text, not a link;
  // the release with its name and the zone are each one unbreakable token, and the row breaks only
  // between them, its "·" never left at the start of a line.
  it('ends below md on the release and the clock’s zone, not drawn from md', () => {
    render(<SiteFooter />);
    const footer = screen.getByRole('contentinfo');
    const about = within(footer).getByTestId('footer-about');
    // the footer's last row: after the links, a line of its own, quiet, lowercase, phone only
    expect(about.parentElement!.lastElementChild).toBe(about);
    expect(about.previousElementSibling).toBe(
      within(footer).getByRole('navigation', { name: 'Footer' })
    );
    expect(about).toHaveClass('basis-full', 'text-text-dim', 'lowercase', 'md:hidden');
    expect(about.tagName).toBe('P');
    expect(about.querySelector('a, button')).toBeNull();
    // two tokens, each unbreakable: the release with its name, the zone
    const tokens = Array.from(about.querySelectorAll<HTMLElement>('[data-token]'));
    expect(tokens.map((t) => t.dataset.token)).toEqual(['release', 'zone']);
    for (const t of tokens) expect(t, t.dataset.token).toHaveClass('whitespace-nowrap');
    const text = (t: HTMLElement) => t.querySelector('[data-text]')!.textContent;
    expect(text(tokens[0])).toBe('ckbadger 0.1.0');
    expect(tokens[0].querySelector('[data-text]')).toHaveAttribute(
      'title',
      'ckbadger 0.1.0+feature/foo@abcdef123456'
    );
    expect(text(tokens[1])).toBe(localZone());
    // each token is led by its "·", decoration; the row is drawn 2ch to the left inside a box
    // that clips it, so the "·" of a line's first token is never seen and the rest read "a · b"
    for (const t of tokens) {
      const sep = t.firstElementChild!;
      expect(sep).toHaveAttribute('aria-hidden', 'true');
      expect(sep.textContent).toBe('· ');
    }
    // re-pinned (ruling b): the box clips the row's left only, never a token on its right
    // re-pinned (A4 review 4): its left edge 0.75ch out, as the separated list's (the dot's cell
    // ends 1ch left of the box)
    expect(about).toHaveClass('min-w-0', '[clip-path:inset(-1em_-100vw_-1em_-0.75ch)]');
    expect(about).not.toHaveClass('overflow-hidden');
    expect(tokens[0].parentElement).toHaveClass('-ml-[2ch]', 'flex', 'flex-wrap', 'gap-x-[1ch]');
  });

  // A4 review 5: where Intl gives no time zone the row ended on a bare "· ". The zone is absent
  // when there is none (localZone says so where the zone is read), and the release stands alone.
  it('writes the release alone where Intl gives no time zone', () => {
    const resolved = Intl.DateTimeFormat().resolvedOptions();
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
      ...resolved,
      timeZone: undefined as unknown as string,
    });
    render(<SiteFooter />);
    const about = screen.getByTestId('footer-about');
    const tokens = [...about.querySelectorAll<HTMLElement>('[data-token]')];
    expect(tokens.map((t) => t.dataset.token)).toEqual(['release']);
    expect(about.textContent).not.toMatch(/·\s*$/);
    expect(about.textContent).not.toMatch(/undefined/);
    vi.restoreAllMocks();
  });

  it('omits the design and coding attribution', () => {
    render(<SiteFooter />);

    const footer = screen.getByRole('contentinfo');
    expect(footer).not.toHaveTextContent(/designed by|coded by|busyforking|Claude|Codex/i);
  });
});
