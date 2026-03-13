import type { ActivityAssetChange, ActivityScriptCall, GlobalActivity } from '@/lib/api';

export type ActivityType =
  | 'daoDeposit'
  | 'daoWithdrawRequest'
  | 'daoWithdrawComplete'
  | 'token'
  | 'object'
  | 'identity'
  | 'scriptCall'
  | 'ckbTransfer';

export interface ClassifiedActivity {
  type: ActivityType;
  activity: GlobalActivity;
  primaryAssetChange: ActivityAssetChange | null;
  primaryScriptCall: ActivityScriptCall | null;
}

const ASSET_TYPE_PRIORITY: Array<{ assetType: string; activityType: ActivityType }> = [
  { assetType: 'daoDeposit', activityType: 'daoDeposit' },
  { assetType: 'daoWithdrawRequest', activityType: 'daoWithdrawRequest' },
  { assetType: 'daoWithdrawComplete', activityType: 'daoWithdrawComplete' },
  { assetType: 'token', activityType: 'token' },
  { assetType: 'object', activityType: 'object' },
  { assetType: 'identity', activityType: 'identity' },
];

export function classifyActivity(activity: GlobalActivity): ClassifiedActivity {
  for (const { assetType, activityType } of ASSET_TYPE_PRIORITY) {
    const match = activity.assetChanges.find((c) => c.type === assetType);
    if (match) {
      return {
        type: activityType,
        activity,
        primaryAssetChange: match,
        primaryScriptCall: activity.scriptCalls[0] ?? null,
      };
    }
  }

  if (activity.scriptCalls.length > 0) {
    return {
      type: 'scriptCall',
      activity,
      primaryAssetChange: null,
      primaryScriptCall: activity.scriptCalls[0],
    };
  }

  return {
    type: 'ckbTransfer',
    activity,
    primaryAssetChange: null,
    primaryScriptCall: null,
  };
}
