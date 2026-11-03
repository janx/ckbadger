import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '../utils/test-utils';
import { Header } from '@/components/layout/header';

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

vi.mock('@/components/layout/logo', () => ({
  Logo: () => <div data-testid="logo">Logo</div>,
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
    expect(screen.getAllByRole('link', { name: 'Activities' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/activities'
    );
    // Lane U (U4, after TX-02): Inventory opens a menu (hover, click, keyboard) with the index and
    // its three views; closed, the menu is not in the page.
    const nav = screen.getByRole('navigation', { name: 'Main' });
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

  it('shows the badge at every width, with a notch reserved for it in the header and the strip', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    const logo = screen.getByTestId('logo');
    // Not wrapped in a desktop-only container any more.
    expect(logo.closest('.hidden')).toBeNull();
    const notch = screen.getByTestId('header-logo-notch');
    expect(notch).toHaveAttribute('aria-hidden', 'true');
    expect(notch.className).toContain('clamp(96px,11vw,157px)');
    // The badge is at its 96 px minimum up to lg; a 52 px row there lets it hang across the border
    // as the larger badge does across the 60 px desktop row.
    expect(notch.parentElement).toHaveClass('min-h-[52px]', 'lg:min-h-[60px]');
    expect(notch.parentElement).not.toHaveClass('md:min-h-[60px]');
    // The nav starts right of the notch.
    expect(notch.nextElementSibling).toBe(screen.getByRole('navigation', { name: 'Main' }));
    expect(screen.getByTestId('vitals-logo-notch').className).toContain('md:block');
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
    // The badge is in the sticky row in both layouts.
    expect(banner).toContainElement(screen.getByTestId('logo'));
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

  it('opens the Inventory menu on click: the index, then Tokens · Objects · Identities (U4)', () => {
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
      ['Inventory index', '/mainnet/inventory'],
      ['Tokens', '/mainnet/inventory/tokens'],
      ['Objects', '/mainnet/inventory/objects'],
      ['Identities', '/mainnet/inventory/identities'],
    ]);
    // the header's look: the nav's mono body text and link inks, no new colours
    for (const item of items) expect(item).toHaveClass('font-mono', 'text-body');
    expect(items[1]).toHaveClass('text-text', 'hover:text-jade');
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
    expect(item('Inventory index')).toHaveFocus();
    fireEvent.keyDown(item('Inventory index'), { key: 'ArrowDown' });
    expect(item('Tokens')).toHaveFocus();
    fireEvent.keyDown(item('Tokens'), { key: 'End' });
    expect(item('Identities')).toHaveFocus();
    fireEvent.keyDown(item('Identities'), { key: 'ArrowDown' });
    expect(item('Inventory index')).toHaveFocus();
    fireEvent.keyDown(item('Inventory index'), { key: 'ArrowUp' });
    expect(item('Identities')).toHaveFocus();
    fireEvent.keyDown(item('Identities'), { key: 'Home' });
    expect(item('Inventory index')).toHaveFocus();
    fireEvent.keyDown(item('Inventory index'), { key: 'Escape' });
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
    expect(item('Inventory index')).toHaveFocus();
  });

  it('opens and closes the mobile menu with navigation links', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    const toggleMenu = screen.getByRole('button', { name: 'Toggle menu' });

    fireEvent.click(toggleMenu);

    expect(screen.getAllByRole('link', { name: 'DAO' })).toHaveLength(2);
    expect(screen.getAllByRole('link', { name: 'Activities' })).toHaveLength(2);
    // The phone menu lists the same four as the desktop Inventory menu: the index (the Inventory
    // row itself, now a link — U4) and its three views under it.
    expect(screen.getAllByRole('link', { name: 'Inventory' })).toHaveLength(1);
    expect(screen.getByRole('link', { name: 'Inventory' })).toHaveAttribute(
      'href',
      '/mainnet/inventory'
    );
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
