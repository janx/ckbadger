import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, it, expect } from 'vitest';
import {
  truncateHash,
  formatUtc8Date,
  formatCkbAmount,
  formatCkbCompact,
  formatCkbValue,
  formatCompactCkbDelta,
  formatTimeAgo,
  formatDurationMs,
  formatUtc8DateTime,
  formatUtc8Time,
  formatUnixSecondsUtc8,
  hexToBytes,
  bytesToHex,
} from '@/lib/utils';

describe('formatCkbCompact', () => {
  it('rounds a large shannon amount without losing the threshold bit', () => {
    expect(formatCkbCompact('100499999999999999999').value).toBe('1.00T');
    expect(formatCkbCompact('100500000000000000000').value).toBe('1.01T');
  });
});

describe('exact CKB decimal readouts', () => {
  it('keeps fractional CKB from a large API decimal string', () => {
    expect(formatCkbValue('9007199254740993.00000001').full).toBe('9,007,199,254,740,993.00000001');
  });

  it('rounds signed activity deltas only at display precision', () => {
    expect(formatCompactCkbDelta('-9007199254740993.005')).toEqual({
      compact: '9007199.25B',
      direction: 'down',
    });
  });
});

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

describe('formatDurationMs', () => {
  it('formats chain ages without raw millisecond dumps', () => {
    expect(formatDurationMs(500)).toBe('500 ms');
    expect(formatDurationMs(30000)).toBe('30 s');
    expect(formatDurationMs(60000)).toBe('1 m');
    expect(formatDurationMs(3661000)).toBe('1 h 1 m');
    expect(formatDurationMs(90000000)).toBe('1 d 1 h');
  });
});

describe('one site clock: UTC+8 (D2)', () => {
  it('writes chain and observer times as UTC+8 whatever the input form', () => {
    expect(formatUtc8DateTime('2026-10-22T14:43:55.651Z')).toBe('2026-10-22 22:43:55 UTC+8');
    expect(formatUtc8DateTime('2026-10-20T16:00:00+00:00')).toBe('2026-10-21 00:00:00 UTC+8');
    expect(formatUtc8DateTime(String(Date.parse('2024-01-01T15:59:59Z')))).toBe(
      '2024-01-01 23:59:59 UTC+8'
    );
    expect(formatUtc8DateTime(Date.parse('2024-01-01T16:00:00Z'))).toBe(
      '2024-01-02 00:00:00 UTC+8'
    );
    expect(formatUtc8Time('2026-10-22T14:43:55.651Z')).toBe('22:43:55 UTC+8');
    expect(formatUnixSecondsUtc8('1700000000')).toBe('2023-11-15 06:13:20 UTC+8');
  });
  it('rejects malformed times instead of printing them', () => {
    expect(() => formatUtc8DateTime('not-a-date')).toThrow('Invalid timestamp "not-a-date"');
    expect(() => formatUtc8DateTime(-1)).toThrow('Invalid timestamp -1');
    expect(() => formatUnixSecondsUtc8('not-a-second')).toThrow(/Invalid Unix seconds/);
  });
  it('is the only time formatter page code uses: no UTC clocks remain', () => {
    const root = join(__dirname, '..', '..');
    const offenders: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) walk(path);
        else if (/\.(tsx|ts)$/.test(name)) {
          const text = readFileSync(path, 'utf8');
          text.split('\n').forEach((line, i) => {
            if (
              /formatUtcClock|formatUnixSecondsUtc\b|formatObserverTimeUtc8|toUTCString\(|toISOString\(\)\.slice\(11/.test(
                line
              )
            )
              offenders.push(`${relative(root, path)}:${i + 1}`);
          });
        }
      }
    };
    walk(join(root, 'app'));
    walk(join(root, 'components'));
    walk(join(root, 'lib'));
    expect(offenders).toEqual([]);
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

describe('formatUtc8Date', () => {
  it('keeps raw event dates in fixed UTC+8 independently of browser timezone', () => {
    expect(formatUtc8Date('2024-01-01T16:30:00Z')).toBe('2024-01-02');
    expect(formatUtc8Date(String(Date.parse('2024-01-01T15:59:59Z')))).toBe('2024-01-01');
    expect(() => formatUtc8Date('broken')).toThrow('Invalid block timestamp');
  });
});
