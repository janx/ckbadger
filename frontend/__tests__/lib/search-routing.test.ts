import { describe, expect, it } from 'vitest';
import { resolveSearchRoute } from '@/lib/search-routing';

describe('resolveSearchRoute', () => {
  it('routes numeric input to block detail', () => {
    expect(resolveSearchRoute('12345')).toBe('/blocks/12345');
  });

  it('routes 0x64-byte hash to transaction detail', () => {
    const hash = `0x${'a'.repeat(64)}`;
    expect(resolveSearchRoute(hash)).toBe(`/tx/${hash}`);
  });

  it('routes ckb addresses to address detail', () => {
    expect(resolveSearchRoute('ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp5w65')).toBe(
      '/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp5w65'
    );
  });

  it('routes outpoint-like values to cell detail', () => {
    expect(resolveSearchRoute('0xabc-0x1')).toBe('/cell/0xabc-0x1');
  });

  it('falls back to search page for unknown query', () => {
    expect(resolveSearchRoute('hello world')).toBe('/search?q=hello%20world');
  });
});
