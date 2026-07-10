import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useNavigate } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NetworkQueryScope } from '@/components/network-query-scope';

// Buttons that drive a router navigation, so the `:network` segment (and thus
// `useActiveNetwork`) changes exactly the way the real network switcher does.
function NavButtons() {
  const navigate = useNavigate();
  return (
    <>
      <button onClick={() => navigate('/mainnet/blocks')}>go-mainnet</button>
      <button onClick={() => navigate('/testnet/blocks')}>go-testnet</button>
      <button onClick={() => navigate('/testnet/transactions')}>go-testnet-other</button>
    </>
  );
}

function renderScope(queryClient: QueryClient) {
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={['/mainnet/blocks']}>
        <Routes>
          <Route
            path="/:network/*"
            element={
              <NetworkQueryScope>
                <NavButtons />
              </NetworkQueryScope>
            }
          />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe('NetworkQueryScope', () => {
  let queryClient: QueryClient;
  let clearSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    queryClient = new QueryClient();
    clearSpy = vi.spyOn(queryClient, 'clear');
  });

  afterEach(() => {
    clearSpy.mockRestore();
    queryClient.clear();
  });

  it('does not clear the cache on initial mount', () => {
    renderScope(queryClient);
    expect(clearSpy).not.toHaveBeenCalled();
  });

  it('clears the cache once when the active network changes', async () => {
    const user = userEvent.setup();
    renderScope(queryClient);

    await user.click(screen.getByRole('button', { name: 'go-testnet' }));

    expect(clearSpy).toHaveBeenCalledTimes(1);
  });

  it('does not clear again while navigating within the same network', async () => {
    const user = userEvent.setup();
    renderScope(queryClient);

    await user.click(screen.getByRole('button', { name: 'go-testnet' }));
    expect(clearSpy).toHaveBeenCalledTimes(1);

    // Same `:network` segment, different sub-path — must NOT clear again.
    await user.click(screen.getByRole('button', { name: 'go-testnet-other' }));
    expect(clearSpy).toHaveBeenCalledTimes(1);
  });

  it('clears again when switching to another network', async () => {
    const user = userEvent.setup();
    renderScope(queryClient);

    await user.click(screen.getByRole('button', { name: 'go-testnet' }));
    await user.click(screen.getByRole('button', { name: 'go-mainnet' }));

    expect(clearSpy).toHaveBeenCalledTimes(2);
  });
});
