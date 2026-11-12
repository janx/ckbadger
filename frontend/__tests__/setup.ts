import '@testing-library/jest-dom';
import { configure } from '@testing-library/dom';
import { vi } from 'vitest';
import { server } from './msw/server';

// How long a findBy* / waitFor waits before it fails: 10 s, not Testing Library's default 1 s.
// Measured with two full suites running at once (load average 50-60 on 24 cores, 7,212 waits):
// every wait resolved, p99 1.6 s, p99.9 2.9 s, the slowest 4.6 s, and 260 waits took longer than
// the old 1 s. 10 s is about twice the slowest; a wait that never resolves costs that and fails
// with Testing Library's own message and DOM. It stays below the 15 s testTimeout
// (vitest.config.mts), which covers a whole test: its waits and its rendering. No test waits for
// something meant not to arrive, so no passing test spends the cap.
configure({ asyncUtilTimeout: 10_000 });

// jsdom logs a not-implemented error even when components correctly handle a null context.
if (typeof HTMLCanvasElement !== 'undefined') {
  HTMLCanvasElement.prototype.getContext = vi.fn(
    () => null
  ) as unknown as typeof HTMLCanvasElement.prototype.getContext;
}

// jsdom does not implement IntersectionObserver; provide a minimal stub.
if (typeof globalThis.IntersectionObserver === 'undefined') {
  globalThis.IntersectionObserver = class IntersectionObserver {
    readonly root = null;
    readonly rootMargin = '';
    readonly thresholds: readonly number[] = [];
    constructor(_cb: IntersectionObserverCallback, _options?: IntersectionObserverInit) {}
    disconnect() {}
    observe() {}
    takeRecords(): IntersectionObserverEntry[] {
      return [];
    }
    unobserve() {}
  } as unknown as typeof globalThis.IntersectionObserver;
}

// jsdom does not implement ResizeObserver either; a stub that never reports. Tests that need a
// measured size stub their own (see __tests__/utils/chart-width.ts).
if (typeof globalThis.ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class ResizeObserver {
    constructor(_cb: ResizeObserverCallback) {}
    disconnect() {}
    observe() {}
    unobserve() {}
  } as unknown as typeof globalThis.ResizeObserver;
}

// jsdom has no matchMedia. Answer width queries from window.innerWidth (jsdom: 1024, a desktop,
// so lists keep their own page size), reduced motion as not asked for and the primary pointer as a
// mouse; any other query is a test that must bring its own stub. Tests that need another answer
// stub their own.
if (typeof window !== 'undefined' && typeof window.matchMedia === 'undefined') {
  window.matchMedia = ((query: string) => {
    const width = /^\((min|max)-width:\s*(\d+)px\)$/.exec(query);
    let matches: boolean;
    if (width)
      matches =
        width[1] === 'min'
          ? window.innerWidth >= Number(width[2])
          : window.innerWidth <= Number(width[2]);
    else if (query === '(prefers-reduced-motion: reduce)') matches = false;
    // a mouse, not a touch screen (C11: a pinned reading is "selected", not "tapped")
    else if (query === '(pointer: coarse)') matches = false;
    // the OS scheme is dark (lib/theme.ts asks for light); a test of `system` stubs its own
    else if (query === '(prefers-color-scheme: light)') matches = false;
    else if (query === '(prefers-color-scheme: dark)') matches = true;
    else throw new Error(`setup.ts matchMedia: unsupported query ${query}`);
    return {
      matches,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    } as MediaQueryList;
  }) as typeof window.matchMedia;
}

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

vi.mock('@/src/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    refresh: vi.fn(),
    prefetch: vi.fn(),
  }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/',
  useParams: () => ({}),
}));
