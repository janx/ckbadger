import { describe, expect, it } from 'vitest';
import type { TransactionDetail } from '@/lib/api';
import type { CellTileData } from '@/components/ui/cell-tile';
import { TYPE_ID_CODE_HASH, txContinuity } from '@/lib/tx-continuity';

const tile = (kind: CellTileData['kind']) => ({ kind }) as CellTileData;
const group = (codeHash: string, inputs: number[], outputs: number[]) => ({
  kind: 'type',
  inputIndices: inputs,
  outputIndices: outputs,
  script: { codeHash, hashType: 'type', args: '0x' },
});

describe('txContinuity', () => {
  it('links one-in one-out Type ID groups as the same type id, in olive kind', () => {
    const tx = {
      hash: '0x01',
      inputs: [{ display: null }],
      outputs: [{ display: null }],
      scriptGroups: [group(TYPE_ID_CODE_HASH, [0], [0]), group(TYPE_ID_CODE_HASH, [0], [])],
    } as unknown as TransactionDetail;
    expect(txContinuity(tx, [tile('other')])).toEqual([
      { from: 'in:0', to: 'out:0', label: 'same type id continues', kind: 'type-id' },
    ]);
  });

  it('links an object only when input and output display the same item', () => {
    const item = { kind: 'object', standard: 'spore', id: '0xaa' };
    const tx = {
      hash: '0x02',
      inputs: [{ display: item }, { display: { ...item, id: '0xbb' } }],
      outputs: [{ display: item }, { display: item }],
      scriptGroups: [
        group('0x' + '33'.repeat(32), [0], [0]),
        group('0x' + '33'.repeat(32), [1], [1]),
      ],
    } as unknown as TransactionDetail;
    expect(txContinuity(tx, [tile('objects'), tile('objects')])).toEqual([
      { from: 'in:0', to: 'out:0', label: 'same object continues', kind: 'objects' },
    ]);
  });
});
