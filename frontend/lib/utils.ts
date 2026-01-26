import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function truncateHash(hash: string, start = 10, end = 8): string {
  if (hash.length <= start + end) return hash;
  return `${hash.slice(0, start)}...${hash.slice(-end)}`;
}

export function formatTimeAgo(timestamp: string | number | Date): string {
  const date = new Date(timestamp);
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

export function formatCkbValue(ckbValue: string | number): {
  integer: string;
  decimal: string;
  full: string;
  isNegative: boolean;
} {
  const num = typeof ckbValue === 'string' ? parseFloat(ckbValue) : ckbValue;
  const isNegative = num < 0;
  const absNum = Math.abs(num);

  const integerPart = Math.floor(absNum);
  const fractionPart = absNum - integerPart;

  const integer = integerPart.toLocaleString('en-US');
  const decimal = fractionPart.toFixed(8).slice(2);
  const full = `${isNegative ? '-' : ''}${integer}.${decimal}`;

  return { integer, decimal, full, isNegative };
}

export function formatCapacity(capacity: string | bigint): string {
  const { full } = formatCkbAmount(capacity);
  return `${full} CKB`;
}

export function formatCkbCompact(shannons: string | bigint): {
  value: string;
  full: string;
} {
  const ckb = Number(BigInt(shannons)) / 100_000_000;
  const full = formatCkbAmount(shannons).full;

  if (ckb >= 1_000_000_000_000) {
    return { value: `${(ckb / 1_000_000_000_000).toFixed(2)}T`, full };
  }
  if (ckb >= 1_000_000_000) {
    return { value: `${(ckb / 1_000_000_000).toFixed(2)}B`, full };
  }
  if (ckb >= 1_000_000) {
    return { value: `${(ckb / 1_000_000).toFixed(2)}M`, full };
  }
  if (ckb >= 1_000) {
    return { value: `${(ckb / 1_000).toFixed(2)}K`, full };
  }
  return { value: ckb.toFixed(2), full };
}

export function formatNumber(num: number | bigint): string {
  return num.toLocaleString();
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
