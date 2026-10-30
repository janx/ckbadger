import { describe, it, expect } from 'vitest';
import {
  balanceNet,
  capacityEquation,
  shortCkb,
  txLinks,
  txMovements,
  type TxLinkDep,
  type TxLinkGroup,
} from '@/lib/tx-detail-model';
import { feedSamples } from '@/__tests__/msw/fixtures/home';

const HASH = '0xbe1b9761b6258b002769a0ff356d4b45469d467a08e978604506c2d2df936cd8';

// The Main.dc.html transaction: 3 inputs, 3 outputs, 3 groups that ran and 3 attached locks.
const GROUPS: TxLinkGroup[] = [
  { id: 'L1', cellIds: ['in:0', 'in:1', 'in:2'], witnessIndex: 0, scriptHash: '0xl1' },
  { id: 'T1', cellIds: ['in:0', 'out:0'], witnessIndex: 1, scriptHash: '0xt1' },
  { id: 'T2', cellIds: ['in:2', 'out:1'], witnessIndex: 2, scriptHash: '0xt2' },
  { id: 'O1', cellIds: ['out:0'], witnessIndex: null, scriptHash: '0xo1' },
  { id: 'O2', cellIds: ['out:1'], witnessIndex: null, scriptHash: '0xo2' },
  { id: 'O3', cellIds: ['out:2'], witnessIndex: null, scriptHash: '0xl1' },
];
const DEPS: TxLinkDep[] = [
  { index: 0, scriptHashes: ['0xl1'] },
  { index: 1, scriptHashes: ['0xt1'] },
  { index: 2, scriptHashes: [] },
  { index: 3, scriptHashes: ['0xt2'] },
];

describe('txLinks (TX-14: linked by group identity)', () => {
  it('lights a cell, its groups, their witnesses and their deps', () => {
    const on = txLinks({ t: 'cell', id: 'in:2' }, GROUPS, DEPS, HASH);
    expect([...on.cells]).toEqual(['in:2']);
    expect([...on.groups].sort()).toEqual(['L1', 'T2']);
    expect([...on.witnesses].sort()).toEqual([0, 2]);
    expect([...on.deps].sort()).toEqual([0, 3]);
  });

  it('lights only the hovered group, its cells, witness and dep — not groups sharing a cell', () => {
    const on = txLinks({ t: 'group', id: 'L1' }, GROUPS, DEPS, HASH);
    expect([...on.groups]).toEqual(['L1']);
    expect([...on.cells].sort()).toEqual(['in:0', 'in:1', 'in:2']);
    expect([...on.witnesses]).toEqual([0]);
    // The attached change lock O3 shares the script hash, so its code dep is the same dep 0.
    expect([...on.deps]).toEqual([0]);
  });

  it('lights the groups a witness is read by, never the groups that merely share its cells', () => {
    const on = txLinks({ t: 'witness', index: 1 }, GROUPS, DEPS, HASH);
    expect([...on.groups]).toEqual(['T1']);
    expect([...on.cells].sort()).toEqual(['in:0', 'out:0']);
    expect([...on.witnesses]).toEqual([1]);
    expect([...on.deps]).toEqual([1]);
  });

  it('lights a dep that supplies no group here on its own', () => {
    const on = txLinks({ t: 'dep', index: 2 }, GROUPS, DEPS, HASH);
    expect([...on.deps]).toEqual([2]);
    expect(on.groups.size).toBe(0);
    expect(on.cells.size).toBe(0);
  });

  it('lights a dep and every group its code serves', () => {
    const on = txLinks({ t: 'dep', index: 3 }, GROUPS, DEPS, HASH);
    expect([...on.groups]).toEqual(['T2']);
    expect([...on.cells].sort()).toEqual(['in:2', 'out:1']);
    expect([...on.witnesses]).toEqual([2]);
  });

  it('lights both cells of a continuity link and the groups of either', () => {
    const on = txLinks({ t: 'cells', ids: ['in:0', 'out:0'] }, GROUPS, DEPS, HASH);
    expect([...on.cells].sort()).toEqual(['in:0', 'out:0']);
    expect([...on.groups].sort()).toEqual(['L1', 'O1', 'T1']);
  });

  it('fails loudly on an unknown group or dep', () => {
    expect(() => txLinks({ t: 'group', id: 'nope' }, GROUPS, DEPS, HASH)).toThrow(
      `${HASH}: no script group nope`
    );
    expect(() => txLinks({ t: 'dep', index: 9 }, GROUPS, DEPS, HASH)).toThrow(
      `${HASH}: no cell dep 9`
    );
  });
});

describe('balanceNet (TX-19 Net row)', () => {
  const participants = feedSamples.objectTransfer.participants;
  it('sums the participant changes and names the fee as their difference', () => {
    expect(
      balanceNet({ hash: HASH, participants, fee: '64119', compensation: null, genesis: false })
    ).toEqual({ ckb: BigInt(-64119), occupied: BigInt(2600000000), note: 'fee' });
  });

  it('names DAO compensation less the fee when a withdrawal pays it', () => {
    const paid = [
      { ckbDelta: '-2605000000000', usedDelta: '-10200000000' },
      { ckbDelta: '2622530633229', usedDelta: '6100000000' },
    ];
    expect(
      balanceNet({
        hash: HASH,
        participants: paid,
        fee: '936',
        compensation: BigInt(17530634165),
        genesis: false,
      })
    ).toEqual({ ckb: BigInt(17530633229), occupied: BigInt(-4100000000), note: 'compensation' });
  });

  it('fails loudly when the changes do not add up to the fee', () => {
    expect(() =>
      balanceNet({ hash: HASH, participants, fee: '58803', compensation: null, genesis: false })
    ).toThrow(`${HASH}: participant CKB changes sum to -64119, expected -58803`);
  });

  it('marks genesis issuance without asserting a fee', () => {
    expect(
      balanceNet({
        hash: HASH,
        participants: [{ ckbDelta: '100', usedDelta: '0' }],
        fee: '0',
        compensation: null,
        genesis: true,
      }).note
    ).toBe('genesis');
  });
});

describe('capacityEquation (TX-19, TX-33)', () => {
  it('states in = out + fee for a plain transaction', () => {
    expect(
      capacityEquation({
        hash: HASH,
        inputs: '4258352741621',
        outputs: '4258352677502',
        fee: '64119',
        compensation: null,
      })
    ).toEqual({
      inputs: BigInt('4258352741621'),
      outputs: BigInt('4258352677502'),
      fee: BigInt(64119),
      compensation: null,
    });
  });

  it('adds the compensation a DAO withdrawal pays on the inflow side', () => {
    expect(
      capacityEquation({
        hash: HASH,
        inputs: '781583043832627',
        outputs: '784155903390101',
        fee: '936',
        compensation: BigInt('2572859558410'),
      }).compensation
    ).toBe(BigInt('2572859558410'));
  });

  it('fails loudly when capacity is not conserved', () => {
    expect(() =>
      capacityEquation({
        hash: HASH,
        inputs: '781583043832627',
        outputs: '784155903390101',
        fee: '936',
        compensation: null,
      })
    ).toThrow(`${HASH}: capacity is not conserved`);
  });
});

describe('txMovements (TX-11 summary)', () => {
  it('names an object moving from the party that lost it to the party that gained it', () => {
    const [move, ...rest] = txMovements(feedSamples.objectTransfer);
    expect(rest).toHaveLength(0);
    expect(move.item).toMatchObject({ kind: 'object' });
    expect(move.from.map((p) => p.address)).toEqual([
      feedSamples.objectTransfer.participants[0].address,
    ]);
    expect(move.to.map((p) => p.address)).toEqual([
      feedSamples.objectTransfer.participants[1].address,
    ]);
  });

  it('names CKB from the parties that lost it to the parties that gained it', () => {
    const [move] = txMovements(feedSamples.eightParties);
    expect(move.item).toBeNull();
    expect(move.from.every((p) => BigInt(p.ckbDelta) < BigInt(0))).toBe(true);
    expect(move.to.every((p) => BigInt(p.ckbDelta) > BigInt(0))).toBe(true);
  });

  it('says nothing moved when a protocol action describes the transaction', () => {
    expect(txMovements(feedSamples.daoWithdrawRequest)).toEqual([]);
  });
});

describe('shortCkb (script-group matrix columns)', () => {
  it('keeps whole CKB and marks a dropped fraction', () => {
    expect(shortCkb(BigInt('19500000000'))).toBe('195');
    expect(shortCkb(BigInt('4208152741621'))).toBe('42,081.5…');
  });
});
