import type { AssetTransfer } from '@/lib/api';

export interface TokenAmountParts {
  /** Whole-token part, digit-grouped for display. */
  integer: string;
  /** Fractional digits, exactly `decimals` wide; empty when there are none. */
  fraction: string;
}

/**
 * The single scaling path for base-unit token amounts: every display of a token
 * amount derives from this split, so the divisor is computed in exactly one
 * place.
 *
 * `decimals: null` means the token's decimals are unknown (no label, no
 * on-chain info cell): the raw base-unit integer is returned unscaled — callers
 * annotate the display, never assume 0.
 *
 * The divisor is exponentiated in BigInt, never `BigInt(10 ** decimals)`:
 * `10 ** 23` and beyond are not representable as JS doubles (`10 ** 24` is
 * 999999999999999983222784), and `decimals` is the raw, unvalidated first byte
 * of the xUDT Unique Cell, so an issuer can legitimately declare 23 or more.
 */
export function splitTokenAmount(amount: string, decimals: number | null): TokenAmountParts {
  const value = BigInt(amount);
  if (decimals == null || decimals === 0) {
    return { integer: value.toLocaleString(), fraction: '' };
  }
  const divisor = BigInt(10) ** BigInt(decimals);
  return {
    integer: (value / divisor).toLocaleString(),
    fraction: (value % divisor).toString().padStart(decimals, '0'),
  };
}

/**
 * Format a token balance for display, trimming trailing fraction zeros.
 * `decimals: null` means the token's decimals are unknown: the raw base-unit
 * integer is returned — callers annotate the display, never assume 0.
 */
export function formatTokenBalance(balance: string, decimals: number | null): string {
  const { integer, fraction } = splitTokenAmount(balance, decimals);
  const trimmedFraction = fraction.replace(/0+$/, '');
  return trimmedFraction === '' ? integer : `${integer}.${trimmedFraction}`;
}

/** Hover text for amounts rendered from a token whose decimals are unknown. */
export const RAW_AMOUNT_TITLE = 'Token decimals unknown \u2014 raw base-unit amount';

/**
 * Format a token balance and append an explicit " (raw)" marker when the
 * token's decimals are unknown, so an unscaled base-unit amount can never be
 * mistaken for a real 0-decimals amount.
 */
export function formatTokenBalanceWithRawMarker(balance: string, decimals: number | null): string {
  const base = formatTokenBalance(balance, decimals);
  return decimals == null ? `${base} (raw)` : base;
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
