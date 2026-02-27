import { describe, expect, it } from 'vitest';
import {
  DID_CKB_COLLECTION_ID,
  DOTBIT_COLLECTION_ID,
  isDidCkbAlias,
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

  it('normalizes did:ckb aliases to the sentinel collection id', () => {
    expect(normalizeNftAssetId('did:ckb')).toBe(DID_CKB_COLLECTION_ID);
    expect(normalizeNftAssetId('did_ckb')).toBe(DID_CKB_COLLECTION_ID);
  });

  it('detects did:ckb aliases and sentinel id', () => {
    expect(isDidCkbAlias('did:ckb')).toBe(true);
    expect(isDidCkbAlias('DID_CKB')).toBe(true);
    expect(isDidCkbAlias(DID_CKB_COLLECTION_ID)).toBe(true);
    expect(isDidCkbAlias('0x1234')).toBe(false);
  });

  it('maps dotbit assets to dotbit slug', () => {
    expect(toNftDetailSlug(DOTBIT_COLLECTION_ID, 'dotbit')).toBe('dotbit');
    expect(toNftDetailSlug(DOTBIT_COLLECTION_ID)).toBe('dotbit');
  });

  it('maps did:ckb assets to did:ckb slug', () => {
    expect(toNftDetailSlug(DID_CKB_COLLECTION_ID, 'did_ckb')).toBe('did:ckb');
    expect(toNftDetailSlug(DID_CKB_COLLECTION_ID)).toBe('did:ckb');
  });

  it('keeps non-dotbit assets as original slug', () => {
    const id = '0xabcdef';
    expect(toNftDetailSlug(id, 'm-nft')).toBe(id);
  });
});
