import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';
import { formatScaledInteger } from '@/lib/exact-integer';
import { parseCkbDecimal } from '@/lib/ckb-decimal';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function truncateHash(hash: string, start = 10, end = 8): string {
  if (hash.length <= start + end) return hash;
  return `${hash.slice(0, start)}...${hash.slice(-end)}`;
}

/** Raw block timestamp displayed in the indexer's fixed UTC+8 calendar timezone. */
export function formatUtc8Date(timestamp: string | number): string {
  const value =
    typeof timestamp === 'string' && /^\d+$/.test(timestamp)
      ? Number(timestamp)
      : typeof timestamp === 'number'
        ? timestamp
        : Date.parse(timestamp);
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`Invalid block timestamp ${timestamp}`);
  return new Date(value + 8 * 60 * 60 * 1000).toISOString().slice(0, 10);
}

export function formatTimeAgo(timestamp: string | number | Date): string {
  const date =
    typeof timestamp === 'string' && /^\d+$/.test(timestamp)
      ? new Date(Number(timestamp))
      : new Date(timestamp);
  const now = new Date();
  const seconds = Math.floor((now.getTime() - date.getTime()) / 1000);

  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

export function formatCkbAmount(shannons: string | bigint): {
  integer: string;
  decimal: string;
  full: string;
  isNegative: boolean;
} {
  const value = BigInt(shannons);
  const isNegative = value < BigInt(0);
  const absValue = isNegative ? -value : value;

  const ckbWhole = absValue / BigInt(100_000_000);
  const ckbFraction = absValue % BigInt(100_000_000);

  const integer = ckbWhole.toLocaleString('en-US');
  const decimal = ckbFraction.toString().padStart(8, '0');
  const full = `${isNegative ? '-' : ''}${integer}.${decimal}`;

  return { integer, decimal, full, isNegative };
}

export function formatCkbValue(ckbValue: string): {
  integer: string;
  decimal: string;
  full: string;
  isNegative: boolean;
} {
  return formatCkbAmount(parseCkbDecimal(ckbValue));
}

export function formatCapacity(capacity: string | bigint): string {
  const { full } = formatCkbAmount(capacity);
  return `${full} CKB`;
}

export function formatCkbCompact(shannons: string | bigint): {
  value: string;
  full: string;
} {
  const amount = BigInt(shannons);
  if (amount < BigInt(0)) throw new Error(`negative capacity ${amount} shannons`);
  const full = formatCkbAmount(shannons).full;
  for (const [suffix, divisor] of [
    ['T', BigInt('100000000000000000000')],
    ['B', BigInt('100000000000000000')],
    ['M', BigInt('100000000000000')],
    ['K', BigInt('100000000000')],
  ] as const) {
    if (amount >= divisor)
      return { value: `${formatScaledInteger(amount, divisor, 2)}${suffix}`, full };
  }
  return { value: formatScaledInteger(amount, BigInt(100000000), 2), full };
}

export function formatNumber(num: number | bigint): string {
  return num.toLocaleString();
}

export function formatCompactCkbDelta(ckbDelta: string): {
  compact: string;
  direction: 'up' | 'down' | 'neutral';
} {
  const shannons = parseCkbDecimal(ckbDelta);
  if (shannons === BigInt(0)) return { compact: '0', direction: 'neutral' };

  const direction = shannons > BigInt(0) ? 'up' : 'down';
  const abs = shannons < BigInt(0) ? -shannons : shannons;

  let compact: string;
  if (abs >= BigInt('100000000000000000')) {
    compact = `${formatScaledInteger(abs, BigInt('100000000000000000'), 2)}B`;
  } else if (abs >= BigInt('100000000000000')) {
    compact = `${formatScaledInteger(abs, BigInt('100000000000000'), 2)}M`;
  } else if (abs >= BigInt('100000000000')) {
    compact = `${formatScaledInteger(abs, BigInt('100000000000'), 2)}K`;
  } else if (abs >= BigInt(100000000)) {
    compact = formatScaledInteger(abs, BigInt(100000000), 2);
  } else {
    compact = formatScaledInteger(abs, BigInt(100000000), 4);
  }

  return { compact, direction };
}

export function hexToBytes(hex: string): Uint8Array {
  const cleanHex = hex.startsWith('0x') ? hex.slice(2) : hex;
  const bytes = new Uint8Array(cleanHex.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(cleanHex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

export function bytesToHex(bytes: Uint8Array): string {
  return (
    '0x' +
    Array.from(bytes)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('')
  );
}
