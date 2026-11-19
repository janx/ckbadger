/**
 * "What moved" (tx page, review 2026-11-03 v2): the flows a transaction's activity states — items
 * first, CKB last; givers and receivers by exact amount; the fee to the miner, compensation as a
 * giver, mint and burn terminals; the fold beyond six parties; ribbon weights.
 */
import { describe, expect, it } from 'vitest';
import {
  daoStepOf,
  flowKey,
  foldEnds,
  onlyFeeMoved,
  ribbonWeight,
  txFlows,
  type FlowEnd,
} from '@/lib/tx-flows';
import type { GlobalActivity, ItemDelta, ParticipantInfo } from '@/lib/api';

const HASH = `0x${'91'.repeat(32)}`;
const ICKB = `0x${'1c'.repeat(32)}`;

function party(
  tail: string,
  ckbDelta: string,
  itemDeltas: ItemDelta[] = [],
  lockKind: string | null = 'Default Lock'
): ParticipantInfo {
  return {
    address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${tail}`,
    lockHash: `0x${tail.padStart(64, '0')}`,
    lockHashPrefix: null,
    roles: [],
    lockKind,
    ckbDelta,
    usedDelta: '0',
    itemDeltas,
    tags: itemDeltas.length ? 1 : 0,
  };
}
const token = (delta: string): ItemDelta => ({
  kind: 'token',
  typeScriptHash: ICKB,
  delta,
  symbol: 'iCKB',
  decimals: 8,
});
// re-pinned (lane P1, third round): an activity with no protocol step, as onlyFeeMoved reads it
const activity = (
  participants: ParticipantInfo[]
): Pick<GlobalActivity, 'participants' | 'protocolActions'> => ({
  participants,
  protocolActions: [],
});
const amounts = (ends: readonly FlowEnd[]) =>
  ends.map((e) => [e.t, e.t === 'party' ? e.party.address!.slice(-6) : e.t, e.amount.toString()]);

/** The participants of the genesis dep-group transaction, verbatim from mainnet :8101. */
const GENESIS_DEP_GROUP_PARTICIPANTS: ParticipantInfo[] = [
  {
    address:
      'ckb1qpcf7076zt6krnavly3883t6nrlduxy28ud9nv0c3rg387wvuzryjq9h89s7gmv7kyvd8hsar68npva00wlnzcqgh76tz',
    lockHash: '0x64b2aa1eadd2ad55a37296d7624c1c822156eee760506a9b67db51badebfdea2',
    lockHashPrefix: null,
    lockKind: null,
    roles: [],
    ckbDelta: '-23400000000',
    usedDelta: '-6100000000',
    itemDeltas: [],
    tags: 128,
  },
  {
    address: 'ckb1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq4maw7d',
    lockHash: '0x77c93b0632b5b6c3ef922c5b7cea208fb0a7c427a13d50e13d3fefad17e0c590',
    lockHashPrefix: null,
    lockKind: 'Zero Lock',
    roles: [],
    ckbDelta: '23400000000',
    usedDelta: '23400000000',
    itemDeltas: [],
    tags: 0,
  },
];

describe('txFlows', () => {
  it('reads a swap as two flows, items first then CKB, each from the left to the right', () => {
    // 0x91429fbc…: 25,442.40236569 iCKB from the limit order to rvxmvg, 30,700.918 CKB the other way.
    const flows = txFlows({
      hash: HASH,
      activity: activity([
        party('rvxmvg', '-3070091791503', [token('2544240236569')]),
        party('0q3akl', '3070091760805', [token('-2544240236569')], 'UDT Limit Order'),
      ]),
      fee: '30698',
      compensation: null,
    });
    expect(flows.map((f) => f.key)).toEqual([`token:${ICKB}`, 'ckb']);
    // the asset is named by the first item delta met: the token's hash and symbol, no amount
    expect(flows[0].asset.kind).toBe('token');
    if (flows[0].asset.kind !== 'token') throw new Error('unreachable');
    expect(flows[0].asset.item.typeScriptHash).toBe(ICKB);
    expect(flows[0].asset.item.symbol).toBe('iCKB');
    expect(amounts(flows[0].from)).toEqual([['party', '0q3akl', '2544240236569']]);
    expect(amounts(flows[0].to)).toEqual([['party', 'rvxmvg', '2544240236569']]);
    expect(flows[0].total).toBe(BigInt('2544240236569'));
    // the CKB flow: the giver, then the receiver and the miner's fee, exactly conserved
    expect(amounts(flows[1].from)).toEqual([['party', 'rvxmvg', '3070091791503']]);
    expect(amounts(flows[1].to)).toEqual([
      ['party', '0q3akl', '3070091760805'],
      ['miner', 'miner', '30698'],
    ]);
    expect(flows[1].total).toBe(BigInt('3070091791503'));
  });

  it('orders each side largest first, keeps equal amounts in participant order, and skips zero', () => {
    const flows = txFlows({
      hash: HASH,
      activity: activity([
        party('aaaaaa', '-100'),
        party('bbbbbb', '-300'),
        party('cccccc', '-100'),
        party('dddddd', '0'),
        party('eeeeee', '490'),
      ]),
      fee: '10',
      compensation: null,
    });
    expect(flows).toHaveLength(1);
    expect(amounts(flows[0].from)).toEqual([
      ['party', 'bbbbbb', '300'],
      ['party', 'aaaaaa', '100'],
      ['party', 'cccccc', '100'],
    ]);
    expect(amounts(flows[0].to)).toEqual([
      ['party', 'eeeeee', '490'],
      ['miner', 'miner', '10'],
    ]);
  });

  it('ends a burn at "burnt" and starts a mint at "minted", with the exact difference', () => {
    const burn = txFlows({
      hash: HASH,
      activity: activity([party('aaaaaa', '-5', [token('-700')])]),
      fee: '5',
      compensation: null,
    });
    expect(amounts(burn[0].to)).toEqual([['burnt', 'burnt', '700']]);
    expect(burn[0].total).toBe(BigInt(700));
    const mint = txFlows({
      hash: HASH,
      activity: activity([
        party('aaaaaa', '-5', [token('-100')]),
        party('bbbbbb', '0', [token('900')]),
      ]),
      fee: '5',
      compensation: null,
    });
    expect(amounts(mint[0].from)).toEqual([
      ['minted', 'minted', '800'],
      ['party', 'aaaaaa', '100'],
    ]);
    expect(mint[0].total).toBe(BigInt(900));
  });

  it('lets Nervos DAO compensation in first', () => {
    const dao = txFlows({
      hash: HASH,
      activity: activity([party('aaaaaa', '-2605000000000'), party('bbbbbb', '2622530633229')]),
      fee: '936',
      compensation: BigInt('17530634165'),
    });
    expect(amounts(dao[0].from)).toEqual([
      ['compensation', 'compensation', '17530634165'],
      ['party', 'aaaaaa', '2605000000000'],
    ]);
    expect(amounts(dao[0].to)).toEqual([
      ['party', 'bbbbbb', '2622530633229'],
      ['miner', 'miner', '936'],
    ]);
  });

  // Review 45fd finding 5 (ruling D6): the only block-0 transaction the page reads an activity for
  // is the genesis dep-group transaction (0x71a7…e46c, mainnet :8101 2026-11-10). It is not a
  // cellbase and issues nothing: an ordinary conserving transfer with a fee of 0.
  it('reads the genesis dep-group transaction as a plain conserving transfer with no fee', () => {
    const flows = txFlows({
      hash: '0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c',
      activity: { participants: GENESIS_DEP_GROUP_PARTICIPANTS },
      fee: '0',
      compensation: null,
    });
    expect(flows).toHaveLength(1);
    expect(flows[0].key).toBe('ckb');
    expect(amounts(flows[0].from)).toEqual([['party', 'gh76tz', '23400000000']]);
    expect(amounts(flows[0].to)).toEqual([['party', '4maw7d', '23400000000']]);
    expect(flows[0].total).toBe(BigInt('23400000000'));
  });

  it('refuses a CKB flow that does not conserve, naming the transaction', () => {
    expect(() =>
      txFlows({
        hash: HASH,
        activity: activity([party('aaaaaa', '-100'), party('bbbbbb', '95')]),
        fee: '1',
        compensation: null,
      })
    ).toThrow(`${HASH}: the CKB flow does not conserve: 100 shannons from the givers, 96`);
    expect(() =>
      txFlows({
        hash: HASH,
        activity: activity([party('aaaaaa', '1.5')]),
        fee: '0',
        compensation: null,
      })
    ).toThrow(`${HASH}: participant ckbDelta "1.5"`);
  });

  // Lane P1 (the plan's P1 paragraph: the summary states the amount from the totals the figure's
  // flows already carry): what reached the receiving parties and what left the giving parties, each
  // a flow's party ends alone — the miner's fee, a burn, a mint and Nervos DAO compensation are
  // terminals, not parties.
  it('counts what reached the receiving parties and what left the giving parties', () => {
    const swap = txFlows({
      hash: HASH,
      activity: activity([
        party('rvxmvg', '-3070091791503', [token('2544240236569')]),
        party('0q3akl', '3070091760805', [token('-2544240236569')], 'UDT Limit Order'),
      ]),
      fee: '30698',
      compensation: null,
    });
    // CKB: 0q3akl received 3,070,091,760,805 shannons; the miner's 30,698 are not a party's
    expect([swap[1].sent, swap[1].received]).toEqual([
      BigInt('3070091791503'),
      BigInt('3070091760805'),
    ]);
    expect([swap[0].sent, swap[0].received]).toEqual([
      BigInt('2544240236569'),
      BigInt('2544240236569'),
    ]);
    const burn = txFlows({
      hash: HASH,
      activity: activity([party('aaaaaa', '-5', [token('-700')])]),
      fee: '5',
      compensation: null,
    });
    expect([burn[0].sent, burn[0].received]).toEqual([BigInt(700), BigInt(0)]);
    const mint = txFlows({
      hash: HASH,
      activity: activity([
        party('aaaaaa', '-5', [token('-100')]),
        party('bbbbbb', '0', [token('900')]),
      ]),
      fee: '5',
      compensation: null,
    });
    expect([mint[0].sent, mint[0].received]).toEqual([BigInt(100), BigInt(900)]);
    const dao = txFlows({
      hash: HASH,
      activity: activity([party('aaaaaa', '-2605000000000'), party('bbbbbb', '2622530633229')]),
      fee: '936',
      compensation: BigInt('17530634165'),
    });
    expect([dao[0].sent, dao[0].received]).toEqual([
      BigInt('2605000000000'),
      BigInt('2622530633229'),
    ]);
  });

  // Lane P1 (the plan: "a transaction that moves only its fee says so in one line"): its flows are
  // the CKB flow alone, from parties to the miner and to no party, with no compensation entering.
  // Re-pinned on purpose (lane P1, third round; review p-logic-chain MUST 2): the one decision reads
  // the activity's protocol steps too, so it takes the activity beside its flows
  it('tells a transaction that moved only its fee', () => {
    const only = (participants: ParticipantInfo[], fee: string, compensation: bigint | null) => {
      const a = activity(participants);
      return onlyFeeMoved(a, txFlows({ hash: HASH, activity: a, fee, compensation }));
    };
    expect(only([party('aaaaaa', '-64119')], '64119', null)).toBe(true);
    expect(only([party('aaaaaa', '-60000'), party('bbbbbb', '-4119')], '64119', null)).toBe(true);
    // a party received CKB
    expect(only([party('aaaaaa', '-100'), party('bbbbbb', '90')], '10', null)).toBe(false);
    // an item moved
    expect(
      only(
        [party('aaaaaa', '-5', [token('-700')]), party('bbbbbb', '0', [token('700')])],
        '5',
        null
      )
    ).toBe(false);
    // compensation entered
    expect(
      only(
        [party('aaaaaa', '-2605000000000'), party('bbbbbb', '2622530633229')],
        '936',
        BigInt('17530634165')
      )
    ).toBe(false);
    // nothing moved at all: no fee either, so not "only the fee"
    expect(only([party('aaaaaa', '0')], '0', null)).toBe(false);
  });

  // Lane P1, third round (review p-logic-chain MUST 2): a Nervos DAO deposit or withdrawal request
  // from the owner's own address leaves its one party −fee and no item, and it took a step: a
  // transaction that takes a protocol step did not move only its fee.
  it('does not call a protocol step "only the fee"', () => {
    for (const action of ['deposit', 'withdraw_request']) {
      const a = {
        ...activity([{ ...party('aaaaaa', '-1000'), tags: 8 }]),
        protocolActions: [{ protocol: 'dao', protocolName: 'Nervos DAO', action, metadata: {} }],
      };
      expect(
        onlyFeeMoved(a, txFlows({ hash: HASH, activity: a, fee: '1000', compensation: null }))
      ).toBe(false);
    }
  });

  it('keys objects and identities by what they are', () => {
    expect(flowKey({ kind: 'object', objectId: '0xo1', delta: 1 })).toBe('object:0xo1');
    expect(flowKey({ kind: 'identity', standard: 'dotbit', identityId: 'a.bit', delta: -1 })).toBe(
      'identity:dotbit:a.bit'
    );
  });
});

describe('foldEnds', () => {
  const ends = (n: number): FlowEnd[] =>
    Array.from({ length: n }, (_, i) => ({
      t: 'party' as const,
      party: party(String(i).padStart(6, 'p'), '-1'),
      amount: BigInt(100 - i),
    }));

  it('shows up to six parties whole, and beyond that five plus one "more" carrying the rest', () => {
    expect(foldEnds(ends(6), false)).toHaveLength(6);
    const folded = foldEnds(ends(41), false);
    expect(folded).toHaveLength(6);
    expect(folded.slice(0, 5).map((e) => e.amount.toString())).toEqual([
      '100',
      '99',
      '98',
      '97',
      '96',
    ]);
    const more = folded[5];
    expect(more.t).toBe('more');
    if (more.t !== 'more') throw new Error('unreachable');
    expect(more.count).toBe(36);
    expect(more.ends).toHaveLength(36);
    // 95 + 94 + … + 60
    expect(more.amount.toString()).toBe(String(((95 + 60) * 36) / 2));
    expect(foldEnds(ends(41), true)).toHaveLength(41);
  });

  it('never folds a terminal: a giving one stays first, the miner stays last', () => {
    const giving = foldEnds([{ t: 'compensation', amount: BigInt(7) }, ...ends(8)], false);
    expect(giving.map((e) => e.t)).toEqual([
      'compensation',
      'party',
      'party',
      'party',
      'party',
      'party',
      'more',
    ]);
    const receiving = foldEnds([...ends(8), { t: 'miner', amount: BigInt(1) }], false);
    expect(receiving.map((e) => e.t)).toEqual([
      'party',
      'party',
      'party',
      'party',
      'party',
      'more',
      'miner',
    ]);
  });
});

describe('ribbonWeight', () => {
  it('is 2 px plus the end’s share of its side, 22 px for the whole of it', () => {
    expect(ribbonWeight(BigInt(1), BigInt(1))).toBe(22);
    expect(ribbonWeight(BigInt(1), BigInt(2))).toBe(12);
    expect(ribbonWeight(BigInt(21500000000), BigInt(131500008888))).toBe(5);
    expect(ribbonWeight(BigInt(0), BigInt(5))).toBe(2);
    expect(() => ribbonWeight(BigInt(1), BigInt(0))).toThrow('total 0');
    expect(() => ribbonWeight(BigInt(3), BigInt(2))).toThrow('not within');
  });
});

// Lane P1 follow-up (review chain-b tx-dao-deposit): a party's Nervos DAO step, from the activity's
// own kind: its DAO tag and the transaction's DAO actions, each named once.
describe('daoStepOf', () => {
  const dao = (...actions: string[]) => ({
    txHash: '0xdao',
    protocolActions: actions.map((action) => ({
      protocol: 'dao',
      protocolName: 'Nervos DAO',
      action,
      metadata: {},
    })),
  });
  it('names the step of a party the activity tags DAO, and of no other', () => {
    expect(daoStepOf(dao('deposit', 'deposit'), { tags: 8 })).toBe('Nervos DAO deposit');
    expect(daoStepOf(dao('deposit'), { tags: 0 })).toBeNull();
    expect(daoStepOf(dao(), { tags: 8 })).toBeNull();
    expect(daoStepOf(dao('withdraw_request'), { tags: 8 | 1 })).toBe(
      'Nervos DAO withdrawal request'
    );
    expect(daoStepOf(dao('withdraw_complete', 'deposit'), { tags: 8 })).toBe(
      'Nervos DAO withdrawal, Nervos DAO deposit'
    );
  });
  it('refuses a DAO step it has no words for', () => {
    expect(() => daoStepOf(dao('merge'), { tags: 8 })).toThrow('0xdao: a Nervos DAO step "merge"');
  });
});
