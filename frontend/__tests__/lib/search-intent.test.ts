import { describe, expect, it } from 'vitest';
import { normalizeHash32, parseOutpoint, parseSearchIntent } from '@/lib/search-intent';

describe('search-intent', () => {
  it('normalizes 32-byte hashes with or without 0x prefix', () => {
    const hex = 'A'.repeat(64);
    expect(normalizeHash32(hex)).toBe(`0x${'a'.repeat(64)}`);
    expect(normalizeHash32(`0x${hex}`)).toBe(`0x${'a'.repeat(64)}`);
  });

  it('parses prefixed queries', () => {
    const intent = parseSearchIntent(' tx:  0xabc ');
    expect(intent.prefix).toBe('tx');
    expect(intent.body).toBe('0xabc');
  });

  it('parses outpoint with : delimiter and hex index', () => {
    const txHash = `0x${'b'.repeat(64)}`;
    expect(parseOutpoint(`${txHash}:0x2`)).toEqual({
      txHash,
      outputIndex: 2,
      normalized: `${txHash}-2`,
    });
  });

  it('returns null for invalid outpoint', () => {
    expect(parseOutpoint('0xabc-xyz')).toBeNull();
  });
});
