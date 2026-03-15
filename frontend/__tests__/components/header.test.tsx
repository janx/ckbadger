import { beforeEach, describe, expect, it, vi } from 'vitest';
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
    expect(screen.getAllByRole('link', { name: 'DAO' }).at(0)).toHaveAttribute('href', '/dao');
    expect(screen.getAllByRole('link', { name: 'Activities' }).at(0)).toHaveAttribute(
      'href',
      '/activities'
    );
    expect(screen.getAllByRole('link', { name: 'Assets' }).at(0)).toHaveAttribute(
      'href',
      '/assets'
    );
    expect(screen.getAllByRole('link', { name: 'Scripts' }).at(0)).toHaveAttribute(
      'href',
      '/scripts'
    );
    expect(screen.getAllByRole('link', { name: 'Charts' }).at(0)).toHaveAttribute(
      'href',
      '/charts'
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
    expect(screen.getAllByRole('link', { name: 'Assets' })).toHaveLength(2);
    expect(screen.queryByRole('link', { name: 'Fiber' })).not.toBeInTheDocument();
    expect(searchBarMock.mock.calls.every(([props]) => props.variant === 'compact')).toBe(true);

    fireEvent.click(toggleMenu);

    expect(screen.getAllByRole('link', { name: 'DAO' })).toHaveLength(1);
  });
});
