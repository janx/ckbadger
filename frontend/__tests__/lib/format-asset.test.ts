import { describe, it, expect } from 'vitest';
import {
  formatTokenBalance,
  formatAssetAmount,
  getAssetLabel,
  getAssetBadgeVariant,
} from '@/lib/format-asset';
import type { AssetTransfer } from '@/lib/api';

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

  it('handles null decimals as 0', () => {
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
