import type {
  ClassifiedItem,
  ActivityLockCall,
  ActivityProtocolAction,
  ActivityTypeCall,
  GlobalActivity,
} from '@/lib/api';
import { TAG_TYPE_CALL, type PartyId } from '@/lib/api';
import { partyIds, rowItems } from '@/lib/activity-parties';

/**
 * Interpretation needs only transaction contents, including before a block is known: its items as
 * the route states them once (lane feed, part 4; an address's own page, its own items), never the
 * parties a row draws.
 */
export type ActivityBody = Readonly<
  Pick<
    GlobalActivity,
    'participants' | 'protocolActions' | 'typeCalls' | 'lockCalls' | 'isCellbase'
  >
> & {
  readonly txHash?: string;
  readonly items: readonly ClassifiedItem[];
  /** Every party, by its short id and tags (the route's; C7 reads a party's TAG_TYPE_CALL here). */
  readonly participantCount: number;
  readonly partyIds: readonly PartyId[];
};

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
export interface ClassifiedActivity<T extends ActivityBody = ActivityBody> {
  /** Lossy projection for badge/icon/color — picks the highest non-empty layer. */
  displayType: ActivityType;
  activity: T;
  /** Layer 3: highest-level interpretation, if a ProtocolDetector matched. */
  primaryProtocolAction: ActivityProtocolAction | null;
  /** Layer 2: the primary item, if present, with its movement as the route states it. */
  primaryItemDelta: ClassifiedItem | null;
  /** Layer 2: primary unrecognized type script call, if present. */
  primaryTypeCall: ActivityTypeCall | null;
  /** Layer 2: primary non-standard lock script, if present. */
  primaryLockCall: ActivityLockCall | null;
  /* Layer 1: CKB position (ckbDelta, usedDelta) is per participant — not repeated here. */
}

/** The first of the row's stated items, in the order they first appear among its parties. */
function findFirstItemDelta(activity: ActivityBody): ClassifiedItem | null {
  return rowItems(activity)[0] ?? null;
}

/** The first of the row's stated items of a kind. */
function findFirstItemDeltaByKind(activity: ActivityBody, kind: string): ClassifiedItem | null {
  return rowItems(activity).find((i) => i.kind === kind) ?? null;
}

/**
 * DAO actions are now in protocolActions, not in itemDeltas.
 * Returns the DAO display type and protocol action, or null if no DAO action found.
 */
function findDaoAction(
  activity: ActivityBody
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

export function classifyActivity<T extends ActivityBody>(activity: T): ClassifiedActivity<T> {
  const primaryLockCall = activity.lockCalls[0] ?? null;

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

  // Layer 2: the row's items, the first kind of the three it moved
  for (const { kind, activityType } of ITEM_KIND_PRIORITY) {
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

  // Layer 2 (C7): a recognised type script ran and moved no item and no DAO step (a wallet merging
  // its own token cells): the indexer sets the party's TAG_TYPE_CALL, as for an unrecognised call,
  // and the code is in the row's `recognizedTypes`. A script call, with no unrecognised call. Read
  // from every party's tags as the route names them (`partyIds`): a row draws only some parties.
  const named = partyIds({
    txHash: activity.txHash ?? 'uncommitted',
    participantCount: activity.participantCount,
    partyIds: activity.partyIds,
  });
  if (named.some(([, tags]) => (tags & TAG_TYPE_CALL) !== 0)) {
    return {
      displayType: 'typeCall',
      activity,
      primaryItemDelta: null,
      primaryTypeCall: null,
      primaryLockCall,
      primaryProtocolAction: null,
    };
  }

  // Cellbase or CKB transfer — Layer 1 only
  return {
    displayType: 'ckbTransfer',
    activity,
    primaryItemDelta: null,
    primaryTypeCall: null,
    primaryLockCall,
    primaryProtocolAction: null,
  };
}
