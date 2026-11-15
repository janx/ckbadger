import { describe, expect, it } from 'vitest';

import { normalizeAssetId, parseActivityCursor, formatCompositionTier } from '@/lib/asset-utils';

describe('normalizeAssetId', () => {
  it('returns input unchanged when already prefixed', () => {
    expect(normalizeAssetId('0xabc')).toBe('0xabc');
  });

  it('adds 0x prefix when missing', () => {
    expect(normalizeAssetId('abc')).toBe('0xabc');
  });

  it('decodes URI-encoded input', () => {
    expect(normalizeAssetId('0x%20abc')).toBe('0x abc');
  });
});

describe('parseActivityCursor', () => {
  it('returns undefined for null', () => {
    expect(parseActivityCursor(null)).toBeUndefined();
  });

  it('returns undefined for empty string', () => {
    expect(parseActivityCursor('')).toBeUndefined();
  });

  it('returns undefined for whitespace-only string', () => {
    expect(parseActivityCursor('   ')).toBeUndefined();
  });

  it('returns trimmed cursor', () => {
    expect(parseActivityCursor('500:0')).toBe('500:0');
  });
});

describe('formatCompositionTier', () => {
  it('keeps DOB decode pending distinct from unknown or pure storage', () => {
    expect(formatCompositionTier('decode_pending')).toBe('Decode pending');
  });
  it('formats btc_ckb', () => {
    expect(formatCompositionTier('btc_ckb')).toBe('BTC+CKB');
  });

  it('formats decentralized_mixture', () => {
    expect(formatCompositionTier('decentralized_mixture')).toBe('Decentralized mix');
  });

  it('formats centralized_mixture', () => {
    expect(formatCompositionTier('centralized_mixture')).toBe('Centralized mix');
  });

  it('returns Unknown for unrecognized tier', () => {
    expect(formatCompositionTier('unknown')).toBe('Unknown');
    expect(formatCompositionTier('something_else')).toBe('Unknown');
  });
});
