import { describe, it, expect } from 'vitest';
import {
  DID_CKB_COLLECTION_ID,
  DOTBIT_COLLECTION_ID,
  getMnftClassDetailHref,
  isDidCkbCollectionAlias,
  isDotbitCollectionAlias,
  resolveObjectRouteTarget,
} from '@/lib/detail-routes';

const SPORE_ID = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
const MNFT_CLASS_ID = '0x1234567890abcdef1234567890abcdef1234567890abcdef';

describe('resolveObjectRouteTarget', () => {
  it('classifies a 32-byte identifier as a spore object lookup', () => {
    expect(resolveObjectRouteTarget(SPORE_ID)).toEqual({
      kind: 'spore-object',
      assetId: SPORE_ID,
    });
  });

  it('routes a 24-byte mNFT class ID to the class page', () => {
    // The class ID is 20 bytes of issuer ID plus a 4-byte class index and is
    // never a spore object ID, so the destination is decided by the identifier
    // itself — no endpoint probe, no error-code guessing.
    expect(resolveObjectRouteTarget(MNFT_CLASS_ID)).toEqual({
      kind: 'redirect',
      href: getMnftClassDetailHref(MNFT_CLASS_ID),
    });
  });

  it('accepts identifiers without the 0x prefix and in mixed case', () => {
    expect(resolveObjectRouteTarget(MNFT_CLASS_ID.slice(2).toUpperCase())).toEqual({
      kind: 'redirect',
      href: getMnftClassDetailHref(MNFT_CLASS_ID),
    });
    expect(resolveObjectRouteTarget(SPORE_ID.slice(2))).toEqual({
      kind: 'spore-object',
      assetId: SPORE_ID.slice(2),
    });
  });

  it('routes identity collection aliases and sentinels to the identity pages', () => {
    for (const alias of ['dotbit', '.bit', 'DotBit', DOTBIT_COLLECTION_ID]) {
      expect(resolveObjectRouteTarget(alias)).toEqual({
        kind: 'redirect',
        href: '/identities/dotbit',
      });
    }
    for (const alias of ['did:ckb', 'did_ckb', DID_CKB_COLLECTION_ID]) {
      expect(resolveObjectRouteTarget(alias)).toEqual({
        kind: 'redirect',
        href: '/identities/did:ckb',
      });
    }
  });

  it('reports identifiers of no known object class as unroutable', () => {
    // 20 bytes (.bit account), 28 bytes (mNFT token) and non-hex junk are all
    // handled by other routes or by nothing at all.
    expect(resolveObjectRouteTarget('0x1234567890abcdef12345678')).toEqual({ kind: 'unroutable' });
    expect(resolveObjectRouteTarget(`${MNFT_CLASS_ID}00000000`)).toEqual({ kind: 'unroutable' });
    expect(resolveObjectRouteTarget('0xnothex')).toEqual({ kind: 'unroutable' });
    expect(resolveObjectRouteTarget('0x123')).toEqual({ kind: 'unroutable' });
    expect(resolveObjectRouteTarget('')).toEqual({ kind: 'unroutable' });
  });
});

describe('identity collection aliases', () => {
  it('recognises every spelling the explorer links with', () => {
    expect(isDotbitCollectionAlias('.BIT')).toBe(true);
    expect(isDotbitCollectionAlias(DOTBIT_COLLECTION_ID.toUpperCase())).toBe(true);
    expect(isDotbitCollectionAlias(SPORE_ID)).toBe(false);
    expect(isDidCkbCollectionAlias('DID_CKB')).toBe(true);
    expect(isDidCkbCollectionAlias(DID_CKB_COLLECTION_ID)).toBe(true);
    expect(isDidCkbCollectionAlias(SPORE_ID)).toBe(false);
  });
});
