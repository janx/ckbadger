# Activity Model

Activities are semantic actions extracted from raw blockchain data. They represent human-readable interpretations of what happened in a transaction - transfers, mints, burns, deposits, etc.

## Overview

The Activity system transforms low-level blockchain data (cells, transactions) into meaningful events that users can understand. Each activity represents a single semantic action within a transaction.

```
Raw Data (Cells, Transactions)
           │
           ▼
    ActivityParser
           │
           ▼
   Unified Activities
           │
           ▼
   API / Frontend
```

## Activity Categories

Activities are grouped into 8 categories:

| Category   | Description                      | Activity Types                                                |
| ---------- | -------------------------------- | ------------------------------------------------------------- |
| `ckb`      | Native CKB transfers             | CKB_TRANSFER                                                  |
| `cellbase` | Mining rewards                   | CELLBASE_REWARD                                               |
| `token`    | Fungible tokens (sUDT/xUDT)      | TOKEN_MINT, TOKEN_TRANSFER, TOKEN_BURN                        |
| `dob`      | Digital Objects (Spore)          | DOB_MINT, DOB_TRANSFER, DOB_BURN                              |
| `nft`      | Non-fungible tokens (mNFT, .bit) | NFT_MINT, NFT_TRANSFER                                        |
| `dao`      | Nervos DAO operations            | DAO_DEPOSIT, DAO_WITHDRAW_REQUEST, DAO_WITHDRAW_COMPLETE      |
| `script`   | Script deployments               | SCRIPT_DEPLOY                                                 |
| `rgbpp`    | RGB++ cross-chain                | RGBPP_TRANSFER, RGBPP_LEAP_IN, RGBPP_LEAP_OUT, RGBPP_ISSUANCE |

## Activity Types

### CKB Category

#### CKB_TRANSFER

A transfer of native CKB between addresses.

- **from_lock_hash**: Sender's lock script hash
- **to_lock_hash**: Recipient's lock script hash
- **amount**: CKB amount in shannons

### Cellbase Category

#### CELLBASE_REWARD

Mining reward received by a miner.

- **to_lock_hash**: Miner's lock script hash
- **amount**: Total reward in shannons
- **metadata**: `{ totalReward, blockReward, proposalReward }`

### Token Category (sUDT/xUDT)

#### TOKEN_MINT

New tokens created (issuance).

- **to_lock_hash**: Recipient of minted tokens
- **amount**: Token amount (raw, needs decimals from metadata)
- **asset_id**: Token type script hash
- **metadata**: `{ symbol, decimals, tokenTypeHash }`

#### TOKEN_TRANSFER

Tokens transferred between addresses.

- **from_lock_hash**: Sender
- **to_lock_hash**: Recipient
- **amount**: Token amount
- **asset_id**: Token type script hash
- **metadata**: `{ symbol, decimals, tokenTypeHash }`

#### TOKEN_BURN

Tokens destroyed.

- **from_lock_hash**: Address that burned tokens
- **amount**: Token amount burned
- **asset_id**: Token type script hash
- **metadata**: `{ symbol, decimals, tokenTypeHash }`

### DOB Category (Spore)

#### DOB_MINT

New Digital Object created.

- **to_lock_hash**: Owner of the new DOB
- **asset_id**: Spore ID
- **metadata**: `{ sporeId, clusterId, contentType }`

#### DOB_TRANSFER

DOB ownership transferred.

- **from_lock_hash**: Previous owner
- **to_lock_hash**: New owner
- **asset_id**: Spore ID
- **metadata**: `{ sporeId, clusterId, contentType }`

#### DOB_BURN

DOB destroyed.

- **from_lock_hash**: Owner who burned the DOB
- **asset_id**: Spore ID
- **metadata**: `{ sporeId, clusterId, contentType }`

### NFT Category (mNFT, .bit)

#### NFT_MINT

New NFT created or .bit account registered.

- **to_lock_hash**: Owner
- **asset_id**: NFT/account ID
- **metadata**: `{ nftType: "mnft"|"dotbit", nftId, name }`

#### NFT_TRANSFER

NFT ownership transferred.

- **from_lock_hash**: Previous owner
- **to_lock_hash**: New owner
- **asset_id**: NFT/account ID
- **metadata**: `{ nftType, nftId, name }`

### DAO Category

#### DAO_DEPOSIT

CKB deposited into Nervos DAO.

- **to_lock_hash**: Depositor
- **amount**: Deposit amount in shannons
- **metadata**: `{ depositAr }`

#### DAO_WITHDRAW_REQUEST

Withdrawal initiated (Phase 1).

- **from_lock_hash**: Depositor requesting withdrawal
- **amount**: Deposit amount
- **asset_id**: Deposit cell outpoint (for tracking)
- **metadata**: `{ depositAr, withdrawAr }`

#### DAO_WITHDRAW_COMPLETE

Withdrawal completed (Phase 2).

- **from_lock_hash**: Withdrawer
- **amount**: Total withdrawn (deposit + compensation)
- **asset_id**: Deposit cell outpoint
- **metadata**: `{ depositAr, withdrawAr, compensation }`

### Script Category

#### SCRIPT_DEPLOY

New script deployed to the blockchain.

- **from_lock_hash**: Deployer
- **metadata**: `{ codeHash, dataSize }`

### RGB++ Category

#### RGBPP_TRANSFER

RGB++ asset transferred on CKB Layer 1.

- **from_lock_hash**: Sender (RGB++ lock)
- **to_lock_hash**: Recipient (RGB++ lock)
- **asset_id**: Asset type script hash
- **metadata**: `{ btcTxid, commitment, assetId }`

#### RGBPP_LEAP_IN

Asset moved from Bitcoin to CKB (BTC → CKB).

- **from_lock_hash**: RGB++ lock
- **to_lock_hash**: BTC_TIME lock (unlocks after timelock)
- **asset_id**: Asset type script hash
- **metadata**: `{ btcTxid, commitment, assetId }`

#### RGBPP_LEAP_OUT

Asset moved from CKB to Bitcoin (CKB → BTC).

- **to_lock_hash**: RGB++ lock on CKB
- **asset_id**: Asset type script hash
- **metadata**: `{ btcTxid, commitment, assetId }`

#### RGBPP_ISSUANCE

New RGB++ asset created.

- **to_lock_hash**: Recipient of new asset
- **asset_id**: New asset type script hash
- **metadata**: `{ btcTxid, commitment, assetId }`

## Database Schema

The `activities` table uses RANGE partitioning by `block_number` for efficient queries and rollback:

```sql
CREATE TABLE activities (
    id BIGSERIAL,
    block_number BIGINT NOT NULL,
    activity_id BYTEA NOT NULL,           -- Deterministic hash
    activity_type VARCHAR(32) NOT NULL,
    activity_category VARCHAR(16) NOT NULL,
    tx_hash BYTEA NOT NULL,
    tx_index INTEGER NOT NULL,
    activity_index SMALLINT NOT NULL,
    from_lock_hash BYTEA,
    to_lock_hash BYTEA,
    amount TEXT NOT NULL,
    asset_id BYTEA,
    metadata JSONB NOT NULL DEFAULT '{}',
    timestamp TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (block_number, id)
) PARTITION BY RANGE (block_number);
```

### Indexes

- `idx_activities_from_lock` - Queries by sender address
- `idx_activities_to_lock` - Queries by recipient address
- `idx_activities_tx_hash` - Queries by transaction
- `idx_activities_type` - Queries by activity type
- `idx_activities_category` - Queries by category
- `idx_activities_asset_id` - Queries by asset

## API Endpoints

### List Activities

```
GET /api/v1/activities
    ?type=CKB_TRANSFER        # Filter by activity type
    &category=token           # Filter by category
    &limit=20                 # Page size (max 100)
    &cursor=<cursor>          # Pagination cursor
```

### Address Activities

```
GET /api/v1/activities/address/{address}
    ?type=TOKEN_TRANSFER
    &category=dao
    &limit=20
    &cursor=<cursor>
```

### Transaction Activities

```
GET /api/v1/activities/transaction/{hash}
```

### Response Format

```json
{
  "data": [
    {
      "id": 123456,
      "activityId": "0x...",
      "activityType": "CKB_TRANSFER",
      "activityCategory": "ckb",
      "blockNumber": 12345678,
      "txHash": "0x...",
      "txIndex": 1,
      "activityIndex": 0,
      "fromLockHash": "0x...",
      "toLockHash": "0x...",
      "amount": "10000000000",
      "assetId": null,
      "metadata": {},
      "timestamp": "2024-01-15T10:30:00Z"
    }
  ],
  "total": 1000,
  "limit": 20,
  "nextCursor": "eyJibG9jayI6MTIzNDU2..."
}
```

## Activity ID Generation

Activity IDs are deterministic 32-byte hashes computed from:

```rust
blake2b(tx_hash || activity_type || activity_index)
```

This ensures:

- Same input always produces same ID
- No duplicates within a transaction
- Enables efficient deduplication

## Frontend Components

Activity components are located in `frontend/components/activity/`:

- `ActivityFeed.tsx` - Paginated list of activities
- `ActivityItem.tsx` - Single activity row
- `ActivityIcon.tsx` - Category-specific icons
- `ActivityBadge.tsx` - Activity type badges

## Implementation Notes

### Bulk Sync Mode

During bulk sync (>1000 blocks behind tip), some activity parsing is simplified:

- DOB/NFT input lookups are skipped (would require DB queries)
- Activities are still generated from output-only data

### Rollback Handling

On chain reorganization, activities are deleted by block_number range:

```sql
DELETE FROM activities WHERE block_number >= $1
```

### Parallel Writing

Activities use the COPY protocol for high-throughput writes during sync:

- `copy_activities_parallel()` - Multi-connection parallel COPY
- ~100k activities/second throughput
