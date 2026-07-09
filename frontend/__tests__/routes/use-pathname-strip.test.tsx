import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
// Import from the REAL module directly: __tests__/setup.ts mocks the barrel
// `@/src/navigation`, so importing `@/src/next-compat/navigation` here bypasses
// that stub and exercises the actual usePathname implementation.
import { usePathname } from '@/src/next-compat/navigation';

function PathnameProbe() {
  return <div data-testid="pathname">{usePathname()}</div>;
}

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <PathnameProbe />
    </MemoryRouter>
  );
}

describe('usePathname network-prefix stripping', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('strips a leading known-network segment', () => {
    renderAt('/testnet/dao');
    expect(screen.getByTestId('pathname')).toHaveTextContent('/dao');
  });

  it('returns "/" when the path is only the network segment', () => {
    renderAt('/mainnet');
    expect(screen.getByTestId('pathname').textContent).toBe('/');
  });

  it('leaves an un-prefixed path unchanged', () => {
    renderAt('/dao');
    expect(screen.getByTestId('pathname')).toHaveTextContent('/dao');
  });

  it('does not strip an unknown first segment', () => {
    renderAt('/devnet/dao');
    expect(screen.getByTestId('pathname')).toHaveTextContent('/devnet/dao');
  });
});
