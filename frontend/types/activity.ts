export type ActivityType =
  | 'CKB_TRANSFER'
  | 'CELLBASE_REWARD'
  | 'TOKEN_MINT'
  | 'TOKEN_TRANSFER'
  | 'TOKEN_BURN'
  | 'DOB_MINT'
  | 'DOB_TRANSFER'
  | 'DOB_BURN'
  | 'NFT_MINT'
  | 'NFT_TRANSFER'
  | 'DAO_DEPOSIT'
  | 'DAO_WITHDRAW_REQUEST'
  | 'DAO_WITHDRAW_COMPLETE'
  | 'SCRIPT_DEPLOY'
  | 'RGBPP_TRANSFER'
  | 'RGBPP_LEAP_IN'
  | 'RGBPP_LEAP_OUT'
  | 'RGBPP_ISSUANCE';

export type ActivityCategory =
  | 'ckb'
  | 'cellbase'
  | 'token'
  | 'dob'
  | 'nft'
  | 'dao'
  | 'script'
  | 'rgbpp';

export interface Activity {
  activityId: string;
  activityType: ActivityType;
  activityCategory: ActivityCategory;
  blockNumber: number;
  txHash: string;
  txIndex: number;
  activityIndex: number;
  fromAddress: string | null;
  toAddress: string | null;
  fromLockHash: string | null;
  toLockHash: string | null;
  amount: string;
  assetId: string | null;
  metadata: Record<string, unknown>;
  timestamp: string;
}

export interface ActivitiesResponse {
  activities: Activity[];
  nextCursor: string | null;
  hasMore: boolean;
}

export interface ActivityQueryParams {
  limit?: number;
  cursor?: string;
  activityType?: ActivityType;
  activityCategory?: ActivityCategory;
}

export interface AddressActivityQueryParams {
  limit?: number;
  cursor?: string;
  direction?: 'in' | 'out' | 'all';
}
