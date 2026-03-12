import type { ActivityAssetChange, ActivityScriptCall, GlobalActivity } from '@/lib/api';

export interface LatestActivityGroup {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  participants: GlobalActivity[];
  participantCount: number;
}

function deltaBucket(delta: bigint): number {
  if (delta < BigInt(0)) return 0;
  if (delta > BigInt(0)) return 1;
  return 2;
}

function absoluteDelta(delta: bigint): bigint {
  return delta < BigInt(0) ? -delta : delta;
}

function compareParticipants(
  left: { item: GlobalActivity; index: number },
  right: { item: GlobalActivity; index: number }
): number {
  const leftDelta = BigInt(left.item.ckbDelta);
  const rightDelta = BigInt(right.item.ckbDelta);
  const bucketDiff = deltaBucket(leftDelta) - deltaBucket(rightDelta);
  if (bucketDiff !== 0) {
    return bucketDiff;
  }

  const absDeltaDiff = absoluteDelta(rightDelta) - absoluteDelta(leftDelta);
  if (absDeltaDiff !== BigInt(0)) {
    return absDeltaDiff > BigInt(0) ? 1 : -1;
  }

  const eventDiff =
    right.item.assetChanges.length +
    right.item.scriptCalls.length -
    (left.item.assetChanges.length + left.item.scriptCalls.length);
  if (eventDiff !== 0) {
    return eventDiff;
  }

  return left.index - right.index;
}

function formatObjectStandard(standard: string): string {
  if (standard === 'spore') return 'Spore';
  if (standard === 'm-nft') return 'M-NFT';
  return standard;
}

function formatIdentityStandard(standard: string): string {
  if (standard === 'dotbit') return '.bit';
  if (standard === 'did_ckb') return 'did:ckb';
  return standard;
}

function collectAssetChanges(group: LatestActivityGroup): ActivityAssetChange[] {
  return group.participants.flatMap((participant) => participant.assetChanges);
}

function collectScriptCalls(group: LatestActivityGroup): ActivityScriptCall[] {
  return group.participants.flatMap((participant) => participant.scriptCalls);
}

function formatEventCount(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export function buildLatestActivityGroupSummary(group: LatestActivityGroup): string {
  const assetChanges = collectAssetChanges(group);
  const scriptCalls = collectScriptCalls(group);

  if (assetChanges.some((change) => change.type === 'daoDeposit')) {
    return 'DAO deposit';
  }
  if (assetChanges.some((change) => change.type === 'daoWithdrawRequest')) {
    return 'DAO withdraw request';
  }
  if (assetChanges.some((change) => change.type === 'daoWithdrawComplete')) {
    return 'DAO withdraw complete';
  }

  const objectChange = assetChanges.find((change) => change.type === 'object');
  if (objectChange?.type === 'object') {
    return `${formatObjectStandard(objectChange.standard)} ${objectChange.action}`;
  }

  const identityChange = assetChanges.find((change) => change.type === 'identity');
  if (identityChange?.type === 'identity') {
    return `${formatIdentityStandard(identityChange.standard)} ${identityChange.action}`;
  }

  const sentCount = group.participants.filter(
    (participant) => BigInt(participant.ckbDelta) < BigInt(0)
  ).length;
  const receivedCount = group.participants.filter(
    (participant) => BigInt(participant.ckbDelta) > BigInt(0)
  ).length;
  const assetEventCount = assetChanges.length;
  const scriptCallCount = scriptCalls.length;

  const summary = `${sentCount} sent · ${receivedCount} received`;
  if (assetEventCount === 0 && scriptCallCount === 0) {
    return summary;
  }

  const parts = [summary];
  if (assetEventCount > 0) {
    parts.push(formatEventCount(assetEventCount, 'asset event', 'asset events'));
  }
  if (scriptCallCount > 0) {
    parts.push(formatEventCount(scriptCallCount, 'script call', 'script calls'));
  }
  return parts.join(' · ');
}

export function groupLatestActivitiesByTx(activities: GlobalActivity[]): LatestActivityGroup[] {
  const grouped = new Map<
    string,
    { meta: GlobalActivity; participants: Array<{ item: GlobalActivity; index: number }> }
  >();

  activities.forEach((activity, index) => {
    const existing = grouped.get(activity.txHash);
    if (existing) {
      existing.participants.push({ item: activity, index });
      return;
    }

    grouped.set(activity.txHash, {
      meta: activity,
      participants: [{ item: activity, index }],
    });
  });

  return Array.from(grouped.values()).map(({ meta, participants }) => {
    const sortedParticipants = [...participants].sort(compareParticipants).map(({ item }) => item);

    return {
      txHash: meta.txHash,
      blockNumber: meta.blockNumber,
      txIndex: meta.txIndex,
      timestamp: meta.timestamp,
      participants: sortedParticipants,
      participantCount: sortedParticipants.length,
    };
  });
}
