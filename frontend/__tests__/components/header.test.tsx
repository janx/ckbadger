import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '../utils/test-utils';
import { Header } from '@/components/layout/header';

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
  });

  it('uses compact search variant on home page', () => {
    usePathnameMock.mockReturnValue('/');
    const { getByTestId } = render(<Header />);

    const search = getByTestId('search-bar');
    const searchWrapper = search.parentElement?.parentElement;
    const desktopStartColumn = getByTestId('desktop-header-start-column');

    expect(search).toBeInTheDocument();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'compact')).toBeTruthy();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'home')).toBeFalsy();
    expect(search.parentElement?.className).toContain('max-w-[clamp(18rem,36vw,36rem)]');
    expect(searchWrapper?.className).toContain('flex-1');
    expect(searchWrapper?.className).not.toContain('md:pl-[96px]');
    expect(desktopStartColumn.className).toContain('md:w-[128px]');
    expect(searchWrapper?.previousElementSibling).toBe(desktopStartColumn);
  });

  it('uses compact search variant on non-home pages', () => {
    usePathnameMock.mockReturnValue('/blocks');
    const { getByTestId } = render(<Header />);

    expect(getByTestId('search-bar')).toBeInTheDocument();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'compact')).toBeTruthy();
  });

  it('renders nav links without pipeline entry', () => {
    usePathnameMock.mockReturnValue('/charts/hash-rate');
    const { container } = render(<Header />);

    const desktopNav = container.querySelector('nav.hidden.shrink-0');
    const labels = Array.from(desktopNav?.querySelectorAll('a') ?? []).map((node) =>
      (node.textContent ?? '').trim()
    );

    expect(labels).toEqual(['DAO', 'Assets', 'Scripts', 'Charts']);
    expect(desktopNav?.querySelector('a')?.getAttribute('href')).toBe('/dao');
    expect(desktopNav?.className).toContain('shrink-0');
    expect(desktopNav?.className).toContain('ml-auto');
    expect(desktopNav?.className).not.toContain('pl-[96px]');

    const chartsLink = Array.from(desktopNav?.querySelectorAll('a') ?? []).find(
      (node) => node.textContent?.trim() === 'Charts'
    );
    const daoLink = Array.from(desktopNav?.querySelectorAll('a') ?? []).find(
      (node) => node.textContent?.trim() === 'DAO'
    );

    expect(chartsLink?.className).toContain('text-jade');
    expect(chartsLink?.className).toContain('bg-jade/8');
    expect(daoLink?.className).toContain('text-text');
    expect(daoLink?.className).toContain('border-transparent');
  });

  it('left-aligns stats row so block starts under the search prompt', () => {
    usePathnameMock.mockReturnValue('/blocks');
    const { getByTestId } = render(<Header />);

    const stats = getByTestId('global-stats-bar');
    const statsStartColumn = getByTestId('desktop-stats-start-column');
    expect(stats.parentElement?.className).not.toContain('md:pl-[96px]');
    expect(stats.parentElement?.className).not.toContain('justify-end');
    expect(statsStartColumn.className).toContain('md:w-[128px]');
    expect(stats.previousElementSibling).toBe(statsStartColumn);
  });

  it('right-aligns mobile menu links below the compact search bar', () => {
    usePathnameMock.mockReturnValue('/');
    const { container } = render(<Header />);

    fireEvent.click(screen.getByRole('button', { name: 'Toggle menu' }));

    const mobilePanel = container.querySelector(
      'div.absolute.z-50.w-full.border-t.shadow-xl.backdrop-blur-sm.md\\:hidden'
    );
    const mobileLinks = Array.from(mobilePanel?.querySelectorAll('a') ?? []);

    expect(mobileLinks.map((node) => node.textContent?.trim())).toEqual([
      'DAO',
      'Assets',
      'Scripts',
      'Charts',
    ]);
    expect(mobileLinks.every((node) => node.className.includes('text-right'))).toBe(true);
    expect(searchBarMock.mock.calls.every(([props]) => props.variant === 'compact')).toBe(true);
  });
});
