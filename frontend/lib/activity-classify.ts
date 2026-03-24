import type {
  ItemDelta,
  ActivityLockCall,
  ActivityProtocolAction,
  ActivityTypeCall,
  GlobalActivity,
} from '@/lib/api';
import { TAG_TOKEN, TAG_OBJECT, TAG_IDENTITY, TAG_CELLBASE } from '@/lib/api';

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
 *   Layer 2: Item Deltas       — WHAT (recognized asset mutations / script calls)
 *   Layer 1: CKB Position      — HOW MUCH (ckbDelta, usedDelta — per participant)
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
  /** Layer 2: primary recognized item delta, if present. */
  primaryItemDelta: ItemDelta | null;
  /** Layer 2: primary unrecognized type script call, if present. */
  primaryTypeCall: ActivityTypeCall | null;
  /** Layer 2: primary non-standard lock script, if present. */
  primaryLockCall: ActivityLockCall | null;
  /* Layer 1: CKB position (ckbDelta, usedDelta) is per participant — not repeated here. */
}

/** Find the first item delta across all participants. */
function findFirstItemDelta(activity: GlobalActivity): ItemDelta | null {
  for (const p of activity.participants) {
    if (p.itemDeltas.length > 0) {
      return p.itemDeltas[0];
    }
  }
  return null;
}

/** Find the first item delta of a specific kind across all participants. */
function findFirstItemDeltaByKind(
  activity: GlobalActivity,
  kind: string
): ItemDelta | null {
  for (const p of activity.participants) {
    const match = p.itemDeltas.find((d) => d.kind === kind);
    if (match) return match;
  }
  return null;
}

/** Compute combined tags bitmask from all participants. */
function combinedTags(activity: GlobalActivity): number {
  let tags = 0;
  for (const p of activity.participants) {
    tags |= p.tags;
  }
  return tags;
}

/**
 * DAO actions are now in protocolActions, not in itemDeltas.
 * Returns the DAO display type and protocol action, or null if no DAO action found.
 */
function findDaoAction(
  activity: GlobalActivity
): { displayType: ActivityType; action: ActivityProtocolAction } | null {
  for (const pa of activity.protocolActions) {
    if (pa.protocol === 'dao') {
      switch (pa.action) {
        case 'deposit':
          return { displayType: 'daoDeposit', action: pa };
        case 'withdraw_request':
          return { displayType: 'daoWithdrawRequest', action: pa };
        case 'withdraw_complete':
          return { displayType: 'daoWithdrawComplete', action: pa };
      }
    }
  }
  return null;
}

const ITEM_KIND_PRIORITY: Array<{ kind: string; activityType: ActivityType }> = [
  { kind: 'token', activityType: 'token' },
  { kind: 'object', activityType: 'object' },
  { kind: 'identity', activityType: 'identity' },
];

export function classifyActivity(activity: GlobalActivity): ClassifiedActivity {
  const primaryLockCall = activity.lockCalls[0] ?? null;
  const tags = combinedTags(activity);

  // Layer 3: Protocol actions — highest level interpretation
  if (activity.protocolActions.length > 0) {
    // Check for DAO actions specifically
    const daoResult = findDaoAction(activity);
    if (daoResult) {
      return {
        displayType: daoResult.displayType,
        activity,
        primaryItemDelta: findFirstItemDelta(activity),
        primaryTypeCall: activity.typeCalls[0] ?? null,
        primaryLockCall,
        primaryProtocolAction: daoResult.action,
      };
    }

    // Non-DAO protocol actions
    return {
      displayType: 'protocolAction',
      activity,
      primaryItemDelta: findFirstItemDelta(activity),
      primaryTypeCall: activity.typeCalls[0] ?? null,
      primaryLockCall,
      primaryProtocolAction: activity.protocolActions[0],
    };
  }

  // Layer 2: Item deltas (via tags bitmask for fast check, then find actual delta)
  for (const { kind, activityType } of ITEM_KIND_PRIORITY) {
    const tagBit =
      kind === 'token' ? TAG_TOKEN : kind === 'object' ? TAG_OBJECT : TAG_IDENTITY;
    if (tags & tagBit) {
      const match = findFirstItemDeltaByKind(activity, kind);
      if (match) {
        return {
          displayType: activityType,
          activity,
          primaryItemDelta: match,
          primaryTypeCall: activity.typeCalls[0] ?? null,
          primaryLockCall,
          primaryProtocolAction: null,
        };
      }
    }
  }

  // Layer 2 (catch-all): Unrecognized type calls
  if (activity.typeCalls.length > 0) {
    return {
      displayType: 'typeCall',
      activity,
      primaryItemDelta: null,
      primaryTypeCall: activity.typeCalls[0],
      primaryLockCall,
      primaryProtocolAction: null,
    };
  }

  // Cellbase or CKB transfer — Layer 1 only
  if (tags & TAG_CELLBASE) {
    return {
      displayType: 'ckbTransfer',
      activity,
      primaryItemDelta: null,
      primaryTypeCall: null,
      primaryLockCall,
      primaryProtocolAction: null,
    };
  }

  return {
    displayType: 'ckbTransfer',
    activity,
    primaryItemDelta: null,
    primaryTypeCall: null,
    primaryLockCall,
    primaryProtocolAction: null,
  };
}
