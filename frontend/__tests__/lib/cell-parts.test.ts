import { describe, expect, it } from 'vitest';
import type { Cell } from '@/lib/api';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';
import { cellDataParts, cellOutputParts } from '@/lib/cell-parts';
import { CELL_COLORS } from '@/lib/design-tokens';

const cell = sporeCell as unknown as Cell;

describe('cell parts (Cell board: every byte named once)', () => {
  it('names the 195 CellOutput bytes field by field, framing not occupied', () => {
    const parts = cellOutputParts(cell);
    expect(parts).toHaveLength(21);
    expect(parts.at(-1)!.end).toBe(195);
    expect(parts.filter((p) => p.occupied).reduce((n, p) => n + p.end - p.start, 0)).toBe(139);
    const codeHash = parts.find((p) => p.label === 'lock · code_hash')!;
    expect(codeHash).toMatchObject({ start: 40, end: 72, type: 'Byte32', group: 'lock' });
    expect(codeHash.value).toBe(cell.lock!.codeHash);
    expect(codeHash.tone).toBe(CELL_COLORS.lock);
    expect(parts.find((p) => p.label === 'type · hash_type')!.value).toBe('0x02 = data1');
    expect(parts.find((p) => p.label === 'capacity')!.value).toBe('32,000,000,000 shannons');
    expect(parts.find((p) => p.label === 'total_size')).toMatchObject({
      occupied: false,
      value: '195',
    });
  });

  it('names the 181 data bytes from the API decode, all occupied', () => {
    const parts = cellDataParts(cell);
    expect(parts.map((p) => p.label)).toEqual([
      'total size',
      'offset content type',
      'offset content',
      'offset cluster id',
      'content type length',
      'content type',
      'content length',
      'content',
      'cluster id length',
      'cluster id',
    ]);
    expect(parts.every((p) => p.occupied)).toBe(true);
    expect(parts.find((p) => p.label === 'content type')!.value).toBe('dob/0');
  });
});
