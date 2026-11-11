import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '../utils/test-utils';
import { Header } from '@/components/layout/header';
import { resetThemeForTests } from '@/lib/theme';

const usePathnameMock = vi.fn(() => '/');
const searchBarMock = vi.fn(({ variant }: { variant?: 'default' | 'compact' | 'home' }) => (
  <div data-testid="search-bar" data-variant={variant ?? 'default'}>
    SearchBar
  </div>
));

vi.mock('@/src/navigation', () => ({
  usePathname: () => usePathnameMock(),
  useSearchAndHash: () => '',
  useRouter: () => ({ push: vi.fn() }),
}));

vi.mock('@/components/command-palette', () => ({
  CommandPalette: () => <div data-testid="command-palette">CommandPalette</div>,
}));

vi.mock('@/components/search-bar', () => ({
  SearchBar: (props: { variant?: 'default' | 'compact' | 'home' }) => searchBarMock(props),
}));

vi.mock('@/components/layout/wordmark', () => ({
  Wordmark: () => <div data-testid="wordmark">Wordmark</div>,
}));

vi.mock('@/components/stats-bar', () => ({
  VitalsStrip: () => <div data-testid="vitals-strip">VitalsStrip</div>,
}));

describe('Header', () => {
  beforeEach(() => {
    usePathnameMock.mockReset();
    searchBarMock.mockClear();
  });

  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('uses compact search variant on home page', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    expect(screen.getByTestId('search-bar')).toBeInTheDocument();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'compact')).toBeTruthy();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'home')).toBeFalsy();
  });

  it('renders nav links and stats bar on non-home pages', () => {
    usePathnameMock.mockReturnValue('/blocks');
    render(<Header />);

    expect(screen.getByTestId('search-bar')).toBeInTheDocument();
    expect(screen.getByTestId('vitals-strip')).toBeInTheDocument();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'compact')).toBeTruthy();
    expect(screen.getAllByRole('link', { name: 'DAO' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/dao'
    );
    // Review 2026-11-03, item 4 (Nav.dc.html): Activities is a menu button now, as Inventory is,
    // so the desktop row holds no plain Activities link; its index is the menu's first item (see
    // the Activities menu tests below).
    const nav = screen.getByRole('navigation', { name: 'Main' });
    expect(within(nav).queryByRole('link', { name: 'Activities' })).toBeNull();
    expect(within(nav).getByRole('button', { name: 'Activities' })).toHaveAttribute(
      'aria-haspopup',
      'menu'
    );
    // Lane U (U4, after TX-02): Inventory opens a menu (hover, click, keyboard) with the index and
    // its three views; closed, the menu is not in the page.
    const inventory = within(nav).getByRole('button', { name: 'Inventory' });
    expect(inventory).toHaveAttribute('aria-haspopup', 'menu');
    expect(inventory).toHaveAttribute('aria-expanded', 'false');
    expect(within(nav).queryByRole('link', { name: 'Inventory' })).toBeNull();
    expect(within(nav).queryByRole('menu')).toBeNull();
    expect(within(nav).queryByRole('link', { name: 'Tokens' })).toBeNull();
    expect(screen.getAllByRole('link', { name: 'Scripts' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/scripts'
    );
    expect(screen.getAllByRole('link', { name: 'Charts' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );
    expect(screen.queryByRole('link', { name: 'Fiber' })).not.toBeInTheDocument();
  });

  it('shows the vitals strip on the home page too, not only after the hero scrolls away', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    expect(screen.getByTestId('vitals-strip')).toBeInTheDocument();
  });

  // Rubbing × Song print, phase 2: the badge and its notches are gone; the wordmark (the prompt
  // square and `ckbadger`) leads the row in the flow, at every width.
  it('shows the wordmark at every width, first in the header row', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    const wordmark = screen.getByTestId('wordmark');
    // Not wrapped in a desktop-only container any more.
    expect(wordmark.closest('.hidden')).toBeNull();
    // A 52 px row up to lg, 60 px from lg.
    expect(wordmark.parentElement).toHaveClass('min-h-[52px]', 'lg:min-h-[60px]');
    expect(wordmark.parentElement).not.toHaveClass('md:min-h-[60px]');
    // The nav follows it directly: nothing is reserved beside it any more.
    expect(wordmark.nextElementSibling).toBe(screen.getByRole('navigation', { name: 'Main' }));
    expect(screen.queryByTestId('header-logo-notch')).toBeNull();
    expect(screen.queryByTestId('vitals-logo-notch')).toBeNull();
  });

  it('keeps only the header row sticky on phones and the row plus the vitals strip from md up', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    const banner = screen.getByRole('banner');
    const strip = screen.getByTestId('vitals-strip');
    // Phones: the row is the sticky element; the strip is outside it and scrolls away.
    expect(banner).toHaveClass('sticky', 'top-0', 'md:static');
    expect(banner).not.toContainElement(strip);
    // md and up: one sticky block holds both. Below md it is display: contents, so the row sticks
    // to the page, not to this wrapper.
    const wrapper = banner.parentElement as HTMLElement;
    expect(wrapper).toHaveClass('contents', 'md:sticky', 'md:top-0', 'md:block');
    expect(wrapper).toContainElement(strip);
    // The wordmark is in the sticky row in both layouts.
    expect(banner).toContainElement(screen.getByTestId('wordmark'));
  });

  it('labels the nav in sentence case with normal tracking', () => {
    usePathnameMock.mockReturnValue('/dao');
    render(<Header />);

    const dao = screen.getAllByRole('link', { name: 'DAO' })[0];
    expect(dao.className).not.toContain('uppercase');
    expect(dao.className).not.toContain('tracking');
    // TX-02: the current page is marked for assistive tech and drawn bright, not jade (jade is hover).
    expect(dao).toHaveAttribute('aria-current', 'page');
    expect(dao).toHaveClass('text-text-bright');
    expect(dao).not.toHaveClass('text-jade');
    const charts = screen.getAllByRole('link', { name: 'Charts' })[0];
    expect(charts).toHaveClass('text-text');
    expect(charts).not.toHaveAttribute('aria-current');
  });

  it('marks Inventory current on every inventory view, and the view itself in its menu (TX-02, U4)', () => {
    usePathnameMock.mockReturnValue('/inventory/objects');
    render(<Header />);
    const nav = screen.getByRole('navigation', { name: 'Main' });
    const inventory = within(nav).getByRole('button', { name: 'Inventory' });
    // the section is current (bright, as a current link reads); the page is the view in the menu
    expect(inventory).toHaveAttribute('aria-current', 'true');
    expect(inventory).toHaveClass('text-text-bright');
    fireEvent.click(inventory);
    const menu = within(nav).getByRole('menu', { name: 'Inventory' });
    expect(within(menu).getByRole('menuitem', { name: 'Objects' })).toHaveAttribute(
      'aria-current',
      'page'
    );
    expect(within(menu).getByRole('menuitem', { name: 'Tokens' })).not.toHaveAttribute(
      'aria-current'
    );
  });

  // Review 2026-11-03: no "Inventory index" entry — /inventory only redirects to Tokens, so the
  // entry repeated the Tokens view. The menu is the three views, with no hairline.
  it('opens the Inventory menu on click: Tokens · Objects · Identities, no index entry (U4)', () => {
    usePathnameMock.mockReturnValue('/blocks');
    render(<Header />);
    const nav = screen.getByRole('navigation', { name: 'Main' });
    const inventory = within(nav).getByRole('button', { name: 'Inventory' });
    fireEvent.click(inventory);
    expect(inventory).toHaveAttribute('aria-expanded', 'true');
    const menu = within(nav).getByRole('menu', { name: 'Inventory' });
    expect(inventory).toHaveAttribute('aria-controls', menu.id);
    const items = within(menu).getAllByRole('menuitem');
    expect(items.map((i) => [i.textContent, i.getAttribute('href')])).toEqual([
      ['Tokens', '/mainnet/inventory/tokens'],
      ['Objects', '/mainnet/inventory/objects'],
      ['Identities', '/mainnet/inventory/identities'],
    ]);
    expect(within(menu).queryByRole('menuitem', { name: /index/i })).toBeNull();
    expect(within(menu).queryByRole('separator')).toBeNull();
    // the header's look: the nav's mono body text and link inks, no new colours
    for (const item of items) expect(item).toHaveClass('font-mono', 'text-body');
    expect(items[0]).toHaveClass('text-text', 'hover:text-jade');
    // a second click closes it; so does a click outside, and choosing an item
    fireEvent.click(inventory);
    expect(inventory).toHaveAttribute('aria-expanded', 'false');
    expect(within(nav).queryByRole('menu')).toBeNull();
    fireEvent.click(inventory);
    fireEvent.pointerDown(document.body);
    expect(within(nav).queryByRole('menu')).toBeNull();
    fireEvent.click(inventory);
    const tokens = within(nav).getByRole('menuitem', { name: 'Tokens' });
    // no router here: keep jsdom from attempting a real navigation
    tokens.addEventListener('click', (event) => event.preventDefault());
    fireEvent.click(tokens);
    expect(within(nav).queryByRole('menu')).toBeNull();
  });

  it('opens the Inventory menu on hover; a click while hovered keeps it open (U4)', () => {
    usePathnameMock.mockReturnValue('/blocks');
    render(<Header />);
    const nav = screen.getByRole('navigation', { name: 'Main' });
    const inventory = within(nav).getByRole('button', { name: 'Inventory' });
    const wrapper = inventory.parentElement!;
    fireEvent.mouseEnter(wrapper);
    expect(within(nav).getByRole('menu', { name: 'Inventory' })).toBeInTheDocument();
    expect(inventory).toHaveAttribute('aria-expanded', 'true');
    fireEvent.mouseLeave(wrapper);
    expect(within(nav).queryByRole('menu')).toBeNull();
    // hover opens, the click pins it: leaving no longer closes it
    fireEvent.mouseEnter(wrapper);
    fireEvent.click(inventory);
    fireEvent.mouseLeave(wrapper);
    expect(within(nav).getByRole('menu', { name: 'Inventory' })).toBeInTheDocument();
  });

  it('works from the keyboard: arrows open and move, Escape closes back to the button (U4)', () => {
    usePathnameMock.mockReturnValue('/blocks');
    render(<Header />);
    const nav = screen.getByRole('navigation', { name: 'Main' });
    const inventory = within(nav).getByRole('button', { name: 'Inventory' });
    inventory.focus();
    fireEvent.keyDown(inventory, { key: 'ArrowDown' });
    const item = (name: string) => within(nav).getByRole('menuitem', { name });
    expect(item('Tokens')).toHaveFocus();
    fireEvent.keyDown(item('Tokens'), { key: 'ArrowDown' });
    expect(item('Objects')).toHaveFocus();
    fireEvent.keyDown(item('Objects'), { key: 'End' });
    expect(item('Identities')).toHaveFocus();
    fireEvent.keyDown(item('Identities'), { key: 'ArrowDown' });
    expect(item('Tokens')).toHaveFocus();
    fireEvent.keyDown(item('Tokens'), { key: 'ArrowUp' });
    expect(item('Identities')).toHaveFocus();
    fireEvent.keyDown(item('Identities'), { key: 'Home' });
    expect(item('Tokens')).toHaveFocus();
    fireEvent.keyDown(item('Tokens'), { key: 'Escape' });
    expect(within(nav).queryByRole('menu')).toBeNull();
    expect(inventory).toHaveFocus();
    // ArrowUp opens on the last view; Tab away closes it
    fireEvent.keyDown(inventory, { key: 'ArrowUp' });
    expect(item('Identities')).toHaveFocus();
    fireEvent.blur(item('Identities'), { relatedTarget: document.body });
    expect(within(nav).queryByRole('menu')).toBeNull();
    // Enter / Space on the button (its click) open it with the first item focused
    fireEvent.keyDown(inventory, { key: 'Enter' });
    fireEvent.click(inventory, { detail: 0 });
    expect(item('Tokens')).toHaveFocus();
  });

  it('opens and closes the mobile menu with navigation links', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    const toggleMenu = screen.getByRole('button', { name: 'Toggle menu' });

    fireEvent.click(toggleMenu);

    expect(screen.getAllByRole('link', { name: 'DAO' })).toHaveLength(2);
    // Review 2026-11-03, item 4: Activities is a section with views, so the desktop row shows a
    // menu button and the phone menu one Activities row (its index link) with the three views
    // under it — one link, as Inventory has.
    expect(screen.getAllByRole('link', { name: 'Activities' })).toHaveLength(1);
    expect(screen.getByRole('link', { name: 'Activities' })).toHaveAttribute(
      'href',
      '/mainnet/activities'
    );
    expect(screen.getByRole('link', { name: 'Transactions' })).toHaveAttribute(
      'href',
      '/mainnet/transactions'
    );
    expect(screen.getByRole('link', { name: 'Blocks' })).toHaveAttribute('href', '/mainnet/blocks');
    expect(screen.getByRole('link', { name: 'Forks' })).toHaveAttribute('href', '/mainnet/forks');
    // The phone menu lists the same three as the desktop Inventory menu under an Inventory
    // heading: no index page, so the row is text, not a link (review 2026-11-03).
    expect(screen.queryByRole('link', { name: 'Inventory' })).toBeNull();
    const phoneNav = screen
      .getAllByRole('navigation')
      .find((n) => n.getAttribute('aria-label') !== 'Main')!;
    expect(within(phoneNav).getByText('Inventory').tagName).toBe('SPAN');
    expect(screen.getAllByRole('link', { name: 'Tokens' })).toHaveLength(1);
    expect(screen.getAllByRole('link', { name: 'Objects' })).toHaveLength(1);
    expect(screen.getAllByRole('link', { name: 'Identities' })).toHaveLength(1);
    expect(screen.getByRole('link', { name: 'Tokens' })).toHaveAttribute(
      'href',
      '/mainnet/inventory/tokens'
    );
    expect(screen.queryByRole('link', { name: 'Fiber' })).not.toBeInTheDocument();
    expect(searchBarMock.mock.calls.every(([props]) => props.variant === 'compact')).toBe(true);

    fireEvent.click(toggleMenu);

    expect(screen.getAllByRole('link', { name: 'DAO' })).toHaveLength(1);
  });

  // Review 2026-11-03, item 4 (Nav.dc.html): Activities opens the way Inventory does — the index,
  // then Transactions · Blocks · Forks, one layer deeper per row. Nothing else in the bar moves.
  it('opens the Activities menu: Activities · Transactions · Blocks · Forks, in the same bar (item 4)', () => {
    usePathnameMock.mockReturnValue('/dao');
    render(<Header />);
    const nav = screen.getByRole('navigation', { name: 'Main' });
    // the bar: DAO · Activities ∨ · Inventory ∨ · Scripts · Charts
    expect(
      Array.from(nav.querySelectorAll(':scope > a, :scope > div > button')).map(
        (el) => el.textContent
      )
    ).toEqual(['DAO', 'Activities', 'Inventory', 'Scripts', 'Charts']);
    const activities = within(nav).getByRole('button', { name: 'Activities' });
    expect(activities).toHaveAttribute('aria-expanded', 'false');
    expect(activities).not.toHaveAttribute('aria-current');
    fireEvent.click(activities);
    expect(activities).toHaveAttribute('aria-expanded', 'true');
    const menu = within(nav).getByRole('menu', { name: 'Activities' });
    expect(activities).toHaveAttribute('aria-controls', menu.id);
    const items = within(menu).getAllByRole('menuitem');
    expect(items.map((i) => [i.textContent, i.getAttribute('href')])).toEqual([
      ['Activities', '/mainnet/activities'],
      ['Transactions', '/mainnet/transactions'],
      ['Blocks', '/mainnet/blocks'],
      ['Forks', '/mainnet/forks'],
    ]);
    // the Inventory menu's materials and inks, no new colours
    for (const item of items) expect(item).toHaveClass('font-mono', 'text-body');
    expect(items[2]).toHaveClass('text-text', 'hover:text-jade');
    // the Inventory menu is untouched and the two never open together
    expect(within(nav).queryByRole('menu', { name: 'Inventory' })).toBeNull();
    fireEvent.click(activities);
    expect(within(nav).queryByRole('menu')).toBeNull();
  });

  it('marks Activities current on its views (a block, the forks), and the view in its menu', () => {
    usePathnameMock.mockReturnValue('/blocks');
    render(<Header />);
    const nav = screen.getByRole('navigation', { name: 'Main' });
    const activities = within(nav).getByRole('button', { name: 'Activities' });
    expect(activities).toHaveAttribute('aria-current', 'true');
    expect(activities).toHaveClass('text-text-bright');
    expect(within(nav).getByRole('button', { name: 'Inventory' })).not.toHaveAttribute(
      'aria-current'
    );
    fireEvent.click(activities);
    const menu = within(nav).getByRole('menu', { name: 'Activities' });
    expect(within(menu).getByRole('menuitem', { name: 'Blocks' })).toHaveAttribute(
      'aria-current',
      'page'
    );
    expect(within(menu).getByRole('menuitem', { name: 'Activities' })).not.toHaveAttribute(
      'aria-current'
    );
    // the phone row names the view
    expect(screen.getByText('Blocks', { selector: 'span' })).toHaveClass('text-jade');
  });

  // Midnight, continued (review 2026-11-03, item 5, move 2): the header's bottom edge is the 1 px
  // border-b coloured by `.hairline-tube-header` — jade fading into the subtle border then, a
  // plain line in the line ink since rubbing × Song print. The sticky geometry is unchanged.
  it('closes the header row on a lit hairline (move 2)', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);
    const banner = screen.getByRole('banner');
    expect(banner).toHaveClass('border-b', 'border-base-border-subtle', 'hairline-tube-header');
  });

  it('separates the network context selector from navbar links', () => {
    usePathnameMock.mockReturnValue('/');
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
    render(<Header />);

    const switcher = screen.getByTestId('network-switcher');
    const daoLink = screen.getAllByRole('link', { name: 'DAO' })[0];

    expect(switcher).toHaveAttribute('data-control', 'network-context');
    expect(switcher.nextElementSibling).toBe(daoLink);
  });

  it('hides the network switcher for single-network deployments', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    expect(screen.queryByTestId('network-switcher')).not.toBeInTheDocument();
  });
});

describe('Header · the theme control', () => {
  beforeEach(() => {
    usePathnameMock.mockReset();
    localStorage.clear();
    resetThemeForTests();
  });
  afterEach(() => {
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
  });

  it('carries the theme menu on the desktop row, right after the search', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);
    const banner = screen.getByRole('banner');
    const switcher = within(banner).getByTestId('theme-switcher');
    expect(switcher).toHaveClass('hidden', 'md:block');
    expect(within(switcher).getByRole('button', { name: 'Theme: Dark' })).toHaveAttribute(
      'aria-haspopup',
      'menu'
    );
    expect(screen.getByTestId('search-bar').parentElement?.nextElementSibling).toBe(switcher);
  });

  it('offers the same choice as a row of the phone menu, which closes on a choice', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);
    expect(screen.queryByTestId('theme-segments')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Toggle menu' }));
    const segments = screen.getByTestId('theme-segments');
    fireEvent.click(within(segments).getByRole('radio', { name: /^Light/ }));
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(localStorage.getItem('ckbadger.theme')).toBe('light');
    expect(screen.queryByTestId('theme-segments')).toBeNull();
  });
});
