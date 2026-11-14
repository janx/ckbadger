import { vi } from 'vitest';

/**
 * A viewport below or from md for useFromMd (usePageSize's R2, and what else follows md): answers
 * `(min-width: 768px)` with `fromMd`. Restore with vi.unstubAllGlobals().
 */
export function stubViewportFromMd(fromMd: boolean): void {
  vi.stubGlobal(
    'matchMedia',
    vi.fn((query: string) => ({
      matches: query === '(min-width: 768px)' ? fromMd : false,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    }))
  );
}
