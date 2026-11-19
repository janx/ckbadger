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
const itemType = (code: string, hashType = 'type') => ({
  typeCodeHash: `0x${code.padStart(64, '0')}`,
  typeHashType: hashType,
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
    itemTypes: [],
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

  // re-pinned on purpose (the third read's Lb NOTE 1, by the rule of the final round's item 1):
  // the actions are a SET of (protocol, action): one deposit against three in one batch did one
  // thing; it was a multiset, which made them two runs
  it('compares what was done as the API names it: the set of protocol actions, not their count or figures', () => {
    const deposit = { protocol: 'dao', action: 'deposit' };
    const request = { protocol: 'dao', action: 'withdraw_request' };
    const dao = (actions: RunSubject['protocolActions']) =>
      runKey(oracle({ title: 'DAO Deposit', typeCalls: [], protocolActions: actions }));
    expect(dao([deposit, request])).not.toBe(dao([deposit]));
    expect(dao([deposit, deposit, deposit])).toBe(dao([deposit]));
    expect(dao([request, deposit])).toBe(dao([deposit, request]));
    expect(dao([deposit, request, deposit])).toBe(dao([request, deposit]));
  });

  // re-pinned on purpose (the lead's final round, item 1): the code is a SET. The owner's rule is
  // "what was done plus the script code": the code that was called, not how many cells carried it
  // (it was a multiset, which cut the live token feed where only the count of one lock's outputs
  // changed)
  it('compares the set of script codes (code hash and hash type), in any order, never the args or the count', () => {
    // the subject carries no args at all: the three oracles' 0x00 / 0x01 / 0x02 are one call
    expect(runKey(oracle({ typeCalls: [typeCall('9e'), typeCall('3a')] }))).toBe(runKey(oracle()));
    expect(runKey(oracle({ typeCalls: [typeCall('3a'), typeCall('5c')] }))).not.toBe(
      runKey(oracle())
    );
    // one code twice is the code: the same set
    expect(runKey(oracle({ typeCalls: [typeCall('3a'), typeCall('3a'), typeCall('9e')] }))).toBe(
      runKey(oracle())
    );
    expect(runKey(oracle({ lockCalls: [lockCall('c1'), lockCall('c1'), lockCall('c1')] }))).toBe(
      runKey(oracle({ lockCalls: [lockCall('c1')] }))
    );
    expect(runKey(oracle({ itemTypes: [itemType('5e'), itemType('5e')] }))).toBe(
      runKey(oracle({ itemTypes: [itemType('5e')] }))
    );
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

// The page-phase review's MUST-1 (the P3 follow-up): the fold compared no code for a token, object
// or identity row (an item's type is never a `typeCall`), so rows that ran different code folded.
// The API states each row's item types (`itemTypes`, the type script of every item and the DAO's,
// as the indexer classified it), and the key compares them as it compares the calls.
describe('runKey: the code an item carries', () => {
  // mainnet's sUDT and xUDT, Spore and m-NFT, .bit Cell and did:ckb codes (docs/metadata/scripts)
  const SUDT = itemType('5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5');
  const XUDT = itemType(
    '50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95',
    'data1'
  );
  const SPORE = itemType(
    '4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
    'data1'
  );
  const MNFT = itemType('2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9');
  const BIT_CELL = itemType('cfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33');
  const DID = itemType('4a06164dc34dccade5afe3e847a97b6db743e79f5477fa3295acf02849c5984a');
  const row = (title: string, types: RunSubject['itemTypes'], n: number) =>
    oracle({
      title,
      typeCalls: [],
      itemTypes: types,
      txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
    });
  const runsOf = (rows: RunSubject[]) => foldRuns(rows, runKey).map((r) => r.length);

  it('makes two token standards side by side two runs', () => {
    expect(
      runsOf([row('tokens: Token transfer', [SUDT], 1), row('tokens: Token transfer', [XUDT], 2)])
    ).toEqual([1, 1]);
  });

  it('makes a Spore and an m-NFT transfer two runs', () => {
    expect(
      runsOf([
        row('objects: Object transfer', [SPORE], 1),
        row('objects: Object transfer', [MNFT], 2),
      ])
    ).toEqual([1, 1]);
  });

  it('makes a .bit Cell and a did:ckb transfer two runs', () => {
    expect(
      runsOf([
        row('identities: Identity transfer', [BIT_CELL], 1),
        row('identities: Identity transfer', [DID], 2),
      ])
    ).toEqual([1, 1]);
  });

  it('makes one standard with two tokens one run: the token is the script’s args, not compared', () => {
    // two transfers of two sUDT tokens: their rows differ only in the items they moved, which
    // the key does not hold (the expanded rows name each token)
    expect(
      runsOf([row('tokens: Token transfer', [SUDT], 1), row('tokens: Token transfer', [SUDT], 2)])
    ).toEqual([2]);
    // a code by its pair, in any order, each once
    expect(runKey(row('t', [SUDT, SPORE], 1))).toBe(runKey(row('t', [SPORE, SUDT], 2)));
    expect(runKey(row('t', [itemType('5e', 'data1')], 1))).not.toBe(
      runKey(row('t', [itemType('5e')], 2))
    );
    // an item's type is not a call: the same code as a type call is another key
    expect(runKey(row('t', [itemType('3a')], 1))).not.toBe(
      runKey(oracle({ title: 't', typeCalls: [typeCall('3a')], itemTypes: [] }))
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

// The live feeds (re-captured for the P3 follow-up, its capture said in the fixture: the first page
// of /activities and of its token, object, identity and DAO filters, read through this lane's API
// build over the live mainnet stores on 2026-11-19 at 12:33 UTC, with itemTypes; the token feed
// from its 22nd row, its first 21 being Force Bridge transactions of up to 774 parties). By the
// rule a run is what was done and the code its rows carry; a row that carries other code ends it.
// What the rule makes of them, read row by row: the whole feed's 50 rows in 14; the token feed's
// 29 sUDT transfers in one run, every row calling the same codes (re-pinned on purpose, the lead's
// final round, item 1: the codes are a set; as a multiset they were 4 runs, [11, 11, 3, 4], cut
// only where the number of Godwoken Withdrawal Lock outputs changed, 2 to 5); the run, #21,016,902
// to #21,016,521, holds transactions that moved six sets of five tokens of the one standard.
const LIVE_ALL_RUNS = [2, 1, 10, 1, 6, 1, 2, 1, 1, 5, 1, 3, 2, 14];
const LIVE_TOKEN_RUNS = [29];
const LIVE_TOKEN_MIXED = new Set([
  'USDC',
  'USDC,USDC|bsc',
  'USDC,USDC|bsc,dCKB',
  'USDC,WBTC,dCKB',
  'BTCB|bsc,USDC,USDC|bsc,dCKB',
  'USDC,dCKB',
]);

describe('the rule on the live feeds', () => {
  const feeds = (live as unknown as { feeds: Record<string, GlobalActivity[]> }).feeds;
  // the title as the row draws it, in words (the one the stream hands over)
  const subject = (a: GlobalActivity): RunSubject => {
    if (a.itemTypes === null) throw new Error(`${a.txHash}: no item types in the live capture`);
    return {
      title: activityTitleText(a),
      txHash: a.txHash,
      blockNumber: a.blockNumber,
      protocolActions: a.protocolActions,
      typeCalls: a.typeCalls,
      lockCalls: a.lockCalls,
      itemTypes: a.itemTypes,
      parties: a.participants,
    };
  };
  const runsOf = (rows: GlobalActivity[]) => foldRuns(rows, (a) => runKey(subject(a)));
  const tokensOf = (a: GlobalActivity) =>
    [
      ...new Set(
        a.participants.flatMap((p) =>
          p.itemDeltas.flatMap((i) => (i.kind === 'token' ? [i.symbol ?? i.typeScriptHash] : []))
        )
      ),
    ].sort();

  it('folds the first page of the whole feed', () => {
    expect(feeds.all).toHaveLength(50);
    const runs = runsOf(feeds.all);
    expect(runs.map((r) => r.length)).toEqual(LIVE_ALL_RUNS);
  });

  it('folds transfers of different tokens of one standard into one run, naming the standard', () => {
    const runs = runsOf(feeds.token);
    expect(runs.map((r) => r.length)).toEqual(LIVE_TOKEN_RUNS);
    // every row carries one code, Simple UDT (sUDT): the run's rows move different tokens
    for (const r of runs)
      expect(new Set(r.map((a) => a.itemTypes!.map((t) => t.scriptName).join(' · ')))).toEqual(
        new Set(['Simple UDT'])
      );
    const mixed = runs.find((r) => new Set(r.map((a) => tokensOf(a).join(','))).size > 1)!;
    expect(new Set(mixed.map((a) => tokensOf(a).join(',')))).toEqual(LIVE_TOKEN_MIXED);
  });

  it('names each object, identity and DAO row by the code the indexer classified', () => {
    for (const [feed, name] of [
      ['object', 'Spore'],
      ['identity', '.bit Account'],
      ['dao', 'Nervos DAO'],
    ] as const) {
      expect(feeds[feed]).toHaveLength(50);
      for (const a of feeds[feed]) expect(a.itemTypes!.map((t) => t.scriptName)).toEqual([name]);
    }
  });
});
