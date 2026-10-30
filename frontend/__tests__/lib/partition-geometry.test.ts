import { describe, expect, it } from 'vitest';
import { partitionSpans } from '@/lib/partition-geometry';

describe('partitionSpans', () => {
  it('lays exact parts side by side over the given width', () => {
    const spans = partitionSpans([BigInt(1), BigInt(3)], BigInt(4), 20, 860, 'split');
    expect(spans).toEqual([
      { x: 20, width: 215 },
      { x: 235, width: 645 },
    ]);
  });
  it('refuses parts that do not partition the total exactly', () => {
    expect(() => partitionSpans([BigInt(1), BigInt(2)], BigInt(4), 0, 100, 'DAO split')).toThrow(
      'DAO split: parts sum to 3, not the total 4'
    );
    expect(() => partitionSpans([BigInt(-1), BigInt(5)], BigInt(4), 0, 100, 'DAO split')).toThrow(
      'DAO split: negative part -1'
    );
  });
});
