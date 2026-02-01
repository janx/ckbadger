import { describe, it, expect } from 'vitest';
import {
  formatTokenBalance,
  formatAssetAmount,
  getAssetLabel,
  getAssetBadgeVariant,
  getActivityLabel,
  formatActivityAmount,
} from '@/lib/format-asset';
import type { AssetTransfer } from '@/lib/api';
import type { Activity } from '@/types/activity';

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
    expect(getAssetBadgeVariant('token')).toBe('amber');
    expect(getAssetBadgeVariant('dob')).toBe('purple');
    expect(getAssetBadgeVariant('nft')).toBe('green');
    expect(getAssetBadgeVariant('dao')).toBe('gray');
    expect(getAssetBadgeVariant('unknown')).toBe('gray');
  });
});

describe('getActivityLabel', () => {
  it('returns labels for known activity types', () => {
    expect(getActivityLabel('CKB_TRANSFER')).toBe('CKB Transfer');
    expect(getActivityLabel('CELLBASE_REWARD')).toBe('Mining Reward');
    expect(getActivityLabel('TOKEN_MINT')).toBe('Token Mint');
    expect(getActivityLabel('DAO_DEPOSIT')).toBe('DAO Deposit');
    expect(getActivityLabel('SCRIPT_DEPLOY')).toBe('Script Deploy');
  });

  it('returns "Activity" for unknown types', () => {
    expect(getActivityLabel('UNKNOWN_TYPE' as never)).toBe('Activity');
  });
});

describe('formatActivityAmount', () => {
  it('returns null for zero or missing amount', () => {
    expect(formatActivityAmount({ amount: '0' } as Activity)).toBeNull();
    expect(formatActivityAmount({ amount: null } as unknown as Activity)).toBeNull();
  });

  it('formats CKB activities with CKB suffix', () => {
    const activity = {
      amount: '10000000000',
      activityCategory: 'ckb',
    } as Activity;
    expect(formatActivityAmount(activity)).toBe('100.00000000 CKB');
  });

  it('formats DAO activities with CKB suffix', () => {
    const activity = {
      amount: '10000000000',
      activityCategory: 'dao',
    } as Activity;
    expect(formatActivityAmount(activity)).toBe('100.00000000 CKB');
  });

  it('formats token activities with decimals and symbol', () => {
    const activity = {
      amount: '150000000',
      activityCategory: 'token',
      metadata: { decimals: 8, symbol: 'SEAL' },
    } as unknown as Activity;
    expect(formatActivityAmount(activity)).toBe('1.5 SEAL');
  });

  it('formats amounts without decimals as integers', () => {
    const activity = {
      amount: '1000',
      activityCategory: 'nft',
      metadata: {},
    } as Activity;
    expect(formatActivityAmount(activity)).toBe('1,000');
  });
});
