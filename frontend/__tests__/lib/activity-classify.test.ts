import { describe, expect, it } from 'vitest';
import type { GlobalActivity } from '@/lib/api';
import { classifyActivity } from '@/lib/activity-classify';

function makeActivity(overrides: Partial<GlobalActivity> = {}): GlobalActivity {
  return {
    txHash: overrides.txHash ?? '0xtx',
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    isCellbase: overrides.isCellbase ?? false,
    typeCalls: overrides.typeCalls ?? [],
    lockCalls: overrides.lockCalls ?? [],
    protocolActions: overrides.protocolActions ?? [],
    participants: overrides.participants ?? [
      {
        address: 'ckb1qtest',
        ckbDelta: '0',
        usedDelta: '0',
        itemDeltas: [],
        tags: 0,
      },
    ],
  };
}

describe('classifyActivity', () => {
  it('classifies DAO deposit from protocolActions', () => {
    const result = classifyActivity(
      makeActivity({
        protocolActions: [
          { protocol: 'dao', action: 'deposit', metadata: { capacity: '10200000000' } },
        ],
      })
    );
    expect(result.displayType).toBe('daoDeposit');
  });

  it('classifies DAO withdraw request from protocolActions', () => {
    const result = classifyActivity(
      makeActivity({
        protocolActions: [
          {
            protocol: 'dao',
            action: 'withdraw_request',
            metadata: { capacity: '10200000000', depositBlock: 100 },
          },
        ],
      })
    );
    expect(result.displayType).toBe('daoWithdrawRequest');
  });

  it('classifies DAO withdraw complete from protocolActions', () => {
    const result = classifyActivity(
      makeActivity({
        protocolActions: [
          {
            protocol: 'dao',
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
        participants: [
          {
            address: 'ckb1qtest',
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
        participants: [
          {
            address: 'ckb1qtest',
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
        participants: [
          {
            address: 'ckb1qtest',
            ckbDelta: '0',
            usedDelta: '0',
            itemDeltas: [{ kind: 'identity', identityId: '0xdotbit', delta: 1 }],
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

  it('classifies CKB transfer as fallback', () => {
    const result = classifyActivity(
      makeActivity({
        participants: [
          {
            address: 'ckb1qtest',
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
        protocolActions: [
          { protocol: 'dao', action: 'deposit', metadata: { capacity: '10200000000' } },
        ],
        participants: [
          {
            address: 'ckb1qtest',
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
        participants: [
          {
            address: 'ckb1qtest',
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
        protocolActions: [
          { protocol: 'dao', action: 'deposit', metadata: { capacity: '10200000000' } },
        ],
      })
    );
    expect(result.displayType).toBe('daoDeposit');
    expect(result.primaryProtocolAction).toEqual({
      protocol: 'dao',
      action: 'deposit',
      metadata: { capacity: '10200000000' },
    });
  });

  it('item delta takes priority over lock calls', () => {
    const result = classifyActivity(
      makeActivity({
        participants: [
          {
            address: 'ckb1qtest',
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
            scriptName: 'RGB++',
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
          { protocol: 'rgbpp', action: 'leap_to_ckb', metadata: { btcTxid: 'abc123' } },
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
        protocolActions: [{ protocol: 'rgbpp', action: 'transfer', metadata: {} }],
        participants: [
          {
            address: 'ckb1qtest',
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
