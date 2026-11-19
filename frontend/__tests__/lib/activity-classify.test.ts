import { describe, expect, it } from 'vitest';
import type { GlobalActivity } from '@/lib/api';
import { TAG_TYPE_CALL } from '@/lib/api';
import { classifyActivity } from '@/lib/activity-classify';
import { activityHeadline } from '@/components/activity-event-row';
import { activityRead, ITEM_TYPE } from '@/__tests__/utils/activity-read';

function makeActivity(overrides: Partial<GlobalActivity> = {}): GlobalActivity {
  return activityRead({
    txHash: overrides.txHash ?? '0xtx',
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    isCellbase: overrides.isCellbase ?? false,
    typeCalls: overrides.typeCalls ?? [],
    lockCalls: overrides.lockCalls ?? [],
    // the row's recognised codes as its caller states them, as the route would (the third read's T
    // SHOULD-1, carried to C7's list)
    recognizedTypes: overrides.recognizedTypes ?? [],
    protocolActions: overrides.protocolActions ?? [],
    participants: overrides.participants ?? [
      {
        address: 'ckb1qtest',
        lockHash: '0xaa',
        lockHashPrefix: null,
        sent: false,
        roles: [],
        ckbDelta: '0',
        usedDelta: '0',
        itemDeltas: [],
        tags: 0,
      },
    ],
  });
}

describe('classifyActivity', () => {
  it('classifies DAO deposit from protocolActions', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.dao],
        protocolActions: [
          {
            protocol: 'dao',
            protocolName: 'Nervos DAO',
            action: 'deposit',
            metadata: { capacity: '10200000000' },
          },
        ],
      })
    );
    expect(result.displayType).toBe('daoDeposit');
  });

  it('classifies DAO withdraw request from protocolActions', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.dao],
        protocolActions: [
          {
            protocol: 'dao',
            protocolName: 'Nervos DAO',
            action: 'withdraw_request',
            metadata: { capacity: '10200000000', depositBlock: '100' },
          },
        ],
      })
    );
    expect(result.displayType).toBe('daoWithdrawRequest');
  });

  it('classifies DAO withdraw complete from protocolActions', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.dao],
        protocolActions: [
          {
            protocol: 'dao',
            protocolName: 'Nervos DAO',
            action: 'withdraw_complete',
            metadata: { capacity: '10200000000', compensation: '42000000' },
          },
        ],
      })
    );
    expect(result.displayType).toBe('daoWithdrawComplete');
  });

  it('classifies token transfer via itemDeltas', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.xudt],
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: false,
            roles: [],
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [
              {
                kind: 'token',
                typeScriptHash: '0xtoken',
                delta: '500',
                symbol: 'SEAL',
                decimals: 8,
              },
            ],
            tags: 1,
          },
        ],
      })
    );
    expect(result.displayType).toBe('token');
  });

  it('classifies object action via itemDeltas', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.spore],
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: false,
            roles: [],
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [{ kind: 'object', objectId: '0xspore', delta: 1 }],
            tags: 2,
          },
        ],
      })
    );
    expect(result.displayType).toBe('object');
  });

  it('classifies identity action via itemDeltas', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.dotbit],
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: false,
            roles: [],
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [
              { kind: 'identity', standard: 'dotbit', identityId: '0xdotbit', delta: 1 },
            ],
            tags: 4,
          },
        ],
      })
    );
    expect(result.displayType).toBe('identity');
  });

  it('classifies script call', () => {
    const result = classifyActivity(
      makeActivity({
        typeCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Omnilock',
          },
        ],
      })
    );
    expect(result.displayType).toBe('typeCall');
  });

  // C7 (resync/type-codes): a recognised type script that ran for a party and moved none of its
  // items, no DAO step (a wallet merging its own sUDT cells), sets the party's TAG_TYPE_CALL in the
  // indexer, the bit an unrecognised call sets; its transaction is a script call, which the global
  // kind already says (Script), never a CKB transfer. It has no unrecognised call to name.
  it('classifies a recognised type that moved nothing (the party’s TAG_TYPE_CALL) as a script call', () => {
    // the indexer's bit 6 (crates/ckbadger-store/src/types.rs)
    expect(TAG_TYPE_CALL).toBe(1 << 6);
    const merge = makeActivity({
      participants: [
        {
          address: 'ckb1qtest',
          lockHash: '0xaa',
          lockHashPrefix: null,
          sent: true,
          roles: [],
          ckbDelta: '-30000',
          usedDelta: '0',
          itemDeltas: [],
          tags: TAG_TYPE_CALL,
        },
      ],
    });
    const result = classifyActivity(merge);
    expect(result.displayType).toBe('typeCall');
    expect(result.primaryTypeCall).toBeNull();
    expect(activityHeadline(merge)).toBe('Script call');
  });

  it('classifies CKB transfer as fallback', () => {
    const result = classifyActivity(
      makeActivity({
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: true,
            roles: [],
            ckbDelta: '-50000000000',
            usedDelta: '0',
            itemDeltas: [],
            tags: 0,
          },
        ],
      })
    );
    expect(result.displayType).toBe('ckbTransfer');
  });

  it('DAO deposit takes priority over token in same activity', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.xudt, ITEM_TYPE.dao],
        protocolActions: [
          {
            protocol: 'dao',
            protocolName: 'Nervos DAO',
            action: 'deposit',
            metadata: { capacity: '10200000000' },
          },
        ],
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: false,
            roles: [],
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [
              { kind: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
            ],
            tags: 9, // TAG_TOKEN | TAG_DAO
          },
        ],
      })
    );
    expect(result.displayType).toBe('daoDeposit');
  });

  it('token takes priority over script call', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.xudt],
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: false,
            roles: [],
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [
              { kind: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
            ],
            tags: 1,
          },
        ],
        typeCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Omnilock',
          },
        ],
      })
    );
    expect(result.displayType).toBe('token');
  });

  it('returns the first matching item delta for the classified type', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.dao],
        protocolActions: [
          {
            protocol: 'dao',
            protocolName: 'Nervos DAO',
            action: 'deposit',
            metadata: { capacity: '10200000000' },
          },
        ],
      })
    );
    expect(result.displayType).toBe('daoDeposit');
    expect(result.primaryProtocolAction).toEqual({
      protocol: 'dao',
      protocolName: 'Nervos DAO',
      action: 'deposit',
      metadata: { capacity: '10200000000' },
    });
  });

  it('item delta takes priority over lock calls', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.xudt],
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: false,
            roles: [],
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [
              { kind: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
            ],
            tags: 1,
          },
        ],
        lockCalls: [
          {
            lockScriptName: null,
            lockCodeHash: '0xintent',
            lockHashType: 'type',
            lockArgs: '0xargs',
            scriptHash: '0xhash',
          },
        ],
      })
    );
    expect(result.displayType).toBe('token');
  });

  it('lock call without protocol action does not create protocolAction type', () => {
    const result = classifyActivity(
      makeActivity({
        lockCalls: [
          {
            lockCodeHash: '0xrgbpp',
            lockHashType: 'type',
            lockArgs: '0xargs',
            scriptHash: '0xhash',
            lockScriptName: 'RGB++',
          },
        ],
      })
    );
    expect(result.displayType).toBe('ckbTransfer');
  });

  it('classifies protocol action as protocolAction', () => {
    const result = classifyActivity(
      makeActivity({
        protocolActions: [
          {
            protocol: 'rgbpp',
            protocolName: 'RGB++',
            action: 'leap_to_ckb',
            metadata: { btcTxid: 'abc123' },
          },
        ],
      })
    );
    expect(result.displayType).toBe('protocolAction');
    expect(result.primaryProtocolAction?.protocol).toBe('rgbpp');
    expect(result.primaryProtocolAction?.action).toBe('leap_to_ckb');
  });

  it('protocol action takes priority over item deltas', () => {
    const result = classifyActivity(
      makeActivity({
        recognizedTypes: [ITEM_TYPE.xudt],
        protocolActions: [
          { protocol: 'rgbpp', protocolName: 'RGB++', action: 'transfer', metadata: {} },
        ],
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: false,
            roles: [],
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [
              { kind: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
            ],
            tags: 1,
          },
        ],
      })
    );
    expect(result.displayType).toBe('protocolAction');
    expect(result.primaryItemDelta?.kind).toBe('token');
  });

  it('ckbTransfer only when no type script involved', () => {
    const result = classifyActivity(
      makeActivity({
        participants: [
          {
            address: 'ckb1qtest',
            lockHash: '0xaa',
            lockHashPrefix: null,
            sent: true,
            roles: [],
            ckbDelta: '-50000000000',
            usedDelta: '0',
            itemDeltas: [],
            tags: 0,
          },
        ],
      })
    );
    expect(result.displayType).toBe('ckbTransfer');
  });
});
