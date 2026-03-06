import { describe, expect, it } from 'vitest';
import pkg from '@/package.json';
import * as navigation from '@/src/navigation';

describe('tooling config', () => {
  it('uses vite for dev and build scripts', () => {
    expect(pkg.scripts.dev).toMatch(/^vite/);
    expect(pkg.scripts.build).toMatch(/^vite build/);
  });

  it('exposes a canonical local navigation module', () => {
    expect(typeof navigation.useRouter).toBe('function');
    expect(typeof navigation.usePathname).toBe('function');
    expect(typeof navigation.useSearchParams).toBe('function');
    expect(typeof navigation.redirect).toBe('function');
    expect(typeof navigation.notFound).toBe('function');
  });
});
