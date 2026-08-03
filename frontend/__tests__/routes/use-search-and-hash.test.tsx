import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
// Import from the REAL module directly: __tests__/setup.ts mocks the barrel
// `@/src/navigation`, so importing `@/src/next-compat/navigation` here bypasses
// that stub and exercises the actual implementation.
import { useSearchAndHash } from '@/src/next-compat/navigation';

function SearchAndHashProbe() {
  return <div data-testid="suffix">{useSearchAndHash()}</div>;
}

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <SearchAndHashProbe />
    </MemoryRouter>
  );
}

describe('useSearchAndHash', () => {
  it('returns the raw search and hash of the current location', () => {
    renderAt('/mainnet/activities?type=dao#row-7');
    expect(screen.getByTestId('suffix').textContent).toBe('?type=dao#row-7');
  });

  it('returns an empty string when the location carries neither', () => {
    renderAt('/mainnet/activities');
    expect(screen.getByTestId('suffix').textContent).toBe('');
  });

  it('falls back to the window location outside a router', () => {
    window.history.replaceState({}, '', '/mainnet/dao?a=1#b');
    render(<SearchAndHashProbe />);
    expect(screen.getByTestId('suffix').textContent).toBe('?a=1#b');
  });
});
