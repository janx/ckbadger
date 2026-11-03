import { describe, expect, it } from 'vitest';
import type { Cell } from '@/lib/api';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';
import daoDeposit from '@/__tests__/fixtures/cell-inventory/dao.json';
import daoWithdraw from '@/__tests__/fixtures/cell-inventory/dao-withdraw.json';
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

  // Lane B (B2): the API described a withdraw request's 8 bytes twice (`dao_state` [0..8) and
  // `deposit_block_number` [0..8)), and this map threw "overlaps" — the cell page crashed. The
  // decode is one field now, for both DAO cells.
  it("names a DAO cell's 8 data bytes once: the deposit block number", () => {
    const request = cellDataParts({
      ...daoWithdraw,
      data: '0x39bd3e0100000000',
    } as unknown as Cell);
    expect(request.map((p) => [p.label, p.start, p.end, p.value])).toEqual([
      ['deposit block number', 0, 8, '20888889'],
    ]);
    const deposit = cellDataParts({ ...daoDeposit, data: '0x0000000000000000' } as unknown as Cell);
    expect(deposit.map((p) => [p.label, p.start, p.end, p.value])).toEqual([
      ['deposit block number', 0, 8, '0 = still deposited'],
    ]);
  });

  // Lane B (B1): an ELF the API cannot parse has no layout; its bytes are unclassified, never a
  // made-up map.
  it('leaves the bytes of an ELF that does not parse unclassified', () => {
    const data = `0x7f454c46020101${'00'.repeat(57)}`;
    const parts = cellDataParts({
      ...daoDeposit,
      type: undefined,
      dataAnalysis: undefined,
      data,
      elf: { status: 'unparsed', reason: 'ELF machine 0 is not RISC-V' },
    } as unknown as Cell);
    expect(parts.map((p) => [p.label, p.start, p.end])).toEqual([['unclassified bytes', 0, 64]]);
  });
});
