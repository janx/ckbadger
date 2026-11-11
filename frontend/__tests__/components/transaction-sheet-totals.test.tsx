// Two sheets and the seam (seals by kind, C11): each sheet's head ends with the exact total of its
// cells, read from the API's sums and never added up from the tiles.
import { describe, expect, it } from 'vitest';
import { sheetTotals } from '@/components/transaction/transaction-cell-tiles';
import type { TransactionDetail } from '@/lib/api';

const HASH = `0x${'c1'.repeat(32)}`;
const detail = (over: Partial<TransactionDetail>) =>
  ({ hash: HASH, ...over }) as unknown as TransactionDetail;

describe('sheetTotals (C11)', () => {
  it('reads the consumed and created totals from the API’s exact sums, as given', () => {
    // inputs and outputs that would sum otherwise: the API's figures are the ones drawn
    const tx = detail({
      inputsCapacity: '55500000000',
      outputsCapacity: '55499935881',
      inputs: [],
      outputs: [],
    });
    expect(sheetTotals(tx)).toEqual({
      consumedCapacity: '55500000000',
      createdCapacity: '55499935881',
    });
  });

  it('reads a cellbase’s consumed total as the 0 the API serves', () => {
    expect(
      sheetTotals(detail({ isCellbase: true, inputsCapacity: '0', outputsCapacity: '55961739927' }))
    ).toEqual({ consumedCapacity: '0', createdCapacity: '55961739927' });
  });

  it('refuses a transaction missing either sum, naming it, with no fallback', () => {
    expect(() => sheetTotals(detail({ outputsCapacity: '1' }))).toThrow(
      `Transaction ${HASH} missing required field "inputsCapacity"`
    );
    expect(() =>
      sheetTotals(detail({ inputsCapacity: '1', outputsCapacity: null as unknown as string }))
    ).toThrow(`Transaction ${HASH} missing required field "outputsCapacity"`);
  });
});
