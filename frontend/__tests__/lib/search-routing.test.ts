import { describe, expect, it } from 'vitest';
import { resolveSearchRoute } from '@/lib/search-routing';

describe('resolveSearchRoute', () => {
  it('routes numeric input to block detail', () => {
    expect(resolveSearchRoute('12345')).toBe('/blocks/12345');
  });

  it('does not force ambiguous 32-byte hash to transaction detail', () => {
    const hash = `0x${'a'.repeat(64)}`;
    expect(resolveSearchRoute(hash)).toBeNull();
  });

  it('routes ckb addresses to address detail', () => {
    expect(resolveSearchRoute('ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp5w65')).toBe(
      '/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp5w65'
    );
  });

  it('normalizes outpoint with alternate delimiters and hex output index', () => {
    const hash = `0x${'b'.repeat(64)}`;
    expect(resolveSearchRoute(`${hash}:0x1`)).toBe(`/cell/${hash}-1`);
    expect(resolveSearchRoute(`${hash}#1`)).toBe(`/cell/${hash}-1`);
  });

  it('supports explicit tx prefix to disambiguate hash', () => {
    const hash = `0x${'c'.repeat(64)}`;
    expect(resolveSearchRoute(`tx:${hash}`)).toBe(`/tx/${hash}`);
  });

  it('routes did:ckb aliases to did collection detail', () => {
    expect(resolveSearchRoute('did:ckb')).toBe('/nfts/did:ckb');
    expect(resolveSearchRoute('DID_CKB')).toBe('/nfts/did:ckb');
  });

  it('returns null for unknown query', () => {
    expect(resolveSearchRoute('hello world')).toBeNull();
  });
});
