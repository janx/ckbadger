// Readability 2026-11-18, lane B4 (pG) item 5: a feed folds consecutive rows that say the same
// thing into one counted row. This pins what "the same" is (lib/activity-runs.ts) and how runs are
// cut. The rule is the owner's of Gate B (2026-11-19), built by the lists lane P3: what was done and
// the script code called, not the args, not the parties (re-pinned on purpose: B4 compared whole
// scripts with their args and the parties with their roles, which left the live feed unfolded,
// 100 activities 99 rows).
import { describe, expect, it } from 'vitest';
import { foldRuns, runKey, runSenders, runTokens, type RunSubject } from '@/lib/activity-runs';
import { partyIds } from '@/lib/activity-parties';
import { liveFeeds } from '@/__tests__/utils/live-activities';
import { TAG_INPUT, type GlobalActivity, type PartyId } from '@/lib/api';
import { activityTitleText } from '@/components/activity/activity-row';

/**
 * A party as the route names it (lane feed): its short id, the first 20 bytes of its lock hash, and
 * its tags; a sender by default (the senders bit, `TAG_INPUT`, which a run counts).
 */
const party = (id: string, tags = TAG_INPUT): PartyId => [`0x${id.padStart(40, '0')}`, tags];
const typeCall = (code: string, hashType = 'type') => ({
  typeCodeHash: `0x${code.padStart(64, '0')}`,
  typeHashType: hashType,
});
const lockCall = (code: string, hashType = 'type') => ({
  lockCodeHash: `0x${code.padStart(64, '0')}`,
  lockHashType: hashType,
});
const recognizedType = (code: string, hashType = 'type') => ({
  typeCodeHash: `0x${code.padStart(64, '0')}`,
  typeHashType: hashType,
});

/** A .bit time oracle's update: one script call by one party, the shape that fills a day's feed. */
function oracle(over: Partial<RunSubject> = {}): RunSubject {
  return {
    title: 'Script call',
    txHash: `0x${'aa'.repeat(32)}`,
    blockNumber: 21047791,
    isCellbase: false,
    protocolActions: [],
    typeCalls: [typeCall('3a'), typeCall('9e')],
    lockCalls: [],
    recognizedTypes: [],
    parties: [party('01')],
    tokens: null,
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
    expect(runKey(oracle({ recognizedTypes: [recognizedType('5e'), recognizedType('5e')] }))).toBe(
      runKey(oracle({ recognizedTypes: [recognizedType('5e')] }))
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
// The API states each row's recognised type codes (`recognizedTypes`, C7: every type script code
// the indexer recognises that ran, read from the row; it stated `itemTypes`, the items' codes
// only), and the key compares them as it compares the calls.
describe('runKey: the recognised codes that ran', () => {
  // mainnet's sUDT and xUDT, Spore and m-NFT, .bit Cell and did:ckb codes (docs/metadata/scripts)
  const SUDT = recognizedType('5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5');
  const XUDT = recognizedType(
    '50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95',
    'data1'
  );
  const SPORE = recognizedType(
    '4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
    'data1'
  );
  const MNFT = recognizedType('2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9');
  const BIT_CELL = recognizedType(
    'cfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33'
  );
  const DID = recognizedType('4a06164dc34dccade5afe3e847a97b6db743e79f5477fa3295acf02849c5984a');
  const row = (title: string, types: RunSubject['recognizedTypes'], n: number) =>
    oracle({
      title,
      typeCalls: [],
      recognizedTypes: types,
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
    expect(runKey(row('t', [recognizedType('5e', 'data1')], 1))).not.toBe(
      runKey(row('t', [recognizedType('5e')], 2))
    );
    // a recognised code is not an unrecognised call: the same code as a type call is another key
    expect(runKey(row('t', [recognizedType('3a')], 1))).not.toBe(
      runKey(oracle({ title: 't', typeCalls: [typeCall('3a')], recognizedTypes: [] }))
    );
  });

  // C7: a wallet merging its own sUDT cells moves no item, so the item types of before held no
  // code for it, and under one title it folded with a plain CKB transfer. The list holds every
  // recognised code that ran: by the code alone, whatever the title, they are two runs.
  it('keeps a wallet’s sUDT merge apart from a plain CKB transfer by the code that ran', () => {
    expect(
      runsOf([
        row('CKB Transfer', [], 1),
        row('CKB Transfer', [SUDT], 2),
        row('CKB Transfer', [], 3),
      ])
    ).toEqual([1, 1, 1]);
    expect(runsOf([row('Script call', [SUDT], 1), row('Script call', [SUDT], 2)])).toEqual([2]);
  });
});

// Re-pinned on purpose (resync/int, step 2: the owner's word at Gate B): a run counts its SENDERS,
// the parties whose lock is an input's, each once; it counted every party (`runParties`) while the
// stores did not say who sent. Re-pinned again at the merge of lane feed (ruling 14 of gate P): a
// party is the route's short id with its tags, every party of each transaction, drawn or not, and
// it sent where its tags carry the senders bit (`TAG_INPUT`, the API's `sent` of a drawn party);
// it was a party sent in full with its `sent`, named by its lock hash, else its prefix, else its
// address (so a protocol-named party and the same party by its full hash were two).
describe('runSenders: how many parties of a run sent', () => {
  it('counts each sender once across the run, by the short id the route names it by', () => {
    expect(runSenders([oracle(), oracle(), oracle({ parties: [party('02')] })])).toBe(2);
    expect(runSenders([oracle({ parties: [party('01'), party('02')] }), oracle()])).toBe(2);
    // its other tags are not its name: one party in two rows
    expect(
      runSenders([
        oracle({ parties: [party('03', TAG_INPUT | 64)] }),
        oracle({ parties: [party('03')] }),
      ])
    ).toBe(1);
  });

  it('counts no party that did not send: a payee, a party a protocol only named', () => {
    expect(
      runSenders([
        oracle({ parties: [party('01'), party('02', 0), party('1234', 0)] }),
        oracle({ parties: [party('03', 0), party('01')] }),
      ])
    ).toBe(1);
  });

  // the refusal of a party it cannot name moved to where the ids are read (lane feed): every party
  // the route names has its short id, and `partyIds` refuses a row naming fewer than its count
  it('refuses a row that names fewer parties than it counts, with the transaction', () => {
    expect(() =>
      partyIds({ txHash: `0x${'aa'.repeat(32)}`, participantCount: 2, partyIds: [party('01')] })
    ).toThrow(`transaction 0x${'aa'.repeat(32)}: it names 1 of its 2 parties`);
  });

  it('refuses a committed transaction no party of which sent, never counting it as none', () => {
    expect(() => runSenders([oracle(), oracle({ parties: [party('02', 0)] })])).toThrow(
      `transaction 0x${'aa'.repeat(32)}: a committed transaction with no party that sent (every transaction but a cellbase has an input)`
    );
  });

  // an API older than the page states no party ids: refused where they are read
  it('refuses a row that states no party ids: an API older than the page', () => {
    expect(() => partyIds({ txHash: `0x${'aa'.repeat(32)}`, participantCount: 1 })).toThrow(
      'partyIds'
    );
  });

  it('has no sender to count in a cellbase', () => {
    expect(() => runSenders([oracle({ isCellbase: true, parties: [party('c0', 0)] })])).toThrow(
      `transaction 0x${'aa'.repeat(32)}: a cellbase has no sender to count`
    );
  });
});

describe('runKey: a cellbase folds with cellbases only', () => {
  it('never gives a cellbase and a transaction one key, whatever else they share', () => {
    expect(runKey(oracle({ isCellbase: true }))).not.toBe(runKey(oracle()));
    expect(runKey(oracle({ isCellbase: true }))).toBe(
      runKey(oracle({ isCellbase: true, txHash: `0x${'bb'.repeat(32)}` }))
    );
  });
});

describe('runTokens: how many tokens a run of token rows moved (ruling 6 of gate P)', () => {
  const tokens = (...ids: string[]) => ({ title: 'tokens: Token transfer', tokens: ids });
  it('counts each token once across the run, by its type script hash', () => {
    expect(
      runTokens([oracle(tokens('0x7e')), oracle(tokens('0x7e', '0x7f')), oracle(tokens('0x7e'))])
    ).toBe(2);
    expect(runTokens([oracle(tokens('0x7e')), oracle(tokens('0x7e'))])).toBe(1);
  });

  it('counts none on a run of another kind, and refuses a run that mixes the two', () => {
    expect(runTokens([oracle(), oracle()])).toBeNull();
    expect(() => runTokens([oracle(tokens('0x7e')), oracle()])).toThrow(
      `transaction 0x${'aa'.repeat(32)}: a run of token rows holds a row of another kind`
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

// The live feeds (re-captured for the P3 follow-up, and again at the third read's head, 574cb27d,
// after the item resolver of 0ab62b48 (T NOTE-7); the capture is said in the fixture: the first
// page of /activities and of its token, object, identity and DAO filters, read through this lane's
// API build over the live mainnet stores on 2026-11-19 at 18:21 UTC, with the item types of then,
// read as the recognised codes, `liveFeeds` says how; the token feed from its 22nd row, its first
// 21 being Force Bridge transactions of more than 100 parties each). By the rule a run is what was
// done and the code its rows carry; a row that carries other code ends it. What the rule makes of
// them, read row by row: the whole feed's 50 rows in 12 (re-pinned on purpose at the recapture: the
// whole feed is the chain's newest page, #21,057,151 to #21,057,212, where it was #21,055,136 to
// #21,055,201 in 14; folded by the rows' code sets alone, read apart in Python, the same 12: .bit
// time cells' runs, CKB transfers and two pairs of Bitcoin SPV Type Lock calls); the token feed's
// 29 sUDT transfers in one run, every row calling the same codes (re-pinned on purpose, the lead's
// final round, item 1: the codes are a set; as a multiset they were 4 runs, [11, 11, 3, 4], cut
// only where the number of Godwoken Withdrawal Lock outputs changed, 2 to 5); the run, #21,016,902
// to #21,016,521, holds transactions that moved six sets of five tokens of the one standard.
const LIVE_ALL_RUNS = [2, 3, 1, 3, 2, 6, 2, 6, 5, 12, 2, 6];
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
  const feeds = liveFeeds();
  // the title as the row draws it, in words (the one the stream hands over)
  const subject = (a: GlobalActivity): RunSubject => ({
    title: activityTitleText(a),
    txHash: a.txHash,
    blockNumber: a.blockNumber,
    isCellbase: a.isCellbase,
    protocolActions: a.protocolActions,
    typeCalls: a.typeCalls,
    lockCalls: a.lockCalls,
    recognizedTypes: a.recognizedTypes,
    parties: partyIds(a),
    tokens: null,
  });
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
      expect(new Set(r.map((a) => a.recognizedTypes.map((t) => t.scriptName).join(' · ')))).toEqual(
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
      for (const a of feeds[feed])
        expect(a.recognizedTypes.map((t) => t.scriptName)).toEqual([name]);
    }
  });
});
