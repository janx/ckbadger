import { describe, expect, it, beforeEach, vi } from 'vitest';
import { render } from '../utils/test-utils';
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

describe('Header', () => {
  beforeEach(() => {
    usePathnameMock.mockReset();
    searchBarMock.mockClear();
  });

  it('uses highlighted home search variant on home page', () => {
    usePathnameMock.mockReturnValue('/');
    const { getByTestId } = render(<Header />);

    const search = getByTestId('search-bar');
    expect(search).toBeInTheDocument();
    expect(searchBarMock.mock.calls.some(([props]) => props.variant === 'home')).toBeTruthy();
    expect(search.parentElement?.className).toContain('max-w-[clamp(18rem,40vw,42rem)]');
    expect(search.parentElement?.parentElement?.className).toContain('flex-1');
    expect(search.parentElement?.parentElement?.className).toContain('justify-center');
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
    expect(desktopNav?.className).toContain('justify-end');

    const chartsLink = Array.from(desktopNav?.querySelectorAll('a') ?? []).find(
      (node) => node.textContent?.trim() === 'Charts'
    );
    const daoLink = Array.from(desktopNav?.querySelectorAll('a') ?? []).find(
      (node) => node.textContent?.trim() === 'DAO'
    );

    expect(chartsLink?.className).toContain('text-amber');
    expect(chartsLink?.className).toContain('bg-amber/8');
    expect(daoLink?.className).toContain('text-text-muted');
    expect(daoLink?.className).toContain('border-transparent');
  });
});
