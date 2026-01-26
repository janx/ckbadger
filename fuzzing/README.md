# Ckbadger Fuzzing Framework

A comprehensive fuzzing framework for testing data consistency in the ckbadger blockchain explorer.

## Overview

This framework provides three types of testing:

1. **API Comparison** (`--mode=api`): Compare ckbadger API responses with the official explorer.nervos.org
2. **Page Consistency** (`--mode=page`): Check internal consistency between summary counts and actual lists
3. **Visual Consistency** (`fuzz:visual`): Playwright tests for UI-level data consistency

## Quick Start

```bash
# Run all fuzzing tests with default settings
pnpm fuzz

# Quick smoke test (5 samples each)
pnpm fuzz:quick

# API comparison only
pnpm fuzz:api

# Page consistency only
pnpm fuzz:page

# Visual consistency tests (requires running frontend)
pnpm fuzz:visual
```

## Configuration

### Environment Variables

```bash
# Ckbadger API (required for all modes)
CKBADGER_API_URL=http://localhost:3001/api/v1

# Official Explorer API (required for API mode)
OFFICIAL_EXPLORER_URL=https://explorer.nervos.org/api/v1

# Frontend URL (required for visual mode)
CKBADGER_FRONTEND_URL=http://localhost:3000

# Enable verbose logging
FUZZING_VERBOSE=true
```

### CLI Options

```
Options:
  --mode=<api|page|all>   Testing mode (default: all)
  -b, --blocks N          Number of blocks to sample (default: 50)
  -t, --transactions N    Number of transactions to sample (default: 30)
  -a, --addresses N       Number of addresses to sample (default: 20)
  -c, --concurrency N     Max concurrent requests (default: 5)
  --timeout N             Request timeout in ms (default: 30000)
  -o, --output DIR        Output directory (default: ./fuzzing/reports)
  -v, --verbose           Enable verbose logging
  --stop-on-error         Stop on first error
```

## Test Types

### 1. API Comparison Tests

Compares data between ckbadger and explorer.nervos.org for:

- **Blocks**: hash, number, transactionsCount, proposalsCount, timestamp, miner
- **Transactions**: hash, blockNumber, inputsCount, outputsCount, fee, isCellbase
- **Addresses**: balance, liveCellsCount, transactionsCount

Critical mismatches (hash, counts) are reported as `critical`, timing differences as `warning`.

### 2. Page Consistency Tests

Checks that summary counts match list totals:

| Page              | Count Field                 | List Source                      |
| ----------------- | --------------------------- | -------------------------------- |
| `/blocks/[id]`    | `block.transactionsCount`   | `getTransactions({blockNumber})` |
| `/blocks/[id]`    | `block.proposalsCount`      | `getBlockProposals()`            |
| `/address/[addr]` | `address.liveCellsCount`    | `getLiveCells().total`           |
| `/address/[addr]` | `address.transactionsCount` | `getAddressTransactions().total` |
| `/tokens/[hash]`  | `token.holdersCount`        | `getTokenHolders().total`        |
| `/tokens/[hash]`  | `token.transfersCount`      | `getTokenTransfers().total`      |
| `/dao`            | `stats.activeDeposits`      | `getDaoDeposits().total`         |

### 3. Visual Consistency Tests

Playwright tests that verify UI-displayed counts match actual rendered items:

- Transaction tab count vs row count on block pages
- Inputs/outputs headers vs actual items on TX pages
- Live cells card vs tab count on address pages

## Output

Reports are saved to `fuzzing/reports/` in JSON format:

```json
{
  "startTime": "2024-01-15T10:30:00.000Z",
  "endTime": "2024-01-15T10:35:00.000Z",
  "duration": 300000,
  "mode": "all",
  "summary": {
    "totalChecks": 150,
    "passed": 148,
    "failed": 2,
    "byEntity": { "block": 1, "address": 1 },
    "bySeverity": { "critical": 0, "warning": 2, "info": 0 }
  },
  "issues": [...],
  "pageConsistencyIssues": [...]
}
```

## Architecture

```
fuzzing/
├── src/
│   ├── config.ts              # Configuration and CLI parsing
│   ├── types.ts               # Type definitions
│   ├── index.ts               # Main entry point
│   ├── samplers/              # Random data samplers
│   │   ├── block-sampler.ts   # Stratified block sampling
│   │   ├── tx-sampler.ts      # Transaction sampling
│   │   └── address-sampler.ts # Top/active address sampling
│   ├── fetchers/              # API clients
│   │   ├── ckbadger.ts        # Ckbadger API
│   │   └── official.ts        # explorer.nervos.org API
│   ├── comparators/           # Data comparison logic
│   │   ├── block-comparator.ts
│   │   ├── tx-comparator.ts
│   │   ├── address-comparator.ts
│   │   └── page-consistency.ts
│   ├── reporters/             # Report generation
│   │   └── inconsistency-reporter.ts
│   └── runners/               # Test runners
│       ├── api-fuzzer.ts      # API comparison runner
│       └── page-fuzzer.ts     # Page consistency runner
├── playwright/                # Visual tests
│   ├── playwright.config.ts
│   └── visual-consistency.spec.ts
└── reports/                   # Generated reports
```

## Sampling Strategy

The framework uses stratified sampling to ensure comprehensive coverage:

- **70%** from recent blocks (last 10,000)
- **20%** from mid-range (10,000 - 1,000,000)
- **10%** from genesis era (first 100,000)

This ensures both current data accuracy and historical data integrity.

## CI Integration

```yaml
# Example GitHub Actions workflow
- name: Run Fuzzing Tests
  run: pnpm fuzz:quick
  env:
    CKBADGER_API_URL: http://localhost:3001/api/v1
```

For production CI, use `pnpm fuzz:quick` with minimal samples to avoid rate limiting.

## Extending

### Adding New Comparators

1. Create a new comparator in `src/comparators/`
2. Implement the `compare()` method returning `ComparisonResult[]`
3. Add to the appropriate runner

### Adding New Page Checks

1. Add a new method to `PageConsistencyChecker`
2. Call it from `PageFuzzer.run()`
