import { describe, it, expect } from 'vitest';
import {
  formatTokenBalance,
  formatAssetAmount,
  getAssetLabel,
  getAssetBadgeVariant,
  formatTokenBalanceWithRawMarker,
  splitTokenAmount,
} from '@/lib/format-asset';
import type { AssetTransfer } from '@/lib/api';

// 10 ** 24 is not representable as a JS double: the nearest double is
// 999999999999999983222784, so a `BigInt(10 ** decimals)` divisor is wrong for
// every decimals >= 23. xUDT decimals are the raw, unvalidated first byte of
// the Unique Cell (crates/indexer/src/parser/token_helpers.rs), so an issuer
// can legitimately declare 23 or more and the divisor must be exact.
const ONE_TOKEN_24_DECIMALS = '1000000000000000000000000';

describe('formatTokenBalance', () => {
  it('formats integer amounts (0 decimals)', () => {
    expect(formatTokenBalance('1000000', 0)).toBe('1,000,000');
  });

  it('formats amounts with decimals', () => {
    expect(formatTokenBalance('123456789', 8)).toBe('1.23456789');
  });

  it('trims trailing zeros', () => {
    expect(formatTokenBalance('100000000', 8)).toBe('1');
    expect(formatTokenBalance('150000000', 8)).toBe('1.5');
  });

  it('handles small amounts', () => {
    expect(formatTokenBalance('1', 8)).toBe('0.00000001');
  });

  it('renders null decimals as the raw base-unit amount', () => {
    // B5: unknown decimals must never be flattened to 0; the raw integer is
    // shown (annotation is applied by the UI at the display site).
    expect(formatTokenBalance('123456789', null)).toBe('123,456,789');
  });

  it('scales exactly for decimals beyond double precision', () => {
    // With BigInt(10 ** 24) this rendered "1.000000000000000016777216".
    expect(formatTokenBalance(ONE_TOKEN_24_DECIMALS, 24)).toBe('1');
    expect(formatTokenBalance('1500000000000000000000000', 24)).toBe('1.5');
    expect(formatTokenBalance('1', 24)).toBe('0.000000000000000000000001');
  });
});

describe('splitTokenAmount', () => {
  it('splits into grouped integer and fixed-width fraction', () => {
    expect(splitTokenAmount('123456789', 8)).toEqual({ integer: '1', fraction: '23456789' });
    expect(splitTokenAmount('123456789012345678', 8)).toEqual({
      integer: '1,234,567,890',
      fraction: '12345678',
    });
  });

  it('keeps trailing fraction zeros (full declared precision)', () => {
    expect(splitTokenAmount('100000000', 8)).toEqual({ integer: '1', fraction: '00000000' });
  });

  it('emits no fraction for 0 or unknown decimals', () => {
    expect(splitTokenAmount('1000000', 0)).toEqual({ integer: '1,000,000', fraction: '' });
    expect(splitTokenAmount('1000000', null)).toEqual({ integer: '1,000,000', fraction: '' });
  });

  it('scales exactly for decimals beyond double precision', () => {
    expect(splitTokenAmount(ONE_TOKEN_24_DECIMALS, 24)).toEqual({
      integer: '1',
      fraction: '000000000000000000000000',
    });
  });
});

describe('formatAssetAmount', () => {
  it('returns "1" for transfers without amount', () => {
    const transfer = { assetType: 'spore' } as AssetTransfer;
    expect(formatAssetAmount(transfer)).toBe('1');
  });

  it('formats amounts with token decimals', () => {
    const transfer = {
      amount: '150000000',
      tokenDecimals: 8,
      assetType: 'xudt',
    } as AssetTransfer;
    expect(formatAssetAmount(transfer)).toBe('1.5');
  });

  it('renders null decimals as the raw base-unit amount', () => {
    const transfer = {
      amount: '1000',
      tokenDecimals: null,
      assetType: 'xudt',
    } as unknown as AssetTransfer;
    expect(formatAssetAmount(transfer)).toBe('1,000');
  });
});

describe('getAssetLabel', () => {
  it('prefers token symbol', () => {
    const transfer = {
      tokenSymbol: 'SEAL',
      tokenName: 'Seal Token',
      assetType: 'xudt',
    } as AssetTransfer;
    expect(getAssetLabel(transfer)).toBe('SEAL');
  });

  it('falls back to token name', () => {
    const transfer = {
      tokenName: 'Seal Token',
      assetType: 'xudt',
    } as AssetTransfer;
    expect(getAssetLabel(transfer)).toBe('Seal Token');
  });

  it('returns asset type label for known types', () => {
    expect(getAssetLabel({ assetType: 'spore' } as AssetTransfer)).toBe('Spore');
    expect(getAssetLabel({ assetType: 'dob/0' } as AssetTransfer)).toBe('DOB');
    expect(getAssetLabel({ assetType: 'dob/1' } as AssetTransfer)).toBe('DOB');
    expect(getAssetLabel({ assetType: 'mnft' } as AssetTransfer)).toBe('M-NFT');
    expect(getAssetLabel({ assetType: 'dotbit' } as AssetTransfer)).toBe('.bit');
    expect(getAssetLabel({ assetType: 'dao' } as AssetTransfer)).toBe('DAO');
  });

  it('uppercases unknown types', () => {
    expect(getAssetLabel({ assetType: 'unknown' } as AssetTransfer)).toBe('UNKNOWN');
  });
});

describe('getAssetBadgeVariant', () => {
  it('returns correct variants for categories', () => {
    expect(getAssetBadgeVariant('token')).toBe('gold');
    expect(getAssetBadgeVariant('object')).toBe('purple');
    expect(getAssetBadgeVariant('identity')).toBe('blue');
    expect(getAssetBadgeVariant('dao')).toBe('gray');
    expect(getAssetBadgeVariant('unknown')).toBe('gray');
  });
});

describe('formatTokenBalanceWithRawMarker', () => {
  it('appends (raw) when decimals are unknown', () => {
    expect(formatTokenBalanceWithRawMarker('12345', null)).toBe('12,345 (raw)');
  });

  it('does not mark true 0-decimals amounts', () => {
    expect(formatTokenBalanceWithRawMarker('12345', 0)).toBe('12,345');
  });

  it('scales known decimals without a marker', () => {
    expect(formatTokenBalanceWithRawMarker('12345', 2)).toBe('123.45');
  });

  it('scales exactly for decimals beyond double precision', () => {
    expect(formatTokenBalanceWithRawMarker(ONE_TOKEN_24_DECIMALS, 24)).toBe('1');
  });
});
