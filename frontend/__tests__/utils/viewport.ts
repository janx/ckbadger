import { vi } from 'vitest';

/**
 * A viewport below or from md for useFromMd (usePageSize's R2, and what else follows md): answers
 * `(min-width: 768px)` with `fromMd`. Restore with vi.unstubAllGlobals().
 */
/**
 * A viewport `width` px wide, for every width query setup.ts answers from `window.innerWidth`
 * (jsdom's own is 1024, a desktop): a phone row drawn on a phone is tested at a phone's viewport
 * (390) as well as in a phone's container (atContainerWidth(358)), since an id's size follows the
 * viewport (useTightBelowSm, R9). Restore with vi.unstubAllGlobals().
 */
export function atViewportWidth(width: number): void {
  vi.stubGlobal('innerWidth', width);
}

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
