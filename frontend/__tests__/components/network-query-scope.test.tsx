import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useNavigate } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NetworkQueryScope } from '@/components/network-query-scope';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';

function NetworkData({ fetchNetwork }: { fetchNetwork: (network: string) => Promise<string> }) {
  const network = useActiveNetwork();
  const { data } = useQuery({
    queryKey: ['stats'],
    queryFn: () => fetchNetwork(network),
  });

  return <span data-testid="network-data">{data ?? 'loading'}</span>;
}

// Drive router navigation so the `:network` segment changes exactly as it does
// through the real network switcher.
function TestPage({ fetchNetwork }: { fetchNetwork: (network: string) => Promise<string> }) {
  const navigate = useNavigate();
  return (
    <>
      <button onClick={() => navigate('/mainnet/blocks')}>go-mainnet</button>
      <button onClick={() => navigate('/testnet/blocks')}>go-testnet</button>
      <button onClick={() => navigate('/testnet/transactions')}>go-testnet-other</button>
      <NetworkData fetchNetwork={fetchNetwork} />
    </>
  );
}

function renderScope(fetchNetwork: (network: string) => Promise<string>) {
  const outerQueryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const rendered = render(
    <QueryClientProvider client={outerQueryClient}>
      <MemoryRouter initialEntries={['/mainnet/blocks']}>
        <Routes>
          <Route
            path="/:network/*"
            element={
              <NetworkQueryScope>
                <TestPage fetchNetwork={fetchNetwork} />
              </NetworkQueryScope>
            }
          />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  );

  return { ...rendered, outerQueryClient };
}

describe('NetworkQueryScope', () => {
  let fetchNetwork: ReturnType<typeof vi.fn<(network: string) => Promise<string>>>;

  beforeEach(() => {
    fetchNetwork = vi.fn(async (network: string) => network);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('re-fetches a network-neutral query when the active network changes', async () => {
    const user = userEvent.setup();
    renderScope(fetchNetwork);

    expect(await screen.findByText('mainnet')).toBeInTheDocument();
    expect(fetchNetwork).toHaveBeenCalledWith('mainnet');

    await user.click(screen.getByRole('button', { name: 'go-testnet' }));

    await waitFor(() => {
      expect(screen.getByTestId('network-data')).toHaveTextContent('testnet');
    });
    expect(fetchNetwork).toHaveBeenCalledWith('testnet');
    expect(fetchNetwork).toHaveBeenCalledTimes(2);
  });

  it('keeps the same query client while navigating within one network', async () => {
    const user = userEvent.setup();
    renderScope(fetchNetwork);

    expect(await screen.findByText('mainnet')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'go-testnet' }));
    await waitFor(() => {
      expect(screen.getByTestId('network-data')).toHaveTextContent('testnet');
    });
    await user.click(screen.getByRole('button', { name: 'go-testnet-other' }));

    expect(screen.getByTestId('network-data')).toHaveTextContent('testnet');
    expect(fetchNetwork).toHaveBeenCalledTimes(2);
  });

  it('does not mutate the parent query cache', async () => {
    const user = userEvent.setup();
    const { outerQueryClient } = renderScope(fetchNetwork);
    const clearSpy = vi.spyOn(outerQueryClient, 'clear');

    expect(await screen.findByText('mainnet')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'go-testnet' }));
    await waitFor(() => {
      expect(screen.getByTestId('network-data')).toHaveTextContent('testnet');
    });

    expect(clearSpy).not.toHaveBeenCalled();
  });
});
