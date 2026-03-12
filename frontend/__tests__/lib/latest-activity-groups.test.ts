import { describe, expect, it } from 'vitest';
import type { ActivityAssetChange, GlobalActivity } from '@/lib/api';
import {
  buildLatestActivityGroupSummary,
  groupLatestActivitiesByTx,
} from '@/lib/latest-activity-groups';

function makeActivity(
  overrides: Partial<GlobalActivity> & Pick<GlobalActivity, 'address' | 'txHash'>
): GlobalActivity {
  return {
    address: overrides.address,
    txHash: overrides.txHash,
    blockNumber: overrides.blockNumber ?? 1_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    isCellbase: overrides.isCellbase ?? false,
    assetChanges: overrides.assetChanges ?? [],
    peers: overrides.peers ?? [],
  };
}

function tokenChange(delta: string, symbol = 'SEAL'): ActivityAssetChange {
  return {
    type: 'token',
    typeScriptHash: '0xtoken',
    delta,
    symbol,
    decimals: 8,
  };
}

describe('latest activity groups', () => {
  it('groups multiple activities sharing the same tx hash into one transaction group', () => {
    const activities: GlobalActivity[] = [
      makeActivity({
        address: 'ckb1latest',
        txHash: '0xtx-latest',
        blockNumber: 1_200,
        timestamp: '1700000200',
      }),
      makeActivity({
        address: 'ckb1sender',
        txHash: '0xtx-shared',
        blockNumber: 1_199,
        timestamp: '1700000100',
        ckbDelta: '-10000000000',
      }),
      makeActivity({
        address: 'ckb1receiver',
        txHash: '0xtx-shared',
        blockNumber: 1_199,
        timestamp: '1700000100',
        ckbDelta: '10000000000',
      }),
    ];

    const groups = groupLatestActivitiesByTx(activities);

    expect(groups).toHaveLength(2);
    expect(groups[0].txHash).toBe('0xtx-latest');
    expect(groups[1].txHash).toBe('0xtx-shared');
    expect(groups[1].participants).toHaveLength(2);
  });

  it('sorts participants with negative deltas first, then positive, then zero', () => {
    const activities: GlobalActivity[] = [
      makeActivity({
        address: 'ckb1zero',
        txHash: '0xtx-sort',
        ckbDelta: '0',
      }),
      makeActivity({
        address: 'ckb1positive-small',
        txHash: '0xtx-sort',
        ckbDelta: '3000000000',
      }),
      makeActivity({
        address: 'ckb1negative-large',
        txHash: '0xtx-sort',
        ckbDelta: '-9000000000',
      }),
      makeActivity({
        address: 'ckb1positive-large',
        txHash: '0xtx-sort',
        ckbDelta: '7000000000',
      }),
      makeActivity({
        address: 'ckb1negative-small',
        txHash: '0xtx-sort',
        ckbDelta: '-1000000000',
      }),
    ];

    const [group] = groupLatestActivitiesByTx(activities);

    expect(group.participants.map((item) => item.address)).toEqual([
      'ckb1negative-large',
      'ckb1negative-small',
      'ckb1positive-large',
      'ckb1positive-small',
      'ckb1zero',
    ]);
  });

  it('prefers dao summaries over structural fallback text', () => {
    const activities: GlobalActivity[] = [
      makeActivity({
        address: 'ckb1dao',
        txHash: '0xtx-dao',
        ckbDelta: '-10200000000',
        assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }],
      }),
      makeActivity({
        address: 'ckb1peer',
        txHash: '0xtx-dao',
        ckbDelta: '10200000000',
      }),
    ];

    const [group] = groupLatestActivitiesByTx(activities);

    expect(buildLatestActivityGroupSummary(group)).toBe('DAO deposit');
  });

  it('prefers object and identity summaries over structural fallback text', () => {
    const sporeActivities: GlobalActivity[] = [
      makeActivity({
        address: 'ckb1spore-sender',
        txHash: '0xtx-spore',
        ckbDelta: '-100000000',
        assetChanges: [
          { type: 'object', objectId: '0xspore', standard: 'spore', action: 'transfer' },
        ],
      }),
      makeActivity({
        address: 'ckb1spore-receiver',
        txHash: '0xtx-spore',
        ckbDelta: '90000000',
      }),
    ];
    const dotbitActivities: GlobalActivity[] = [
      makeActivity({
        address: 'ckb1dotbit',
        txHash: '0xtx-dotbit',
        assetChanges: [
          { type: 'identity', identityId: '0xdotbit', standard: 'dotbit', action: 'update' },
        ],
      }),
    ];

    const [sporeGroup] = groupLatestActivitiesByTx(sporeActivities);
    const [dotbitGroup] = groupLatestActivitiesByTx(dotbitActivities);

    expect(buildLatestActivityGroupSummary(sporeGroup)).toBe('Spore transfer');
    expect(buildLatestActivityGroupSummary(dotbitGroup)).toBe('.bit update');
  });

  it('falls back to sent and received counts and appends asset event totals', () => {
    const activities: GlobalActivity[] = [
      makeActivity({
        address: 'ckb1sender-a',
        txHash: '0xtx-fallback',
        ckbDelta: '-10000000000',
        assetChanges: [tokenChange('-500')],
      }),
      makeActivity({
        address: 'ckb1sender-b',
        txHash: '0xtx-fallback',
        ckbDelta: '-2000000000',
      }),
      makeActivity({
        address: 'ckb1receiver',
        txHash: '0xtx-fallback',
        ckbDelta: '11900000000',
        assetChanges: [tokenChange('500')],
      }),
    ];

    const [group] = groupLatestActivitiesByTx(activities);

    expect(buildLatestActivityGroupSummary(group)).toBe('2 sent · 1 received · 2 asset events');
  });
});
