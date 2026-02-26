import { normalizeNftAssetId } from '@/lib/nft-collections';
import type { ScriptRefHashType } from '@/lib/script-ref';

const API_BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001/api/v1';

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
  isCellbase: boolean;
  timestamp: string;
}

interface SyncStatus {
  isSyncing: boolean;
  syncedBlock: number;
  tipBlock: number;
  progress: number;
  estimatedTime: string | null;
  chartDataMayBeIncomplete: boolean;
  blocksPerSecond: number | null;
  emaBlocksPerSecond: number | null;
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

type NftItemStatusFilter = 'all' | 'live' | 'recycled';

interface NftCollectionItemsParams extends CursorQueryParams {
  search?: string;
  status?: NftItemStatusFilter;
}

interface MnftItemActivitiesParams extends CursorQueryParams {
  action?: 'mint' | 'transfer' | 'burn';
}

interface Script {
  codeHash: string;
  hashType: string;
  args: string;
}

interface TransactionDetail extends Transaction {
  feeRate?: string;
  txSize?: number;
  cycles?: number;
  confirmations: number;
  inputsCapacity: string;
  outputsCapacity: string;
  inputsOccupiedCapacity: string;
  outputsOccupiedCapacity: string;
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
    occupiedCapacity: number;
    virtualOccupiedCapacity?: string;
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

interface OccupiedCapacityBreakdown {
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
  occupiedCapacity?: number;
  occupiedCapacityBreakdown?: OccupiedCapacityBreakdown;
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
  virtualOccupiedCapacity?: string;
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
  occupiedCapacity: string;
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
  assetCategory: 'token' | 'dob' | 'nft' | 'dao';
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
  category?: 'token' | 'dob' | 'nft' | 'dao';
}

type ActivityAssetChange =
  | { type: 'token'; typeScriptHash: string; delta: string; symbol?: string; decimals?: number }
  | { type: 'dob'; dobId: string; standard: string; action: string }
  | { type: 'nft'; nftId: string; standard: string; action: string }
  | { type: 'daoDeposit'; capacity: string }
  | { type: 'daoWithdrawRequest'; capacity: string; depositBlock: number }
  | { type: 'daoWithdrawComplete'; capacity: string; compensation: string };

interface Activity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  ckbDelta: string;
  occupiedDelta: string;
  isCellbase: boolean;
  assetChanges: ActivityAssetChange[];
  peers: string[];
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
  totalOccupiedCapacity: string | null;
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
  assetType: 'token' | 'nft';
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
  liveCapacity: string | null;
  liveOccupiedCapacity: string | null;
}

interface AssetQueryParams {
  limit?: number;
  type?: 'token' | 'nft';
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
    | 'occupied'
    | 'capacity';
  sortDirection?: 'asc' | 'desc';
}

interface TokenHolderParams {
  limit?: number;
  cursor?: string;
}

interface TokenTransferParams {
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
  createdAtBlock: number;
  liveCapacity?: string | null;
  liveOccupiedCapacity?: string | null;
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
  liveCapacity?: string | null;
  liveOccupiedCapacity?: string | null;
}

interface DobTrait {
  name: string;
  value: string;
}

interface SporeDobDecoded {
  sporeId: string;
  contentType: string;
  dnaHex: string | null;
  traits: DobTrait[];
  svgMarkup: string | null;
  issues: string[];
}

interface NftCollection {
  collectionId: string;
  standard: string;
  name: string | null;
  totalCount: number;
  liveCount: number;
  liveCapacity: string;
  liveOccupiedCapacity: string;
}

interface NftCollectionItem {
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
  occupiedShare: StackedAreaChartResponse;
  capacityShare: StackedAreaChartResponse;
}

interface MostUtilizedAssetsChartResponse {
  title: string;
  occupiedShare: StackedAreaChartResponse;
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
  liveCapacitySum?: string;
  liveOccupiedCapacitySum?: string;
}

interface DeploymentUsage {
  codeHash: string;
  scriptKind: string | null;
  cellsCount: number;
  liveCellsCount: number;
  capacitySum: string;
  liveCapacitySum: string;
  occupiedCapacitySum: string;
  liveOccupiedCapacitySum: string;
}

interface ScriptUsage {
  name: string;
  cellsCount: number;
  liveCellsCount: number;
  capacitySum: string;
  liveCapacitySum: string;
  occupiedCapacitySum: string;
  liveOccupiedCapacitySum: string;
  byDeployment: DeploymentUsage[];
}

interface ScriptQueryParams {
  limit?: number;
  cursor?: string;
  network?: string;
  decoderType?: string;
  search?: string;
  sortKey?: 'name' | 'kind' | 'description' | 'occupied' | 'capacity' | 'occupiedRatio';
  sortDirection?: 'asc' | 'desc';
}

interface OccupationChartRangeParams {
  from?: string;
  to?: string;
}

interface ScriptLookupInfo {
  codeHash: string;
  name: string;
  scriptKind: string | null;
  decoderType: string | null;
  hashType: string | null;
  deploymentTypeHash?: string | null;
  deploymentDataHash?: string | null;
  codeCellTxHash: string | null;
  codeCellOutputIndex: number | null;
  liveCellsCount: number;
  liveCapacitySum: string;
  liveOccupiedCapacitySum: string;
}

type ScriptLookupResponse = Record<string, ScriptLookupInfo>;

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
  const res = await fetch(`${API_BASE}${endpoint}`);
  if (!res.ok) {
    let detail = '';
    try {
      const payload = (await res.json()) as { message?: unknown };
      if (typeof payload?.message === 'string' && payload.message.trim().length > 0) {
        detail = payload.message.trim();
      }
    } catch {
      // Ignore parse failures and keep the status-only error.
    }
    throw new Error(detail ? `API error: ${res.status} - ${detail}` : `API error: ${res.status}`);
  }
  return res.json();
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
  OccupiedCapacityBreakdown,
  CellDaoInfo,
  CellDep,
  CodeCellScript,
  Transaction,
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
  DaoDeposit,
  AddressDaoSummary,
  DaoStatistics,
  DaoCalculatorResult,
  SporeCluster,
  SporeNft,
  NftCollection,
  NftCollectionItem,
  NftItemStatusFilter,
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
  ActivityAssetChange,
};

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

  getAddressStatsHistory: (addr: string): Promise<StackedAreaChartResponse> => {
    return fetchApi(`/addresses/${addr}/stats-history`);
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
      query.set('sort_key', params.sortKey === 'transfers24h' ? 'transfers_24h' : params.sortKey);
    }
    if (params.sortDirection) query.set('sort_direction', params.sortDirection);
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

  getTokenOccupationChart: (
    typeHash: string,
    range: OccupationChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(`/tokens/${typeHash}/charts/occupation${suffix ? `?${suffix}` : ''}`);
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

  getSporeClusterOccupationChart: (
    clusterId: string,
    range: OccupationChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(`/spore/clusters/${clusterId}/charts/occupation${suffix ? `?${suffix}` : ''}`);
  },

  getSporeNfts: (params: CursorQueryParams = {}): Promise<CursorPaginatedResponse<SporeNft>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/spore/nfts?${query}`);
  },

  getSporeNft: (sporeId: string): Promise<SporeNft> => {
    return fetchApi(`/spore/nfts/${sporeId}`);
  },

  getSporeNftDecoded: (sporeId: string): Promise<SporeDobDecoded> => {
    return fetchApi(`/spore/nfts/${sporeId}/decode`);
  },

  getSporeNftOccupationChart: (
    sporeId: string,
    range: OccupationChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(`/spore/nfts/${sporeId}/charts/occupation${suffix ? `?${suffix}` : ''}`);
  },

  getNftCollection: (collectionId: string): Promise<NftCollection> => {
    return fetchApi(`/assets/nfts/${normalizeNftAssetId(collectionId)}`);
  },

  getNftCollectionOccupationChart: (
    collectionId: string,
    range: OccupationChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(
      `/assets/nfts/${normalizeNftAssetId(collectionId)}/charts/occupation${suffix ? `?${suffix}` : ''}`
    );
  },

  getNftCollectionItems: (
    collectionId: string,
    params: NftCollectionItemsParams = {}
  ): Promise<CursorPaginatedResponse<NftCollectionItem>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.search) query.set('search', params.search);
    if (params.status) query.set('status', params.status);
    const suffix = query.toString();
    return fetchApi(
      `/assets/nfts/${normalizeNftAssetId(collectionId)}/items${suffix ? `?${suffix}` : ''}`
    );
  },

  getDotbitItemDetail: (nftId: string): Promise<NftCollectionItem> => {
    return fetchApi(`/assets/nfts/dotbit/items/${encodeURIComponent(nftId)}`);
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
      `/assets/nfts/dotbit/items/${encodeURIComponent(nftId)}/activities${suffix ? `?${suffix}` : ''}`
    );
  },

  getMnftItemDetail: (nftId: string): Promise<MnftItemDetail> => {
    return fetchApi(`/assets/nfts/items/${encodeURIComponent(nftId)}`);
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
      `/assets/nfts/items/${encodeURIComponent(nftId)}/activities${suffix ? `?${suffix}` : ''}`
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

  getCellAgeVsOccupiedCapacityChart: (): Promise<StackedAreaChartResponse> => {
    return fetchApi('/charts/cell-age-vs-occupied-capacity');
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
    return fetchApi('/mempool/transactions');
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
      query.set('sort_key', params.sortKey === 'occupiedRatio' ? 'occupied_ratio' : params.sortKey);
    }
    if (params.sortDirection) query.set('sort_direction', params.sortDirection);
    return fetchApi(`/scripts?${query}`);
  },

  getScript: (name: string): Promise<KnownScript[]> => {
    return fetchApi(`/scripts/${encodeURIComponent(name)}`);
  },

  getScriptUsage: (name: string): Promise<ScriptUsage> => {
    return fetchApi(`/scripts/${encodeURIComponent(name)}/usage`);
  },

  getScriptOccupationChart: (
    name: string,
    range: OccupationChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    const suffix = query.toString();
    return fetchApi(
      `/scripts/${encodeURIComponent(name)}/charts/occupation${suffix ? `?${suffix}` : ''}`
    );
  },

  getScriptOccupationChartByCodeHash: (
    codeHash: string,
    scriptKind?: 'lock' | 'type' | 'both',
    range: OccupationChartRangeParams = {}
  ): Promise<StackedAreaChartResponse> => {
    const query = new URLSearchParams();
    query.set('code_hash', codeHash);
    if (scriptKind && scriptKind !== 'both') query.set('script_kind', scriptKind);
    if (range.from) query.set('from', range.from);
    if (range.to) query.set('to', range.to);
    return fetchApi(`/scripts/charts/occupation?${query}`);
  },

  lookupScripts: async (codeHashes: string[]): Promise<ScriptLookupResponse> => {
    if (codeHashes.length === 0) return {};
    const res = await fetch(`${API_BASE}/scripts/lookup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ codeHashes }),
    });
    if (!res.ok) {
      throw new Error(`API error: ${res.status}`);
    }
    return res.json();
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

  getCyclesStatus: (hash: string): Promise<CyclesStatusResponse> => {
    return fetchApi(`/transactions/${hash}/cycles`);
  },

  triggerCyclesCalculation: async (hash: string): Promise<CyclesStatusResponse> => {
    const res = await fetch(`${API_BASE}/transactions/${hash}/calculate-cycles`, {
      method: 'POST',
    });
    if (!res.ok) {
      throw new Error(`API error: ${res.status}`);
    }
    return res.json();
  },
};
