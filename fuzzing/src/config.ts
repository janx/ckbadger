/**
 * Fuzzing Framework Configuration
 */

import type { FuzzerOptions, SamplingStrategy } from './types';

export const config = {
  // API endpoints
  ckbadger: {
    baseUrl: process.env.CKBADGER_API_URL || 'http://localhost:3001/api/v1',
  },
  official: {
    // CKB Explorer API (mainnet)
    baseUrl: process.env.OFFICIAL_EXPLORER_URL || 'https://explorer.nervos.org/api/v1',
  },
  frontend: {
    baseUrl: process.env.CKBADGER_FRONTEND_URL || 'http://localhost:3000',
  },

  // Sampling configuration
  sampling: {
    defaultStrategy: {
      recentWeight: 0.7, // 70% from recent blocks
      midRangeWeight: 0.2, // 20% from mid-range
      genesisWeight: 0.1, // 10% from genesis era
    } as SamplingStrategy,
    // Block ranges for sampling
    recentBlocksRange: 10000, // Last 10k blocks
    midRangeStart: 10000,
    midRangeEnd: 1000000,
    genesisRange: 100000,
  },

  // Tolerance settings for comparison
  tolerance: {
    // Timestamp difference tolerance (ms) - accounts for timezone/rounding
    timestampDiffMs: 1000,
    // Balance must match exactly (in shannon)
    balanceDiffShannon: BigInt(0),
    // Capacity must match exactly
    capacityDiffShannon: BigInt(0),
  },

  // Request settings
  request: {
    timeout: 30000, // 30 seconds
    retries: 3,
    retryDelay: 1000, // 1 second
    concurrency: 5,
  },

  // Output settings
  output: {
    reportsDir: './fuzzing/reports',
    verbose: process.env.FUZZING_VERBOSE === 'true',
  },
};

export const defaultFuzzerOptions: FuzzerOptions = {
  blockSampleSize: 50,
  txSampleSize: 30,
  addressSampleSize: 20,
  concurrency: 5,
  timeout: 30000,
  continueOnError: true,
  outputDir: './fuzzing/reports',
  verbose: false,
};

/**
 * Parse CLI arguments to override default options
 */
export function parseCliOptions(args: string[]): Partial<FuzzerOptions> {
  const options: Partial<FuzzerOptions> = {};

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    const next = args[i + 1];

    switch (arg) {
      case '--blocks':
      case '-b':
        options.blockSampleSize = parseInt(next, 10);
        i++;
        break;
      case '--transactions':
      case '-t':
        options.txSampleSize = parseInt(next, 10);
        i++;
        break;
      case '--addresses':
      case '-a':
        options.addressSampleSize = parseInt(next, 10);
        i++;
        break;
      case '--concurrency':
      case '-c':
        options.concurrency = parseInt(next, 10);
        i++;
        break;
      case '--timeout':
        options.timeout = parseInt(next, 10);
        i++;
        break;
      case '--output':
      case '-o':
        options.outputDir = next;
        i++;
        break;
      case '--verbose':
      case '-v':
        options.verbose = true;
        break;
      case '--stop-on-error':
        options.continueOnError = false;
        break;
    }
  }

  return options;
}

/**
 * Field mappings between ckbadger and official explorer
 * Used for automated comparison
 */
export const fieldMappings = {
  block: {
    hash: ['hash', 'attributes.block_hash'],
    number: ['number', 'attributes.number'],
    transactionsCount: ['transactionsCount', 'attributes.transactions_count'],
    proposalsCount: ['proposalsCount', 'attributes.proposals_count'],
    unclesCount: ['unclesCount', 'attributes.uncles_count'],
    difficulty: ['difficulty', 'attributes.difficulty'],
    nonce: ['nonce', 'attributes.nonce'],
    minerAddress: ['minerAddress', 'attributes.miner_hash'],
  },
  transaction: {
    hash: ['hash', 'attributes.transaction_hash'],
    blockNumber: ['blockNumber', 'attributes.block_number'],
    isCellbase: ['isCellbase', 'attributes.is_cellbase'],
    fee: ['fee', 'attributes.transaction_fee'],
    inputsCount: ['inputsCount', 'attributes.display_inputs.length'],
    outputsCount: ['outputsCount', 'attributes.display_outputs.length'],
  },
  address: {
    balance: ['balance', 'attributes.balance'],
    liveCellsCount: ['liveCellsCount', 'attributes.live_cells_count'],
    transactionsCount: ['transactionsCount', 'attributes.transactions_count'],
  },
};

/**
 * Critical fields that must match exactly
 */
export const criticalFields = {
  block: ['hash', 'number', 'transactionsCount', 'parentHash'],
  transaction: ['hash', 'blockNumber', 'isCellbase', 'inputsCount', 'outputsCount'],
  address: ['balance'],
};

/**
 * Fields that allow some tolerance
 */
export const tolerantFields = {
  block: ['timestamp'],
  transaction: ['fee'], // May have rounding differences
  address: ['transactionsCount'], // May be slightly delayed
};
