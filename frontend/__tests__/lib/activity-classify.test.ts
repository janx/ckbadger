import { describe, expect, it } from 'vitest';
import type { GlobalActivity } from '@/lib/api';
import { classifyActivity } from '@/lib/activity-classify';

function makeActivity(overrides: Partial<GlobalActivity> = {}): GlobalActivity {
  return {
    address: overrides.address ?? 'ckb1qtest',
    txHash: overrides.txHash ?? '0xtx',
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    isCellbase: overrides.isCellbase ?? false,
    assetChanges: overrides.assetChanges ?? [],
    typeCalls: overrides.typeCalls ?? [],
    lockCalls: overrides.lockCalls ?? [],
    protocolActions: overrides.protocolActions ?? [],
    peers: overrides.peers ?? [],
  };
}

describe('classifyActivity', () => {
  it('classifies DAO deposit', () => {
    const result = classifyActivity(
      makeActivity({ assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }] })
    );
    expect(result.displayType).toBe('daoDeposit');
  });

  it('classifies DAO withdraw request', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [{ type: 'daoWithdrawRequest', capacity: '10200000000', depositBlock: 100 }],
      })
    );
    expect(result.displayType).toBe('daoWithdrawRequest');
  });

  it('classifies DAO withdraw complete', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'daoWithdrawComplete', capacity: '10200000000', compensation: '42000000' },
        ],
      })
    );
    expect(result.displayType).toBe('daoWithdrawComplete');
  });

  it('classifies token transfer', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'token', typeScriptHash: '0xtoken', delta: '500', symbol: 'SEAL', decimals: 8 },
        ],
      })
    );
    expect(result.displayType).toBe('token');
  });

  it('classifies object action', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [{ type: 'object', objectId: '0xspore', standard: 'spore', action: 'mint' }],
      })
    );
    expect(result.displayType).toBe('object');
  });

  it('classifies identity action', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'identity', identityId: '0xdotbit', standard: 'dotbit', action: 'update' },
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
    const result = classifyActivity(makeActivity({ ckbDelta: '-50000000000' }));
    expect(result.displayType).toBe('ckbTransfer');
  });

  it('DAO deposit takes priority over token in same activity', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
          { type: 'daoDeposit', capacity: '10200000000' },
        ],
      })
    );
    expect(result.displayType).toBe('daoDeposit');
  });

  it('token takes priority over script call', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
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

  it('returns the first matching asset change for the classified type', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }],
      })
    );
    expect(result.displayType).toBe('daoDeposit');
    expect(result.primaryAssetChange).toEqual({ type: 'daoDeposit', capacity: '10200000000' });
  });

  it('asset change takes priority over lock calls', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
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

  it('protocol action takes priority over asset changes', () => {
    const result = classifyActivity(
      makeActivity({
        protocolActions: [{ protocol: 'rgbpp', action: 'transfer', metadata: {} }],
        assetChanges: [
          { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
        ],
      })
    );
    expect(result.displayType).toBe('protocolAction');
    expect(result.primaryAssetChange?.type).toBe('token');
  });
});
