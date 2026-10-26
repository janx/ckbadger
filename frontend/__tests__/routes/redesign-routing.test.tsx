import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';
import { RouteErrorBoundary } from '@/components/route-error-boundary';

vi.mock('react-router-dom', async (original) => ({
  ...(await original<typeof import('react-router-dom')>()),
  useRouteError: () => new Error('transaction 0xabc: invalid capacity'),
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
vi.mock('@/app/inventory/tokens/page', () => ({ default: () => <p>Token inventory</p> }));
vi.mock('@/app/inventory/objects/page', () => ({ default: () => <p>Object inventory</p> }));
vi.mock('@/app/charts/page', () => ({ default: () => <p>Charts index</p> }));

describe('redesign routes', () => {
  beforeEach(() => {
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
      await screen.findByRole('heading', { name: 'Something went wrong' })
    ).toBeInTheDocument();
    expect(screen.getByText('transaction 0xabc: invalid capacity')).toBeInTheDocument();
    expect(screen.queryByText(/cell_not_found/)).toBeNull();
  });
});
