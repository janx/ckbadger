/**
 * "What moved" (tx page, review 2026-11-03 v2): the flows a transaction's activity states — items
 * first, CKB last; givers and receivers by exact amount; the fee to the miner, compensation as a
 * giver, mint and burn terminals; the fold beyond six parties; ribbon weights.
 */
import { describe, expect, it } from 'vitest';
import { flowKey, foldEnds, ribbonWeight, txFlows, type FlowEnd } from '@/lib/tx-flows';
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
const activity = (participants: ParticipantInfo[]): Pick<GlobalActivity, 'participants'> => ({
  participants,
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
