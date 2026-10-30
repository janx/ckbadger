import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';
import { RouteErrorBoundary } from '@/components/route-error-boundary';

const routeError = vi.hoisted(() => ({
  current: new Error('transaction 0xabc: invalid capacity') as unknown,
}));
vi.mock('react-router-dom', async (original) => ({
  ...(await original<typeof import('react-router-dom')>()),
  useRouteError: () => routeError.current,
}));

function Harness() {
  const location = useLocation();
  return (
    <>
      <p data-testid="location">
        {location.pathname}
        {location.search}
        {location.hash}
      </p>
      {useRoutes(createAppRouter())}
    </>
  );
}

vi.mock('@/components/layout/header', () => ({ Header: () => null }));
vi.mock('@/components/layout/site-footer', () => ({ SiteFooter: () => null }));
vi.mock('@/components/error-vessels', () => ({
  ErrorVessels: () => <div data-testid="error-vessels" />,
  VesselsCaption: () => <span>Bounded live cell sample</span>,
}));
vi.mock('@/app/inventory/tokens/page', () => ({ default: () => <p>Token inventory</p> }));
vi.mock('@/app/inventory/objects/page', () => ({ default: () => <p>Object inventory</p> }));
vi.mock('@/app/charts/page', () => ({ default: () => <p>Charts index</p> }));

describe('redesign routes', () => {
  beforeEach(() => {
    routeError.current = new Error('transaction 0xabc: invalid capacity');
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });
  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });
  it.each([
    ['tokens', 'inventory/tokens'],
    ['objects', 'inventory/objects'],
    ['dao/charts', 'charts'],
    // F2 objects (IV-25): the bare Inventory path opens its first view.
    ['inventory', 'inventory/tokens'],
  ])('redirects legacy %s preserving network, query and fragment', async (legacy, target) => {
    render(
      <MemoryRouter initialEntries={[`/testnet/${legacy}?range=30d#history`]}>
        <Harness />
      </MemoryRouter>
    );
    await waitFor(() =>
      expect(screen.getByTestId('location')).toHaveTextContent(
        `/testnet/${target}?range=30d#history`
      )
    );
  });

  it('shows a crash for a failed loader, never a fabricated missing entity', async () => {
    render(<RouteErrorBoundary />);
    expect(
      await screen.findByRole('heading', { name: 'The page fell. The cause is unclear.' })
    ).toBeInTheDocument();
    expect(screen.getByText('transaction 0xabc: invalid capacity')).toBeInTheDocument();
    expect(screen.queryByText(/cell_not_found/)).toBeNull();
  });

  it('keeps route HTTP 404 distinct from HTTP 500', () => {
    routeError.current = { status: 404, statusText: 'Not Found', data: null, internal: true };
    const notFound = render(<RouteErrorBoundary />);
    expect(screen.getByRole('heading', { name: 'The road ends here.' })).toBeInTheDocument();
    expect(screen.queryByRole('alert')).toBeNull();
    notFound.unmount();

    routeError.current = {
      status: 500,
      statusText: 'Index read failed',
      data: null,
      internal: true,
    };
    render(<RouteErrorBoundary />);
    expect(
      screen.getByRole('heading', { name: 'The page fell. The cause is unclear.' })
    ).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Index read failed');
  });
});
