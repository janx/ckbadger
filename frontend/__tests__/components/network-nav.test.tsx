import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { Header } from '@/components/layout/header';

// Mirror header.test.tsx: stub the header's heavy sub-widgets and the pathname hook so the real
// nav (and the real Header rendered inside the Peers page) mounts without Router/stats plumbing.
const usePathnameMock = vi.fn(() => '/network');
vi.mock('@/src/navigation', () => ({
  usePathname: () => usePathnameMock(),
}));
vi.mock('@/components/command-palette', () => ({
  CommandPalette: () => <div data-testid="command-palette" />,
}));
vi.mock('@/components/search-bar', () => ({
  SearchBar: () => <div data-testid="search-bar" />,
}));
vi.mock('@/components/layout/logo', () => ({
  Logo: () => <div data-testid="logo" />,
}));
vi.mock('@/components/stats-bar', () => ({
  GlobalStatsBar: () => <div data-testid="global-stats-bar" />,
}));

describe('Peers nav entry', () => {
  // Peers was removed from the navbar; it is now reachable via the `g p` keyboard
  // shortcut and the command palette (see command-palette.test.tsx).
  it('does not render a "Peers" link in the navbar', () => {
    render(<Header />);

    expect(screen.queryByRole('link', { name: 'Peers' })).not.toBeInTheDocument();
  });
});
