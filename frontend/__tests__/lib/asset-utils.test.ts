import { describe, expect, it } from 'vitest';

import {
  normalizeAssetId,
  parseActivityCursor,
  formatActivityTimestamp,
  normalizeActivityAction,
  formatStorageTier,
  formatExpiry,
} from '@/lib/asset-utils';

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

describe('formatActivityTimestamp', () => {
  it('parses millisecond-level numeric timestamps', () => {
    const result = formatActivityTimestamp('1700000300000');
    expect(result).toContain('2023');
  });

  it('parses second-level numeric timestamps', () => {
    const result = formatActivityTimestamp('1700000300');
    expect(result).toContain('2023');
  });

  it('parses ISO date strings', () => {
    const result = formatActivityTimestamp('2023-01-01T00:00:00.000Z');
    expect(result).toContain('2023');
  });

  it('returns input unchanged for non-parseable values', () => {
    expect(formatActivityTimestamp('not-a-date')).toBe('not-a-date');
  });
});

describe('normalizeActivityAction', () => {
  it('normalizes burn to recycled', () => {
    expect(normalizeActivityAction('burn')).toBe('recycled');
  });

  it('normalizes Burn (case-insensitive) to recycled', () => {
    expect(normalizeActivityAction('Burn')).toBe('recycled');
  });

  it('lowercases other actions', () => {
    expect(normalizeActivityAction('Transfer')).toBe('transfer');
  });
});

describe('formatStorageTier', () => {
  it('formats fully_on_ckb_and_btc', () => {
    expect(formatStorageTier('fully_on_ckb_and_btc')).toBe('Fully on Bitcoin+CKB');
  });

  it('formats decentralized_external as merged offchain label', () => {
    expect(formatStorageTier('decentralized_external')).toBe('Offchain Dependent');
  });

  it('formats centralized_dependent as merged offchain label', () => {
    expect(formatStorageTier('centralized_dependent')).toBe('Offchain Dependent');
  });

  it('formats offchain_dependent', () => {
    expect(formatStorageTier('offchain_dependent')).toBe('Offchain Dependent');
  });

  it('returns Unknown for unrecognized tier', () => {
    expect(formatStorageTier('unknown')).toBe('Unknown');
    expect(formatStorageTier('something_else')).toBe('Unknown');
  });
});

describe('formatExpiry', () => {
  it('returns Not available for null', () => {
    expect(formatExpiry(null)).toBe('Not available');
  });

  it('returns Not available for undefined', () => {
    expect(formatExpiry(undefined)).toBe('Not available');
  });

  it('returns Not available for zero', () => {
    expect(formatExpiry(0)).toBe('Not available');
  });

  it('formats a valid Unix timestamp', () => {
    const result = formatExpiry(1800000000);
    expect(result).toContain('2027');
  });
});
