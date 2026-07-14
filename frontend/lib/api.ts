import type { ScriptRefHashType } from '@/lib/script-ref';
import { apiBaseFor, resolveActiveNetwork } from '@/lib/active-network';
export { resolveApiBase } from '@/lib/runtime-config';

// The active network is derived from the URL path per call (not module-init): the
// network can change as the user navigates, so the API base must be recomputed each
// request rather than frozen at import time.
function activeApiBase(): string {
  return apiBaseFor(resolveActiveNetwork());
}

interface ApiErrorPayload {
  error?: unknown;
  message?: unknown;
}

export class ApiRequestError extends Error {
  status: number;
  code: string;
  apiMessage: string;

  constructor(status: number, code: string, apiMessage: string) {
    super(apiMessage ? `API error: ${status} - ${apiMessage}` : `API error: ${status}`);
    this.name = 'ApiRequestError';
    this.status = status;
    this.code = code;
    this.apiMessage = apiMessage;
  }
}

function parseApiErrorPayload(
  payload: ApiErrorPayload | undefined,
  status: number
): ApiRequestError {
  const code =
    typeof payload?.error === 'string' && payload.error.trim().length > 0
      ? payload.error.trim()
      : 'unknown_error';
  const apiMessage =
    typeof payload?.message === 'string' && payload.message.trim().length > 0
      ? payload.message.trim()
      : '';
  return new ApiRequestError(status, code, apiMessage);
}

async function readApiErrorPayload(res: Response): Promise<ApiErrorPayload | undefined> {
  try {
    return (await res.json()) as ApiErrorPayload;
  } catch {
    return undefined;
  }
}

async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  if (!res.ok) {
    throw parseApiErrorPayload(await readApiErrorPayload(res), res.status);
  }
  return res.json();
}

export function isWarmupPendingError(error: unknown): error is ApiRequestError {
  if (!error || typeof error !== 'object') {
    return false;
  }
  const candidate = error as Partial<ApiRequestError>;
  return candidate.code === 'warmup_pending' && candidate.status === 503;
}

export function isNetworkInitializingError(error: unknown): error is ApiRequestError {
  if (!error || typeof error !== 'object') {
    return false;
  }
  const candidate = error as Partial<ApiRequestError>;
  return candidate.code === 'initializing' && candidate.status === 503;
}

interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  limit: number;
}

interface CursorPaginatedResponse<T> {
  data: T[];
  total?: number;
  limit: number;
  hasMore: boolean;
  nextCursor: string | null;
}

interface Block {
  number: number;
  hash: string;
  parentHash: string;
  timestamp: string;
  transactionsCount: number;
  proposalsCount: number;
  unclesCount: number;
  difficulty: string;
  epoch: string;
  epochNumber: number;
  epochIndex: number;
  epochLength: number;
  nonce: string;
  transactionsRoot: string;
  minerAddress: string | null;
  minerMessage: string | null;
  miningReward: string | null;
  miningRewardTxHash: string | null;
  hardforkActivation?: HardforkActivation | null;
  compactTarget: string;
  version: number;
}

interface HardforkActivation {
  id: string;
  name: string;
  shortName: string;
  activationEpoch: number;
  activationDate: string;
  resources?: HardforkResource[];
}

interface Transaction {
  hash: string;
  blockNumber: number;
  blockHash: string;
  index: number;
  inputsCount: number;
  outputsCount: number;
  fee: string;
  txSize?: number;
  cycles?: number;
  cyclesStatus?: 'pending' | 'failed' | null;
  isCellbase: boolean;
  timestamp: string;
}

type TransactionStatus = 'committed' | 'pending' | 'proposed';

interface SyncStatus {
  isSyncing: boolean;
  syncedBlock: number;
  tipBlock: number;
  progress: number;
  estimatedTime: string | null;
  chartDataMayBeIncomplete: boolean;
  blocksPerSecond: number | null;
  emaBlocksPerSecond: number | null;
  txsPerSecond?: number | null;
  emaTxsPerSecond?: number | null;
  syncMode: string;
  startedAt: number | null;
  elapsedTime: string | null;
  totalTime: string | null;
}

interface NetworkStats {
  latestBlock: number;
  avgBlockTime: string;
  hashRate: string;
  difficulty: string;
  epoch: string;
  tps: string;
  estimatedEpochTime: string;
  transactionsPerMinute: string;
  transactionsPerDay: string;
  syncStatus: SyncStatus;
  deepForkStatus: DeepForkStatus;
  knowledgeSize: string | null;
  circulatingSupply: string | null;
  daoLocked: string | null;
}

interface DeepForkStatus {
  detected: boolean;
  detectedAt: string | null;
  depth: number | null;
  dbTip: number | null;
  chainTip: number | null;
  forkPoint: number | null;
}

interface ReorgEvent {
  id: number;
  eventType: string;
  depth: number;
  oldTipNumber: number;
  oldTipHash: string;
  newTipNumber: number;
  newTipHash: string;
  forkPointNumber: number;
  forkPointHash: string;
  orphanedBlocksCount: number;
  orphanedTxsCount: number;
  detectedAt: string;
  resolvedAt: string | null;
  resolvedBy: string | null;
  resolutionAction: string | null;
  resolutionNotes: string | null;
}

interface OrphanedBlock {
  number: number;
  hash: string;
  parentHash: string;
  timestamp: string;
  transactionsCount: number;
  minerLockHash: string | null;
}

interface OrphanedTransaction {
  hash: string;
  blockNumber: number;
  blockHash: string;
  txIndex: number;
  inputsCount: number | null;
  outputsCount: number | null;
  totalCapacity: string | null;
}

interface ReorgDetail {
  event: ReorgEvent;
  orphanedBlocks: OrphanedBlock[];
  orphanedTransactions: OrphanedTransaction[];
}

interface RecentReorgResponse {
  hasRecentReorg: boolean;
  reorg: ReorgEvent | null;
  deepFork: DeepForkStatus;
}

interface HardforkResource {
  label: string;
  url: string;
}

interface HardforkEvent {
  id: string;
  name: string;
  shortName: string;
  editionYear: number;
  activationEpoch: number;
  activationDate: string;
  activationBlock: number | null;
  status: 'activated' | 'upcoming';
  summary: string;
  resources: HardforkResource[];
}

interface HardforkTimelineResponse {
  network: string;
  tipEpoch: number;
  tipBlock: number;
  events: HardforkEvent[];
}

interface CursorQueryParams {
  limit?: number;
  cursor?: string;
}

type ItemStatusFilter = 'all' | 'live' | 'recycled';

interface CollectionItemsParams extends CursorQueryParams {
  search?: string;
  status?: ItemStatusFilter;
}

type CollectionHoldersParams = CursorQueryParams;

interface CollectionActivitiesParams extends CursorQueryParams {
  action?: 'mint' | 'transfer' | 'burn' | 'recycle' | 'renew' | 'update';
}

interface MnftItemActivitiesParams extends CursorQueryParams {
  action?: 'mint' | 'transfer' | 'burn' | 'recycle' | 'renew' | 'update';
}

interface Script {
  codeHash: string;
  hashType: string;
  args: string;
}

interface TransactionDetail extends Omit<
  Transaction,
  'blockNumber' | 'blockHash' | 'index' | 'timestamp'
> {
  status: TransactionStatus;
  pendingSince: string | null;
  blockNumber: number | null;
  blockHash: string | null;
  index: number | null;
  feeRate?: string;
  txSize?: number;
  cycles?: number;
  cyclesStatus?: 'pending' | 'failed' | null;
  timestamp: string | null;
  confirmations: number | null;
  inputsCapacity: string | null;
  outputsCapacity: string | null;
  inputsCommonKnowledgeSize: string | null;
  outputsCommonKnowledgeSize: string | null;
  inputs?: Array<{
    previousOutput?: {
      txHash: string;
      index: number;
    };
    since?: string;
    capacity?: string;
    lock?: Script;
    type?: Script;
    address?: string;
  }>;
  outputs?: Array<{
    capacity: string;
    commonKnowledgeSize: number;
    virtualCommonKnowledgeSize?: string;
    cellType?: string;
    lock?: Script;
    type?: Script;
    address?: string;
  }>;
  witnesses?: string[];
  witnessesAvailable?: boolean;
}

interface DepGroupItem {
  txHash: string;
  outputIndex: number;
}

interface CodeCellScript {
  name: string;
  codeHash: string;
  hashType: string;
  deploymentTypeHash?: string | null;
  deploymentDataHash?: string | null;
}

interface CodeCellEntry {
  txHash: string;
  outputIndex: number;
  status: 'live' | 'consumed';
  createdAtBlock: number;
  capacity: string;
}

interface ScriptResolutionAmbiguity {
  versionHashes: string[];
}

interface CodeCellsResponse {
  codeCells: CodeCellEntry[];
  liveCount: number;
  totalCount: number;
  resolvedVersionHash?: string | null;
  ambiguity?: ScriptResolutionAmbiguity | null;
}

interface CellDaoInfo {
  isDaoCell: boolean;
  daoStatus: string;
  depositBlockNumber: number;
  depositTimestamp: string;
  withdrawRequestBlock?: number;
  withdrawRequestTimestamp?: string;
  withdrawBlock?: number;
  withdrawTimestamp?: string;
  compensation?: string;
  compensationCkb?: string;
  estimatedApc?: string;
}

interface CommonKnowledgeSizeBreakdown {
  capacityFieldBytes: number;
  lockScriptBytes: number;
  typeScriptBytes: number;
  dataBytes: number;
  totalBytes: number;
}

interface CellDataSegment {
  label: string;
  start: number;
  end: number;
  meaning: string;
  humanValue: string;
}

interface CellDeterministicDecode {
  kind: string;
  summary: string;
  segments: CellDataSegment[];
}

interface CellDataGuess {
  kind: string;
  confidence: string;
  reason: string;
  mimeType?: string;
  humanValue?: string;
}

interface CellDataAnalysis {
  deterministic?: CellDeterministicDecode;
  heuristicGuesses: CellDataGuess[];
}

interface Cell {
  txHash: string;
  outputIndex: number;
  capacity: string;
  commonKnowledgeSize?: number;
  commonKnowledgeSizeBreakdown?: CommonKnowledgeSizeBreakdown;
  lockScriptHash: string;
  address?: string;
  typeScriptHash?: string;
  typeCodeHash?: string;
  dataSize: number;
  createdAtBlock: number;
  status?: 'live' | 'dead';
  consumedAtBlock?: number;
  consumedByTx?: string;
  lock?: Script;
  type?: Script;
  data?: string;
  dataAnalysis?: CellDataAnalysis;
  isDepGroup?: boolean;
  depGroupItems?: DepGroupItem[];
  codeCellOf?: CodeCellScript[];
  cellType?: string;
  virtualCommonKnowledgeSize?: string;
  udtAmount?: string;
  daoInfo?: CellDaoInfo;
}

interface CellDep {
  outPointTxHash: string;
  outPointIndex: number;
  depType: string;
}

interface LockScriptInfo {
  codeHash: string;
  name: string;
  scriptKind?: string;
  deprecated: boolean;
}

interface Address {
  lockScriptHash: string;
  address?: string;
  balance: string;
  commonKnowledgeSize: string;
  liveCellsCount: number;
  transactionsCount: number;
  lockScript?: Script;
  lockScriptInfo?: LockScriptInfo;
}

interface TopAddress {
  lockScriptHash: string;
  balance: string;
  liveCellsCount: number;
  transactionsCount: number;
}

interface ActiveAddress {
  lockScriptHash: string;
  balance: string;
  liveCellsCount: number;
  transactionsCount: number;
  lastActivityBlock: number;
}

interface AddressTransaction {
  txHash: string;
  blockNumber: number;
  txType: 'received' | 'sent' | 'internal';
  capacityChange: string;
  timestamp: string;
  inputsCount: number;
  outputsCount: number;
  fee: string;
  isCellbase: boolean;
  txSize: number | null;
  cycles: number | null;
  scriptLabels: string[];
}

interface AddressToken {
  typeScriptHash: string;
  standard: string;
  name: string | null;
  symbol: string | null;
  decimals: number;
  iconUrl: string | null;
  balance: string;
}

interface AssetTransfer {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  eventIndex: number;
  assetCategory: 'token' | 'object' | 'identity' | 'dao';
  assetType: string;
  assetId: string | null;
  direction: 'in' | 'out';
  peerAddress: string | null;
  amount: string | null;
  eventType: string | null;
  timestamp: string;
  tokenName: string | null;
  tokenSymbol: string | null;
  tokenDecimals: number | null;
}

interface AssetTransferParams {
  limit?: number;
  cursor?: string;
  category?: 'token' | 'object' | 'identity' | 'dao';
}

type ItemDelta =
  | { kind: 'token'; typeScriptHash: string; delta: string; symbol?: string; decimals?: number }
  | { kind: 'object'; objectId: string; delta: number }
  | { kind: 'identity'; identityId: string; delta: number };

interface ActivityTypeCall {
  typeCodeHash: string;
  typeHashType: string;
  typeArgs: string;
  scriptHash: string;
  scriptName?: string;
}

interface ActivityLockCall {
  lockCodeHash: string;
  lockHashType: string;
  lockArgs: string;
  scriptHash: string;
  scriptName?: string;
  decoded?: Record<string, unknown>;
}

interface ActivityProtocolAction {
  protocol: string;
  action: string;
  metadata: Record<string, unknown>;
}

/** Tag bitmask constants for activity classification. */
const TAG_TOKEN = 1;
const TAG_OBJECT = 2;
const TAG_IDENTITY = 4;
const TAG_DAO = 8;
const TAG_PROTOCOL = 16;
const TAG_CELLBASE = 32;

/** Address activity response (GET /addresses/{addr}/activities). */
interface Activity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  ckbDelta: string;
  usedDelta: string;
  isCellbase: boolean;
  itemDeltas: ItemDelta[];
  typeCalls: ActivityTypeCall[];
  lockCalls: ActivityLockCall[];
  protocolActions: ActivityProtocolAction[];
  participants: string[];
  tags: number;
}

/** Per-participant data within a global activity response. */
interface ParticipantInfo {
  address: string;
  ckbDelta: string;
  usedDelta: string;
  itemDeltas: ItemDelta[];
  tags: number;
}

/** Global activity response (GET /activities, GET /activities/latest). */
interface GlobalActivity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  isCellbase: boolean;
  protocolActions: ActivityProtocolAction[];
  typeCalls: ActivityTypeCall[];
  lockCalls: ActivityLockCall[];
  participants: ParticipantInfo[];
}

type GlobalActivityFilter =
  | 'all'
  | 'ckb'
  | 'token'
  | 'object'
  | 'identity'
  | 'dao'
  | 'script'
  | 'protocol';

interface ScriptCountEntry {
  codeHash: string;
  name: string | null;
  count: number;
}

interface DailyActivityStats {
  date: string;
  transferCount: number;
  daoDepositCount: number;
  daoWithdrawRequestCount: number;
  daoWithdrawCompleteCount: number;
  tokenCount: number;
  objectCount: number;
  identityCount: number;
  scriptCallCount: number;
  unknownCount: number;
  coinbaseCount: number;
  uniqueAddressCount: number;
  totalCkbMoved: string;
  scriptCounts: ScriptCountEntry[];
}

interface ActivitySummary24h {
  transferCount: number;
  daoDepositCount: number;
  daoWithdrawRequestCount: number;
  daoWithdrawCompleteCount: number;
  tokenCount: number;
  objectCount: number;
  identityCount: number;
  scriptCallCount: number;
  unknownCount: number;
  coinbaseCount: number;
  uniqueAddressCount: number;
  totalCkbMoved: string;
  scriptCounts: ScriptCountEntry[];
  hoursCovered: number;
}

interface GraphNode {
  id: string;
  nodeType: string;
  label: string;
  data: {
    txHash?: string;
    outputIndex?: number;
    capacity?: string;
    status?: 'live' | 'dead';
    createdAtBlock?: number;
    hash?: string;
    blockNumber?: number;
    fee?: string;
    isCellbase?: boolean;
  };
}

interface GraphLink {
  source: string;
  target: string;
  linkType: string;
}

interface GraphResponse {
  nodes: GraphNode[];
  links: GraphLink[];
}

interface ProposalCommitmentWindow {
  close: number;
  far: number;
  earliestCommitBlock: number;
  latestCommitBlock: number;
}

interface ProposalGraphMetadata {
  sourceBlock: number;
  totalProposals: number;
  committedCount: number;
  commitmentWindow: ProposalCommitmentWindow;
}

interface ProposalGraphResponse {
  nodes: GraphNode[];
  links: GraphLink[];
  metadata: ProposalGraphMetadata;
}

interface CellQueryParams {
  limit?: number;
  lockScriptHash?: string;
  typeScriptHash?: string;
  typeCodeHash?: string;
  cursor?: string;
}

interface SearchResult {
  resultType: string;
  id: string;
  label: string;
  url: string;
  matchKind?: string;
}

interface SearchResponse {
  results: SearchResult[];
  query: string;
  normalizedQuery?: string;
  ambiguous?: boolean;
}

interface Token {
  typeScriptHash: string;
  typeCodeHash: string;
  typeHashType: string;
  typeArgs: string;
  standard: string;
  name: string | null;
  symbol: string | null;
  decimals: number;
  description: string | null;
  iconUrl: string | null;
  published: boolean;
  famous: boolean;
  tags: string[] | null;
  udtType: string | null;
  manager: string | null;
  email: string | null;
  operatorWebsite: string | null;
  totalSupply: string;
  maximumSupply: string | null;
  maximumSupplyStatus: 'limited' | 'unlimited' | 'unknown';
  holdersCount: number;
  transfersCount: number;
  transfers24h: number;
  cellsCount: number | null;
  totalCapacity: string | null;
  totalCommonKnowledgeSize: string | null;
}

interface TokenHolder {
  lockScriptHash: string;
  address?: string;
  balance: string;
}

interface TokenTransfer {
  txHash: string;
  blockNumber: number;
  fromLockHash: string | null;
  fromAddress?: string | null;
  toLockHash: string;
  toAddress?: string;
  amount: string;
  isMint: boolean;
  isBurn: boolean;
  timestamp: string;
}

interface TokenQueryParams {
  limit?: number;
  standard?: string;
  cursor?: string;
  search?: string;
}

interface Asset {
  id: string;
  assetType: 'token' | 'object' | 'identity';
  standard: string;
  name: string | null;
  symbol: string | null;
  iconUrl: string | null;
  published: boolean;
  famous: boolean;
  tags: string[] | null;
  holdersCount: number;
  transfersCount: number;
  transfers24h: number;
  decimals: number | null;
  totalSupply: string | null;
  contentType: string | null;
  contentSize: number | null;
  clusterId: string | null;
  clusterName: string | null;
  ownedCapacity: string | null;
  ownedKnowledge: string | null;
  compositionTier?:
    | 'btc_ckb'
    | 'pure_ckb'
    | 'decentralized_mixture'
    | 'centralized_mixture'
    | 'unknown';
  fullyOnchainRatio?: string | null;
  fullyOnchainCount?: number | null;
  hMultiplier: number | null;
}

interface AssetQueryParams {
  limit?: number;
  type?: 'token' | 'object' | 'identity';
  standard?: string;
  cursor?: string;
  search?: string;
  sortKey?:
    | 'name'
    | 'type'
    | 'supply'
    | 'transfers24h'
    | 'holders'
    | 'transfers'
    | 'used'
    | 'capacity'
    | 'onchainRatio'
    | 'hMultiplier';
  sortDirection?: 'asc' | 'desc';
  compositionTier?:
    | 'pure_ckb'
    | 'btc_ckb'
    | 'decentralized_mixture'
    | 'centralized_mixture'
    | 'unknown';
}

interface TokenHolderParams {
  limit?: number;
  cursor?: string;
}

interface TokenTransferParams {
  limit?: number;
  cursor?: string;
}

interface TokenTransferDetail {
  fromLockHash: string | null;
  fromAddress: string | null;
  toLockHash: string;
  toAddress: string | null;
  amount: string;
  isMint: boolean;
  isBurn: boolean;
}

interface TokenActivity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  actions: string[];
  transfers: TokenTransferDetail[];
}

interface TokenActivityParams {
  limit?: number;
  cursor?: string;
}

interface DaoDeposit {
  txHash: string;
  outputIndex: number;
  lockScriptHash: string;
  address?: string;
  lockCodeHash: string | null;
  capacity: string;
  depositBlockNumber: number;
  depositTimestamp: string;
  status: string;
  withdrawRequestBlock: number | null;
  withdrawRequestTimestamp: string | null;
  withdrawRequestTxHash: string | null;
  withdrawRequestOutputIndex: number | null;
  withdrawBlock: number | null;
  withdrawTimestamp: string | null;
  withdrawTxHash: string | null;
  withdrawToOutputIndex: number | null;
  compensation: string | null;
}

interface AddressDaoSummary {
  hasDaoActivity: boolean;
  activeDepositsCount: number;
  pendingWithdrawalsCount: number;
  completedWithdrawalsCount: number;
  totalLockedCapacity: string;
  totalLockedCkb: string;
  unclaimedCompensation: string;
  unclaimedCompensationCkb: string;
  totalCompensationEarned: string;
  totalCompensationEarnedCkb: string;
  estimatedApc: string;
}

interface DaoStatistics {
  totalDeposited: string;
  totalDepositedCkb: string;
  totalDepositors: number;
  activeDeposits: number;
  totalCompensationPaid: string;
  totalCompensationPaidCkb: string;
  unclaimedCompensation: string;
  unclaimedCompensationCkb: string;
  averageDepositDays: string;
  estimatedApc: string;
  miningReward: string;
  miningRewardCkb: string;
  depositCompensation: string;
  depositCompensationCkb: string;
  burnt: string;
  burntCkb: string;
  depositChange24h?: string;
  depositorsChange24h?: number;
  claimedCompensationChange24h?: string;
  unclaimedCompensationChange24h?: string;
}

interface DaoTopDepositor {
  rank: number;
  lockScriptHash: string;
  address: string | null;
  totalCapacity: string;
  totalCapacityCkb: string;
  depositCount: number;
  averageDepositDays: string;
}

interface DaoTopDepositorsResponse {
  depositors: DaoTopDepositor[];
}

interface DaoCalculatorResult {
  capacity: string;
  capacityCkb: string;
  depositBlock: number;
  withdrawBlock: number;
  estimatedCompensation: string;
  estimatedCompensationCkb: string;
  totalWithdrawable: string;
  totalWithdrawableCkb: string;
  apc: string;
}

interface DaoQueryParams {
  limit?: number;
  status?: number;
  cursor?: string;
}

interface SporeCluster {
  clusterId: string;
  name: string | null;
  description: string | null;
  ownerLockHash: string;
  ownerAddress?: string;
  sporesCount: number;
  holdersCount: number;
  activitiesCount: number;
  createdAtBlock: number;
  ownedCapacity?: string | null;
  ownedKnowledge?: string | null;
  composition?: {
    tier: 'btc_ckb' | 'pure_ckb' | 'decentralized_mixture' | 'centralized_mixture' | 'unknown';
    fullyOnchainCount: number;
    pureCkbCount: number;
    decentralizedMixtureCount: number;
    centralizedMixtureCount: number;
    unknownCount: number;
    fullyOnchainRatio: string;
  };
}

interface SporeNft {
  sporeId: string;
  txHash: string;
  outputIndex: number;
  clusterId: string | null;
  contentType: string;
  contentSize: number;
  ownerLockHash: string;
  ownerAddress?: string;
  isLive: boolean;
  createdAtBlock: number;
  ownedCapacity?: string | null;
  ownedKnowledge?: string | null;
  mediaProfile?: {
    tier: 'btc_ckb' | 'pure_ckb' | 'decentralized_mixture' | 'centralized_mixture' | 'unknown';
    sources: Array<{
      uri: string;
      scheme: string;
      sourceLocation: string;
      dependencyTier:
        | 'btc_ckb'
        | 'pure_ckb'
        | 'decentralized_mixture'
        | 'centralized_mixture'
        | 'unknown';
    }>;
    issues: string[];
  } | null;
}

interface DobTrait {
  name: string;
  value: string;
}

export interface DecodedMediaItem {
  mediaType: string;
  role: string | null;
  size: number;
  hash: string;
  step: number | null;
  url: string;
}

interface SporeDobDecoded {
  status: string;
  sporeId: string;
  contentType: string;
  dnaHex: string | null;
  traits: DobTrait[];
  media: DecodedMediaItem[];
  issues: string[];
}

interface ObjectCollection {
  collectionId: string;
  standard: string;
  name: string | null;
  totalCount: number;
  liveCount: number;
  holdersCount: number;
  activitiesCount: number;
  ownedCapacity: string;
  ownedKnowledge: string;
  composition?: {
    tier: 'btc_ckb' | 'pure_ckb' | 'decentralized_mixture' | 'centralized_mixture' | 'unknown';
    fullyOnchainCount: number;
    pureCkbCount: number;
    decentralizedMixtureCount: number;
    centralizedMixtureCount: number;
    unknownCount: number;
    fullyOnchainRatio: string;
  };
  classDetail?: {
    classId: string;
    issuerId: string;
    name: string | null;
    description: string | null;
    renderer: string | null;
    total: number;
    issued: number;
    configure: number;
  };
  issuerDetail?: {
    issuerId: string;
    name: string | null;
    classCount: number;
    setCount: number;
    infoHex: string | null;
  };
  createdAtBlock?: number;
  ownerLockHash?: string;
}

export interface IdentityCollection {
  collectionId: string;
  standard: string;
  name: string | null;
  totalCount: number;
  liveCount: number;
  holdersCount: number;
  activitiesCount: number;
  ownedCapacity: string;
  ownedKnowledge: string;
}

interface CollectionItem {
  nftId: string;
  name: string | null;
  standard: string;
  ownerLockHash: string | null;
  isLive: boolean;
  createdAtBlock: number;
  expiredAt?: number | null;
  txHash?: string | null;
  outputIndex?: number | null;
}

interface CollectionHolder {
  lockScriptHash: string;
  address: string | null;
  itemCount: number;
}

interface CollectionActivity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  actions: string[];
}

interface MnftClassSummary {
  classId: string;
  issuerId: string;
  name: string | null;
  description: string | null;
  renderer: string | null;
  total: number;
  issued: number;
  configure: number;
}

interface MnftIssuerSummary {
  issuerId: string;
  name: string | null;
  classCount: number;
  setCount: number;
  infoHex: string | null;
}

interface MnftLifecycleEvent {
  event: string;
  blockNumber: number | null;
  txHash: string | null;
  outputIndex: number | null;
  note: string | null;
}

interface MnftItemDetail {
  nftId: string;
  standard: string;
  isLive: boolean;
  ownerLockHash: string | null;
  createdAtBlock: number;
  tokenIndex: number;
  characteristicHex: string;
  configure: number;
  state: number;
  txHash: string | null;
  outputIndex: number | null;
  class: MnftClassSummary;
  issuer: MnftIssuerSummary;
  lifecycle: MnftLifecycleEvent[];
  composition?: {
    tier: 'btc_ckb' | 'pure_ckb' | 'decentralized_mixture' | 'centralized_mixture' | 'unknown';
    onchainCount: number;
    pureCkbCount: number;
    decentralizedMixtureCount: number;
    centralizedMixtureCount: number;
    unknownCount: number;
    onchainRatio: string;
  };
}

interface MnftItemActivity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  actions: string[];
}

interface ChartDataPoint {
  date: string;
  value: string;
  value2?: string;
}

interface ChartResponse {
  data: ChartDataPoint[];
  title: string;
  yAxisLabel: string;
  y2AxisLabel?: string;
}

interface TxStatsDataPoint {
  label: string;
  value: number;
}

interface TxStatsResponse {
  currentHour: number;
  currentDay: number;
  hourlyData: TxStatsDataPoint[];
  dailyData: TxStatsDataPoint[];
}

interface RecentBlockItem {
  timestamp: number;
  transactionsCount: number;
}

interface RecentBlocksResponse {
  blocks: RecentBlockItem[];
}

interface MinerDistributionDataPoint {
  address: string;
  minerName: string | null;
  blocksMined: number;
  percentage: string;
}

interface MinerDistributionResponse {
  data: MinerDistributionDataPoint[];
  title: string;
  totalBlocks: number;
}

interface StackedAreaDataPoint {
  date: string;
  values: Record<string, string>;
}

interface StackedAreaSeries {
  key: string;
  label: string;
  color: string;
}

interface StackedAreaChartResponse {
  data: StackedAreaDataPoint[];
  series: StackedAreaSeries[];
  title: string;
}

interface MostUtilizedScriptsChartResponse {
  title: string;
  usedShare: StackedAreaChartResponse;
  capacityShare: StackedAreaChartResponse;
}

interface MostUtilizedAssetsChartResponse {
  title: string;
  usedShare: StackedAreaChartResponse;
  capacityShare: StackedAreaChartResponse;
}

interface KnownScript {
  codeHash: string;
  name: string;
  description: string | null;
  scriptKind: string | null;
  rfc: string | null;
  website: string | null;
  sourceUrl: string | null;
  decoderType: string | null;
  network: string;
  hashType: string | null;
  dataHash: string | null;
  typeHash: string | null;
  tag: string | null;
  deprecated: boolean;
  isSystem: boolean;
  codeCellTxHash: string | null;
  codeCellOutputIndex: number | null;
  deployedAt?: number | null;
  ownedCapacitySum?: string;
  ownedKnowledgeSum?: string;
  liveCellsCount?: number;
  cellsCount?: number;
  codeCellsLiveCount?: number;
  codeCellsTotal?: number;
  observedReferences?: ScriptObservedReferenceApi[];
}

interface DeploymentUsage {
  codeHash: string;
  scriptKind: string | null;
  cellsCount: number;
  liveCellsCount: number;
  capacitySum: string;
  ownedCapacitySum: string;
  commonKnowledgeSizeSum: string;
  ownedKnowledgeSum: string;
}

interface ScriptUsage {
  name: string;
  cellsCount: number;
  liveCellsCount: number;
  capacitySum: string;
  ownedCapacitySum: string;
  commonKnowledgeSizeSum: string;
  ownedKnowledgeSum: string;
  byDeployment: DeploymentUsage[];
}

interface ScriptFamilyListItemApi {
  familyId: string;
  name: string;
  description: string | null;
  scriptKind: string | null;
  deprecated: boolean;
  website: string | null;
  liveCellsCount: number;
  cellsCount: number;
  ownedCapacitySum: string;
  ownedKnowledgeSum: string;
  versionsCount: number;
}

interface ScriptObservedReferenceApi {
  referenceHash: string;
  hashType: string;
  liveCellsCount: number;
  cellsCount: number;
  ownedCapacitySum: string;
  ownedKnowledgeSum: string;
}

interface ScriptVersionDetailApi {
  versionHash: string;
  name: string;
  description: string | null;
  scriptKind: string | null;
  website: string | null;
  deprecated: boolean;
  canonicalReferenceHash: string | null;
  canonicalHashType: string | null;
  deployedAt: number | null;
  liveCellsCount: number;
  cellsCount: number;
  ownedCapacitySum: string;
  ownedKnowledgeSum: string;
  codeCellsLiveCount: number;
  codeCellsTotal: number;
  deployments: ScriptVersionDeploymentApi[];
  references: ScriptObservedReferenceApi[];
}

interface ScriptVersionDeploymentApi {
  hashType: string;
  typeReferenceHash: string | null;
  dataReferenceHash: string;
  codeCellTxHash: string;
  codeCellOutputIndex: number;
  deployedAt: number;
}

interface ScriptFamilyDetailApi {
  familyId: string;
  name: string;
  description: string | null;
  scriptKind: string | null;
  website: string | null;
  liveCellsCount: number;
  cellsCount: number;
  ownedCapacitySum: string;
  ownedKnowledgeSum: string;
  versionsCount: number;
  versions: ScriptVersionDetailApi[];
}

interface ScriptQueryParams {
  limit?: number;
  cursor?: string;
  network?: string;
  decoderType?: string;
  search?: string;
  sortKey?:
    | 'name'
    | 'usedAs'
    | 'description'
    | 'used'
    | 'capacity'
    | 'usedRatio'
    | 'liveCells'
    | 'cells';
  sortDirection?: 'asc' | 'desc';
}

interface CapacityChartRangeParams {
  from?: string;
  to?: string;
}

interface ScriptLookupInfo {
  referenceHash?: string;
  codeHash: string;
  name: string;
  deprecated?: boolean;
  scriptKind: string | null;
  decoderType: string | null;
  hashType: string | null;
  deploymentTypeHash?: string | null;
  deploymentDataHash?: string | null;
  codeCellTxHash: string | null;
  codeCellOutputIndex: number | null;
  liveCellsCount: number;
  ownedCapacitySum: string;
  ownedKnowledgeSum: string;
  codeCellsLiveCount: number;
  codeCellsTotal: number;
  resolutionState?: 'resolved' | 'ambiguous';
  ambiguity?: ScriptResolutionAmbiguity | null;
}

type ScriptLookupResponse = Record<string, ScriptLookupInfo>;

function scriptFamilyListItemToKnownScript(script: ScriptFamilyListItemApi): KnownScript {
  return {
    codeHash: `family:${script.familyId}`,
    name: script.name,
    description: script.description,
    scriptKind: script.scriptKind,
    rfc: null,
    website: script.website,
    sourceUrl: null,
    decoderType: null,
    network: '',
    hashType: null,
    dataHash: null,
    typeHash: null,
    tag: null,
    deprecated: script.deprecated,
    isSystem: false,
    codeCellTxHash: null,
    codeCellOutputIndex: null,
    deployedAt: null,
    ownedCapacitySum: script.ownedCapacitySum,
    ownedKnowledgeSum: script.ownedKnowledgeSum,
    liveCellsCount: script.liveCellsCount,
    cellsCount: script.cellsCount,
  };
}

function scriptVersionDetailToKnownScripts(
  family: ScriptFamilyDetailApi,
  version: ScriptVersionDetailApi
): KnownScript[] {
  const baseDeployment: Omit<
    KnownScript,
    'hashType' | 'dataHash' | 'typeHash' | 'codeCellTxHash' | 'codeCellOutputIndex' | 'deployedAt'
  > = {
    codeHash: version.versionHash,
    name: version.name || family.name,
    description: version.description ?? family.description,
    scriptKind: version.scriptKind ?? family.scriptKind,
    rfc: null,
    website: version.website ?? family.website,
    sourceUrl: null,
    decoderType: null,
    network: '',
    tag: null,
    deprecated: version.deprecated,
    isSystem: false,
    ownedCapacitySum: version.ownedCapacitySum,
    ownedKnowledgeSum: version.ownedKnowledgeSum,
    liveCellsCount: version.liveCellsCount,
    cellsCount: version.cellsCount,
    codeCellsLiveCount: version.codeCellsLiveCount,
    codeCellsTotal: version.codeCellsTotal,
    observedReferences: version.references,
  };

  return version.deployments.map((deployment) => ({
    ...baseDeployment,
    hashType: deployment.hashType,
    dataHash: deployment.dataReferenceHash,
    typeHash:
      deployment.typeReferenceHash ??
      (version.canonicalHashType === 'type' ? version.canonicalReferenceHash : null),
    codeCellTxHash: deployment.codeCellTxHash,
    codeCellOutputIndex: deployment.codeCellOutputIndex,
    deployedAt: deployment.deployedAt,
  }));
}

function scriptFamilyDetailToKnownScripts(family: ScriptFamilyDetailApi): KnownScript[] {
  return family.versions.flatMap((version) => scriptVersionDetailToKnownScripts(family, version));
}

interface MempoolInfo {
  pendingCount: number;
  proposedCount: number;
  orphanCount: number;
  totalSize: number;
  totalCycles: number;
  minFeeRate: number;
  tipNumber: number;
  tipHash: string;
  lastUpdatedAt: number;
}

interface MempoolTransaction {
  txHash: string;
  fee: number;
  size: number;
  cycles: number;
  feeRate: number;
  ancestorsCount: number;
  timestamp: number;
  status: string;
}

interface FeeRateRange {
  min: number;
  max: number;
}

interface MempoolBlock {
  index: number;
  transactionCount: number;
  totalSize: number;
  totalFee: number;
  totalCycles: number;
  feeRateRange: FeeRateRange;
  medianFeeRate: number;
  estimatedTimeMinutes: number;
}

interface MempoolBlocksResponse {
  pendingBlocks: MempoolBlock[];
  totalPendingCount: number;
  totalProposedCount: number;
}

interface PendingProposal {
  proposalId: string;
  fullTxHash: string | null;
  proposedAtBlock: number;
  proposedAtIndex: number;
  blocksUntilExpiry: number;
  fee: number | null;
  size: number | null;
  cycles: number | null;
  feeRate: number | null;
}

interface PendingProposalsResponse {
  proposals: PendingProposal[];
  tipBlockNumber: number;
  totalCount: number;
}

interface BlockFeeStats {
  blockNumber: number;
  totalSize: number;
  totalCycles: number;
  avgFeeRate: number;
  minFeeRate: number;
  maxFeeRate: number;
  transactionCount: number;
  cyclesPending?: boolean;
}

interface BlockProposal {
  proposalIndex: number;
  proposalId: string;
  committedTxHash: string | null;
  committedBlockNumber: number | null;
}

type CyclesCalculationStatus = 'done' | 'calculating' | 'queued' | 'failed' | 'notFound';

interface CyclesStatusResponse {
  status: CyclesCalculationStatus;
  cycles: number | null;
  error: string | null;
}

interface TransactionLifecycle {
  hash: string;
  phase: 'pending' | 'committed';
  proposalId: string;
  proposedIn: { blockNumber: number; blockHash: string; timestamp: string } | null;
  committedIn: { blockNumber: number; blockHash: string; timestamp: string } | null;
  commitmentDistance: number | null;
  commitmentWindow: { close: number; far: number };
  isCellbase: boolean;
  confirmations: number | null;
}

async function fetchApi<T>(endpoint: string): Promise<T> {
  return fetchJson(`${activeApiBase()}${endpoint}`);
}

interface TopTokenEntry {
  typeScriptHash: string;
  name: string | null;
  symbol: string | null;
  holdersCount: number;
  totalCapacityCkb: string;
}

interface CapacityCategory {
  category: string;
  capacityCkb: string;
  percentage: string;
}

interface AssetEcosystemResponse {
  topTokens: TopTokenEntry[];
  capacityBreakdown: CapacityCategory[];
  totalKnowledgeSizeCkb: string;
}

type FiberChannelState = 'open' | 'cooperativelyClosed' | 'forceClosed' | 'settled';

interface FiberChannel {
  channelId: string;
  state: FiberChannelState;
  capacity: string;
  fundingTxHash: string;
  fundingOutputIndex: number;
  openBlock: number;
  openTimestamp: string;
  closeBlock: number | null;
  closeTimestamp: string | null;
  closeTxHash: string | null;
  settlementBlock: number | null;
  settlementTimestamp: string | null;
  settlementTxHash: string | null;
  participants: string[];
}

interface FiberTimelineEvent {
  event: string;
  blockNumber: number;
  txHash: string;
  timestamp: string;
}

interface FiberChannelDetail extends FiberChannel {
  timeline: FiberTimelineEvent[];
}

interface FiberStatsApiResponse {
  totalChannels: number;
  openChannels: number;
  totalCapacityLocked: string;
}

interface FiberStats {
  totalChannels: number;
  openChannels: number;
  closedChannels: number;
  totalCapacityLocked: string;
}

interface FiberChannelQueryParams {
  limit?: number;
  cursor?: string;
  state?: FiberChannelState;
}

export type {
  GraphNode,
  GraphLink,
  GraphResponse,
  Cell,
  CellDataAnalysis,
  CellDeterministicDecode,
  CellDataSegment,
  CellDataGuess,
  CommonKnowledgeSizeBreakdown,
  CellDaoInfo,
  CellDep,
  CodeCellScript,
  CodeCellEntry,
  CodeCellsResponse,
  Transaction,
  TransactionStatus,
  TransactionDetail,
  Block,
  NetworkStats,
  SyncStatus,
  Address,
  LockScriptInfo,
  TopAddress,
  ActiveAddress,
  AddressTransaction,
  AddressToken,
  AssetTransfer,
  AssetTransferParams,
  SearchResult,
  SearchResponse,
  Asset,
  Token,
  TokenHolder,
  TokenTransfer,
  TokenTransferDetail,
  TokenActivity,
  DaoDeposit,
  AddressDaoSummary,
  DaoStatistics,
  DaoTopDepositor,
  DaoTopDepositorsResponse,
  DaoCalculatorResult,
  SporeCluster,
  SporeNft,
  ObjectCollection,
  CollectionItem,
  CollectionHolder,
  CollectionActivity,
  ItemStatusFilter,
  MnftClassSummary,
  MnftIssuerSummary,
  MnftLifecycleEvent,
  MnftItemDetail,
  MnftItemActivity,
  ChartDataPoint,
  ChartResponse,
  TxStatsDataPoint,
  TxStatsResponse,
  RecentBlockItem,
  RecentBlocksResponse,
  MinerDistributionDataPoint,
  MinerDistributionResponse,
  StackedAreaDataPoint,
  StackedAreaSeries,
  StackedAreaChartResponse,
  MostUtilizedScriptsChartResponse,
  MostUtilizedAssetsChartResponse,
  CursorPaginatedResponse,
  PaginatedResponse,
  MempoolInfo,
  MempoolTransaction,
  MempoolBlock,
  MempoolBlocksResponse,
  FeeRateRange,
  PendingProposal,
  PendingProposalsResponse,
  BlockFeeStats,
  BlockProposal,
  KnownScript,
  ScriptObservedReferenceApi as ScriptObservedReference,
  ScriptVersionDeploymentApi as ScriptVersionDeployment,
  ScriptVersionDetailApi as ScriptVersionDetail,
  ScriptFamilyDetailApi as ScriptFamilyDetail,
  ScriptUsage,
  ScriptLookupInfo,
  ScriptLookupResponse,
  CyclesCalculationStatus,
  CyclesStatusResponse,
  TransactionLifecycle,
  ProposalCommitmentWindow,
  ProposalGraphMetadata,
  ProposalGraphResponse,
  DeepForkStatus,
  ReorgEvent,
  OrphanedBlock,
  OrphanedTransaction,
  ReorgDetail,
  RecentReorgResponse,
  HardforkResource,
  HardforkEvent,
  HardforkTimelineResponse,
  HardforkActivation,
  Activity,
  ItemDelta,
  ActivityTypeCall,
  ActivityLockCall,
  ActivityProtocolAction,
  ParticipantInfo,
  GlobalActivity,
  GlobalActivityFilter,
  ScriptCountEntry,
  DailyActivityStats,
  ActivitySummary24h,
  TopTokenEntry,
  CapacityCategory,
  AssetEcosystemResponse,
  FiberChannel,
  FiberChannelDetail,
  FiberTimelineEvent,
  FiberStats,
  FiberChannelState,
  FiberChannelQueryParams,
};

export { TAG_TOKEN, TAG_OBJECT, TAG_IDENTITY, TAG_DAO, TAG_PROTOCOL, TAG_CELLBASE };

export interface LabelCount {
  label: string;
  count: number;
}

export interface NetworkLastRound {
  roundId: number;
  started: number;
  finished: number;
  dialed: number;
  reachable: number;
  unreachable: number;
  foreignDropped: number;
  newNodes: number;
  totalKnown: number;
  frontierDrained: boolean;
}

export interface NetworkSummary {
  enabled: boolean;
  hasData: boolean;
  lastRound: NetworkLastRound | null;
}

export interface NetworkDistributions {
  totalKnown: number;
  reachable: number;
  unreachable: number;
  versions: LabelCount[];
  countries: LabelCount[];
  asns: LabelCount[];
  protocols: LabelCount[];
}

export interface NetworkHistoryPoint {
  ts: number;
  scalar: number;
  buckets: LabelCount[];
}

export interface NetworkHistory {
  metric: string;
  granularity: string;
  points: NetworkHistoryPoint[];
}

export interface NodeSummary {
  peerId: string;
  addr: string;
  version: string;
  country: string;
  asn: string;
  reachable: boolean;
  lastSeen: number;
  lastReachableAt: number;
  rttMs: number | null;
}

export interface NetworkNodesPage {
  items: NodeSummary[];
  nextCursor: string | null;
}

export const api = {
  getForks: (params: CursorQueryParams = {}): Promise<CursorPaginatedResponse<ReorgEvent>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/forks?${query}`);
  },

  getForkDetail: (id: number): Promise<ReorgDetail> => {
    return fetchApi(`/forks/${id}`);
  },

  getRecentReorg: (): Promise<RecentReorgResponse> => {
    return fetchApi(`/forks/recent`);
  },

  getHardforks: (params: { network?: string } = {}): Promise<HardforkTimelineResponse> => {
    const query = new URLSearchParams();
    if (params.network) query.set('network', params.network);
    const queryString = query.toString();
    return fetchApi(`/hardforks${queryString ? `?${queryString}` : ''}`);
  },

  getBlocks: (params: CursorQueryParams = {}): Promise<CursorPaginatedResponse<Block>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/blocks?${query}`);
  },

  getBlock: (id: string | number): Promise<Block> => {
    return fetchApi(`/blocks/${id}`);
  },

  getBlockFeeStats: (id: string | number): Promise<BlockFeeStats> => {
    return fetchApi(`/blocks/${id}/fee-stats`);
  },

  getBlockProposals: (id: string | number): Promise<BlockProposal[]> => {
    return fetchApi(`/blocks/${id}/proposals`);
  },

  getTransactions: (
    params: CursorQueryParams & { blockNumber?: number } = {}
  ): Promise<CursorPaginatedResponse<Transaction>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.blockNumber !== undefined) query.set('block_number', String(params.blockNumber));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/transactions?${query}`);
  },

  getTransaction: (hash: string): Promise<Transaction> => {
    return fetchApi(`/transactions/${hash}`);
  },

  getTransactionDetail: (hash: string): Promise<TransactionDetail> => {
    return fetchApi(`/transactions/${hash}/detail`);
  },

  getTransactionCellDeps: (hash: string): Promise<CellDep[]> => {
    return fetchApi(`/transactions/${hash}/cell-deps`);
  },

  getTransactionLifecycle: (hash: string): Promise<TransactionLifecycle> => {
    return fetchApi(`/transactions/${hash}/lifecycle`);
  },

  getAddress: (addr: string): Promise<Address> => {
    return fetchApi(`/addresses/${addr}`);
  },

  getTopAddresses: (limit: number = 100): Promise<TopAddress[]> => {
    const query = new URLSearchParams();
    query.set('limit', String(limit));
    return fetchApi(`/addresses/top?${query}`);
  },

  getActiveAddresses: (
    params: { limit?: number; days?: number } = {}
  ): Promise<ActiveAddress[]> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.days) query.set('days', String(params.days));
    return fetchApi(`/addresses/active?${query}`);
  },

  getAddressTransactions: (
    addr: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<AddressTransaction>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/addresses/${addr}/transactions?${query}`);
  },

  getAddressTokens: (
    addr: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<AddressToken>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/addresses/${addr}/tokens?${query}`);
  },

  getAddressActivities: (
    addr: string,
    params: CursorQueryParams & { filter?: string } = {}
  ): Promise<CursorPaginatedResponse<Activity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.filter && params.filter !== 'all') query.set('filter', params.filter);
    return fetchApi(`/addresses/${addr}/activities?${query}`);
  },

  getGlobalActivities: (
    params: CursorQueryParams & { filter?: GlobalActivityFilter } = {}
  ): Promise<CursorPaginatedResponse<GlobalActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.filter && params.filter !== 'all') query.set('filter', params.filter);
    return fetchApi(`/activities?${query}`);
  },

  getLatestActivities: (limit: number = 8): Promise<GlobalActivity[]> => {
    return fetchApi(`/activities/latest?limit=${limit}`);
  },

  getDailyActivityStats: (days: number = 30): Promise<DailyActivityStats[]> => {
    return fetchApi(`/stats/daily-activities?days=${days}`);
  },

  getActivitySummary24h: (): Promise<ActivitySummary24h> => {
    return fetchApi('/stats/activity-summary-24h');
  },

  getLiveCells: (params: CellQueryParams = {}): Promise<CursorPaginatedResponse<Cell>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.lockScriptHash) query.set('lock_script_hash', params.lockScriptHash);
    if (params.typeScriptHash) query.set('type_script_hash', params.typeScriptHash);
    if (params.typeCodeHash) query.set('type_code_hash', params.typeCodeHash);
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/cells/live?${query}`);
  },

  getCellsByScriptRef: (params: {
    codeHash: string;
    hashType: ScriptRefHashType;
    scriptKind?: 'lock' | 'type' | 'both';
    limit?: number;
    cursor?: string;
  }): Promise<CursorPaginatedResponse<Cell>> => {
    const query = new URLSearchParams();
    query.set('code_hash', params.codeHash);
    query.set('hash_type', params.hashType);
    if (params.scriptKind) query.set('script_kind', params.scriptKind);
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/cells/by-script?${query}`);
  },

  getCell: (txHash: string, outputIndex: number): Promise<Cell> => {
    return fetchApi(`/cells/${txHash}/${outputIndex}`);
  },

  getNetworkStats: (): Promise<NetworkStats> => {
    return fetchApi('/statistics/network');
  },

  getTxStats: (): Promise<TxStatsResponse> => {
    return fetchApi('/statistics/tx-stats');
  },

  getRecentBlocks: (): Promise<RecentBlocksResponse> => {
    return fetchApi('/statistics/recent-blocks');
  },

  getAssetEcosystem: (): Promise<AssetEcosystemResponse> => {
    return fetchApi('/statistics/asset-ecosystem');
  },

  getCellGraph: (
    txHash: string,
    outputIndex: number,
    depth: number = 2
  ): Promise<GraphResponse> => {
    const query = new URLSearchParams();
    query.set('depth', String(depth));
    return fetchApi(`/graph/cell/${txHash}/${outputIndex}?${query}`);
  },

  getTransactionGraph: (hash: string, depth: number = 2): Promise<GraphResponse> => {
    const query = new URLSearchParams();
    query.set('depth', String(depth));
    return fetchApi(`/graph/transaction/${hash}?${query}`);
  },

  getProposalGraph: (blockNumber: number): Promise<ProposalGraphResponse> => {
    return fetchApi(`/graph/proposals/${blockNumber}`);
  },

  search: (q: string): Promise<SearchResponse> => {
    const query = new URLSearchParams();
    query.set('q', q);
    return fetchApi(`/search?${query}`);
  },

  getAssets: (params: AssetQueryParams = {}): Promise<CursorPaginatedResponse<Asset>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.type) query.set('type', params.type);
    if (params.standard) query.set('standard', params.standard);
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.search) query.set('search', params.search);
    if (params.sortKey) {
      if (params.sortKey === 'transfers24h') {
        query.set('sort_key', 'transfers_24h');
      } else if (params.sortKey === 'onchainRatio') {
        query.set('sort_key', 'onchain_ratio');
      } else if (params.sortKey === 'hMultiplier') {
        query.set('sort_key', 'h_multiplier');
      } else {
        query.set('sort_key', params.sortKey);
      }
    }
    if (params.sortDirection) query.set('sort_direction', params.sortDirection);
    if (params.compositionTier) query.set('composition_tier', params.compositionTier);
    return fetchApi(`/assets?${query}`);
  },

  getTokens: (params: TokenQueryParams = {}): Promise<CursorPaginatedResponse<Token>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.standard) query.set('standard', params.standard);
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.search) query.set('search', params.search);
    return fetchApi(`/tokens?${query}`);
  },

  getToken: (typeHash: string): Promise<Token> => {
    return fetchApi(`/tokens/${typeHash}`);
  },

  getTokenCapacityChart: (
    typeHash: string,
    range: CapacityChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(`/tokens/${typeHash}/charts/capacity-history${suffix ? `?${suffix}` : ''}`);
  },

  getTokenHolders: (
    typeHash: string,
    params: TokenHolderParams = {}
  ): Promise<CursorPaginatedResponse<TokenHolder>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/tokens/${typeHash}/holders?${query}`);
  },

  getTokenTransfers: (
    typeHash: string,
    params: TokenTransferParams = {}
  ): Promise<CursorPaginatedResponse<TokenTransfer>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/tokens/${typeHash}/transfers?${query}`);
  },

  getTokenActivities: (
    typeHash: string,
    params: TokenActivityParams = {}
  ): Promise<CursorPaginatedResponse<TokenActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/tokens/${typeHash}/activities?${query}`);
  },

  getDaoDeposits: (params: DaoQueryParams = {}): Promise<CursorPaginatedResponse<DaoDeposit>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.status !== undefined) query.set('status', String(params.status));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/dao/deposits?${query}`);
  },

  getDaoDepositsByAddress: (
    lockHash: string,
    params: DaoQueryParams = {}
  ): Promise<CursorPaginatedResponse<DaoDeposit>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/dao/deposits/${lockHash}?${query}`);
  },

  getAddressDaoSummary: (lockHash: string): Promise<AddressDaoSummary> => {
    return fetchApi(`/dao/summary/${lockHash}`);
  },

  getDaoStatistics: (): Promise<DaoStatistics> => {
    return fetchApi('/dao/statistics');
  },

  getDaoTopDepositors: (): Promise<DaoTopDepositorsResponse> => {
    return fetchApi('/dao/top-depositors');
  },

  calculateDaoCompensation: (
    capacity: string,
    depositBlock: number,
    withdrawBlock?: number
  ): Promise<DaoCalculatorResult> => {
    const query = new URLSearchParams();
    query.set('capacity', capacity);
    query.set('deposit_block', String(depositBlock));
    if (withdrawBlock) query.set('withdraw_block', String(withdrawBlock));
    return fetchApi(`/dao/calculator?${query}`);
  },

  getSporeClusters: (
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<SporeCluster>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/spore/clusters?${query}`);
  },

  getSporeCluster: (clusterId: string): Promise<SporeCluster> => {
    return fetchApi(`/spore/clusters/${clusterId}`);
  },

  getSporeClusterHolders: (
    clusterId: string,
    params: CollectionHoldersParams = {}
  ): Promise<CursorPaginatedResponse<CollectionHolder>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    const suffix = query.toString();
    return fetchApi(`/spore/clusters/${clusterId}/holders${suffix ? `?${suffix}` : ''}`);
  },

  getSporeClusterActivities: (
    clusterId: string,
    params: CollectionActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<CollectionActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(`/spore/clusters/${clusterId}/activities${suffix ? `?${suffix}` : ''}`);
  },

  getSporeClusterCapacityChart: (
    clusterId: string,
    range: CapacityChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(
      `/spore/clusters/${clusterId}/charts/capacity-history${suffix ? `?${suffix}` : ''}`
    );
  },

  getSporeObjects: (params: CursorQueryParams = {}): Promise<CursorPaginatedResponse<SporeNft>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/spore/objects?${query}`);
  },

  getSporeObject: (sporeId: string): Promise<SporeNft> => {
    return fetchApi(`/spore/objects/${sporeId}`);
  },

  getSporeObjectDecoded: (sporeId: string): Promise<SporeDobDecoded> => {
    return fetchApi(`/spore/objects/${sporeId}/decode`);
  },

  getSporeItemActivities: (
    sporeId: string,
    params: MnftItemActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<MnftItemActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(
      `/spore/objects/${encodeURIComponent(sporeId)}/activities${suffix ? `?${suffix}` : ''}`
    );
  },

  getSporeObjectCapacityChart: (
    sporeId: string,
    range: CapacityChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(
      `/spore/objects/${sporeId}/charts/capacity-history${suffix ? `?${suffix}` : ''}`
    );
  },

  getObjectCollection: (collectionId: string): Promise<ObjectCollection> => {
    return fetchApi(`/assets/objects/${collectionId}`);
  },

  getObjectCollectionCapacityChart: (
    collectionId: string,
    range: CapacityChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(
      `/assets/objects/${collectionId}/charts/capacity-history${suffix ? `?${suffix}` : ''}`
    );
  },

  getObjectCollectionItems: (
    collectionId: string,
    params: CollectionItemsParams = {}
  ): Promise<CursorPaginatedResponse<CollectionItem>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.search) query.set('search', params.search);
    if (params.status) query.set('status', params.status);
    const suffix = query.toString();
    return fetchApi(`/assets/objects/${collectionId}/items${suffix ? `?${suffix}` : ''}`);
  },

  getObjectCollectionHolders: (
    collectionId: string,
    params: CollectionHoldersParams = {}
  ): Promise<CursorPaginatedResponse<CollectionHolder>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    const suffix = query.toString();
    return fetchApi(`/assets/objects/${collectionId}/holders${suffix ? `?${suffix}` : ''}`);
  },

  getObjectCollectionActivities: (
    collectionId: string,
    params: CollectionActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<CollectionActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(`/assets/objects/${collectionId}/activities${suffix ? `?${suffix}` : ''}`);
  },

  // Identity collection endpoints
  getIdentityCollection: (collectionId: string): Promise<IdentityCollection> => {
    return fetchApi(`/assets/identities/${collectionId}`);
  },

  getIdentityCollectionItems: (
    collectionId: string,
    params: CollectionItemsParams = {}
  ): Promise<CursorPaginatedResponse<CollectionItem>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.search) query.set('search', params.search);
    if (params.status) query.set('status', params.status);
    const suffix = query.toString();
    return fetchApi(`/assets/identities/${collectionId}/items${suffix ? `?${suffix}` : ''}`);
  },

  getIdentityCollectionHolders: (
    collectionId: string,
    params: CollectionHoldersParams = {}
  ): Promise<CursorPaginatedResponse<CollectionHolder>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    const suffix = query.toString();
    return fetchApi(`/assets/identities/${collectionId}/holders${suffix ? `?${suffix}` : ''}`);
  },

  getIdentityCollectionActivities: (
    collectionId: string,
    params: CollectionActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<CollectionActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(`/assets/identities/${collectionId}/activities${suffix ? `?${suffix}` : ''}`);
  },

  getIdentityCollectionCapacityChart: (
    collectionId: string,
    range: CapacityChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(
      `/assets/objects/${collectionId}/charts/capacity-history${suffix ? `?${suffix}` : ''}`
    );
  },

  getDotbitItemDetail: (nftId: string): Promise<CollectionItem> => {
    return fetchApi(`/assets/identities/dotbit/items/${encodeURIComponent(nftId)}`);
  },

  getDotbitItemActivities: (
    nftId: string,
    params: MnftItemActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<MnftItemActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(
      `/assets/identities/dotbit/items/${encodeURIComponent(nftId)}/activities${suffix ? `?${suffix}` : ''}`
    );
  },

  getDidCkbItemDetail: (nftId: string): Promise<CollectionItem> => {
    return fetchApi(`/assets/identities/did/items/${encodeURIComponent(nftId)}`);
  },

  getDidCkbItemActivities: (
    nftId: string,
    params: MnftItemActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<MnftItemActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(
      `/assets/identities/did/items/${encodeURIComponent(nftId)}/activities${suffix ? `?${suffix}` : ''}`
    );
  },

  getMnftItemDetail: (nftId: string): Promise<MnftItemDetail> => {
    return fetchApi(`/assets/objects/items/${encodeURIComponent(nftId)}`);
  },

  getMnftItemActivities: (
    nftId: string,
    params: MnftItemActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<MnftItemActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(
      `/assets/objects/items/${encodeURIComponent(nftId)}/activities${suffix ? `?${suffix}` : ''}`
    );
  },

  getSporesByOwner: (
    lockHash: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<SporeNft>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/spore/owner/${lockHash}?${query}`);
  },

  getSporesByCluster: (
    clusterId: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<SporeNft>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/spore/clusters/${clusterId}/spores?${query}`);
  },

  getDaoTotalDepositChart: (): Promise<ChartResponse> => {
    return fetchApi('/dao/charts/total-deposit');
  },

  getDaoDailyDepositChart: (): Promise<ChartResponse> => {
    return fetchApi('/dao/charts/daily-deposit');
  },

  getDaoCirculationRatioChart: (): Promise<ChartResponse> => {
    return fetchApi('/dao/charts/circulation-ratio');
  },

  getTransactionCountChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/transaction-count');
  },

  getCellCountChart: (): Promise<StackedAreaChartResponse> => {
    return fetchApi('/charts/cell-count');
  },

  getKnowledgeSizeChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/knowledge-size');
  },

  getCommonKnowledgeCompositionChart: (): Promise<StackedAreaChartResponse> => {
    return fetchApi('/charts/common-knowledge-composition');
  },

  getCapacityTurnoverRatioChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/capacity-turnover-ratio');
  },

  getCellSizeDistributionChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/cell-size-distribution');
  },

  getAddressCohortRetentionChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/address-cohort-retention');
  },

  getMostUtilizedScriptsChart: (): Promise<MostUtilizedScriptsChartResponse> => {
    return fetchApi('/charts/most-utilized-scripts');
  },

  getMostUtilizedAssetsChart: (): Promise<MostUtilizedAssetsChartResponse> => {
    return fetchApi('/charts/most-utilized-assets');
  },

  getBlockTimeDistributionChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/block-time-distribution');
  },

  getEpochTimeDistributionChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/epoch-time-distribution');
  },

  getEpochTimeLengthChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/epoch-time-length');
  },

  getAverageBlockTimeChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/average-block-time');
  },

  getHashRateChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/hash-rate');
  },

  getDifficultyChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/difficulty');
  },

  getUncleRateChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/uncle-rate');
  },

  getMinerAddressDistributionChart: (): Promise<MinerDistributionResponse> => {
    return fetchApi('/charts/miner-address-distribution');
  },

  getTotalSupplyChart: (): Promise<StackedAreaChartResponse> => {
    return fetchApi('/charts/total-supply');
  },

  getNominalApcChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/nominal-apc');
  },

  getSecondaryIssuanceChart: (): Promise<StackedAreaChartResponse> => {
    return fetchApi('/charts/secondary-issuance');
  },

  getInflationRateChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/inflation-rate');
  },

  getHodlWaveChart: (): Promise<StackedAreaChartResponse> => {
    return fetchApi('/charts/hodl-wave');
  },

  getMempoolInfo: (): Promise<MempoolInfo> => {
    return fetchApi('/mempool/info');
  },

  getMempoolTransactions: (): Promise<MempoolTransaction[]> => {
    return fetchApi<{ transactions: MempoolTransaction[]; total: number }>(
      '/mempool/transactions'
    ).then((res) => res.transactions);
  },

  getMempoolBlocks: (): Promise<MempoolBlocksResponse> => {
    return fetchApi('/mempool/blocks');
  },

  getPendingProposals: (): Promise<PendingProposalsResponse> => {
    return fetchApi('/mempool/pending-proposals');
  },

  getScripts: (params: ScriptQueryParams = {}): Promise<CursorPaginatedResponse<KnownScript>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.network) query.set('network', params.network);
    if (params.decoderType) query.set('decoder_type', params.decoderType);
    if (params.search) query.set('search', params.search);
    if (params.sortKey) {
      const snakeKey =
        params.sortKey === 'usedRatio'
          ? 'used_ratio'
          : params.sortKey === 'usedAs'
            ? 'used_as'
            : params.sortKey;
      query.set('sort_key', snakeKey);
    }
    if (params.sortDirection) query.set('sort_direction', params.sortDirection);
    return fetchApi<CursorPaginatedResponse<ScriptFamilyListItemApi>>(`/scripts?${query}`).then(
      (response) => ({
        ...response,
        data: response.data.map(scriptFamilyListItemToKnownScript),
      })
    );
  },

  getScript: (name: string): Promise<KnownScript[]> => {
    return fetchApi<ScriptFamilyDetailApi>(`/scripts/${encodeURIComponent(name)}`).then(
      scriptFamilyDetailToKnownScripts
    );
  },

  getScriptFamilyDetail: (name: string): Promise<ScriptFamilyDetailApi> => {
    return fetchApi(`/scripts/${encodeURIComponent(name)}`);
  },

  getScriptUsage: (name: string): Promise<ScriptUsage> => {
    return fetchApi(`/scripts/${encodeURIComponent(name)}/usage`);
  },

  getScriptCapacityChart: (
    name: string,
    range: CapacityChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(
      `/scripts/${encodeURIComponent(name)}/charts/capacity-history${suffix ? `?${suffix}` : ''}`
    );
  },

  getScriptCapacityChartByCodeHash: (
    codeHash: string,
    scriptKind?: 'lock' | 'type' | 'both',
    range: CapacityChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    query.set('code_hash', codeHash);
    if (scriptKind && scriptKind !== 'both') query.set('script_kind', scriptKind);
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    return fetchApi(`/scripts/charts/capacity-history?${query}`);
  },

  lookupScripts: async (codeHashes: string[], txHash?: string): Promise<ScriptLookupResponse> => {
    if (codeHashes.length === 0) return {};
    const body: Record<string, unknown> = { codeHashes };
    if (txHash) body.txHash = txHash;
    return fetchJson(`${activeApiBase()}/scripts/lookup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  },

  getCodeCell: (
    codeHash: string,
    hashType: ScriptRefHashType
  ): Promise<{ txHash: string | null; outputIndex: number | null }> => {
    const query = new URLSearchParams();
    query.set('code_hash', codeHash);
    query.set('hash_type', hashType);
    return fetchApi(`/scripts/code-cell?${query}`);
  },

  getCodeCells: (codeHash: string, hashType: ScriptRefHashType): Promise<CodeCellsResponse> => {
    const query = new URLSearchParams();
    query.set('code_hash', codeHash);
    query.set('hash_type', hashType);
    return fetchApi(`/scripts/code-cells?${query}`);
  },

  getCyclesStatus: (hash: string): Promise<CyclesStatusResponse> => {
    return fetchApi(`/transactions/${hash}/cycles`);
  },

  triggerCyclesCalculation: async (hash: string): Promise<CyclesStatusResponse> => {
    return fetchJson(`${activeApiBase()}/transactions/${hash}/calculate-cycles`, {
      method: 'POST',
    });
  },

  getActivityVolumeChart: async (): Promise<ChartResponse> => {
    const stats = await api.getDailyActivityStats(0);
    return {
      data: stats.map((s) => ({
        date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
        value: String(
          s.transferCount +
            s.daoDepositCount +
            s.daoWithdrawRequestCount +
            s.daoWithdrawCompleteCount +
            s.tokenCount +
            s.objectCount +
            s.identityCount +
            s.scriptCallCount
        ),
      })),
      title: 'Daily Activity Volume',
      yAxisLabel: 'Activities',
    };
  },

  getActivityTypeBreakdownChart: async (): Promise<StackedAreaChartResponse> => {
    const stats = await api.getDailyActivityStats(0);
    return {
      data: stats.map((s) => ({
        date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
        values: {
          transfer: String(s.transferCount),
          dao: String(s.daoDepositCount + s.daoWithdrawRequestCount + s.daoWithdrawCompleteCount),
          token: String(s.tokenCount),
          object: String(s.objectCount),
          identity: String(s.identityCount),
          scriptCall: String(s.scriptCallCount),
        },
      })),
      series: [
        { key: 'transfer', label: 'Transfer', color: '#8ce00a' },
        { key: 'dao', label: 'DAO', color: '#00d7eb' },
        { key: 'token', label: 'Token', color: '#a78bfa' },
        { key: 'object', label: 'Object', color: '#f472b6' },
        { key: 'identity', label: 'Identity', color: '#2dd4bf' },
        { key: 'scriptCall', label: 'Script Call', color: '#f97316' },
      ],
      title: 'Activity Type Breakdown',
    };
  },

  getActiveAddressesChart: async (): Promise<ChartResponse> => {
    const stats = await api.getDailyActivityStats(0);
    return {
      data: stats.map((s) => ({
        date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
        value: String(s.uniqueAddressCount),
      })),
      title: 'Daily Active Addresses',
      yAxisLabel: 'Addresses',
    };
  },

  getFiberChannels: (
    params: FiberChannelQueryParams = {}
  ): Promise<CursorPaginatedResponse<FiberChannel>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.state) query.set('state', params.state);
    return fetchApi(`/fiber/channels?${query}`);
  },

  getFiberChannel: (channelId: string): Promise<FiberChannelDetail> => {
    return fetchApi(`/fiber/channels/${channelId}`);
  },

  getAddressFiberChannels: (
    addr: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<FiberChannel>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/addresses/${addr}/fiber/channels?${query}`);
  },

  getFiberStats: (): Promise<FiberStats> => {
    return fetchApi<FiberStatsApiResponse>('/fiber/stats').then((stats) => {
      if (stats.openChannels > stats.totalChannels) {
        throw new Error(
          `invalid fiber stats: openChannels ${stats.openChannels} exceeds totalChannels ${stats.totalChannels}`
        );
      }

      return {
        totalChannels: stats.totalChannels,
        openChannels: stats.openChannels,
        closedChannels: stats.totalChannels - stats.openChannels,
        totalCapacityLocked: stats.totalCapacityLocked,
      };
    });
  },

  getCkbVolumeChart: async (): Promise<ChartResponse> => {
    const stats = await api.getDailyActivityStats(0);
    return {
      data: stats.map((s) => ({
        date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
        value: s.totalCkbMoved,
      })),
      title: 'Daily CKB Transfer Volume',
      yAxisLabel: 'CKB (shannons)',
    };
  },

  getNetworkSummary: async (): Promise<NetworkSummary> => fetchApi('/network/summary'),

  getNetworkDistributions: async (): Promise<NetworkDistributions> =>
    fetchApi('/network/distributions'),

  getNetworkHistory: async (
    metric: string,
    granularity: string,
    from?: number,
    to?: number
  ): Promise<NetworkHistory> => {
    const p = new URLSearchParams({ metric, granularity });
    if (from != null) p.set('from', String(from));
    if (to != null) p.set('to', String(to));
    return fetchApi(`/network/history?${p.toString()}`);
  },

  getNetworkNodes: async (params?: {
    cursor?: string;
    reachable?: boolean;
    country?: string;
    version?: string;
  }): Promise<NetworkNodesPage> => {
    const p = new URLSearchParams();
    if (params?.cursor) p.set('cursor', params.cursor);
    if (params?.reachable != null) p.set('reachable', String(params.reachable));
    if (params?.country) p.set('country', params.country);
    if (params?.version) p.set('version', params.version);
    const qs = p.toString();
    return fetchApi(`/network/nodes${qs ? `?${qs}` : ''}`);
  },
};
