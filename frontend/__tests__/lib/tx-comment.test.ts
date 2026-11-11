import { describe, expect, it } from 'vitest';
import { exactCkbText } from '@/lib/ckb-decimal';
import { txComment, type TxCommentInput } from '@/lib/tx-comment';

// Rubbing × Song print, phase 5 (T4, the Transaction board's comment): three lowercase sentences,
// each true by construction from what the page draws — cells, fee, common knowledge.

const TRANSFER: TxCommentInput = {
  inputs: 3,
  outputs: 2,
  isCellbase: false,
  feeCkb: exactCkbText(BigInt(1647), false),
  knowledgeDeltaBytes: BigInt(61),
};

describe('txComment', () => {
  it('says how many cells became how many, the fee to the miner and how common knowledge grew', () => {
    expect(txComment(TRANSFER)).toEqual([
      '3 cells became 2.',
      '0.00001647 ckb to the miner.',
      'common knowledge grew by 61 bytes.',
    ]);
  });

  it('agrees the noun with one cell and one byte, and groups thousands', () => {
    expect(txComment({ ...TRANSFER, inputs: 1, outputs: 2, knowledgeDeltaBytes: 1 })).toEqual([
      '1 cell became 2.',
      '0.00001647 ckb to the miner.',
      'common knowledge grew by 1 byte.',
    ]);
    expect(
      txComment({ ...TRANSFER, inputs: 1_204, outputs: 1_000, knowledgeDeltaBytes: 12_345 })
    ).toEqual([
      '1,204 cells became 1,000.',
      '0.00001647 ckb to the miner.',
      'common knowledge grew by 12,345 bytes.',
    ]);
  });

  it('says common knowledge shrank, or did not change, by the sign of the delta', () => {
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: BigInt(-1) })[2]).toBe(
      'common knowledge shrank by 1 byte.'
    );
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: -4_096 })[2]).toBe(
      'common knowledge shrank by 4,096 bytes.'
    );
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: BigInt(0) })[2]).toBe(
      'common knowledge unchanged.'
    );
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: 0 })[2]).toBe(
      'common knowledge unchanged.'
    );
  });

  it('writes the fee as the facts rail prints it: every decimal, zero as 0', () => {
    const fee = (shannons: number) =>
      txComment({ ...TRANSFER, feeCkb: exactCkbText(BigInt(shannons), false) })[1];
    expect(fee(64_119)).toBe('0.00064119 ckb to the miner.');
    expect(fee(100_000)).toBe('0.00100000 ckb to the miner.');
    expect(fee(123_456_700_000)).toBe('1,234.56700000 ckb to the miner.');
    expect(fee(0)).toBe('0 ckb to the miner.');
  });

  it('says where a cellbase’s cells come from, and has no fee line', () => {
    expect(
      txComment({
        inputs: 0,
        outputs: 2,
        isCellbase: true,
        feeCkb: null,
        knowledgeDeltaBytes: BigInt(122),
      })
    ).toEqual([
      '2 cells from the block reward; none consumed.',
      'common knowledge grew by 122 bytes.',
    ]);
    expect(
      txComment({ inputs: 0, outputs: 1, isCellbase: true, feeCkb: null, knowledgeDeltaBytes: 61 })
    ).toEqual([
      '1 cell from the block reward; none consumed.',
      'common knowledge grew by 61 bytes.',
    ]);
  });

  it('writes lowercase sentences, each ending with a period', () => {
    for (const lines of [
      txComment(TRANSFER),
      txComment({ inputs: 0, outputs: 1, isCellbase: true, feeCkb: null, knowledgeDeltaBytes: 0 }),
    ])
      for (const line of lines) {
        expect(line).toBe(line.toLowerCase());
        expect(line).toMatch(/\.$/);
      }
  });

  it('refuses a count that is negative or not whole, with no clamp', () => {
    expect(() => txComment({ ...TRANSFER, inputs: -1 })).toThrow(
      'txComment: inputs -1 is not a count of cells'
    );
    expect(() => txComment({ ...TRANSFER, outputs: -2 })).toThrow(
      'txComment: outputs -2 is not a count of cells'
    );
    expect(() => txComment({ ...TRANSFER, outputs: 1.5 })).toThrow('outputs 1.5');
  });

  it('refuses a common knowledge delta that is not finite or not whole bytes', () => {
    for (const delta of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY, 0.5])
      expect(() => txComment({ ...TRANSFER, knowledgeDeltaBytes: delta })).toThrow(
        `txComment: common knowledge delta ${delta} is not whole bytes`
      );
  });

  it('refuses a line that would not be true: a cellbase that consumes, a fee where none is paid', () => {
    expect(() =>
      txComment({ inputs: 1, outputs: 1, isCellbase: true, feeCkb: null, knowledgeDeltaBytes: 0 })
    ).toThrow('txComment: a cellbase consumes no cell, got 1 inputs');
    expect(() =>
      txComment({ inputs: 0, outputs: 1, isCellbase: true, feeCkb: '0', knowledgeDeltaBytes: 0 })
    ).toThrow('txComment: a cellbase has no fee line, got fee 0');
    expect(() => txComment({ ...TRANSFER, feeCkb: null })).toThrow(
      'txComment: a transaction that is not a cellbase needs its fee'
    );
    for (const feeCkb of ['', '0.00001647 CKB', '-0.00001647', '+0.00001647', '0.000016', '1647'])
      expect(() => txComment({ ...TRANSFER, feeCkb })).toThrow('is not exact CKB');
  });
});
