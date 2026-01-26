import { describe, it, expect } from 'vitest';
import { truncateHash, formatCkbAmount, formatTimeAgo, hexToBytes, bytesToHex } from '@/lib/utils';

describe('truncateHash', () => {
  it('truncates long hashes', () => {
    const hash = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
    expect(truncateHash(hash)).toBe('0x12345678...90abcdef');
  });

  it('does not truncate short hashes', () => {
    const hash = '0x1234';
    expect(truncateHash(hash)).toBe('0x1234');
  });

  it('uses custom start and end lengths', () => {
    const hash = '0x1234567890abcdef';
    expect(truncateHash(hash, 4, 4)).toBe('0x12...cdef');
  });
});

describe('formatCkbAmount', () => {
  it('formats whole CKB amounts', () => {
    const result = formatCkbAmount('10000000000');
    expect(result.integer).toBe('100');
    expect(result.decimal).toBe('00000000');
    expect(result.full).toBe('100.00000000');
    expect(result.isNegative).toBe(false);
  });

  it('formats fractional amounts', () => {
    const result = formatCkbAmount('12345678901');
    expect(result.integer).toBe('123');
    expect(result.decimal).toBe('45678901');
  });

  it('handles bigint input', () => {
    const result = formatCkbAmount(BigInt('50000000000000'));
    expect(result.integer).toBe('500,000');
  });

  it('handles negative values', () => {
    const result = formatCkbAmount('-10000000000');
    expect(result.isNegative).toBe(true);
    expect(result.full).toBe('-100.00000000');
  });

  it('pads small decimals with zeros', () => {
    const result = formatCkbAmount('1');
    expect(result.integer).toBe('0');
    expect(result.decimal).toBe('00000001');
  });

  it('handles zero', () => {
    const result = formatCkbAmount('0');
    expect(result.integer).toBe('0');
    expect(result.decimal).toBe('00000000');
    expect(result.isNegative).toBe(false);
  });
});

describe('formatTimeAgo', () => {
  it('formats seconds ago', () => {
    const date = new Date(Date.now() - 30 * 1000);
    expect(formatTimeAgo(date)).toBe('30s ago');
  });

  it('formats minutes ago', () => {
    const date = new Date(Date.now() - 5 * 60 * 1000);
    expect(formatTimeAgo(date)).toBe('5m ago');
  });

  it('formats hours ago', () => {
    const date = new Date(Date.now() - 3 * 60 * 60 * 1000);
    expect(formatTimeAgo(date)).toBe('3h ago');
  });

  it('formats days ago', () => {
    const date = new Date(Date.now() - 2 * 24 * 60 * 60 * 1000);
    expect(formatTimeAgo(date)).toBe('2d ago');
  });

  it('handles string timestamps', () => {
    const timestamp = new Date(Date.now() - 60 * 1000).toISOString();
    expect(formatTimeAgo(timestamp)).toBe('1m ago');
  });
});

describe('hexToBytes', () => {
  it('converts hex string to bytes', () => {
    const bytes = hexToBytes('0x1234');
    expect(bytes).toEqual(new Uint8Array([0x12, 0x34]));
  });

  it('handles hex without 0x prefix', () => {
    const bytes = hexToBytes('abcd');
    expect(bytes).toEqual(new Uint8Array([0xab, 0xcd]));
  });
});

describe('bytesToHex', () => {
  it('converts bytes to hex string', () => {
    const hex = bytesToHex(new Uint8Array([0x12, 0x34, 0xab, 0xcd]));
    expect(hex).toBe('0x1234abcd');
  });

  it('pads single digit bytes', () => {
    const hex = bytesToHex(new Uint8Array([0x01, 0x02]));
    expect(hex).toBe('0x0102');
  });

  it('roundtrips correctly', () => {
    const original = '0x1234567890abcdef';
    const bytes = hexToBytes(original);
    const result = bytesToHex(bytes);
    expect(result).toBe(original);
  });
});
