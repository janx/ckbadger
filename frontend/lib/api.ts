import type { Activity } from '@/types/activity';

const API_BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001/api/v1';

interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  limit: number;
}

interface CursorPaginatedResponse<T> {
  data: T[];
  total: number;
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
  compactTarget: string;
  version: number;
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

interface CursorQueryParams {
  limit?: number;
  cursor?: string;
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
}

interface DepGroupItem {
  txHash: string;
  outputIndex: number;
}

interface CodeCellScript {
  name: string;
  codeHash: string;
  hashType: string;
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

interface Cell {
  txHash: string;
  outputIndex: number;
  capacity: string;
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
}

interface SearchResponse {
  results: SearchResult[];
  query: string;
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
  holdersCount: number;
  transfersCount: number;
  transfers24h: number;
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
  assetType: 'token' | 'nft' | 'dob';
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
}

interface AssetQueryParams {
  limit?: number;
  type?: 'token' | 'nft' | 'dob';
  cursor?: string;
  search?: string;
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
  withdrawBlock: number | null;
  withdrawTimestamp: string | null;
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
}

interface DeploymentUsage {
  codeHash: string;
  scriptKind: string | null;
  cellsCount: number;
  liveCellsCount: number;
  capacitySum: string;
  liveCapacitySum: string;
}

interface ScriptUsage {
  name: string;
  cellsCount: number;
  liveCellsCount: number;
  capacitySum: string;
  liveCapacitySum: string;
  byDeployment: DeploymentUsage[];
}

interface ScriptQueryParams {
  limit?: number;
  cursor?: string;
  network?: string;
  decoderType?: string;
  search?: string;
}

interface ScriptLookupInfo {
  codeHash: string;
  name: string;
  scriptKind: string | null;
  decoderType: string | null;
  hashType: string | null;
  codeCellTxHash: string | null;
  codeCellOutputIndex: number | null;
  liveCellsCount: number;
  liveCapacitySum: string;
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

interface IndexRebuildStatus {
  status: 'pending' | 'running';
  isRebuilding: boolean;
  total: number;
  completed: number;
  currentIndex: string | null;
  failed: string[];
  progress: number;
  startedAt: string | null;
}

interface ActiveTasksResponse {
  indexRebuild: IndexRebuildStatus | null;
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

interface RecommendedFees {
  fastestFee: number;
  halfHourFee: number;
  hourFee: number;
  economyFee: number;
  minimumFee: number;
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
    throw new Error(`API error: ${res.status}`);
  }
  return res.json();
}

export type {
  GraphNode,
  GraphLink,
  GraphResponse,
  Cell,
  CellDaoInfo,
  CellDep,
  CodeCellScript,
  Transaction,
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
  CursorPaginatedResponse,
  PaginatedResponse,
  MempoolInfo,
  MempoolTransaction,
  MempoolBlock,
  MempoolBlocksResponse,
  RecommendedFees,
  FeeRateRange,
  BlockFeeStats,
  BlockProposal,
  ActiveTasksResponse,
  IndexRebuildStatus,
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
};

export type {
  Activity,
  ActivityType,
  ActivityCategory,
  ActivitiesResponse,
  ActivityQueryParams,
  AddressActivityQueryParams,
} from '@/types/activity';

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

  getTransactionAssetTransfers: (hash: string): Promise<AssetTransfer[]> => {
    return fetchApi(`/transactions/${hash}/asset-transfers`);
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

  getAddressAssetTransfers: (
    addr: string,
    params: AssetTransferParams = {}
  ): Promise<CursorPaginatedResponse<AssetTransfer>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.category) query.set('category', params.category);
    return fetchApi(`/addresses/${addr}/asset-transfers?${query}`);
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
    hashType: string;
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
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.search) query.set('search', params.search);
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

  getSporeNfts: (params: CursorQueryParams = {}): Promise<CursorPaginatedResponse<SporeNft>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    return fetchApi(`/spore/nfts?${query}`);
  },

  getSporeNft: (sporeId: string): Promise<SporeNft> => {
    return fetchApi(`/spore/nfts/${sporeId}`);
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

  getCellCountChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/cell-count');
  },

  getKnowledgeSizeChart: (): Promise<ChartResponse> => {
    return fetchApi('/charts/knowledge-size');
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

  getMempoolInfo: (): Promise<MempoolInfo> => {
    return fetchApi('/mempool/info');
  },

  getMempoolTransactions: (): Promise<MempoolTransaction[]> => {
    return fetchApi('/mempool/transactions');
  },

  getMempoolBlocks: (): Promise<MempoolBlocksResponse> => {
    return fetchApi('/mempool/blocks');
  },

  getRecommendedFees: (): Promise<RecommendedFees> => {
    return fetchApi('/mempool/fees');
  },

  getActiveTasks: (): Promise<ActiveTasksResponse> => {
    return fetchApi('/tasks/active');
  },

  getScripts: (params: ScriptQueryParams = {}): Promise<CursorPaginatedResponse<KnownScript>> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.network) query.set('network', params.network);
    if (params.decoderType) query.set('decoder_type', params.decoderType);
    if (params.search) query.set('search', params.search);
    return fetchApi(`/scripts?${query}`);
  },

  getScript: (name: string): Promise<KnownScript[]> => {
    return fetchApi(`/scripts/${encodeURIComponent(name)}`);
  },

  getScriptUsage: (name: string): Promise<ScriptUsage> => {
    return fetchApi(`/scripts/${encodeURIComponent(name)}/usage`);
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
    hashType: string
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

  getActivities: (
    params: {
      limit?: number;
      cursor?: string;
      activityType?: string;
      activityCategory?: string;
    } = {}
  ): Promise<{
    activities: Activity[];
    nextCursor: string | null;
    hasMore: boolean;
  }> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.activityType) query.set('activity_type', params.activityType);
    if (params.activityCategory) query.set('activity_category', params.activityCategory);
    return fetchApi(`/activities?${query}`);
  },

  getAddressActivities: (
    address: string,
    params: {
      limit?: number;
      cursor?: string;
      direction?: 'in' | 'out' | 'all';
    } = {}
  ): Promise<{
    activities: Activity[];
    nextCursor: string | null;
    hasMore: boolean;
  }> => {
    const query = new URLSearchParams();
    if (params.limit) query.set('limit', String(params.limit));
    if (params.cursor) query.set('cursor', params.cursor);
    if (params.direction) query.set('direction', params.direction);
    return fetchApi(`/activities/address/${address}?${query}`);
  },

  getTransactionActivities: (txHash: string): Promise<Activity[]> => {
    return fetchApi(`/activities/transaction/${txHash}`);
  },
};
