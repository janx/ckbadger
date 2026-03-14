import type {
  ActivityAssetChange,
  ActivityLockCall,
  ActivityProtocolAction,
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

/**
 * Layered activity analysis.
 *
 * Three layers are composable, not mutually exclusive — a single activity
 * may have signals at all three layers simultaneously:
 *
 *   Layer 3: Protocol Action   — WHY (cross-script pattern interpretation)
 *   Layer 2: Asset Change      — WHAT (recognized asset mutations / script calls)
 *   Layer 1: CKB Position      — HOW MUCH (ckbDelta, usedDelta — always present)
 *
 * `displayType` is a lossy projection of the layered analysis, used for
 * badge/icon/color selection. It picks a headline; it does NOT mean the
 * other layers are absent or unimportant.
 */
export interface ClassifiedActivity {
  /** Lossy projection for badge/icon/color — picks the highest non-empty layer. */
  displayType: ActivityType;
  activity: GlobalActivity;
  /** Layer 3: highest-level interpretation, if a ProtocolDetector matched. */
  primaryProtocolAction: ActivityProtocolAction | null;
  /** Layer 2: primary recognized asset change, if present. */
  primaryAssetChange: ActivityAssetChange | null;
  /** Layer 2: primary unrecognized type script call, if present. */
  primaryTypeCall: ActivityTypeCall | null;
  /** Layer 2: primary non-standard lock script, if present. */
  primaryLockCall: ActivityLockCall | null;
  /* Layer 1: CKB position (ckbDelta, usedDelta) is always in activity — not repeated here. */
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

  // Layer 3: Protocol actions — highest level interpretation
  if (activity.protocolActions.length > 0) {
    return {
      displayType: 'protocolAction',
      activity,
      primaryAssetChange: activity.assetChanges[0] ?? null,
      primaryTypeCall: activity.typeCalls[0] ?? null,
      primaryLockCall,
      primaryProtocolAction: activity.protocolActions[0],
    };
  }

  // Layer 2: Asset changes
  for (const { assetType, activityType } of ASSET_TYPE_PRIORITY) {
    const match = activity.assetChanges.find((c) => c.type === assetType);
    if (match) {
      return {
        displayType: activityType,
        activity,
        primaryAssetChange: match,
        primaryTypeCall: activity.typeCalls[0] ?? null,
        primaryLockCall,
        primaryProtocolAction: null,
      };
    }
  }

  // Layer 2 (catch-all): Protocol action lock calls
  const protocolAction = activity.lockCalls.find((lc) => lc.role === 'protocol_action');
  if (protocolAction) {
    return {
      displayType: 'protocolAction',
      activity,
      primaryAssetChange: null,
      primaryTypeCall: activity.typeCalls[0] ?? null,
      primaryLockCall: protocolAction,
      primaryProtocolAction: null,
    };
  }

  // Layer 2 (catch-all): Unrecognized type calls
  if (activity.typeCalls.length > 0) {
    return {
      displayType: 'typeCall',
      activity,
      primaryAssetChange: null,
      primaryTypeCall: activity.typeCalls[0],
      primaryLockCall,
      primaryProtocolAction: null,
    };
  }

  // 4. CKB transfer — Layer 1 only (Layers 2 and 3 are empty)
  return {
    displayType: 'ckbTransfer',
    activity,
    primaryAssetChange: null,
    primaryTypeCall: null,
    primaryLockCall,
    primaryProtocolAction: null,
  };
}
