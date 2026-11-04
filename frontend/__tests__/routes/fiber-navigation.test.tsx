import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';

vi.mock('@/components/command-palette', () => ({
  CommandPalette: () => null,
}));

vi.mock('@/components/search-bar', () => ({
  SearchBar: () => <div data-testid="search-bar">SearchBar</div>,
}));

vi.mock('@/components/layout/logo', () => ({
  Logo: () => <div data-testid="logo">Logo</div>,
}));

vi.mock('@/components/stats-bar', () => ({
  VitalsStrip: () => <div data-testid="vitals-strip">VitalsStrip</div>,
}));

vi.mock('@/components/layout/site-footer', () => ({
  SiteFooter: () => <div data-testid="site-footer">SiteFooter</div>,
}));

vi.mock('@/components/not-found-page', () => ({
  NotFoundPage: () => <div>not found page</div>,
}));

vi.mock('@/app/page', () => ({
  default: () => <div>home page</div>,
}));

vi.mock('@/app/fiber/channels/page', () => ({
  default: () => <div>fiber channels page</div>,
}));

function AppHarness() {
  const location = useLocation();
  const element = useRoutes(createAppRouter());

  return (
    <>
      <div data-testid="pathname">{location.pathname}</div>
      {/* the frame renders the header (visual finish V6) */}
      {element}
    </>
  );
}

describe('fiber navigation', () => {
  it('renders the fiber channels route instead of the 404 page', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels']}>
        <AppHarness />
      </MemoryRouter>
    );

    expect(await screen.findByText('fiber channels page')).toBeInTheDocument();
    expect(screen.getByTestId('pathname')).toHaveTextContent('/mainnet/fiber/channels');
    expect(screen.queryByText('not found page')).not.toBeInTheDocument();
  });

  it('does not expose a stale Fiber header link after the nav refresh', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet']}>
        <AppHarness />
      </MemoryRouter>
    );

    await screen.findByText('home page');
    await waitFor(() => {
      expect(screen.queryByRole('link', { name: 'Fiber' })).not.toBeInTheDocument();
    });
    // Review 2026-11-03, item 4 (Nav.dc.html): Activities is a menu button in the header row (its
    // index link is the menu's first item), so the nav still names the section without a plain
    // Activities link.
    const nav = screen.getByRole('navigation', { name: 'Main' });
    expect(within(nav).getByRole('button', { name: 'Activities' })).toHaveAttribute(
      'aria-haspopup',
      'menu'
    );
    expect(within(nav).queryByRole('link', { name: 'Activities' })).not.toBeInTheDocument();
  });
});
