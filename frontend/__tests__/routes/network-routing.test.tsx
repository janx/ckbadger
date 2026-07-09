import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';
import Link from '@/components/ui/link';

vi.mock('@/components/layout/site-footer', () => ({
  SiteFooter: () => <div data-testid="site-footer">SiteFooter</div>,
}));

vi.mock('@/components/not-found-page', () => ({
  NotFoundPage: () => <div>not found page</div>,
}));

vi.mock('@/app/page', () => ({
  default: () => <div>home page</div>,
}));

vi.mock('@/app/blocks/page', () => ({
  default: () => <div>blocks page</div>,
}));

function AppHarness() {
  const location = useLocation();
  const element = useRoutes(createAppRouter());

  return (
    <>
      <div data-testid="pathname">{location.pathname}</div>
      {element}
    </>
  );
}

describe('network routing', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
    window.scrollTo = vi.fn();
  });

  afterEach(() => {
    // jsdom's window persists across tests; clear the seeded config so it does not
    // leak into sibling files sharing the same jsdom instance.
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('redirects the root path to the default network', async () => {
    render(
      <MemoryRouter initialEntries={['/']}>
        <AppHarness />
      </MemoryRouter>
    );

    expect(await screen.findByText('home page')).toBeInTheDocument();
    expect(screen.getByTestId('pathname')).toHaveTextContent('/mainnet');
  });

  it('prepends the default network to an old un-prefixed deep link (DECISION 2)', async () => {
    render(
      <MemoryRouter initialEntries={['/blocks']}>
        <AppHarness />
      </MemoryRouter>
    );

    // `/blocks` has an unknown first segment, so the guard prepends the default
    // network to the ENTIRE original path rather than stripping the segment.
    expect(await screen.findByText('blocks page')).toBeInTheDocument();
    expect(screen.getByTestId('pathname')).toHaveTextContent('/mainnet/blocks');
    expect(screen.queryByText('not found page')).not.toBeInTheDocument();
  });

  it('prefixes links with the active network when browsing a testnet page', async () => {
    const user = userEvent.setup();

    function LinkHarness() {
      const location = useLocation();
      return (
        <>
          <div data-testid="pathname">{location.pathname}</div>
          <Link href="/scripts">Scripts</Link>
        </>
      );
    }

    render(
      <MemoryRouter initialEntries={['/testnet/dao']}>
        <LinkHarness />
      </MemoryRouter>
    );

    const link = screen.getByRole('link', { name: 'Scripts' });
    // The rendered href is prefixed with the active (testnet) network.
    expect(link).toHaveAttribute('href', '/testnet/scripts');

    await user.click(link);

    await waitFor(() => {
      expect(screen.getByTestId('pathname')).toHaveTextContent('/testnet/scripts');
    });
  });
});
