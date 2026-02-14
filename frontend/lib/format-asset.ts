import type { AssetTransfer } from '@/lib/api';

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
