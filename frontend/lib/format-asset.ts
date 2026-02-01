import type { AssetTransfer } from '@/lib/api';
import type { Activity, ActivityType } from '@/types/activity';
import { formatCkbAmount } from '@/lib/utils';

export function formatTokenBalance(balance: string, decimals: number): string {
  if (decimals === 0) return BigInt(balance).toLocaleString();
  const balanceBigInt = BigInt(balance);
  const divisor = BigInt(10 ** decimals);
  const wholePart = balanceBigInt / divisor;
  const fractionalPart = balanceBigInt % divisor;
  const fractionalStr = fractionalPart.toString().padStart(decimals, '0');
  const trimmedFractional = fractionalStr.replace(/0+$/, '');
  if (trimmedFractional === '') {
    return wholePart.toLocaleString();
  }
  return `${wholePart.toLocaleString()}.${trimmedFractional}`;
}

export function formatAssetAmount(transfer: AssetTransfer): string {
  if (!transfer.amount) return '1';
  const decimals = transfer.tokenDecimals ?? 0;
  return formatTokenBalance(transfer.amount, decimals);
}

export function getAssetLabel(transfer: AssetTransfer): string {
  if (transfer.tokenSymbol) return transfer.tokenSymbol;
  if (transfer.tokenName) return transfer.tokenName;
  switch (transfer.assetType) {
    case 'spore':
      return 'Spore';
    case 'dob/0':
    case 'dob/1':
      return 'DOB';
    case 'mnft':
      return 'M-NFT';
    case 'dotbit':
      return '.bit';
    case 'dao':
      return 'DAO';
    default:
      return transfer.assetType.toUpperCase();
  }
}

export function getAssetBadgeVariant(
  category: string
): 'green' | 'amber' | 'red' | 'gray' | 'purple' {
  switch (category) {
    case 'token':
      return 'amber';
    case 'dob':
      return 'purple';
    case 'nft':
      return 'green';
    case 'dao':
      return 'gray';
    default:
      return 'gray';
  }
}

export function getActivityLabel(activityType: ActivityType): string {
  const labels: Record<ActivityType, string> = {
    CKB_TRANSFER: 'CKB Transfer',
    CELLBASE_REWARD: 'Mining Reward',
    TOKEN_MINT: 'Token Mint',
    TOKEN_TRANSFER: 'Token Transfer',
    TOKEN_BURN: 'Token Burn',
    DOB_MINT: 'DOB Mint',
    DOB_TRANSFER: 'DOB Transfer',
    DOB_BURN: 'DOB Burn',
    NFT_MINT: 'NFT Mint',
    NFT_TRANSFER: 'NFT Transfer',
    DAO_DEPOSIT: 'DAO Deposit',
    DAO_WITHDRAW_REQUEST: 'DAO Withdraw Request',
    DAO_WITHDRAW_COMPLETE: 'DAO Withdraw Complete',
    SCRIPT_DEPLOY: 'Script Deploy',
    RGBPP_TRANSFER: 'RGB++ Transfer',
    RGBPP_LEAP_IN: 'RGB++ Leap In',
    RGBPP_LEAP_OUT: 'RGB++ Leap Out',
    RGBPP_ISSUANCE: 'RGB++ Issuance',
  };
  return labels[activityType] || 'Activity';
}

export function formatActivityAmount(activity: Activity): string | null {
  if (!activity.amount || activity.amount === '0') return null;

  const isCkbActivity =
    activity.activityCategory === 'ckb' ||
    activity.activityCategory === 'cellbase' ||
    activity.activityCategory === 'dao';

  if (isCkbActivity) {
    const ckb = formatCkbAmount(activity.amount);
    return `${ckb.full} CKB`;
  }

  const metadata = activity.metadata as {
    symbol?: string;
    decimals?: number;
    tokenName?: string;
  };

  if (metadata.decimals !== undefined) {
    const decimals = metadata.decimals;
    const rawAmount = BigInt(activity.amount);
    const divisor = BigInt(10 ** decimals);
    const whole = rawAmount / divisor;
    const fraction = rawAmount % divisor;
    const formatted =
      decimals > 0
        ? `${whole.toLocaleString()}.${fraction.toString().padStart(decimals, '0').slice(0, 4).replace(/0+$/, '') || '0'}`
        : whole.toLocaleString();

    const symbol = metadata.symbol || metadata.tokenName || '';
    return symbol ? `${formatted} ${symbol}` : formatted;
  }

  return BigInt(activity.amount).toLocaleString();
}
