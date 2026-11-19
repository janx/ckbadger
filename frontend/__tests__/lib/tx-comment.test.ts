import { describe, expect, it } from 'vitest';
import { txComment, type TxCommentInput } from '@/lib/tx-comment';

// Rubbing × Song print, phase 5 (T4, the Transaction board's comment): lowercase sentences, each
// true by construction from what the page draws — cells and common knowledge. Re-pinned on purpose
// (lane D2, gate D0′'s "said once"): the fee line ("0.00001647 ckb to the miner.") is gone, the fee
// stands in the status strip and the created sheet's balance row; the comment's input carries no
// fee, so the tests that wrote one, or refused one, test what the comment writes instead.

const TRANSFER: TxCommentInput = {
  inputs: 3,
  outputs: 2,
  isCellbase: false,
  knowledgeDeltaBytes: BigInt(61),
};

describe('txComment', () => {
  // re-pinned (lane D2): two lines, the cells and common knowledge (it was three, the fee between)
  it('says how many cells became how many and how common knowledge grew', () => {
    expect(txComment(TRANSFER)).toEqual([
      '3 cells became 2.',
      'common knowledge grew by 61 bytes.',
    ]);
  });

  it('agrees the noun with one cell and one byte, and groups thousands', () => {
    expect(txComment({ ...TRANSFER, inputs: 1, outputs: 2, knowledgeDeltaBytes: 1 })).toEqual([
      '1 cell became 2.',
      'common knowledge grew by 1 byte.',
    ]);
    expect(
      txComment({ ...TRANSFER, inputs: 1_204, outputs: 1_000, knowledgeDeltaBytes: 12_345 })
    ).toEqual(['1,204 cells became 1,000.', 'common knowledge grew by 12,345 bytes.']);
  });

  it('says common knowledge shrank, or did not change, by the sign of the delta', () => {
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: BigInt(-1) })[1]).toBe(
      'common knowledge shrank by 1 byte.'
    );
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: -4_096 })[1]).toBe(
      'common knowledge shrank by 4,096 bytes.'
    );
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: BigInt(0) })[1]).toBe(
      'common knowledge unchanged.'
    );
    expect(txComment({ ...TRANSFER, knowledgeDeltaBytes: 0 })[1]).toBe(
      'common knowledge unchanged.'
    );
  });

  // re-pinned (lane D2): it was "writes the fee as the facts rail prints it"; a fact is said once,
  // so no line of the comment names the fee, the miner or an amount
  it('writes no fee line: the fee is said in the status strip, once', () => {
    for (const input of [
      TRANSFER,
      { ...TRANSFER, inputs: 158, outputs: 2, knowledgeDeltaBytes: -9_475 },
      { inputs: 0, outputs: 1, isCellbase: true, knowledgeDeltaBytes: 61 },
    ])
      for (const line of txComment(input)) expect(line).not.toMatch(/fee|miner|ckb|\d\.\d/);
  });

  it('says where a cellbase’s cells come from', () => {
    expect(
      txComment({
        inputs: 0,
        outputs: 2,
        isCellbase: true,
        knowledgeDeltaBytes: BigInt(122),
      })
    ).toEqual([
      '2 cells from the block reward; none consumed.',
      'common knowledge grew by 122 bytes.',
    ]);
    expect(txComment({ inputs: 0, outputs: 1, isCellbase: true, knowledgeDeltaBytes: 61 })).toEqual(
      ['1 cell from the block reward; none consumed.', 'common knowledge grew by 61 bytes.']
    );
  });

  it('writes lowercase sentences, each ending with a period', () => {
    for (const lines of [
      txComment(TRANSFER),
      txComment({ inputs: 0, outputs: 1, isCellbase: true, knowledgeDeltaBytes: 0 }),
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

  // re-pinned (lane D2): it also refused a fee where none is paid and a fee not in the rail's
  // form; the comment takes no fee, so a cellbase that consumes is the line it can refuse
  it('refuses a line that would not be true: a cellbase that consumes', () => {
    expect(() =>
      txComment({ inputs: 1, outputs: 1, isCellbase: true, knowledgeDeltaBytes: 0 })
    ).toThrow('txComment: a cellbase consumes no cell, got 1 inputs');
  });
});
