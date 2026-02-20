import { describe, expect, it, vi } from 'vitest';
import { render } from '../utils/test-utils';
import { Header } from '@/components/layout/header';

vi.mock('@/components/search-bar', () => ({
  SearchBar: () => <div data-testid="search-bar">SearchBar</div>,
}));

vi.mock('@/components/layout/logo', () => ({
  Logo: () => <div data-testid="logo">Logo</div>,
}));

describe('Header', () => {
  it('renders nav links without pipeline entry', () => {
    const { container } = render(<Header />);

    const desktopNav = container.querySelector('nav.hidden.shrink-0');
    const labels = Array.from(desktopNav?.querySelectorAll('a') ?? []).map((node) =>
      (node.textContent ?? '').trim()
    );

    expect(labels).toEqual(['DAO', 'Assets', 'Scripts', 'Charts']);
    expect(desktopNav?.querySelector('a')?.getAttribute('href')).toBe('/dao');
  });
});
