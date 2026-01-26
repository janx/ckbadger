/**
 * Fuzzing Framework Types
 *
 * Core type definitions for the ckbadger fuzzing framework.
 */

// ============================================================================
// Comparison Results
// ============================================================================

export type Severity = 'critical' | 'warning' | 'info';

export interface ComparisonResult {
  /** Entity type being compared (block, transaction, address, page) */
  entity: string;
  /** Unique identifier for the entity */
  id: string;
  /** Field name that has a discrepancy */
  field: string;
  /** Value from ckbadger */
  ckbadger: unknown;
  /** Value from official explorer */
  official: unknown;
  /** Severity level of the issue */
  severity: Severity;
  /** Human-readable description of the issue */
  message: string;
}

export interface PageConsistencyCheck {
  /** Page URL path */
  page: string;
  /** Name of the count field being checked */
  countField: string;
  /** Displayed count value */
  countValue: number;
  /** Actual list length from API */
  listLength: number;
  /** Whether the count matches the list */
  isConsistent: boolean;
  /** Additional details (e.g., pagination info) */
  details?: string;
}

// ============================================================================
// Official Explorer API Types (explorer.nervos.org)
// ============================================================================

export interface OfficialBlockResponse {
  data: {
    id: string;
    type: 'block';
    attributes: {
      block_hash: string;
      number: number;
      transactions_count: number;
      proposals_count: number;
      uncles_count: number;
      uncle_block_hashes: string[];
      reward: string;
      total_transaction_fee: string;
      cell_consumed: string;
      total_cell_capacity: string;
      miner_hash: string;
      miner_message: string;
      timestamp: number;
      difficulty: string;
      epoch: number;
      length: number;
      start_number: number;
      version: number;
      nonce: string;
      size: number;
      miner_reward: string;
      block_index_in_epoch: number;
      cycles: number | null;
    };
  };
}

export interface OfficialTransactionResponse {
  data: {
    id: string;
    type: 'ckb_transaction';
    attributes: {
      transaction_hash: string;
      block_number: number;
      block_timestamp: number;
      is_cellbase: boolean;
      transaction_fee: string;
      bytes: number;
      cycles: number | null;
      display_inputs: OfficialCellDisplay[];
      display_outputs: OfficialCellDisplay[];
      income: string | null;
    };
  };
}

export interface OfficialCellDisplay {
  id: number;
  from_cellbase: boolean;
  capacity: string;
  address_hash: string | null;
  generated_tx_hash: string;
  cell_index: number;
  cell_type: string;
  since?: {
    raw: string;
    median_timestamp: string | null;
  };
}

export interface OfficialAddressResponse {
  data: {
    id: string;
    type: 'address';
    attributes: {
      address_hash: string;
      balance: string;
      live_cells_count: string;
      mined_blocks_count: string;
      transactions_count: string;
      dao_deposit: string;
      interest: string;
      is_special: boolean;
      special_address?: string;
      lock_script: {
        code_hash: string;
        hash_type: string;
        args: string;
      };
    };
  };
}

export interface OfficialDaoResponse {
  data: {
    id: string;
    type: 'dao_statistic';
    attributes: {
      total_deposit: string;
      depositors_count: string;
      claimed_compensation: string;
      unclaimed_compensation: string;
      deposit_compensation: string;
      mining_reward: string;
      treasury_amount: string;
      estimated_apc: string;
    };
  };
}

// ============================================================================
// Ckbadger API Types (subset for fuzzing)
// ============================================================================

export interface CkbadgerBlock {
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
}

export interface CkbadgerTransaction {
  hash: string;
  blockNumber: number;
  blockHash: string;
  index: number;
  inputsCount: number;
  outputsCount: number;
  fee: string;
  isCellbase: boolean;
  timestamp: string;
}

export interface CkbadgerTransactionDetail extends CkbadgerTransaction {
  feeRate?: string;
  txSize?: number;
  cycles?: string;
  confirmations: number;
  inputsCapacity: string;
  outputsCapacity: string;
  inputs?: CkbadgerInput[];
  outputs?: CkbadgerOutput[];
}

export interface CkbadgerInput {
  previousOutput?: {
    txHash: string;
    index: number;
  };
  since?: string;
  capacity?: string;
  address?: string;
}

export interface CkbadgerOutput {
  capacity: string;
  address?: string;
}

export interface CkbadgerAddress {
  lockScriptHash: string;
  address?: string;
  balance: string;
  liveCellsCount: number;
  transactionsCount: number;
}

export interface CkbadgerNetworkStats {
  latestBlock: number;
  avgBlockTime: string;
  hashRate: string;
  difficulty: string;
  epoch: string;
}

export interface CkbadgerDaoStatistics {
  totalDeposited: string;
  totalDepositors: number;
  activeDeposits: number;
  totalCompensationPaid: string;
  unclaimedCompensation: string;
  estimatedApc: string;
}

export interface CkbadgerToken {
  typeScriptHash: string;
  name: string | null;
  symbol: string | null;
  holdersCount: number;
  transfersCount: number;
}

export interface CursorPaginatedResponse<T> {
  data: T[];
  total: number;
  limit: number;
  hasMore: boolean;
  nextCursor: string | null;
}

// ============================================================================
// Report Types
// ============================================================================

export interface FuzzingReport {
  startTime: string;
  endTime: string;
  duration: number;
  mode: 'api' | 'page' | 'visual' | 'all';
  config: {
    sampleSize: number;
    ckbadgerUrl: string;
    officialUrl: string;
  };
  summary: {
    totalChecks: number;
    passed: number;
    failed: number;
    byEntity: Record<string, number>;
    bySeverity: Record<Severity, number>;
  };
  issues: ComparisonResult[];
  pageConsistencyIssues?: PageConsistencyCheck[];
}

// ============================================================================
// Sampler Types
// ============================================================================

export interface SamplingStrategy {
  /** Percentage of samples from recent blocks (last 10k) */
  recentWeight: number;
  /** Percentage from mid-range (10k-1M) */
  midRangeWeight: number;
  /** Percentage from genesis era (first 100k) */
  genesisWeight: number;
}

export interface SamplerConfig {
  /** Number of items to sample */
  count: number;
  /** Sampling strategy weights */
  strategy: SamplingStrategy;
  /** Maximum retries for failed samples */
  maxRetries: number;
}

// ============================================================================
// Runner Options
// ============================================================================

export interface FuzzerOptions {
  /** Number of blocks to sample */
  blockSampleSize: number;
  /** Number of transactions to sample */
  txSampleSize: number;
  /** Number of addresses to sample */
  addressSampleSize: number;
  /** Maximum concurrent API requests */
  concurrency: number;
  /** Request timeout in milliseconds */
  timeout: number;
  /** Whether to continue on errors */
  continueOnError: boolean;
  /** Output directory for reports */
  outputDir: string;
  /** Verbose logging */
  verbose: boolean;
}
