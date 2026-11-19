// Readability 2026-11-18, lane B4 (pG) item 5: a feed folds consecutive rows that say the same
// thing into one counted row. This pins what "the same" is (lib/activity-runs.ts) and how runs are
// cut. The rule is the owner's of Gate B (2026-11-19), built by the lists lane P3: what was done and
// the script code called, not the args, not the parties (re-pinned on purpose: B4 compared whole
// scripts with their args and the parties with their roles, which left the live feed unfolded,
// 100 activities 99 rows).
import { describe, expect, it } from 'vitest';
import { foldRuns, runKey, runParties, type RunSubject } from '@/lib/activity-runs';
import live from '@/__tests__/fixtures/activities-live-mainnet.json';
import type { GlobalActivity } from '@/lib/api';
import { activityTitleText } from '@/components/activity/activity-row';

const party = (id: string) => ({
  lockHash: `0x${id.padStart(64, '0')}`,
  lockHashPrefix: null,
  address: `ckb1${id}`,
});
const typeCall = (code: string, hashType = 'type') => ({
  typeCodeHash: `0x${code.padStart(64, '0')}`,
  typeHashType: hashType,
});
const lockCall = (code: string, hashType = 'type') => ({
  lockCodeHash: `0x${code.padStart(64, '0')}`,
  lockHashType: hashType,
});

/** A .bit time oracle's update: one script call by one party, the shape that fills a day's feed. */
function oracle(over: Partial<RunSubject> = {}): RunSubject {
  return {
    title: 'Script call',
    txHash: `0x${'aa'.repeat(32)}`,
    blockNumber: 21047791,
    protocolActions: [],
    typeCalls: [typeCall('3a'), typeCall('9e')],
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

  it('compares what was done in words: the title as drawn', () => {
    expect(runKey(oracle({ title: 'CKB Transfer' }))).not.toBe(runKey(oracle()));
    // the class mark is part of the title the feed hands over
    expect(runKey(oracle({ title: 'tokens|Token transfer' }))).not.toBe(
      runKey(oracle({ title: 'objects|Token transfer' }))
    );
  });

  it('compares what was done as the API names it: the protocol actions, counted, not their figures', () => {
    const deposit = { protocol: 'dao', action: 'deposit' };
    const request = { protocol: 'dao', action: 'withdraw_request' };
    const dao = (actions: RunSubject['protocolActions']) =>
      runKey(oracle({ title: 'DAO Deposit', typeCalls: [], protocolActions: actions }));
    expect(dao([deposit, request])).not.toBe(dao([deposit]));
    expect(dao([deposit, deposit])).not.toBe(dao([deposit]));
    expect(dao([request, deposit])).toBe(dao([deposit, request]));
  });

  it('compares the script code (code hash and hash type), counted, in any order, never the args', () => {
    // the subject carries no args at all: the three oracles' 0x00 / 0x01 / 0x02 are one call
    expect(runKey(oracle({ typeCalls: [typeCall('9e'), typeCall('3a')] }))).toBe(runKey(oracle()));
    expect(runKey(oracle({ typeCalls: [typeCall('3a'), typeCall('5c')] }))).not.toBe(
      runKey(oracle())
    );
    expect(
      runKey(oracle({ typeCalls: [typeCall('3a'), typeCall('3a'), typeCall('9e')] }))
    ).not.toBe(runKey(oracle()));
    // a script is the pair: one code hash referenced by data is another script
    expect(runKey(oracle({ typeCalls: [typeCall('3a', 'data1'), typeCall('9e')] }))).not.toBe(
      runKey(oracle())
    );
    // a lock call is a script too, and a lock is not a type
    expect(runKey(oracle({ lockCalls: [lockCall('c1')] }))).not.toBe(runKey(oracle()));
    expect(runKey(oracle({ typeCalls: [typeCall('9e')], lockCalls: [lockCall('3a')] }))).not.toBe(
      runKey(oracle())
    );
  });

  it('never compares the parties', () => {
    expect(runKey(oracle({ parties: [party('02')] }))).toBe(runKey(oracle()));
    expect(runKey(oracle({ parties: [party('01'), party('02'), party('03')] }))).toBe(
      runKey(oracle())
    );
  });

  it('never folds a row still in the pool, with anything', () => {
    const pending = oracle({ blockNumber: null });
    expect(runKey(pending)).not.toBe(runKey(oracle()));
    expect(runKey(pending)).not.toBe(
      runKey(oracle({ blockNumber: null, txHash: `0x${'cc'.repeat(32)}` }))
    );
  });
});

describe('runParties: how many parties a run names', () => {
  it('counts each party once across the run, by its lock hash, else its prefix, else its address', () => {
    expect(runParties([oracle(), oracle(), oracle({ parties: [party('02')] })])).toBe(2);
    expect(runParties([oracle({ parties: [party('01'), party('02')] }), oracle()])).toBe(2);
    const prefixed = { lockHash: null, lockHashPrefix: '0x1234', address: null };
    const addressed = { lockHash: null, lockHashPrefix: null, address: 'ckb1only' };
    expect(runParties([oracle({ parties: [prefixed, addressed] }), oracle()])).toBe(3);
  });

  it('refuses a party it cannot name, with the transaction', () => {
    const nobody = { lockHash: null, lockHashPrefix: null, address: null };
    expect(() => runParties([oracle({ parties: [nobody] })])).toThrow(
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

// The live feed (mainnet, the first page of /activities read from the API on 2026-11-19, blocks
// #21,054,083 to #21,054,163): the three .bit time oracles take turns (args 0x00, 0x01, 0x02, each
// its own party), a Bitcoin SPV client's update now and then, and one address's run of plain
// transfers. By the rule, the oracles' turns fold; a row that called other code ends a run.
describe('the rule on the live feed', () => {
  const rows = live as unknown as GlobalActivity[];
  // the title as the row draws it, in words (the one the stream hands over)
  const subject = (a: GlobalActivity): RunSubject => ({
    title: activityTitleText(a),
    txHash: a.txHash,
    blockNumber: a.blockNumber,
    protocolActions: a.protocolActions,
    typeCalls: a.typeCalls,
    lockCalls: a.lockCalls,
    parties: a.participants,
  });
  it('folds 50 activities into 9 rows, the oracles’ turns among them', () => {
    expect(rows).toHaveLength(50);
    const runs = foldRuns(rows, (a) => runKey(subject(a)));
    expect(runs.map((r) => r.length)).toEqual([7, 1, 2, 1, 9, 1, 21, 1, 7]);
    const named = (a: GlobalActivity) => a.typeCalls.map((c) => c.scriptName).join(' · ');
    expect(runs.map((r) => named(r[0]))).toEqual([
      '.bit Time Index State · .bit Time Info',
      'Bitcoin SPV Type Lock',
      '.bit Time Index State · .bit Time Info',
      'Bitcoin SPV Type Lock',
      '.bit Time Index State · .bit Time Info',
      '.bit Income Cell',
      '.bit Time Index State · .bit Time Info',
      '',
      '.bit Time Index State · .bit Time Info',
    ]);
    expect(runs.map((r) => activityTitleText(r[0]))).toEqual([
      'Script call',
      'Script call',
      'Script call',
      'Script call',
      'Script call',
      '.bit · recycle expired account',
      'Script call',
      'CKB Transfer',
      'Script call',
    ]);
    // the longest run: three oracles, each with its own args and its own party
    const longest = runs[6];
    expect(new Set(longest.map((a) => a.typeCalls[0].typeArgs))).toEqual(
      new Set(['0x00', '0x01', '0x02'])
    );
    expect(runParties(longest.map(subject))).toBe(3);
  });
});
