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
      await screen.findByRole('heading', { name: 'the chain keeps what succeeded. this did not.' })
    ).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('transaction 0xabc: invalid capacity');
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.queryByText(/cell_not_found/)).toBeNull();
  });

  // Consistency plan C2: whatever carries HTTP 404 to the boundary is a not-found — the
  // `notFound()` signal of a route that names no page, and an API not-found thrown in render
  // (the body is the API's real answer for a missing fork, curl 2026-11-02).
  it('renders the 404 design for notFound() and for an API not-found, never the crash page', async () => {
    const { notFound } = await vi.importActual<typeof import('@/src/next-compat/navigation')>(
      '@/src/next-compat/navigation'
    );
    try {
      notFound();
    } catch (signal) {
      routeError.current = signal;
    }
    const route = render(<RouteErrorBoundary />);
    expect(
      screen.getByRole('heading', {
        name: "no page at this path. the chain was not asked; nothing on it is lost. honey badger don't care.",
      })
    ).toBeInTheDocument();
    expect(screen.queryByRole('alert')).toBeNull();
    route.unmount();

    const { ApiRequestError } = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
    routeError.current = new ApiRequestError(
      404,
      'not_found',
      'No fork event at #20894012 dropped a block starting 0xc121e093'
    );
    render(<RouteErrorBoundary />);
    expect(
      screen.getByRole('heading', {
        name: "no page at this path. the chain was not asked; nothing on it is lost. honey badger don't care.",
      })
    ).toBeInTheDocument();
    expect(screen.queryByText(/the chain keeps/)).toBeNull();
  });

  it('keeps route HTTP 404 distinct from HTTP 500', () => {
    routeError.current = { status: 404, statusText: 'Not Found', data: null, internal: true };
    const notFound = render(<RouteErrorBoundary />);
    expect(
      screen.getByRole('heading', {
        name: "no page at this path. the chain was not asked; nothing on it is lost. honey badger don't care.",
      })
    ).toBeInTheDocument();
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
      screen.getByRole('heading', { name: 'the chain keeps what succeeded. this did not.' })
    ).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Index read failed');
  });
});
