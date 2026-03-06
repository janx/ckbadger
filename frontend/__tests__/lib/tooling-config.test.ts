import { describe, expect, it } from 'vitest';
import pkg from '@/package.json';

describe('tooling config', () => {
  it('uses vite for dev and build scripts', () => {
    expect(pkg.scripts.dev).toMatch(/^vite/);
    expect(pkg.scripts.build).toMatch(/^vite build/);
  });
});
