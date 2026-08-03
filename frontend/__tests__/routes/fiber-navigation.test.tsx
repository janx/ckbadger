import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { Header } from '@/components/layout/header';
import { useHomeScrollStore } from '@/hooks/useHomeScrollStore';
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
  GlobalStatsBar: () => <div data-testid="global-stats-bar">GlobalStatsBar</div>,
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
      <Header />
      {element}
    </>
  );
}

describe('fiber navigation', () => {
  beforeEach(() => {
    useHomeScrollStore.setState({ heroVisible: true });
  });

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
    expect(screen.getByRole('link', { name: 'Activities' })).toHaveAttribute(
      'href',
      '/mainnet/activities'
    );
  });
});
