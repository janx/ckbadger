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

/**
 * The resource exists but cannot be served YET (503 `service_unavailable`):
 * something it derives from has not reached the node — e.g. a pending
 * transaction whose parent the node does not know. Transient; the caller
 * retries. Never partial data.
 */
export function isServiceUnavailableError(error: unknown): error is ApiRequestError {
  if (!error || typeof error !== 'object') {
    return false;
  }
  const candidate = error as Partial<ApiRequestError>;
  return candidate.code === 'service_unavailable' && candidate.status === 503;
}

/**
 * "The resource does not exist", read from the HTTP status the API answered
 * with — never from the rendered message text, which is prose and changes.
 */
export function isNotFoundApiError(error: unknown): error is ApiRequestError {
  if (!error || typeof error !== 'object') {
    return false;
  }
  return (error as Partial<ApiRequestError>).status === 404;
}

interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  limit: number;
}

interface CursorPaginatedResponse<T> {
  data: T[];
  /** Always the COMMITTED count. Unconfirmed rows are reported in `pool`, never added to it. */
  total?: number;
  limit: number;
  hasMore: boolean;
  nextCursor: string | null;
  /** State of the tx-pool segment. Present only on page one of the address lists. */
  pool?: PoolSummary;
}

/** How far a transaction still in the node's pool could be interpreted. */
interface PoolInterpretation {
  status: 'complete' | 'partial';
  reasons?: { code: string; detail?: string }[];
}

/** Where an unconfirmed transaction stands. */
type PoolStatus = 'pending' | 'proposed' | 'committed_awaiting_index';

/** The `pool` object on a page-one address list response. */
interface PoolSummary {
  enabled: boolean;
  /** False when the mirror cannot reach the node — show "pool view unavailable", never "0 pending". */
  healthy: boolean;
  lastPolledAt: string | null;
  /** Unconfirmed rows served for this address on this page. */
  count: number;
  /** Net CKB this address would gain or lose if every pool transaction committed. Never added to Balance. */
  pendingCkbDelta: string;
  truncated: boolean;
}

interface Block {
  maxBlockBytes: number;
  maxBlockCycles: string;
  minerMessageText: string | null;
  size: number;
  userTransactionsSize: number;
  overheadSize: number;
  totalFees: string;
  cycles: string | null;
  cyclesStatus: 'pending' | 'complete' | 'failed';
  transactionSizes: number[];
  rewardBreakdown: {
    primary: string;
    secondary: string;
    committed: string;
    proposal: string;
  } | null;
  rewardPaidAtBlock: number | null;
  rewardFinalizesAt: number | null;
  confirmations: number;
  parentTimestamp: string | null;
  rawHeader: { epoch: string; dao: string; proposalsHash: string; extraHash: string };
  uncles: Array<{
    number: number;
    hash: string;
    parentHash: string;
    timestamp: string;
    proposalIds: string[];
  }>;
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

/**
 * `/blocks/{id}/uncles/{index}`: one uncle as its including block recorded it. `index` is its
 * position in that block's uncles list; the canonical block at the uncle's height is named
 * beside it. The DAO field carries the raw 32 bytes and the four decoded u64 fields.
 */
interface BlockUncle {
  number: number;
  hash: string;
  parentHash: string;
  timestamp: string;
  epoch: { number: number; index: number; length: number; display: string };
  compactTarget: string;
  difficulty: string;
  nonce: string;
  version: number;
  transactionsRoot: string;
  proposalsHash: string;
  extraHash: string;
  dao: { raw: string; c: string; ar: string; s: string; u: string };
  proposalIds: string[];
  includedBy: { number: number; hash: string; index: number };
  canonicalAtHeight: { number: number; hash: string };
}

interface HardforkActivation {
  id: string;
  name: string;
  shortName: string;
  activationEpoch: number;
  activationDate: string;
  resources?: HardforkResource[];
}

type TransactionActivityType =
  | 'ckb'
  | 'script'
  | 'cellbase'
  | 'dao'
  | 'token'
  | 'object'
  | 'identity';

interface Transaction {
  value: string;
  outputCapacity?: string;
  activity?: GlobalActivity | null;
  proposalOrigin?: {
    blockNumber: number;
    distance: number;
    uncle?: { number: number; hash: string } | null;
  } | null;
  hash: string;
  blockNumber: number;
  blockHash: string;
  index: number;
  inputsCount: number;
  outputsCount: number;
  fee: string;
  txSize?: number;
  cycles: number | null;
  cyclesStatus: 'complete' | 'pending' | 'failed';
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
  avgBlockTime: string | null;
  hashRate: string | null;
  difficulty: string;
  epoch: string;
  tps: string | null;
  estimatedEpochTime: string | null;
  transactionsPerMinute: string | null;
  transactionsPerDay: string | null;
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
  revertedByEvent?: number | null;
  reversesEvents?: number[];
  id: number;
  eventType: string;
  depth: number;
  oldTipNumber: number;
  oldTipHash: string;
  newTipNumber: number;
  newTipHash: string;
  supersededByTipHash: string | null;
  reconcileInterrupted: boolean;
  forkPointNumber: number;
  forkPointHash: string;
  orphanedBlocksCount: number;
  orphanedTxsCount: number;
  detectedAt: string;
  resolvedAt: string | null;
  resolvedBy: string | null;
  resolutionAction: string | null;
  resolutionNotes: string | null;
  previewStatus:
    | 'available'
    | 'no_old_branch'
    | 'old_branch_unavailable'
    | 'node_reader_unavailable'
    | 'not_loaded';
  preview: {
    oldBlockHash: string;
    oldMinerLockHash: string | null;
    competingMinerLockHash: string | null;
    fate: string;
    uncleInBlock: number | null;
    timestampGapMs: number | null;
    noncellbaseTransactions: number;
    committedLaterTransactions: number;
  } | null;
}

interface OrphanedBlock {
  number: number;
  hash: string;
  parentHash: string;
  timestamp: string;
  transactionsCount: number;
  minerLockHash: string | null;
  fate:
    | 'canonical_now'
    | 'included_as_uncle'
    | 'not_included_as_uncle'
    | 'uncle_fate_pending'
    | 'awaiting_canonical_tip'
    | 'canonical_evidence_unavailable';
  uncleIn: { blockNumber: number; blockHash: string; distance: number } | null;
  competingBlock: CanonicalBranchBlock | null;
  timestampGapMs: number | null;
}

interface CanonicalBranchBlock {
  number: number;
  hash: string;
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
  fate:
    | 'cellbase'
    | 'canonical_now'
    | 'committed_later'
    | 'committed_on_canonical_branch'
    | 'not_committed_as_of_tip'
    | 'canonical_evidence_unavailable';
  committedAt: { blockNumber: number; blockHash: string; txIndex: number } | null;
}

interface ReorgDetail {
  event: ReorgEvent;
  orphanedBlocks: OrphanedBlock[];
  orphanedTransactions: OrphanedTransaction[];
  adoptedBranch: CanonicalBranchBlock[];
  canonicalBranch: CanonicalBranchBlock[];
  canonicalTipNumber: number | null;
}

interface ForkOverview {
  scope: string;
  throughDate: string | null;
  days: { date: string; canonicalBlocks: number; uncles: number; nodeSwitches: number }[];
  unclesInWindow: number;
  nodeSwitchesInWindow: number;
  nodeSwitchesAllTime: number;
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
  consensusFeatures: Array<{ rfc: string; activationEpoch: number | null }>;
  scriptDeployments: HardforkScriptDeployment[];
}

export interface VesselCell {
  outpoint: string;
  capacityShannons: string;
  occupiedShannons: string;
  anatomy: {
    capacityFieldBytes: number;
    lockScriptBytes: number;
    typeScriptBytes: number;
    dataBytes: number;
    totalBytes: number;
  };
  createdAtBlock: number;
}

export interface VesselSnapshot {
  anchorBlock: number;
  anchorHash: string;
  firstSampledBlock: number;
  blocksInWindow: number;
  transactionsPerBlock: number;
  outputsPerTransaction: number;
  maxVisible: number;
  examinedOutputs: number;
  cells: VesselCell[];
  tracked: Array<{
    outpoint: string;
    status: 'live' | 'consumed' | 'notCanonical';
    consumedAtBlock: number | null;
    consumingTxHash: string | null;
  }>;
}

export interface HardforkScriptDeployment {
  familyName: string;
  versionName: string;
  versionHash: string;
  codeCellTxHash: string;
  txIndex: number;
  codeCellOutputIndex: number;
  blockNumber: number;
  timestampMs: number;
  epochNumber: number;
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

interface TransactionScriptGroup {
  scriptHash: string;
  kind: 'lock' | 'type';
  ran: boolean;
  inputIndices: number[];
  outputIndices: number[];
  witnessIndex: number | null;
  script: Script;
  cycles: string | null;
}
interface TransactionWitness {
  index: number;
  size: number;
  kind: 'empty' | 'witness_args' | 'ckbfs' | 'cobuild' | 'opaque';
  fields: Array<{ name: string; start: number; end: number; hex: string }>;
}

/** Per-cell semantics read from its payload and the canonical item indexes. */
interface TransactionCellDisplay {
  kind: 'token' | 'object' | 'identity' | 'file';
  standard: string;
  /** Token/file type-script hash, or the canonical object/identity item id. */
  id: string;
  name: string | null;
  collectionId: string | null;
  collectionName: string | null;
  symbol: string | null;
  decimals: number | null;
  /** Exact token base units; null for non-token cells. */
  amount: string | null;
  compositionTier: NonNullable<SporeNft['mediaProfile']>['tier'] | null;
}

interface PoolTransaction {
  hash: string;
  status: PoolStatus;
  value: string | null;
  fee: string;
  feeRate: string;
  txSize: number;
  cycles: string;
  inputsCount: number;
  outputsCount: number;
  timestamp: string;
  interpretation: PoolInterpretation;
  activity: Pick<
    GlobalActivity,
    'participants' | 'protocolActions' | 'typeCalls' | 'lockCalls' | 'isCellbase'
  > | null;
}
interface PoolTransactionsResponse {
  summary: {
    size: string;
    cycles: string;
    minFeeRate: string | null;
    maxFeeRate: string | null;
    newestSeenAt: string | null;
  };
  data: PoolTransaction[];
  total: number;
  limit: number;
  nextCursor: string | null;
  totals: { pending: number; proposed: number; awaitingIndex: number };
  truncated: boolean;
  lastPolledAt: string | null;
}

interface TransactionDetail extends Omit<
  Transaction,
  'blockNumber' | 'blockHash' | 'index' | 'timestamp' | 'value'
> {
  status: TransactionStatus;
  /**
   * Present only while this explorer has not indexed the transaction: `pending`
   * or `proposed` in the node's pool, or `committed_awaiting_index` when the node
   * has it in a block (`blockNumber`/`blockHash` set; `timestamp`, `confirmations`
   * and `index` not yet known). Absent once the store has it.
   */
  poolStatus?: PoolStatus;
  pendingSince: string | null;
  blockNumber: number | null;
  blockHash: string | null;
  index: number | null;
  feeRate?: string;
  txSize?: number;
  cycles: number | null;
  cyclesStatus: 'complete' | 'pending' | 'failed';
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
    /** Exact occupied bytes (not shannons). */
    commonKnowledgeSize?: string | null;
    cellType?: string | null;
    display: TransactionCellDisplay | null;
    lock?: Script;
    type?: Script;
    address?: string;
  }>;
  outputs?: Array<{
    capacity: string;
    commonKnowledgeSize: string;
    virtualCommonKnowledgeSize?: string;
    status?: 'pending' | 'live' | 'consumed' | null;
    consumedByTx?: string | null;
    cellType?: string;
    display: TransactionCellDisplay | null;
    lock?: Script;
    type?: Script;
    address?: string;
  }>;
  scriptGroups?: TransactionScriptGroup[];
  witnessDetails?: TransactionWitness[];
  witnesses: string[];
  witnessesAvailable: boolean;
  /**
   * The `.cell` names this transaction creates, each with the records payload
   * decoded (server-side, hash-verified) from the witness at its own output
   * index. Absent when there are none.
   */
  dotcellNames?: DotCellNameWitness[];
}

/** A `.cell` name a transaction creates, with the records its witness carries. */
interface DotCellNameWitness {
  outputIndex: number;
  label: string;
  name: string;
  identityId: string;
  recordsHash: string;
  records: DotCellRecord[];
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
  token?: {
    name: string | null;
    symbol: string | null;
    decimals: number | null;
    rawAmount: string;
    formattedAmount: string | null;
  };
  deterministic?: CellDeterministicDecode;
  heuristicGuesses: CellDataGuess[];
}

interface CellOutputMolecule {
  bytes: string;
  fields: Array<{ label: string; start: number; end: number; occupied: boolean }>;
  lockScriptHash: string;
  typeScriptHash: string | null;
}

interface CellElf {
  header: {
    class: string;
    endian: string;
    machine: number;
    entry: string;
    programHeaderOffset: string;
    sectionHeaderOffset: string;
    flags: number;
    headerSize: number;
    programHeaderEntrySize: number;
    programHeaderCount: number;
    sectionHeaderEntrySize: number;
    sectionHeaderCount: number;
    sectionNameIndex: number;
  };
  programHeaders: Array<{
    index: number;
    kind: number;
    flags: number;
    fileOffset: string;
    fileSize: string;
    virtualAddress: string;
    memorySize: string;
    alignment: string;
  }>;
  sections: Array<{
    index: number;
    name: string;
    kind: number;
    flags: string;
    address: string;
    fileOffset: string | null;
    fileSize: string | null;
    memorySize: string;
    alignment: string;
  }>;
  unassignedFileRanges: Array<{ start: number; end: number }>;
  comment: string | null;
  riscvAttributes: string | null;
}

interface Cell {
  txHash: string;
  outputIndex: number;
  capacity: string;
  commonKnowledgeSize: string;
  commonKnowledgeSizeBreakdown?: CommonKnowledgeSizeBreakdown;
  lockScriptHash: string;
  address?: string;
  typeScriptHash?: string;
  typeCodeHash?: string;
  typeHashType?: string;
  typeProtocolName?: string;
  dataSize: number;
  createdAtBlock: number;
  createdTimestamp?: string;
  status?: 'live' | 'dead';
  consumedAtBlock?: number;
  consumedByTx?: string;
  spentTimestamp?: string | null;
  consumingInputIndex?: number | null;
  lifetimeMs?: number | null;
  liveAgeMs?: number | null;
  liveAgeAtBlock?: number | null;
  lock?: Script;
  type?: Script;
  /**
   * `/cells/{tx}/{index}` only: the registry protocol slug of each script
   * (`dotcell-account`, `did-ckb`, …), `null` when that script is not a
   * registered protocol. Protocols are recognised by these names, never by
   * comparing code hashes in the frontend.
   */
  protocolScript?: { lock: string | null; type: string | null };
  data?: string;
  dataAnalysis?: CellDataAnalysis;
  cellOutputMolecule?: CellOutputMolecule;
  elf?: CellElf;
  codeCell?: {
    versionHash: string;
    currentReferences: Array<{
      referenceHash: string;
      hashType: string;
      lockLiveCellsCount: number;
      typeLiveCellsCount: number;
    }>;
    typeIdVersions: Array<{
      typeScriptHash: string;
      ordinal: number;
      versionHash: string;
      codeCellTxHash: string;
      codeCellOutputIndex: number;
      observedAtBlock: number;
      reactivation: boolean;
      thisBinary: boolean;
    }>;
    upgradeCohort: Array<{
      previousTxHash: string;
      previousOutputIndex: number;
      currentTxHash: string;
      currentOutputIndex: number;
      typeScriptHash: string;
      previousVersionHash: string | null;
      currentVersionHash: string;
      binaryChanged: boolean | null;
    }>;
  };
  currentSpendRequirements?: Array<{
    kind: 'lock' | 'type';
    referenceHash: string;
    hashType: 'data' | 'type' | 'data1' | 'data2';
    resolution: 'builtin' | 'resolved' | 'ambiguous' | 'not_found' | 'unavailable';
    versionHashes: string[];
    candidateCells: Array<{
      txHash: string;
      outputIndex: number;
      versionHash: string;
      live: boolean;
    }>;
    historicalCells: Array<{
      txHash: string;
      outputIndex: number;
      versionHash: string;
      live: boolean;
    }>;
  }>;
  isDepGroup?: boolean;
  depGroupItems?: DepGroupItem[];
  codeCellOf?: CodeCellScript[];
  cellType?: string;
  virtualCommonKnowledgeSize?: string;
  udtAmount?: string;
  daoInfo?: CellDaoInfo;
  /** `/cells/by-script` only: which index enumerated this row. */
  matchedScriptKind?: 'lock' | 'type';
}

interface LiveCellSummary {
  tip: {
    block: number;
    hash: string;
  };
  liveCells: number;
  classes: {
    dao: number;
    typedNonDao: number;
    plain: number;
  };
  dataBearing: number;
}

interface CellDep {
  outPointTxHash: string;
  outPointIndex: number;
  depType: string;
  scriptHashes: string[];
  resolvedCells: Array<{
    txHash: string;
    index: number;
    dataHash: string;
    typeHash: string | null;
    scriptHashes: string[];
  }>;
}

interface LockScriptInfo {
  codeHash: string;
  name: string;
  scriptKind?: string;
  deprecated: boolean;
}

/**
 * An address's unconfirmed state as a whole: every pool transaction touching
 * it, independent of which list, tab, page or filter is being viewed.
 */
interface AddressPendingSummary {
  txCount: number;
  /** Signed net change in shannons, as a decimal string, if every one commits. Never added to Balance. */
  capacityDelta: string;
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
  /** Null when the tx-pool mirror is disabled or cannot reach the node: nothing may be claimed then. */
  pendingSummary: AddressPendingSummary | null;
}

/** Exact current owner holdings, calculated by the API's live-cell read path. */
export type AddressCellClass = 'dao' | 'token' | 'object' | 'identity' | 'script' | 'plain';
export interface AddressClassTotals {
  cellCount: number;
  capacity: string;
  occupiedCapacity: string;
}
export interface AddressObjectComposition {
  pureCkb: number;
  btcCkb: number;
  decentralizedMixture: number;
  centralizedMixture: number;
  unknown: number;
  decodePending: number;
}
export interface AddressHoldings {
  daoUnclaimedCompensation: string;
  estimatedApc: string | null;
  depositCellsCount: number;
  withdrawalRequestCellsCount: number;
  lockScriptHash: string;
  balance: string;
  occupiedCapacity: string;
  /** Ordinary free capacity; DAO free is a disjoint partition. */
  freeCapacity: string;
  daoFreeCapacity: string;
  liveCellsCount: number;
  holdings: { dao: number; token: number; object: number; identity: number };
  classes: Record<AddressCellClass, AddressClassTotals>;
  objectComposition: AddressObjectComposition;
  objectCollections: Array<{
    id: string | null;
    standard: string;
    name: string | null;
    itemCount: number;
    compositionCounts: AddressObjectComposition;
  }>;
}
export interface AddressCell {
  txHash: string;
  outputIndex: number;
  capacity: string;
  occupiedCapacity: string;
  lockScriptHash: string;
  typeScriptHash: string | null;
  typeCodeHash: string | null;
  typeHashType: string | null;
  dataSize: number;
  createdAtBlock: number;
  class: AddressCellClass;
  display: TransactionCellDisplay | null;
  expiresAt: number | null;
  /** Exact start of uninterrupted identity ownership, null for other classes. */
  heldSinceBlock: number | null;
  /** Original deposit evidence for a currently owned DAO cell, null for other classes. */
  dao: DaoDeposit | null;
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
  /** Null while the transaction is still in the node's pool: it has no block yet. */
  blockNumber: number | null;
  /** `named`: a protocol named this address as a party (by lock-hash prefix) without it holding a cell. */
  txType: 'received' | 'sent' | 'internal' | 'named';
  capacityChange: string;
  /** Block time. Null for a pool row, which carries `timeAddedToPool` instead. */
  timestamp: string | null;
  poolStatus?: PoolStatus;
  timeAddedToPool?: string;
  interpretation?: PoolInterpretation;
  inputsCount: number;
  outputsCount: number;
  fee: string;
  isCellbase: boolean;
  txSize: number | null;
  cycles: number | null;
  scriptLabels: string[];
  cyclesStatus: 'complete' | 'pending' | 'failed';
}

interface AddressToken {
  typeScriptHash: string;
  standard: string;
  name: string | null;
  symbol: string | null;
  /** null when unknown (no label and no on-chain info cell) */
  decimals: number | null;
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

/** The identity standards, spelled as the indexer's `IdentityStandard::as_str()` puts them on the wire. */
type IdentityStandard = 'dotbit' | 'bit_cell' | 'did_ckb' | 'dotcell';

type ItemDelta =
  | {
      kind: 'token';
      typeScriptHash: string;
      delta: string;
      symbol?: string;
      decimals?: number | null;
    }
  | { kind: 'object'; objectId: string; delta: number }
  | { kind: 'identity'; standard: IdentityStandard; identityId: string; delta: number };

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
  /** Decoded lock args (see ExactJson): integers are exact decimal strings. */
  decoded?: Record<string, ExactJson>;
}

/**
 * Free-form JSON the API passes through (protocol-action metadata, decoded lock args): every
 * integer is an exact decimal string, because a JSON number above 2^53 would be rounded by the
 * browser. Read integers from it with BigInt, never Number().
 */
type ExactJson = string | boolean | null | ExactJson[] | { [key: string]: ExactJson };

interface ActivityProtocolAction {
  /** Machine id (`dao`, `fiber`, …): compare and key by this, never show it. */
  protocol: string;
  /** The protocol's display name from the backend registry (`Fiber`, `RGB++`, …). */
  protocolName: string;
  action: string;
  metadata: Record<string, ExactJson>;
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
  /** Null while the transaction is still in the node's pool. */
  blockNumber: number | null;
  txIndex: number | null;
  /** Block time. Null for a pool row, which carries `timeAddedToPool` instead. */
  timestamp: string | null;
  poolStatus?: PoolStatus;
  timeAddedToPool?: string;
  interpretation?: PoolInterpretation;
  ckbDelta: string;
  usedDelta: string;
  isCellbase: boolean;
  itemDeltas: ItemDelta[];
  typeCalls: ActivityTypeCall[];
  lockCalls: ActivityLockCall[];
  protocolActions: ActivityProtocolAction[];
  /** The OTHER parties of this transaction. */
  participants: ParticipantRef[];
  /** Roles the protocol gave this participant, if any. */
  roles: string[];
  tags: number;
}

/** Per-participant data within a global activity response. */
/**
 * One party of a transaction.
 *
 * A party a protocol named by the first 20 bytes of its lock hash may not be
 * resolvable to a full lock hash, so `address` and `lockHash` are nullable and
 * `lockHashPrefix` is what the chain actually recorded. An unresolved party is
 * rendered as its prefix, never as a made-up address.
 */
interface ParticipantRef {
  /** Registered display name of the participant lock, if resolved. */
  lockKind?: string | null;
  address: string | null;
  lockHash: string | null;
  lockHashPrefix: string | null;
  roles: string[];
}

interface ParticipantInfo extends ParticipantRef {
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
  /** Non-cellbase transactions, counted once even when activity kinds overlap. */
  activityCount: number;
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

export interface InventorySummary {
  source: 'canonical_asset_indexes';
  anchor: { blockNumber: number; blockHash: string } | null;
  counts: { token: number; object: number; identity: number };
  objectStandards: { standard: string; collectionsCount: number; liveCount: number }[];
  objectComposition: { tier: string; collectionsCount: number; liveCount: number }[];
  fullyOnchainObjectsCount: number;
  pendingItemsCount: number;
}
export interface TokenHistory {
  source: 'indexed_chain_time';
  completeThrough: string;
  anchor: { blockNumber: number; blockHash: string };
  daily: { date: string; supply: string; capacity: string; occupiedCapacity: string }[];
  weekly: { weekStart: string; transfersCount: number }[];
  supplyEvents: {
    txHash: string;
    blockNumber: number;
    txIndex: number;
    transferSequence: number;
    timestamp: string;
    kind: 'mint' | 'burn';
    amount: string;
    supplyAfter: string;
    lockHash: string;
    address: string | null;
    lockKind: string | null;
  }[];
  mintCount: number;
  burnCount: number;
  transfersCount: number;
}
export interface TokenDistribution {
  holdersCount: number;
  totalSupply: string;
  largestHolderBalance: string;
  nextNineBalance: string;
  remainingBalance: string;
  unit: 'token' | 'raw';
  decimals: number | null;
  buckets: {
    lowerBound: string;
    upperBound: string | null;
    holdersCount: number;
    balance: string;
  }[];
}
export interface ItemCellVersion {
  txHash: string;
  outputIndex: number;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  isLive: boolean;
  consumedByTx: string | null;
  consumedAtBlock: number | null;
  ownerLockHash: string;
  ownerAddress: string;
  capacity: string;
  occupiedCapacity: string;
  lock: Script;
  type: Script | null;
  anatomy: { capacityBytes: number; lockBytes: number; typeBytes: number; dataBytes: number };
  dataHash: string | null;
  lockName: string | null;
  typeName: string | null;
}
export type ItemKind = 'spore' | 'mnft' | 'dotbit' | 'bit-cell' | 'did' | 'dotcell';
export interface ItemCellVersions {
  data: ItemCellVersion[];
  total: number;
}

export interface GeneDefinition {
  index: number;
  traitIndex: number;
  valueIndex: number;
  name: string;
  typeTag: string;
  byteOffset: string;
  byteLength: string;
  rule: string;
  ruleArguments: unknown;
}
export type PatternDescription =
  | {
      status: 'supported';
      format: 'moleculeDob0' | 'jsonDob0' | 'nervapeJsonDob0';
      rangePolicy: 'strict' | 'clipToDna';
      genes: GeneDefinition[];
    }
  | { status: 'unsupported'; reason: string };
export interface GeneBytes {
  geneIndex: number;
  declaredByteOffset: string;
  declaredByteLength: string;
  byteOffset: number;
  byteLength: number;
  truncated: boolean;
  rawHex: string;
}
export type CurrentPattern =
  | { status: 'notDob'; steps: [] }
  | {
      status: 'dob';
      version: number | null;
      source: 'currentCanonicalDecoder';
      steps: {
        step: number;
        decoderReference: { type: string; hash: string };
        decoderBinaryHash: string | null;
        description: PatternDescription;
        bytes: GeneBytes[];
      }[];
    };
export interface ObjectGenetics {
  dnaHex: string | null;
  number: string | null;
  cachedVmTraits: {
    status: 'decoded' | 'failed' | 'pending' | 'notDob';
    decodedAt: string | null;
    traits: DobTrait[];
    source: 'cachedVm';
    originalDecoderHashRecorded: false;
    reason?: string;
  };
  currentPattern: CurrentPattern;
}
export interface CollectionInsights {
  completeThrough: string | null;
  calendarAnchor: { blockNumber: number; blockHash: string; chainTime: string } | null;
  calendarSource: 'indexedChainTimeUtc8';
  holderDistribution: { range: string; holders: number; items: number }[];
  weeklyActivity: {
    weekStart: string;
    mintTransactions: number;
    transferTransactions: number;
    burnTransactions: number;
  }[];
  genetics: {
    liveItems: number;
    eligibleItems: number;
    nonDobItems: number;
    decodedItems: number;
    pendingItems: number;
    failedItems: number;
    complete: boolean;
    traits: { name: string; values: { value: string; count: number }[] }[];
  };
  currentPattern: CurrentPattern;
}
export interface GalleryObject extends SporeNft {
  dnaHex: string | null;
  number: string | null;
  traits: DobTrait[];
  decodeStatus: string;
}
export interface ItemFeedContext {
  itemId: string;
  action: string;
  createdCell: ItemCellVersion | null;
  consumedCell: ItemCellVersion | null;
}
export type ItemActivity = GlobalActivity & { itemContext: ItemFeedContext };
export type ItemTransaction = Transaction & { itemContext: ItemFeedContext };
export interface DidProtocol {
  did: string;
  itemId: string;
  documentVersion: number;
  documentEncoding: 'cborLosslessJson';
  documentCborHex: string;
  document: unknown;
  localId: string | null;
  pdsEndpoints: string[];
}
export interface DotbitProtocol {
  accountId: string;
  account: string;
  nextAccountId: string | null;
  nextAccountName: string | null;
  expiredAt: string | null;
  witnessVersion: number;
  registeredAt: string;
  status: number;
  owner: { algorithm: number; argsHex: string };
  manager: { algorithm: number; argsHex: string };
  records: {
    type: string;
    key: string;
    label: string;
    value: string | null;
    valueHex: string;
    ttl: number;
  }[];
}
export interface BitCellProtocol {
  linkedAccountSource: 'currentCanonicalAccount';
  identityId: string;
  accountId: string;
  account: string;
  expiredAt: string;
  linkedAccount: {
    id: string;
    name: string | null;
    isLive: boolean;
    registeredAt: string | null;
    expiredAt: string | null;
  } | null;
}
export interface DotCellProtocol {
  name: string;
  label: string;
  layoutVersion: number;
  expiredAt: string;
  recordsHash: string;
  nextId: string;
  owner: DotCellPartyRef;
  manager: DotCellPartyRef;
  records: DotCellRecord[];
}
export interface MnftProtocol {
  characteristicHex: string;
  configure: number;
  state: number;
  claimed: boolean;
  locked: boolean;
  permissions: {
    allowClaim: boolean;
    allowLock: boolean;
    allowExtension: boolean;
    allowCharacteristicUpdate: boolean;
    allowTransferBeforeClaim: boolean;
    allowTransferAfterClaim: boolean;
    allowDestroyBeforeClaim: boolean;
    allowDestroyAfterClaim: boolean;
  };
}
export interface ProtocolVersion<T> {
  cell: ItemCellVersion;
  dataHex: string;
  fields: { name: string; start: number; end: number; valueHex: string }[];
  decoded: T;
}
export type ItemProtocol =
  | {
      kind: 'spore';
      itemId: string;
      versions: ProtocolVersion<{ contentHex: string; dnaHex: string | null }>[];
    }
  | { kind: 'm-nft'; itemId: string; versions: ProtocolVersion<MnftProtocol>[] }
  | { kind: 'dotbit'; itemId: string; versions: ProtocolVersion<DotbitProtocol>[] }
  | { kind: 'bit_cell'; itemId: string; versions: ProtocolVersion<BitCellProtocol>[] }
  | { kind: 'did_ckb'; itemId: string; versions: ProtocolVersion<DidProtocol>[] }
  | { kind: 'dotcell'; itemId: string; versions: ProtocolVersion<DotCellProtocol>[] };
export interface DidPdsDistribution {
  source: 'currentCanonicalDocuments';
  totalLiveIdentities: number;
  withoutPds: number;
  groups: { endpoint: string; count: number }[];
}

interface Token {
  typeScriptHash: string;
  typeCodeHash: string;
  typeHashType: string;
  typeArgs: string;
  standard: string;
  name: string | null;
  symbol: string | null;
  /** null when unknown (no label and no on-chain info cell) */
  decimals: number | null;
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
  sameSymbolTokens?: { typeScriptHash: string; holdersCount: number }[];
  issuer: { lockHash: string; address: string | null; lockKind: string | null } | null;
  totalCapacity: string | null;
  totalCommonKnowledgeSize: string | null;
}

interface TokenHolder {
  lockScriptHash: string;
  address: string | null;
  balance: string;
  lockKind: string | null;
}

export interface TokenHoldersResponse extends CursorPaginatedResponse<TokenHolder> {
  totalSupply: string;
  anchor: { blockNumber: number; blockHash: string } | null;
}

interface TokenTransfer {
  txHash: string;
  blockNumber: number;
  fromLockHash: string | null;
  fromAddress: string | null;
  toLockHash: string;
  toAddress: string | null;
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
  /** Token transfer count; omitted for object/identity collections. */
  transfersCount?: number;
  liveCount: number | null;
  activitiesCount: number | null;
  transfers24h: number;
  decimals: number | null;
  totalSupply: string | null;
  contentType: string | null;
  contentSize: number | null;
  clusterId: string | null;
  clusterName: string | null;
  ownedCapacity: string | null;
  ownedKnowledge: string | null;
  compositionTier:
    | 'btc_ckb'
    | 'pure_ckb'
    | 'decentralized_mixture'
    | 'centralized_mixture'
    | 'unknown'
    | 'decode_pending'
    | null;
  onchainRatio: string | null;
  onchainCount: number | null;
  hMultiplier: string | null;
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
    | 'unknown'
    | 'decode_pending';
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
  transferSequence: number;
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
  address?: string | null;
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
  tipBlockNumber: number;
  /** Exact block-0 burn-policy capacity from the persisted genesis baseline, shannons. */
  genesisBaselineBurnt: string;
  totalDeposited: string;
  totalDepositedCkb: string;
  totalDepositors: number;
  activeDeposits: number;
  depositCellsCount: number;
  withdrawalRequestCellsCount: number;
  pendingWithdrawalCapacity: string;
  pendingWithdrawalCapacityCkb: string;
  daoCapacity: string;
  daoFreeCapacity: string;
  /** Exact secondary-issuance weight for depositors (phase-0 free + accrued). */
  depositorAllocationWeight: string;
  tipTotalIssuance: string;
  tipProtocolOccupiedCapacity: string;
  rolling24hBaselineBlockNumber: number | null;
  rolling24hCutoffChainTimeMs: string | null;
  totalCompensationPaid: string;
  totalCompensationPaidCkb: string;
  unclaimedCompensation: string;
  unclaimedCompensationCkb: string;
  averageDepositDays: string | null;
  estimatedApc: string;
  miningReward: string;
  miningRewardCkb: string;
  depositCompensation: string;
  depositCompensationCkb: string;
  burnt: string;
  burntCkb: string;
  depositChange24h: string | null;
  depositorsChange24h: number | null;
  claimedCompensationChange24h: string | null;
  unclaimedCompensationChange24h: string | null;
}

interface DaoEpoch {
  number: number;
  index: number;
  length: number;
  display: string;
}

interface DaoAmountCount {
  capacity: string;
  count: number;
}

interface DaoWindowBucket extends DaoAmountCount {
  nominalDay: number;
  fromEpochOffset: number;
  throughEpochOffset: number;
}

interface DaoOwner {
  rank: number;
  lockScriptHash: string;
  address: string;
  lockCodeHash: string;
  lockHashType: string;
  lockKind: string | null;
  depositCount: number;
  capacity: string;
  sharePercent: string;
}

interface DaoEvidence {
  tipBlockNumber: number;
  tipEpoch: DaoEpoch;
  phase0: DaoAmountCount;
  phase1: DaoAmountCount;
  phase1ReadyByEpochLock: DaoAmountCount;
  phase1WaitingForEpochLock: DaoAmountCount;
  windows: DaoWindowBucket[];
  concentration: {
    currentDepositors: number;
    top1Capacity: string;
    top10Capacity: string;
    top100Capacity: string;
    top1SharePercent: string | null;
    top10SharePercent: string | null;
    top100SharePercent: string | null;
    owners: DaoOwner[];
  };
  ar: {
    tipAr: string;
    genesisAr: string;
    genesisBlockNumber: number;
    tipChainTimeMs: string;
    sinceGenesisGrowthPercent: string;
    milestones: { blockNumber: number; chainTimeMs: string; ar: string; arIndex: string }[];
    year365: {
      anchorBlockNumber: number;
      anchorChainTimeMs: string;
      anchorAr: string;
      elapsedChainTimeMs: string;
      observedGrowthPercent: string;
    } | null;
  };
  header: {
    blockNumber: number;
    daoHex: string;
    c: string;
    ar: string;
    s: string;
    u: string;
  };
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
  apc: string | null;
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
  ownerLockHash: string | null;
  ownerAddress?: string | null;
  sporesCount: number;
  totalCount: number;
  holdersCount: number;
  activitiesCount: number;
  createdAtBlock: number | null;
  /** Shannons; null when the index holds no aggregate for the cluster. */
  ownedCapacity: string | null;
  /** Shannons; null when the index holds no aggregate for the cluster. */
  ownedKnowledge: string | null;
  composition?: {
    tier:
      | 'btc_ckb'
      | 'pure_ckb'
      | 'decentralized_mixture'
      | 'centralized_mixture'
      | 'unknown'
      | 'decode_pending';
    onchainCount: number;
    pureCkbCount: number;
    decentralizedMixtureCount: number;
    centralizedMixtureCount: number;
    unknownCount: number;
    onchainRatio: string | null;
  };
}

interface SporeNft {
  createdAtTx?: string;
  sporeId: string;
  txHash: string;
  outputIndex: number;
  clusterId: string | null;
  contentType: string;
  contentSize: number;
  ownerLockHash: string | null;
  ownerAddress?: string | null;
  isLive: boolean;
  createdAtBlock: number;
  mediaProfile?: {
    tier:
      | 'btc_ckb'
      | 'pure_ckb'
      | 'decentralized_mixture'
      | 'centralized_mixture'
      | 'unknown'
      | 'decode_pending';
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

/** `/spore/objects/{id}`: a spore plus the exact capacity its cell owns (lists carry neither). */
interface SporeDetail extends SporeNft {
  /** Hash of the spore cell's type script (0x + 64 hex): the key DOB marketplaces use. */
  typeScriptHash: string;
  /** Shannons the spore's cell holds; `"0"` once melted. */
  ownedCapacity: string;
  /** The occupied part of `ownedCapacity` (common knowledge), in shannons. */
  ownedKnowledge: string;
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

export interface SporeDobDecoded {
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
  ownedCapacity: string | null;
  ownedKnowledge: string | null;
  composition?: {
    tier:
      | 'btc_ckb'
      | 'pure_ckb'
      | 'decentralized_mixture'
      | 'centralized_mixture'
      | 'unknown'
      | 'decode_pending';
    onchainCount: number;
    pureCkbCount: number;
    decentralizedMixtureCount: number;
    centralizedMixtureCount: number;
    unknownCount: number;
    onchainRatio: string | null;
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
    permissions: MnftProtocol['permissions'];
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
  /**
   * `null` when the chain only gave a 20-byte owner prefix (`.cell`) that
   * resolves to no known lock — a holder the API can name but not address.
   */
  lockScriptHash: string | null;
  /** Set only for collections whose chain-level owner is a lock-hash prefix. */
  ownerHashPrefix?: string;
  address: string | null;
  itemCount: number;
}

// ── `.cell` (DotCell) names ────────────────────────────────────────────────
//
// A `.cell` name stores its owner and manager as the FIRST 20 BYTES of a lock
// script hash. The API reports that prefix verbatim and resolves it to a full
// lock hash/address only when exactly one known lock matches, so an
// unresolved prefix must never be displayed as an address.

interface DotCellPartyRef {
  hashPrefix: string;
  lockHash: string | null;
  address: string | null;
  scriptName: string | null;
}

interface DotCellDecodedAddress {
  address: string;
  lockHash: string;
}

interface DotCellRecord {
  key: string;
  label: string;
  valueHex: string;
  valueUtf8: string | null;
  ttl: number;
  decodedAddress: DotCellDecodedAddress | null;
}

interface DotCellNameRef {
  identityId: string;
  label: string;
  name: string;
}

interface DotCellOutPoint {
  txHash: string;
  index: number;
}

interface DotCellSale {
  priceShannons: string;
  seller: DotCellPartyRef;
  offerOutPoint: DotCellOutPoint | null;
}

type DotCellState = 'active' | 'grace' | 'free' | 'recycled';

interface DotCellItem {
  identityId: string;
  label: string;
  name: string;
  isLive: boolean;
  createdAtBlock: number;
  createdAtTx: string;
  layoutVersion: number;
  namespaceArgs: string;
  expiredAt: number;
  state: DotCellState;
  graceEndsAt: number;
  owner: DotCellPartyRef;
  manager: DotCellPartyRef;
  sale: DotCellSale | null;
  records: DotCellRecord[];
  recordsHash: string;
  nextId: string;
  parent: DotCellNameRef | null;
  /** The first page of live sub-names; the rest page through `getDotCellItemChildren`. */
  children: DotCellNameRef[];
  childrenHasMore: boolean;
  /** Cursor for the next page of sub-names; null when `children` is the whole list. */
  childrenNextCursor: string | null;
  liveOutPoint: DotCellOutPoint | null;
}

interface DotCellRing {
  namespaceArgs: string;
  rootOutPoint: DotCellOutPoint;
  firstId: string;
  liveCount: number;
}

interface AddressDotCellName {
  identityId: string;
  label: string;
  name: string;
  expiredAt: number;
}

interface CollectionActivity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  actions: string[];
}

interface MnftClassSummary {
  permissions: MnftProtocol['permissions'];
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
  permissions: MnftProtocol['permissions'];
  claimed: boolean;
  locked: boolean;
  createdAtTx?: string;
  ownerAddress?: string | null;
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
    tier:
      | 'btc_ckb'
      | 'pure_ckb'
      | 'decentralized_mixture'
      | 'centralized_mixture'
      | 'unknown'
      | 'decode_pending';
    onchainCount: number;
    pureCkbCount: number;
    decentralizedMixtureCount: number;
    centralizedMixtureCount: number;
    unknownCount: number;
    onchainRatio: string | null;
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
  value2?: string | null;
}

interface ChartResponse {
  data: ChartDataPoint[];
  title: string;
  yAxisLabel: string;
  y2AxisLabel?: string;
}

/**
 * `/charts/uncle-rate`: the daily uncle rate plus the NC-Max orphan-rate target it is measured
 * against, a consensus parameter (`orphan_rate_target` of the node's `get_consensus`).
 */
interface UncleRateChartResponse extends ChartResponse {
  orphanRateTarget: { numerator: number; denominator: number };
}

/**
 * `/statistics/ckbytes-allocation`: circulation split into three exact, non-overlapping parts at
 * one block (`blockNumber`); every amount is a shannon string. `circulating` = knowledge + daoFree
 * + free exactly. `knowledge` is the occupied capacity of every live cell (DAO cells included),
 * `daoFree` the free capacity of the live NervosDAO cells (deposits and withdraw requests), `free`
 * everything else. `daoCapacity` is those DAO cells' whole capacity — context, not a part.
 */
interface CkbytesAllocation {
  blockNumber: number;
  circulating: string;
  knowledge: string;
  daoFree: string;
  free: string;
  daoCapacity: string;
}

interface TxStatsDataPoint {
  label: string;
  value: number;
  /** The bucket ends at or before running-maximum chain time; only the newest bucket is in progress. */
  complete: boolean;
}

/**
 * `/statistics/tx-stats`, oldest first. Every bucket says whether it is `complete`: complete ones
 * come first, then the in-progress ones; the newest is always in progress and, in the first minutes
 * of a new hour or day, the previous one still is too. `hourlyData` is UTC hours (`HH:00`),
 * `dailyData` UTC+8 days (`MM/DD`). Hours and days without blocks have no bucket, so a series can
 * be short or empty (then `currentHour` / `currentDay` are 0).
 */
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
  minerLockHash: string;
  address: string | null;
  minerName: string | null;
  blocksMined: number;
  percentage: string;
}

interface MinerDistributionResponse {
  data: MinerDistributionDataPoint[];
  title: string;
  totalBlocks: number;
  windowDays: number;
  fromDate: string;
  toDate: string;
}

interface StackedAreaDataPoint {
  date: string;
  values: Record<string, string>;
  protocolTotalShannons?: string;
  phase1CompensationShannons?: string;
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

export interface AssetCapacityHistory extends StackedAreaChartResponse {
  source: 'indexedChainTimeUtc8';
  completeThrough: string;
  anchor: { blockNumber: number; blockHash: string; chainTime: string };
}
export interface ClusterProtocol {
  kind: 'spore_cluster';
  clusterId: string;
  source: 'canonicalLiveTypeIndex';
  current: {
    cell: ItemCellVersion;
    dataHex: string;
    fields: { name: string; start: number; end: number; valueHex: string }[];
    decoded: { name: string; description: string };
  } | null;
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
  codeClasses?: ScriptCodeClassTotalApi[];
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
  codeClasses?: ScriptCodeClassTotalApi[];
}

export interface ScriptCodeClassTotalApi {
  class:
    | 'fixed'
    | 'unspendable'
    | 'one_key'
    | 'multisig'
    | 'unsupported'
    | 'retired'
    | 'mixed'
    | 'ambiguous';
  lockCellsCount: number;
  lockOwnedCapacitySum: string;
  referenceCount: number;
}

export interface ScriptsListApi extends CursorPaginatedResponse<KnownScript> {
  classSummary?: ScriptCodeClassTotalApi[];
  roleCounts?: { all: number; locks: number; types: number };
}

export interface ScriptCustodyApi {
  class: ScriptCodeClassTotalApi['class'];
  lockCodeHash: string | null;
  lockHashType: string | null;
  lockArgs: string | null;
  threshold: number | null;
  keyCount: number | null;
  requireFirstN: number | null;
  keyHashes: string[];
  verifiedByTx: string | null;
  witnessState: 'never_signed' | 'verified' | 'unavailable' | 'not_applicable';
  owners: {
    lockCodeHash: string;
    lockHashType: string;
    lockArgs: string;
    class: ScriptCodeClassTotalApi['class'];
    codeCells: string[];
  }[];
}

interface ScriptObservedReferenceApi {
  referenceHash: string;
  hashType: string;
  liveCellsCount: number;
  cellsCount: number;
  ownedCapacitySum: string;
  ownedKnowledgeSum: string;
  currentVersionHash?: string | null;
  custody?: ScriptCustodyApi;
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
  firstDeployedBlock?: number | null;
  firstDeployedTxIndex?: number | null;
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
  createdAtBlock?: number;
  status?: 'live' | 'consumed';
  consumedAtBlock?: number | null;
  consumedByTx?: string | null;
  custody?: ScriptCustodyApi;
}

export interface ScriptHistoryMarkerApi {
  referenceHash: string;
  hashType: string;
  versionHash: string | null;
  event: 'deployed' | 'consumed';
  blockNumber: number;
  txIndex: number | null;
  timestamp: number;
  chainTimeMs: number;
  indexedDay: string;
  codeCellTxHash: string;
  codeCellOutputIndex: number;
  eventTxHash: string;
  custody: ScriptCustodyApi;
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
  references?: ScriptObservedReferenceApi[];
  codeClasses?: ScriptCodeClassTotalApi[];
  history?: ScriptHistoryMarkerApi[];
}

interface ScriptQueryParams {
  limit?: number;
  cursor?: string;
  network?: string;
  decoderType?: string;
  search?: string;
  role?: 'lock' | 'type';
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
  /** The queried reference's own label description; null when it has none. */
  description?: string | null;
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
    codeClasses: script.codeClasses,
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
  fee: string;
  size: number;
  cycles: number;
  /** Shannon per byte, rounded half-up to two decimals. */
  feeRate: string;
  ancestorsCount: number;
  timestamp: number;
  status: string;
}

interface FeeRateRange {
  min: string | null;
  max: string | null;
}

interface MempoolBlock {
  index: number;
  transactionCount: number;
  totalSize: number;
  totalFee: string;
  totalCycles: number;
  feeRateRange: FeeRateRange;
  medianFeeRate: string | null;
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
  fee: string | null;
  size: number | null;
  cycles: number | null;
  /** Shannon per byte, rounded half-up to two decimal places. */
  feeRate: string | null;
}

interface PendingProposalsResponse {
  proposals: PendingProposal[];
  tipBlockNumber: number;
  totalCount: number;
}

interface BlockFeeStats {
  blockNumber: number;
  totalSize: number;
  totalCycles: number | null;
  cyclesStatus: 'pending' | 'complete' | 'failed';
  /** Shannon per 1000 bytes, rounded half-up to two decimals; null without fee samples. */
  avgFeeRate: string | null;
  minFeeRate: string | null;
  maxFeeRate: string | null;
  transactionCount: number;
  cyclesPending?: boolean;
}

interface BlockProposal {
  proposalIndex: number;
  proposalId: string;
  committedTxHash: string | null;
  committedBlockNumber: number | null;
  /** The uncle that carried this proposal; null for the block's own proposals. */
  uncle: { number: number; hash: string } | null;
}

/**
 * `GET /pipeline` — one snapshot of the homepage transaction pipeline. Every chain fact here
 * (proposal states, `proposedIn`, cellbase payee, pool stages, the next-block prediction) is
 * derived once on the backend from the same proposal zones; the frontend only lays it out.
 */
type PipelineStatus = 'ok' | 'poolTipMismatch';

interface PipelineConsensus {
  proposalWindowClosest: number;
  proposalWindowFarthest: number;
  maxBlockBytes: number;
  maxBlockCycles: number;
  maxBlockProposals: number;
}

interface PipelineBlockRef {
  number: number;
  hash: string;
}

type PipelineProposalState = 'early' | 'waiting' | 'committed' | 'expired';

interface PipelineProposal {
  /** `0x` + 10 bytes. */
  shortId: string;
  /** Index into `PipelineBlock.uncles`; null = the block's own proposal. */
  uncleIndex: number | null;
  state: PipelineProposalState;
  /** The committed tx, or the matching pool tx; null when neither is known. */
  txHash: string | null;
  committedBlock: number | null;
}

interface PipelineProposedIn {
  block: number;
  uncleIndex: number | null;
}

type PipelineCyclesStatus = 'done' | 'pending' | 'failed';

interface PipelineTx {
  hash: string;
  size: number;
  fee: string;
  /** Shannons per KB. */
  feeRate: string;
  cycles: number | null;
  cyclesStatus: PipelineCyclesStatus;
  /** The earliest proposal inside the commit window [c − farthest, c − closest]. */
  proposedIn: PipelineProposedIn;
}

/**
 * The reward of the block a cellbase pays, as the node's `get_block_economic_state(target)
 * .miner_reward` reports it: exact shannons, as decimal strings.
 */
interface PipelineCellbaseReward {
  /** Primary issuance: the target's whole primary block reward. */
  primary: string;
  /** The miner's share of the target's secondary issuance. */
  secondary: string;
  /** The fees of the transactions the target committed, less the proposers' share. */
  committed: string;
  /** The proposers' share of the fees of the transactions the target proposed first. */
  proposal: string;
  /** primary + secondary + committed + proposal. */
  total: string;
  /**
   * False only when `total` cannot cover the occupied capacity of its own reward cell: CKB then
   * creates no cellbase output and the reward is not issued. Otherwise the cellbase's one output
   * holds exactly `total`.
   */
  paid: boolean;
}

/** The block a cellbase pays (`finalized_at` names the cellbase's block) and its reward. */
interface PipelineCellbasePays extends PipelineBlockRef {
  reward: PipelineCellbaseReward;
}

interface PipelineCellbase {
  hash: string;
  size: number;
  /**
   * Null exactly for blocks at height `proposalWindowFarthest + 1` or below (1–11 on mainnet): their
   * target would be genesis or earlier, so they pay no reward and their cellbase has no output.
   */
  pays: PipelineCellbasePays | null;
}

interface PipelineBlock {
  number: number;
  hash: string;
  parentHash: string;
  timestamp: string;
  /** Includes the cellbase. */
  txCount: number;
  /** Σ serialized size in block (raw + 4), cellbase included. */
  bytes: number;
  cycles: number | null;
  cyclesPending: boolean;
  /** Zone order: the block's own proposals first, then the uncles'. */
  proposals: PipelineProposal[];
  uncles: PipelineBlockRef[];
  /** Non-cellbase txs by bytes, descending; capped by the backend. */
  txs: PipelineTx[];
  /** txCount − 1 − txs.length. */
  omittedTxs: number;
  cellbase: PipelineCellbase;
}

interface PipelinePoolTx {
  hash: string;
  size: number;
  fee: string;
  feeRate: string;
  cycles: number;
  /** Milliseconds since the epoch. */
  arrivedAt: number;
  /** Non-null only in `proposed`. */
  proposedIn: PipelineProposedIn | null;
}

interface PipelinePool {
  pending: PipelinePoolTx[];
  proposed: PipelinePoolTx[];
  /** Totals before truncation. */
  pendingTotal: number;
  proposedTotal: number;
}

interface PipelineNextTx {
  hash: string;
  /** Serialized size in block (raw + 4). */
  size: number;
}

/** Predicted next block, cellbase excluded. */
interface PipelineNext {
  /** Predicted txs by size, descending (like `PipelineBlock.txs`); capped by the backend. */
  txs: PipelineNextTx[];
  /** Predicted count − txs.length. */
  omittedTxs: number;
  bytes: number;
  cycles: number;
  proposals: number;
}

interface PipelineSnapshot {
  status: PipelineStatus;
  consensus: PipelineConsensus;
  /** ckbadger sync tip (store). */
  tip: PipelineBlockRef;
  /** Node tx pool tip. */
  poolTip: PipelineBlockRef;
  /** Newest → older. */
  blocks: PipelineBlock[];
  /** null when status != 'ok'. */
  pool: PipelinePool | null;
  /** null when status != 'ok'. */
  next: PipelineNext | null;
}

type CyclesCalculationStatus = 'done' | 'calculating' | 'queued' | 'failed' | 'paused' | 'notFound';

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
  /** Shares of total live capacity (C − S at the tip), not of knowledge size. */
  capacityBreakdown: CapacityCategory[];
  totalLiveCapacityCkb: string;
  totalKnowledgeSizeCkb: string;
}

type FiberChannelState = 'open' | 'cooperativelyClosed' | 'forceClosed' | 'settled';

interface FiberChannel {
  channelId: string;
  state: FiberChannelState;
  /** Funding cell CKB capacity in shannons — always CKB, even for UDT-funded channels. */
  capacity: string;
  /** Type script hash of the funding token; null for plain-CKB channels. */
  udtTypeHash: string | null;
  /** Token amount held by the funding cell (u128 decimal); null for plain-CKB channels. */
  udtAmount: string | null;
  fundingTxHash: string;
  fundingOutputIndex: number;
  openBlock: number;
  openTimestamp: string;
  closeBlock: number | null;
  closeTimestamp: string | null;
  closeTxHash: string | null;
  commitmentTxHash: string | null;
  /**
   * Revocation delay from the commitment lock args, `since`-encoded, as an exact u64 decimal
   * string (its relative-epoch flag bits put it above 2^53); null unless force-closed. Read it
   * with BigInt, never Number.
   */
  delayEpoch: string | null;
  /** `delayEpoch` decoded; null exactly when `delayEpoch` is null. */
  delayEpochDecoded: FiberEpochDelay | null;
  settlementBlock: number | null;
  settlementTimestamp: string | null;
  settlementTxHash: string | null;
  /** Non-fiber owners of the funding tx; the fiber locks are never participants. */
  participants: string[];
  observedParties: { lockScriptHash: string; address: string | null }[];
  observedLifetimeMs: string | null;
  lifetimeAnchorBlock: number | null;
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
  cooperativelyClosedChannels: number;
  forceClosedWaitingChannels: number;
  forceClosedSettledChannels: number;
  observedParties: number;
  observedPairs: number;
  firstOpenTimestamp: string | null;
  typicalClosedLifetimeMs: string | null;
  closedWithinHour: number;
  tipBlockNumber: number;
  tipChainTimeMs: string;
}

interface FiberStats {
  totalChannels: number;
  openChannels: number;
  closedChannels: number;
  totalCapacityLocked: string;
  cooperativelyClosedChannels: number;
  forceClosedWaitingChannels: number;
  forceClosedSettledChannels: number;
  observedParties: number;
  observedPairs: number;
  firstOpenTimestamp: string | null;
  typicalClosedLifetimeMs: string | null;
  closedWithinHour: number;
  tipBlockNumber: number;
  tipChainTimeMs: string;
}

interface FiberParty {
  lockScriptHash: string;
  address: string | null;
  channels: number;
  openChannels: number;
  openCapacity: string;
}

interface FiberPair {
  firstLockScriptHash: string;
  secondLockScriptHash: string;
  channels: number;
  openChannels: number;
  cooperativelyClosedChannels: number;
  forceClosedWaitingChannels: number;
  forceClosedSettledChannels: number;
  openCapacity: string;
  firstOpenBlock: number;
  lastOpenBlock: number;
  typicalClosedLifetimeMs: string | null;
}

interface FiberNetwork {
  summary: FiberStatsApiResponse;
  parties: FiberParty[];
  pairs: FiberPair[];
}

interface FiberCellEvidence {
  txHash: string;
  outputIndex: number;
  blockNumber: number;
  capacity: string;
  lockCodeHash: string;
  lockHashType: string;
  lockArgs: string;
  isLive: boolean;
  consumedByTx: string | null;
}

/** A `since` epoch delay: relative or absolute, `epochs + index/length` epochs. */
interface FiberEpochDelay {
  relative: boolean;
  epochs: number;
  index: number;
  length: number;
}

interface FiberCommitmentEvidence extends FiberCellEvidence {
  pubkeyHash: string;
  /** The commitment lock's `since`-encoded delay as an exact u64 decimal string. */
  delayEpoch: string;
  /** `delayEpoch` decoded by the channel's one since-epoch decoder. */
  delayEpochDecoded: FiberEpochDelay;
  version: string;
  settlementHash: string | null;
  settlementFlag: number | null;
  lockFormat: 'legacy36' | 'full57';
  epochLock:
    | 'firstSettlementDelay'
    | 'subsequentSettlementNoReappliedDelay'
    | 'legacyFormatRuleUnverified';
  readyEpoch: string | null;
  epochLockMetAtTip: boolean | null;
  tipEpoch: string;
}

interface FiberChannelEvidence {
  channelId: string;
  funding: FiberCellEvidence;
  currentCommitment: FiberCommitmentEvidence | null;
  openingPartyFlows: {
    lockScriptHash: string;
    openingInputCapacity: string;
    openingOutputCapacity: string;
    observedNetInput: string;
  }[];
  stages: {
    stage: string;
    txHash: string;
    blockNumber: number;
    timestamp: string;
    inputs: number;
    outputs: number;
    witnessCount: number;
  }[];
  settlementOutputs: {
    txHash: string;
    outputIndex: number;
    ownerLockHash: string;
    observedParty: boolean;
    capacity: string;
  }[];
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
  LiveCellSummary,
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
  TransactionCellDisplay,
  TransactionScriptGroup,
  TransactionWitness,
  PoolTransaction,
  PoolTransactionsResponse,
  Block,
  BlockUncle,
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
  DaoEvidence,
  DaoTopDepositor,
  DaoTopDepositorsResponse,
  DaoCalculatorResult,
  SporeCluster,
  SporeNft,
  SporeDetail,
  ObjectCollection,
  CollectionItem,
  CollectionHolder,
  CollectionActivity,
  ItemStatusFilter,
  DotCellPartyRef,
  DotCellDecodedAddress,
  DotCellRecord,
  DotCellNameWitness,
  DotCellNameRef,
  DotCellOutPoint,
  DotCellSale,
  DotCellState,
  DotCellItem,
  DotCellRing,
  AddressDotCellName,
  MnftClassSummary,
  MnftIssuerSummary,
  MnftLifecycleEvent,
  MnftItemDetail,
  MnftItemActivity,
  ChartDataPoint,
  ChartResponse,
  CkbytesAllocation,
  ExactJson,
  UncleRateChartResponse,
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
  PoolSummary,
  PoolStatus,
  AddressPendingSummary,
  PoolInterpretation,
  MempoolInfo,
  MempoolTransaction,
  MempoolBlock,
  MempoolBlocksResponse,
  FeeRateRange,
  PendingProposal,
  PendingProposalsResponse,
  BlockFeeStats,
  BlockProposal,
  PipelineStatus,
  PipelineConsensus,
  PipelineBlockRef,
  PipelineProposalState,
  PipelineProposal,
  PipelineProposedIn,
  PipelineCyclesStatus,
  PipelineTx,
  PipelineCellbaseReward,
  PipelineCellbasePays,
  PipelineCellbase,
  PipelineBlock,
  PipelinePoolTx,
  PipelinePool,
  PipelineNextTx,
  PipelineNext,
  PipelineSnapshot,
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
  CanonicalBranchBlock,
  ReorgDetail,
  ForkOverview,
  RecentReorgResponse,
  HardforkResource,
  HardforkEvent,
  HardforkTimelineResponse,
  HardforkActivation,
  Activity,
  IdentityStandard,
  ItemDelta,
  ActivityTypeCall,
  ActivityLockCall,
  ActivityProtocolAction,
  ParticipantInfo,
  ParticipantRef,
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
  FiberNetwork,
  FiberPair,
  FiberParty,
  FiberChannelEvidence,
  FiberEpochDelay,
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
  startedAt: number;
  finishedAt: number;
  candidatePeers: number;
  verifiedRetainedPeers: number;
  reachablePeers: number;
  verifiedUnavailablePeers: number;
  exhaustedCandidates: number;
  foreignPeers: number;
  addressAttempts: number;
  nonSuccessfulAddressAttempts: number;
  malformedAddresses: number;
  newVerifiedPeers: number;
  peerOutcomes: NetworkPeerOutcomes;
  addressObservations: NetworkAddressObservations;
  discovery: NetworkDiscoveryEvidence;
  localObserver: NetworkLocalObserverEvidence | null;
  directSessionObservations: NetworkDirectSessionObservationSummary;
}

export interface NetworkLocalObserverProtocol {
  id: number;
  name: string;
  supportVersions: string[];
}

export interface NetworkLocalObserverEvidence {
  peerId: string;
  firstObservedAt: number;
  lastObservedAt: number;
  firstObservedRound: number;
  lastObservedRound: number;
  observationCount: number;
  clientVersion: string;
  active: boolean;
  addresses: string[];
  protocols: NetworkLocalObserverProtocol[];
  connections: number;
}

export interface NetworkDirectSessionObservationSummary {
  observerInitiated: number;
  peerInitiated: number;
}

export interface NetworkRetentionSplit {
  withRetainedVerification: number;
  withoutRetainedVerification: number;
}

export interface NetworkPeerOutcomes {
  sameNetworkIdentified: number;
  exhausted: NetworkRetentionSplit;
  foreignNetwork: NetworkRetentionSplit;
}

export interface NetworkAddressObservations {
  dialRequestFailed: number;
  noAuthenticatedSessionBeforeDeadline: number;
  authenticatedSessionWithoutIdentifyBeforeDeadline: number;
  malformedIdentify: number;
  foreignNetwork: number;
  sameNetworkIdentified: number;
}

export interface NetworkDiscoveryEvidence {
  validNodesMessages: number;
  validResponseMessages: number;
  validAnnounceMessages: number;
  malformedMessages: number;
  unexpectedMessages: number;
  normalizedAdvertisedAddresses: number;
  rejectedAdvertisedAddresses: number;
}

export interface NetworkActiveRound {
  roundId: number;
  startedAt: number;
  lastCheckpointAt: number;
  candidatePeers: number;
  completedPeers: number;
  addressAttempts: number;
  blockedReason: string | null;
}

export interface NetworkSummary {
  enabled: boolean;
  hasData: boolean;
  lastRound: NetworkLastRound | null;
  activeRound: NetworkActiveRound | null;
}

export interface NetworkDistributions {
  verifiedRetained: number;
  sameNetworkReachable: number;
  verifiedUnavailable: number;
  versions: LabelCount[];
  countries: LabelCount[];
  asns: LabelCount[];
  protocols: LabelCount[];
}

export interface NetworkMap {
  roundId: number | null;
  verifiedRetained: number;
  located: number;
  locationUnavailable: number;
  points: Array<{
    peerId: string;
    country: string;
    lat: number;
    lon: number;
    reachable: boolean;
    clientVersion: string;
    asn: string | null;
    firstSeen: number;
    lastSeen: number;
    advertiserCount: number;
    localSessionObserved: boolean;
  }>;
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
  dailyCoverage: Array<{
    ts: number;
    hasRetainedObservation: boolean;
    roundsPerDay: number | null;
  }>;
  coverageFrom: number | null;
  coverageTo: number | null;
  roundCountAvailability: 'notRecorded';
}

export interface NetworkRoundAdvertisements {
  roundId: number | null;
  finishedAt: number | null;
  coverage: 'noCompletedRound' | 'retainedLatestObservationOnly';
  distinctPeerPairs: number;
  edges: Array<{
    advertiserPeerId: string;
    targetPeerId: string;
    alias: string;
    observedAt: number;
  }>;
}

export type NetworkPeerDisplayState =
  | 'reachable'
  | 'verifiedUnavailable'
  | 'advertisedUnverified'
  | 'foreignNetwork'
  | 'noCompletedObservation';

export type NetworkAddressProbeResult =
  | 'dialRequestFailed'
  | 'noAuthenticatedSessionBeforeDeadline'
  | 'authenticatedSessionWithoutIdentifyBeforeDeadline'
  | 'malformedIdentify'
  | 'foreignNetwork'
  | 'sameNetworkIdentified';

export interface NetworkPeerSummary {
  peerId: string;
  crawlerDialState: NetworkPeerDisplayState;
  participation: NetworkParticipationEvidence;
  sessionInitiators: NetworkSessionInitiator[];
  primaryAddr: string | null;
  version: string | null;
  country: string | null;
  asn: string | null;
  lastAdvertisedAt: number | null;
  lastDialObservedAt: number | null;
  latestPositiveObservedAt: number;
  lastReachableAt: number | null;
  rttMs: number | null;
}

export interface NetworkParticipationEvidence {
  discoveryAdvertised: boolean;
  directSessionObserved: boolean;
  crawlerIdentified: boolean;
}

export type NetworkSessionInitiator = 'observerInitiated' | 'peerInitiated';

export interface NetworkPeersPage {
  items: NetworkPeerSummary[];
  nextCursor: string | null;
}

export interface NetworkAddressProbeEvidence {
  address: string;
  roundId: number;
  observedAt: number;
  elapsedMs: number;
  result: NetworkAddressProbeResult;
}

export interface NetworkCompletedPeerEvidence {
  roundId: number;
  outcome: 'sameNetworkIdentified' | 'exhausted' | 'foreignNetwork';
  observations: NetworkAddressProbeEvidence[];
  consecutiveExhaustedRounds: number;
}

export interface NetworkActivePeerEvidence {
  roundId: number;
  state: 'pending' | 'retryAlias' | 'succeeded' | 'exhausted' | 'foreignNetwork';
  observations: NetworkAddressProbeEvidence[];
}

export interface NetworkPeerAliasEvidence {
  address: string;
  firstAdvertisedAt: number;
  lastAdvertisedAt: number;
  lastVerifiedAt: number | null;
}

export interface NetworkVerifiedPeerEvidence {
  ownAddrs: string[];
  clientVersion: string;
  flags: number;
  protocols: string[];
  firstSeen: number;
  lastSeen: number;
  lastReachableAt: number;
  country: string;
  asn: string;
  rttMs: number | null;
  discovery: NetworkDiscoveryEvidence;
}

export interface NetworkAdvertiserEvidence {
  advertiserPeerId: string;
  alias: string;
  firstObservedAt: number;
  lastObservedAt: number;
  firstObservedRound: number;
  lastObservedRound: number;
  observationCount: number;
}

export interface NetworkDirectSessionProtocol {
  id: number;
  version: string;
}

export interface NetworkDirectSessionEvidence {
  observerPeerId: string;
  initiator: NetworkSessionInitiator;
  firstObservedAt: number;
  lastObservedAt: number;
  firstObservedRound: number;
  lastObservedRound: number;
  observationCount: number;
  clientVersion: string;
  sessionAddresses: string[];
  connectedDurationMs: number;
  lastPingDurationMs: number | null;
  protocols: NetworkDirectSessionProtocol[];
}

export interface NetworkPeerDetail {
  peerId: string;
  observationVantage: string;
  crawlerDialState: NetworkPeerDisplayState;
  participation: NetworkParticipationEvidence;
  sessionInitiators: NetworkSessionInitiator[];
  firstDiscoveredAt: number | null;
  lastAdvertisedAt: number | null;
  latestPositiveObservedAt: number;
  aliases: NetworkPeerAliasEvidence[];
  lastCompleted: NetworkCompletedPeerEvidence | null;
  active: NetworkActivePeerEvidence | null;
  verified: NetworkVerifiedPeerEvidence | null;
  advertisers: NetworkAdvertiserEvidence[];
  directSessions: NetworkDirectSessionEvidence[];
}

function itemReadPath(kind: ItemKind, id: string): string {
  const value = encodeURIComponent(id);
  return kind === 'spore'
    ? `/spore/objects/${value}`
    : kind === 'mnft'
      ? `/assets/objects/items/${value}`
      : `/assets/identities/${kind}/items/${value}`;
}
function collectionReadPath(kind: 'spore' | 'object' | 'identity', id: string): string {
  const value = encodeURIComponent(id);
  return kind === 'spore'
    ? `/spore/clusters/${value}`
    : `/assets/${kind === 'object' ? 'objects' : 'identities'}/${value}`;
}
function cursorReadQuery(params: CursorQueryParams): URLSearchParams {
  const query = new URLSearchParams();
  if (params.limit !== undefined) query.set('limit', String(params.limit));
  if (params.cursor !== undefined) query.set('cursor', params.cursor);
  return query;
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

  getForkOverview: (days = 30): Promise<ForkOverview> => {
    return fetchApi(`/forks/overview?days=${days}`);
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

  getVesselSnapshot: (tracked: string[] = []): Promise<VesselSnapshot> => {
    const query = new URLSearchParams();
    if (tracked.length) query.set('tracked', tracked.join(','));
    return fetchApi(`/vessels/snapshot${tracked.length ? `?${query}` : ''}`);
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

  getBlockUncle: (id: string | number, index: number): Promise<BlockUncle> => {
    return fetchApi(`/blocks/${id}/uncles/${index}`);
  },

  getTransactions: (
    params: CursorQueryParams & {
      blockNumber?: number;
      activityType?: TransactionActivityType;
    } = {}
  ): Promise<CursorPaginatedResponse<Transaction>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.activityType) query.set('activity_type', params.activityType);
    if (params.blockNumber !== undefined) query.set('block_number', String(params.blockNumber));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/transactions?${query}`);
  },

  getPoolTransactions: (
    params: CursorQueryParams & {
      status?: PoolStatus | 'all';
      activityType?: TransactionActivityType;
    } = {}
  ): Promise<PoolTransactionsResponse> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.activityType) query.set('activity_type', params.activityType);
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.status) query.set('status', params.status);
    return fetchApi(`/transactions/pool?${query}`);
  },

  getTransactionActivity: (hash: string): Promise<GlobalActivity | null> =>
    fetchApi(`/transactions/${hash}/activity`),

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

  getAddressHoldings: (addr: string): Promise<AddressHoldings> => {
    return fetchApi(`/addresses/${encodeURIComponent(addr)}/holdings`);
  },

  getAddressCells: (
    addr: string,
    params: CursorQueryParams & { class?: AddressCellClass } = {}
  ): Promise<CursorPaginatedResponse<AddressCell>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.class) query.set('class', params.class);
    return fetchApi(`/addresses/${encodeURIComponent(addr)}/cells?${query}`);
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

  /** The `.cell` names an address owns. `addr` may be an address or lock hash. */
  getAddressDotCellNames: (
    addr: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<AddressDotCellName>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    const suffix = query.toString();
    return fetchApi(
      `/addresses/${encodeURIComponent(addr)}/dotcell-names${suffix ? `?${suffix}` : ''}`
    );
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
    params: CursorQueryParams & { filter?: GlobalActivityFilter; blockNumber?: number } = {}
  ): Promise<CursorPaginatedResponse<GlobalActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.blockNumber !== undefined) query.set('blockNumber', String(params.blockNumber));
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

  getLiveCellSummary: (): Promise<LiveCellSummary> => {
    return fetchApi('/cells/live-summary');
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

  getCkbytesAllocation: (): Promise<CkbytesAllocation> => {
    return fetchApi('/statistics/ckbytes-allocation');
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

  getInventorySummary: (): Promise<InventorySummary> => fetchApi('/assets/summary'),
  getItemCellVersions: (kind: ItemKind, id: string): Promise<ItemCellVersions> =>
    fetchApi(`${itemReadPath(kind, id)}/cell-versions`),
  getItemProtocol: (kind: ItemKind, id: string): Promise<ItemProtocol> =>
    fetchApi(`${itemReadPath(kind, id)}/protocol`),
  getItemActivityFeed: (
    kind: ItemKind,
    id: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<ItemActivity>> =>
    fetchApi(`${itemReadPath(kind, id)}/activity-feed?${cursorReadQuery(params)}`),
  getItemTransactions: (
    kind: ItemKind,
    id: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<ItemTransaction>> =>
    fetchApi(`${itemReadPath(kind, id)}/transactions?${cursorReadQuery(params)}`),
  getCollectionActivityFeed: (
    kind: 'spore' | 'object' | 'identity',
    id: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<GlobalActivity>> =>
    fetchApi(`${collectionReadPath(kind, id)}/activity-feed?${cursorReadQuery(params)}`),
  getCollectionTransactions: (
    kind: 'spore' | 'object' | 'identity',
    id: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<Transaction>> =>
    fetchApi(`${collectionReadPath(kind, id)}/transactions?${cursorReadQuery(params)}`),
  getObjectGenetics: (id: string): Promise<ObjectGenetics> =>
    fetchApi(`/spore/objects/${encodeURIComponent(id)}/genetics`),
  getCollectionInsights: (
    id: string,
    kind: 'spore' | 'object' = 'spore'
  ): Promise<CollectionInsights> =>
    kind === 'object'
      ? fetchApi(`/assets/objects/${encodeURIComponent(id)}/insights`)
      : fetchApi(`/spore/clusters/${encodeURIComponent(id)}/insights`),
  getCollectionGallery: (
    id: string,
    params: CursorQueryParams & { traits?: Record<string, string> } = {}
  ): Promise<CursorPaginatedResponse<GalleryObject>> => {
    const query = cursorReadQuery(params);
    if (params.traits && Object.keys(params.traits).length)
      query.set('traits', JSON.stringify(params.traits));
    return fetchApi(`/spore/clusters/${encodeURIComponent(id)}/gallery?${query}`);
  },
  getDidPdsDistribution: (): Promise<DidPdsDistribution> => fetchApi('/assets/identities/did/pds'),
  getTokenHistory: (typeHash: string): Promise<TokenHistory> =>
    fetchApi(`/tokens/${typeHash}/history`),
  getTokenDistribution: (typeHash: string): Promise<TokenDistribution> =>
    fetchApi(`/tokens/${typeHash}/distribution`),
  getTokenActivityFeed: (
    typeHash: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<GlobalActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/tokens/${typeHash}/activity-feed?${query}`);
  },
  getTokenTransactions: (
    typeHash: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<Transaction>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/tokens/${typeHash}/transactions?${query}`);
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
  ): Promise<TokenHoldersResponse> => {
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

  getDaoEvidence: (): Promise<DaoEvidence> => {
    return fetchApi('/dao/evidence');
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

  getSporeClusterProtocol: (clusterId: string): Promise<ClusterProtocol> =>
    fetchApi(`/spore/clusters/${clusterId}/protocol`),

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
  ): Promise<AssetCapacityHistory> => {
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

  getSporeObject: (sporeId: string): Promise<SporeDetail> => {
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
  ): Promise<AssetCapacityHistory> => {
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
  ): Promise<AssetCapacityHistory> => {
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

  getBitCellItemDetail: (nftId: string): Promise<CollectionItem> => {
    return fetchApi(`/assets/identities/bit-cell/items/${encodeURIComponent(nftId)}`);
  },

  getBitCellItemActivities: (
    nftId: string,
    params: MnftItemActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<MnftItemActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(
      `/assets/identities/bit-cell/items/${encodeURIComponent(nftId)}/activities${suffix ? `?${suffix}` : ''}`
    );
  },

  /** `idOrName` accepts a 20-byte `0x…` name id, `alice`, or `alice.cell`. */
  getDotCellItemDetail: (idOrName: string): Promise<DotCellItem> => {
    return fetchApi(`/assets/identities/dotcell/items/${encodeURIComponent(idOrName)}`);
  },

  /** A page of a `.cell` name's live sub-names, after `cursor`. */
  getDotCellItemChildren: (
    idOrName: string,
    params: CursorQueryParams = {}
  ): Promise<CursorPaginatedResponse<DotCellNameRef>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    const suffix = query.toString();
    return fetchApi(
      `/assets/identities/dotcell/items/${encodeURIComponent(idOrName)}/children${suffix ? `?${suffix}` : ''}`
    );
  },

  getDotCellItemActivities: (
    idOrName: string,
    params: MnftItemActivitiesParams = {}
  ): Promise<CursorPaginatedResponse<MnftItemActivity>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.action) query.set('action', params.action);
    const suffix = query.toString();
    return fetchApi(
      `/assets/identities/dotcell/items/${encodeURIComponent(idOrName)}/activities${suffix ? `?${suffix}` : ''}`
    );
  },

  getDotCellRing: (): Promise<DotCellRing> => {
    return fetchApi('/assets/identities/dotcell/ring');
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

  getDaoEverDepositorsChart: (): Promise<ChartResponse> => {
    return fetchApi('/dao/charts/ever-depositors');
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

  getUncleRateChart: (): Promise<UncleRateChartResponse> => {
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

  getPipeline: (): Promise<PipelineSnapshot> => {
    return fetchApi('/pipeline');
  },

  getScripts: (params: ScriptQueryParams = {}): Promise<ScriptsListApi> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.network) query.set('network', params.network);
    if (params.decoderType) query.set('decoder_type', params.decoderType);
    if (params.search) query.set('search', params.search);
    if (params.role) query.set('role', params.role);
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
    return fetchApi<
      CursorPaginatedResponse<ScriptFamilyListItemApi> & { classSummary: ScriptCodeClassTotalApi[] }
    >(`/scripts?${query}`).then((response) => ({
      ...response,
      data: response.data.map(scriptFamilyListItemToKnownScript),
    }));
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
        value: [
          s.transferCount,
          s.daoDepositCount,
          s.daoWithdrawRequestCount,
          s.daoWithdrawCompleteCount,
          s.tokenCount,
          s.objectCount,
          s.identityCount,
          s.scriptCallCount,
        ]
          .reduce((sum, count) => sum + BigInt(count), BigInt(0))
          .toString(),
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
          dao: (
            BigInt(s.daoDepositCount) +
            BigInt(s.daoWithdrawRequestCount) +
            BigInt(s.daoWithdrawCompleteCount)
          ).toString(),
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

  getFiberChannelEvidence: (channelId: string): Promise<FiberChannelEvidence> =>
    fetchApi(`/fiber/channels/${channelId}/evidence`),

  getFiberNetwork: (): Promise<FiberNetwork> => fetchApi('/fiber/network'),

  getAddressFiberChannels: (
    addr: string,
    params: { limit?: number } = {}
  ): Promise<FiberChannel[]> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
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
        cooperativelyClosedChannels: stats.cooperativelyClosedChannels,
        forceClosedWaitingChannels: stats.forceClosedWaitingChannels,
        forceClosedSettledChannels: stats.forceClosedSettledChannels,
        observedParties: stats.observedParties,
        observedPairs: stats.observedPairs,
        firstOpenTimestamp: stats.firstOpenTimestamp,
        typicalClosedLifetimeMs: stats.typicalClosedLifetimeMs,
        closedWithinHour: stats.closedWithinHour,
        tipBlockNumber: stats.tipBlockNumber,
        tipChainTimeMs: stats.tipChainTimeMs,
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

  getNetworkAdvertisements: async (): Promise<NetworkRoundAdvertisements> =>
    fetchApi('/network/advertisements'),

  getNetworkDistributions: async (): Promise<NetworkDistributions> =>
    fetchApi('/network/distributions'),

  getNetworkMap: async (): Promise<NetworkMap> => fetchApi('/network/map'),

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

  getNetworkPeers: async (params?: {
    cursor?: string;
    limit?: number;
    state?: NetworkPeerDisplayState;
    observation?: NetworkAddressProbeResult;
    country?: string;
    version?: string;
  }): Promise<NetworkPeersPage> => {
    const p = new URLSearchParams();
    if (params?.cursor) p.set('cursor', params.cursor);
    if (params?.limit != null) p.set('limit', String(params.limit));
    if (params?.state) p.set('state', params.state);
    if (params?.observation) p.set('observation', params.observation);
    if (params?.country) p.set('country', params.country);
    if (params?.version) p.set('version', params.version);
    const qs = p.toString();
    return fetchApi(`/network/peers${qs ? `?${qs}` : ''}`);
  },

  getNetworkPeer: async (peerId: string): Promise<NetworkPeerDetail> =>
    fetchApi(`/network/peers/${encodeURIComponent(peerId)}`),
};
