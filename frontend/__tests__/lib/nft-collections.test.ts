import { describe, expect, it } from 'vitest';
import {
  DOTBIT_COLLECTION_ID,
  isDotbitAlias,
  normalizeNftAssetId,
  toNftDetailSlug,
} from '@/lib/nft-collections';

describe('nft collection id mapping', () => {
  it('normalizes dotbit aliases to the sentinel collection id', () => {
    expect(normalizeNftAssetId('dotbit')).toBe(DOTBIT_COLLECTION_ID);
    expect(normalizeNftAssetId('.bit')).toBe(DOTBIT_COLLECTION_ID);
  });

  it('detects dotbit aliases and sentinel id', () => {
    expect(isDotbitAlias('dotbit')).toBe(true);
    expect(isDotbitAlias('DOTBIT')).toBe(true);
    expect(isDotbitAlias('.bit')).toBe(true);
    expect(isDotbitAlias(DOTBIT_COLLECTION_ID)).toBe(true);
    expect(isDotbitAlias('0x1234')).toBe(false);
  });

  it('keeps non-dotbit ids unchanged', () => {
    const id = '0x1234';
    expect(normalizeNftAssetId(id)).toBe(id);
  });

  it('maps dotbit assets to dotbit slug', () => {
    expect(toNftDetailSlug(DOTBIT_COLLECTION_ID, 'dotbit')).toBe('dotbit');
    expect(toNftDetailSlug(DOTBIT_COLLECTION_ID)).toBe('dotbit');
  });

  it('keeps non-dotbit assets as original slug', () => {
    const id = '0xabcdef';
    expect(toNftDetailSlug(id, 'm-nft')).toBe(id);
  });
});
