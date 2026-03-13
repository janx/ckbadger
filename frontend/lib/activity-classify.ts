import type {
  ActivityAssetChange,
  ActivityLockCall,
  ActivityTypeCall,
  GlobalActivity,
} from '@/lib/api';

export type ActivityType =
  | 'daoDeposit'
  | 'daoWithdrawRequest'
  | 'daoWithdrawComplete'
  | 'token'
  | 'object'
  | 'identity'
  | 'protocolAction'
  | 'typeCall'
  | 'ckbTransfer';

export interface ClassifiedActivity {
  type: ActivityType;
  activity: GlobalActivity;
  primaryAssetChange: ActivityAssetChange | null;
  primaryTypeCall: ActivityTypeCall | null;
  primaryLockCall: ActivityLockCall | null;
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
  const primaryLockCall = activity.lockCalls[0] ?? null;

  // 1. Asset changes take priority
  for (const { assetType, activityType } of ASSET_TYPE_PRIORITY) {
    const match = activity.assetChanges.find((c) => c.type === assetType);
    if (match) {
      return {
        type: activityType,
        activity,
        primaryAssetChange: match,
        primaryTypeCall: activity.typeCalls[0] ?? null,
        primaryLockCall,
      };
    }
  }

  // 2. Protocol action lock calls
  const protocolAction = activity.lockCalls.find((lc) => lc.role === 'protocol_action');
  if (protocolAction) {
    return {
      type: 'protocolAction',
      activity,
      primaryAssetChange: null,
      primaryTypeCall: activity.typeCalls[0] ?? null,
      primaryLockCall: protocolAction,
    };
  }

  // 3. Type calls
  if (activity.typeCalls.length > 0) {
    return {
      type: 'typeCall',
      activity,
      primaryAssetChange: null,
      primaryTypeCall: activity.typeCalls[0],
      primaryLockCall,
    };
  }

  // 4. CKB transfer (fallback)
  return {
    type: 'ckbTransfer',
    activity,
    primaryAssetChange: null,
    primaryTypeCall: null,
    primaryLockCall,
  };
}
