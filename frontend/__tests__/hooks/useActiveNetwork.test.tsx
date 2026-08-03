import type { ReactNode } from 'react';
import { renderHook } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';

// Render the hook UNDER a `/:network/*` route so router-native useParams()
// resolves the active-network segment. The hook imports useParams from
// `react-router-dom` directly, so the global `@/src/navigation` useParams mock
// (which returns `{}`) does NOT apply here.
function networkWrapper(initialPath: string) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <MemoryRouter initialEntries={[initialPath]}>
        <Routes>
          <Route path="/:network/*" element={children} />
          <Route path="/" element={children} />
        </Routes>
      </MemoryRouter>
    );
  };
}

describe('useActiveNetwork', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('returns the `:network` segment when browsing testnet', () => {
    const { result } = renderHook(() => useActiveNetwork(), {
      wrapper: networkWrapper('/testnet/blocks'),
    });
    expect(result.current).toBe('testnet');
  });

  it('returns the `:network` segment when browsing mainnet', () => {
    const { result } = renderHook(() => useActiveNetwork(), {
      wrapper: networkWrapper('/mainnet/tx/0xabc'),
    });
    expect(result.current).toBe('mainnet');
  });

  it('falls back to the default network when there is no `:network` segment', () => {
    const { result } = renderHook(() => useActiveNetwork(), {
      wrapper: networkWrapper('/'),
    });
    expect(result.current).toBe('mainnet');
  });
});
