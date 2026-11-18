// Readability 2026-11-18, lane B4 (pG) item 5: a feed folds consecutive rows that say the same
// thing into one counted row. This pins what "the same" is (lib/activity-runs.ts) and how runs are
// cut, so the later lists lane can take the same rule for the transactions table.
import { describe, expect, it } from 'vitest';
import { foldRuns, runKey, type RunSubject } from '@/lib/activity-runs';

const party = (id: string, roles: string[] = []) => ({
  lockHash: `0x${id.padStart(64, '0')}`,
  lockHashPrefix: null,
  address: `ckb1${id}`,
  roles,
});
const typeCall = (h: string) => ({ scriptHash: `0x${h.padStart(64, '0')}` });

/** A .bit time oracle's update: one script call by one party, the shape that fills a day's feed. */
function oracle(over: Partial<RunSubject> = {}): RunSubject {
  return {
    title: 'Script call',
    txHash: `0x${'aa'.repeat(32)}`,
    blockNumber: 21047791,
    typeCalls: [typeCall('a1'), typeCall('a2')],
    lockCalls: [],
    parties: [party('01')],
    ...over,
  };
}

describe('runKey: what makes two rows the same', () => {
  it('is equal for rows that differ only in what the folded row does not show', () => {
    // another transaction, another block: amounts, items and times are not in the subject at all
    expect(runKey(oracle({ txHash: `0x${'bb'.repeat(32)}`, blockNumber: 21047785 }))).toBe(
      runKey(oracle())
    );
  });

  it('compares the title as drawn', () => {
    expect(runKey(oracle({ title: 'CKB Transfer' }))).not.toBe(runKey(oracle()));
    // the class mark is part of the title the feed hands over
    expect(runKey(oracle({ title: 'tokens|Token transfer' }))).not.toBe(
      runKey(oracle({ title: 'objects|Token transfer' }))
    );
  });

  it('compares the scripts as whole scripts (their hashes, args included), counted, in any order', () => {
    // the same code with other args is another script: the three .bit oracles' 0x00 / 0x01 / 0x02
    expect(runKey(oracle({ typeCalls: [typeCall('a1'), typeCall('a3')] }))).not.toBe(
      runKey(oracle())
    );
    expect(runKey(oracle({ typeCalls: [typeCall('a2'), typeCall('a1')] }))).toBe(runKey(oracle()));
    expect(
      runKey(oracle({ typeCalls: [typeCall('a1'), typeCall('a1'), typeCall('a2')] }))
    ).not.toBe(runKey(oracle()));
    // a lock call is a script too
    expect(runKey(oracle({ lockCalls: [typeCall('c1')] }))).not.toBe(runKey(oracle()));
  });

  it('compares the parties by identity and roles, counted, in any order', () => {
    expect(runKey(oracle({ parties: [party('02')] }))).not.toBe(runKey(oracle()));
    expect(runKey(oracle({ parties: [party('01', ['owner_from'])] }))).not.toBe(runKey(oracle()));
    const two = oracle({ parties: [party('01'), party('02')] });
    expect(runKey(oracle({ parties: [party('02'), party('01')] }))).toBe(runKey(two));
    // a party named only by the prefix a protocol recorded is that prefix
    const prefixed = { lockHash: null, lockHashPrefix: '0x1234', address: null, roles: [] };
    expect(runKey(oracle({ parties: [prefixed] }))).toBe(runKey(oracle({ parties: [prefixed] })));
    expect(runKey(oracle({ parties: [prefixed] }))).not.toBe(runKey(oracle()));
  });

  it('never folds a row still in the pool, with anything', () => {
    const pending = oracle({ blockNumber: null });
    expect(runKey(pending)).not.toBe(runKey(oracle()));
    expect(runKey(pending)).not.toBe(
      runKey(oracle({ blockNumber: null, txHash: `0x${'cc'.repeat(32)}` }))
    );
  });

  it('refuses a party it cannot name, with the transaction', () => {
    const nobody = { lockHash: null, lockHashPrefix: null, address: null, roles: [] };
    expect(() => runKey(oracle({ parties: [nobody] }))).toThrow(
      `transaction 0x${'aa'.repeat(32)}: a party with no lock hash, prefix or address`
    );
  });
});

describe('foldRuns: consecutive rows with one key', () => {
  const key = (row: string) => row[0];

  it('folds each run of consecutive equal keys, in order, every row once', () => {
    expect(foldRuns(['a1', 'a2', 'b1', 'a3', 'a4', 'a5', 'c1'], key)).toEqual([
      ['a1', 'a2'],
      ['b1'],
      ['a3', 'a4', 'a5'],
      ['c1'],
    ]);
  });

  it('never lets a run cross a row that differs', () => {
    // the three oracles of mainnet take turns: no two consecutive rows are the same
    expect(foldRuns(['x1', 'y1', 'z1', 'x2', 'y2', 'z2'], key)).toEqual([
      ['x1'],
      ['y1'],
      ['z1'],
      ['x2'],
      ['y2'],
      ['z2'],
    ]);
  });

  it('folds only what it is given: a page boundary starts a new run', () => {
    const page1 = ['a1', 'a2'];
    const page2 = ['a3', 'b1'];
    expect(foldRuns(page1, key)).toEqual([['a1', 'a2']]);
    expect(foldRuns(page2, key)).toEqual([['a3'], ['b1']]);
    // rows appended to the rows already shown are consecutive with them, and fold with them
    expect(foldRuns([...page1, ...page2], key)).toEqual([['a1', 'a2', 'a3'], ['b1']]);
  });

  it('is empty for no rows', () => {
    expect(foldRuns([], key)).toEqual([]);
  });
});
