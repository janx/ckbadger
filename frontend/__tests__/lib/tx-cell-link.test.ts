/**
 * The link to a transaction at one of its cells (review of phases C and D, SHOULD 6; lane P1): the
 * one builder of a link from a cell's context into a transaction, carrying the out point the
 * transaction page keeps drawn under its fold.
 */
import { describe, expect, it } from 'vitest';
import { transactionAtCellHref } from '@/lib/tx-cell-link';

const CREATED = `0x1422d49b${'04b8'.repeat(13)}4c02`;
const SPENT = `0x${'cd'.repeat(32)}`;

describe('transactionAtCellHref', () => {
  it('opens the transaction that created the cell at the cell', () => {
    expect(transactionAtCellHref(CREATED, { txHash: CREATED, index: 57 })).toBe(
      `/tx/${CREATED}#${CREATED}-57`
    );
  });

  it('opens the transaction that spent the cell at the same cell, its input', () => {
    expect(transactionAtCellHref(SPENT, { txHash: CREATED, index: 0 })).toBe(
      `/tx/${SPENT}#${CREATED}-0`
    );
  });

  it('writes hashes lowercase, the anchor’s form the transaction page reads', () => {
    const upper = `0x${'AB'.repeat(32)}`;
    expect(transactionAtCellHref(upper, { txHash: upper, index: 3 })).toBe(
      `/tx/0x${'ab'.repeat(32)}#0x${'ab'.repeat(32)}-3`
    );
  });

  it('refuses a hash that is not one and an index that is not an output’s', () => {
    expect(() => transactionAtCellHref('0x12', { txHash: CREATED, index: 0 })).toThrow(
      'transaction hash "0x12" is not a 32-byte hash'
    );
    expect(() => transactionAtCellHref(CREATED, { txHash: 'abc', index: 0 })).toThrow(
      'cell transaction hash "abc" is not a 32-byte hash'
    );
    expect(() => transactionAtCellHref(CREATED, { txHash: CREATED, index: -1 })).toThrow(
      `cell index -1 of ${CREATED} is not an output index`
    );
    expect(() => transactionAtCellHref(CREATED, { txHash: CREATED, index: 1.5 })).toThrow(
      'is not an output index'
    );
  });
});
