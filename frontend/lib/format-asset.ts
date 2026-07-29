import type { AssetTransfer } from '@/lib/api';

/**
 * Format a token balance for display. `decimals: null` means the token's
 * decimals are unknown (no label, no on-chain info cell): the raw base-unit
 * integer is returned — callers annotate the display, never assume 0.
 */
export function formatTokenBalance(balance: string, decimals: number | null): string {
  if (decimals == null || decimals === 0) return BigInt(balance).toLocaleString();
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
  return formatTokenBalance(transfer.amount, transfer.tokenDecimals ?? null);
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
): 'green' | 'gold' | 'red' | 'gray' | 'purple' | 'blue' {
  switch (category) {
    case 'token':
      return 'gold';
    case 'object':
      return 'purple';
    case 'identity':
      return 'blue';
    case 'dao':
      return 'gray';
    default:
      return 'gray';
  }
}
