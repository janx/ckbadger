import type { ActivityAssetChange, ActivityTypeCall, GlobalActivity } from '@/lib/api';

export type ActivityType =
  | 'daoDeposit'
  | 'daoWithdrawRequest'
  | 'daoWithdrawComplete'
  | 'token'
  | 'object'
  | 'identity'
  | 'typeCall'
  | 'ckbTransfer';

export interface ClassifiedActivity {
  type: ActivityType;
  activity: GlobalActivity;
  primaryAssetChange: ActivityAssetChange | null;
  primaryTypeCall: ActivityTypeCall | null;
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
        primaryTypeCall: activity.typeCalls[0] ?? null,
      };
    }
  }

  if (activity.typeCalls.length > 0) {
    return {
      type: 'typeCall',
      activity,
      primaryAssetChange: null,
      primaryTypeCall: activity.typeCalls[0],
    };
  }

  return {
    type: 'ckbTransfer',
    activity,
    primaryAssetChange: null,
    primaryTypeCall: null,
  };
}
