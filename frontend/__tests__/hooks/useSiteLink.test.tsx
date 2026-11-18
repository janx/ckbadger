import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, render, renderHook, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import Link, { useSiteLink } from '@/components/ui/link';

// Lane K: the one place an href becomes a URL on this site and reaches the router. Link follows its
// href through it, and so does the pipeline canvas, which opens a page on a click with no anchor.
describe('useSiteLink', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });
  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  const at =
    (path: string) =>
    ({ children }: { children: ReactNode }) => (
      <MemoryRouter initialEntries={[path]}>{children}</MemoryRouter>
    );

  it('writes an internal href on the network of the router’s location, others unchanged', () => {
    const { result } = renderHook(() => useSiteLink(), { wrapper: at('/testnet/dao') });
    expect(result.current.url('/tx/0xab')).toBe('/testnet/tx/0xab');
    expect(result.current.url('/')).toBe('/testnet');
    expect(result.current.url('/mainnet/blocks/7')).toBe('/mainnet/blocks/7');
    expect(result.current.url('https://example.com/x')).toBe('https://example.com/x');
    expect(result.current.navigator).not.toBeNull();
  });

  it('has no navigator outside a router', () => {
    const { result } = renderHook(() => useSiteLink());
    expect(result.current.navigator).toBeNull();
    expect(result.current.url('/blocks/7')).toBe('/mainnet/blocks/7');
  });

  it('is what Link writes and follows: the same URL, pushed to the same router', () => {
    function Harness() {
      const { url, navigator } = useSiteLink();
      return (
        <>
          <output data-testid="location">{useLocation().pathname}</output>
          <Link href="/blocks/7">block</Link>
          <button type="button" onClick={() => navigator?.push(url('/tx/0xab'))}>
            go
          </button>
          <output data-testid="url">{url('/blocks/7')}</output>
        </>
      );
    }
    render(
      <MemoryRouter initialEntries={['/testnet']}>
        <Harness />
      </MemoryRouter>
    );
    const link = screen.getByRole('link', { name: 'block' });
    expect(link).toHaveAttribute('href', screen.getByTestId('url').textContent as string);
    fireEvent.click(link);
    expect(screen.getByTestId('location')).toHaveTextContent('/testnet/blocks/7');
    fireEvent.click(screen.getByRole('button', { name: 'go' }));
    expect(screen.getByTestId('location')).toHaveTextContent('/testnet/tx/0xab');
  });
});
