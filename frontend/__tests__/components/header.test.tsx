import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '../utils/test-utils';
import { Header } from '@/components/layout/header';
import { useHomeScrollStore } from '@/hooks/useHomeScrollStore';

const usePathnameMock = vi.fn(() => '/');
const searchBarMock = vi.fn(({ variant }: { variant?: 'default' | 'compact' | 'home' }) => (
  <div data-testid="search-bar" data-variant={variant ?? 'default'}>
    SearchBar
  </div>
));

vi.mock('@/src/navigation', () => ({
  usePathname: () => usePathnameMock(),
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
  GlobalStatsBar: () => <div data-testid="global-stats-bar">GlobalStatsBar</div>,
}));

describe('Header', () => {
  beforeEach(() => {
    usePathnameMock.mockReset();
    searchBarMock.mockClear();
    useHomeScrollStore.setState({ heroVisible: true });
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
    expect(screen.getByTestId('global-stats-bar')).toBeInTheDocument();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'compact')).toBeTruthy();
    expect(screen.getAllByRole('link', { name: 'DAO' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/dao'
    );
    expect(screen.getAllByRole('link', { name: 'Activities' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/activities'
    );
    expect(screen.getAllByRole('link', { name: 'Tokens' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/inventory/tokens'
    );
    expect(screen.getAllByRole('link', { name: 'Objects' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/inventory/objects'
    );
    expect(screen.getAllByRole('link', { name: 'Identities' }).at(0)).toHaveAttribute(
      'href',
      '/mainnet/inventory/identities'
    );
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

  it('opens and closes the mobile menu with navigation links', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    const toggleMenu = screen.getByRole('button', { name: 'Toggle menu' });

    fireEvent.click(toggleMenu);

    expect(screen.getAllByRole('link', { name: 'DAO' })).toHaveLength(2);
    expect(screen.getAllByRole('link', { name: 'Activities' })).toHaveLength(2);
    expect(screen.getAllByRole('link', { name: 'Tokens' })).toHaveLength(2);
    expect(screen.getAllByRole('link', { name: 'Objects' })).toHaveLength(2);
    expect(screen.getAllByRole('link', { name: 'Identities' })).toHaveLength(2);
    expect(screen.queryByRole('link', { name: 'Fiber' })).not.toBeInTheDocument();
    expect(searchBarMock.mock.calls.every(([props]) => props.variant === 'compact')).toBe(true);

    fireEvent.click(toggleMenu);

    expect(screen.getAllByRole('link', { name: 'DAO' })).toHaveLength(1);
  });

  it('places the network dropdown immediately before the DAO link in the navbar', () => {
    usePathnameMock.mockReturnValue('/');
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
    render(<Header />);

    const switcher = screen.getByTestId('network-switcher');
    const daoLink = screen.getAllByRole('link', { name: 'DAO' })[0];
    const networkTrigger = screen.getByRole('button', { name: 'Select network' });
    const inventoryTrigger = screen.getByRole('button', { name: 'Inventory' });

    expect(networkTrigger.className).toBe(inventoryTrigger.className);
    expect(switcher.nextElementSibling).toBe(daoLink);
  });

  it('hides the network switcher for single-network deployments', () => {
    usePathnameMock.mockReturnValue('/');
    render(<Header />);

    expect(screen.queryByTestId('network-switcher')).not.toBeInTheDocument();
  });
});
