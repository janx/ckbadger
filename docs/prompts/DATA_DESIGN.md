# Insights

- Blocks and transactions are raw data, can be fetched from ckb node rocksdb directly
- Live cells are the core state, limited size
- Consumed cells are the history of states, unlimited size
- An address
  - is a derived semantic concept defined by a lock script
  - owns live cells and consumed cells
  - occuping ckbytes with stored data, owning ckbytes through live cells capacities
  - CKB balance is the sum of owned live cells' capacities
  - Fungible token balance is the sum of owned udt cells' amounts
  - NFTs is all NFT live cells
  - Assets are all the token standard type scripts stored in owned live cells, plus CKB (CKByte) the native asset (live cell capacity)
  - Activities are asset actions parsed from associated transactions
- An asset
  - is a semantic concept defined/implemented by one or several scripts
  - has associated live cells and consumed cells
  - occuping and managing ckbytes through all associated live cells
  - Circulating balance can be derived from all live cells if it's a fungible token
  - Circulating NFTs can be derived from all live cells if it's a NFT asset
  - Holders can be derived from all live cells' lock scripts
  - Activities are actions parsed from associated transactions
- A script
  - is a syntax unit used to define transition rules
  - has associated live cells and consumed cells
  - occpuing and managing ckbytes through all associated live cells
  - has associated transactions
  - has associated addresses

# Source of Truth

Primitive Truth: blocks, transactions, cells, already stored in ckb node rocksdb

Derived Truth: various indices and aggregations on blocks, transactions, cells for fast queries

3 rocksdb for reading: ckb node rocksdb, ckbadger rocksdb 'domain', ckbadger rocksdb 'append-only'

2 rocksdb for writing: ckbadger rocksdb 'domain', ckbadger rocksdb 'append-only'

# Principles

1. Keep single source of truth.
2. Don't store redundant data, don't write the same data multiple times/places.
3. Use 'pointers' to link derived data with raw data, associate extended fields with original/core fields.
4. Append-only should keep all happened histories, never delete - uncle blocks, reverted transactions, etc. are histories kept. Reorgs should not delete from append-only store. Hashes should be used as keys in append-only store because hashes are global unique content addresses. Block number should not be used as keys in append-only store because 'block number -> hash' mapping could change on reorgs.

# Column Family Suggestions

## Layer 0: Core state

CF 1: `cells`, append only
Key: `tx_hash` + `output_index` = `outpoint`
Value: Cell info
Purpose: all cells, live or dead.

CF 2: `live_cells`, index to `cells`
Key: `outpoint`
Value: empty
Purpose: The UTXO set.

CF 3: `live_cells_by_lock`, index to `cells`
Purpose: Find live cells by `lock_script_hash`, primarily for address pages.

CF 4: `live_cells_by_type`, index to `cells`
Purpose: Find live cells by `type_script_hash`, primarily for asset pages.

CF 5: `live_cells_by_lock_code`, index to `cells`
Purpose: Find live cells by lock `code_hash`, primarily for scripts.

CF 6: `live_cells_by_type_code`, index to `cells`
Purpose: Find live cells by type `code_hash`, primarily for scripts.

CF 7: `consumed_cells`, index to `cells`
Key: `outpoint`
Purpose: Spent cell archive for history queries.

CF 8: `tx_meta`,
Key: `tx_hash` (32B)
Purpose: Per-tx metadata not in CKB node

CF 9: `tx_index`, index to `tx_meta`
Key: `block_number` + `tx_index`
Value: `tx_hash`

CF 10: `block_meta`,
Key: `block_hash` (32B)
Purpose: Per-block metadata not in CKB node

CF 11: `block_index`, index to `tx_meta`
Key: `block_number`

## Layer 1: Assets

CF 12: `asset_meta`
Key: `script_hash`
Value: fungible token and non-fungible token information
Purpose: Token metadata from info cells and label import. No aggregate stored.

CF 13: `object_meta`
Key: `object_type` + `object_id`
Enums `object_type`: Spore, SporeCluster, DIDCKB, MnftIssuer, MnftClass, MnftToken, DOTBIT

CF 14: `object_item_index`,
Key: `object_type` + `object_id` + `outpoint`
Value: empty
Purpose: Find all outpoints for a digital object. Check which is live to get current owner/capacity.

CF 15: `object_outpoints`, index to `object_item_meta`
Key: `outpoint`
Value: `object_type` + `object_id`
Purpose: Map cell outpoint to NFT.

CF 16: `object_item_by_collection`
Key: `object_type` + `collection_id` + `object_id`
Value: empty
Purpose: List digital objects in a collection/cluster. Derive collection counts from scan.

CF 17: `ft_index`,
Key: `ft_type` + `script_hash` + `outpoint`
Value: empty
Purpose: Find all outpoints for a fungible token.

CF 18: `ft_outpoints`
Key: `outpoint`
Value: `ft_type` + `script_hash`

CF 19: `dao_deposits`
Key: deposit `outpoint` (deposit `tx_hash` + `output_index`)
Value: dao deposit data
Purpose: DAO lifecycle tracking.

CF 20: `dao_withdraw_index`, index to `dao_deposits`
Key: withdraw `tx_hash`
Value: deposit `outpoint`
Purpose: Fast lookup withdraw -> deposit

## Layer 2: Aggregates (Threshold-based)

CF 22: `addr_stats`
Purpose: Pre-computed aggregates for addresses.

CF 23: `ft_stats`
Purpose: Pre-computed aggregates for fungible tokens.

CF 24: `object_collection_stats`
Purpose: Pre-computed aggregates for non-fungible token collections.

CF 24: `addr_txs`, index to `tx_meta`
Purpose: Per-address tx history for paginated listing.

CF 25: `addr_assets`
Purpose: Per-address asset holdings.

CF 26: `activities`,
Key: `activity_id` (associated transaction's `block_number` + `tx_index` + generated sequential index)
Value: Activity data
Purpose: All activities parsed from transactions.

CF 27: `addr_activities`, index to `activities`
Key: `lock_hash`
value: empty

CF 28: `object_collection_activities`, index to `activities`
Key: `object_type` + `collection_id` + `activity_id`
value: empty

CF 29: `ft_activities`, index to `activities`
Key: `ft_type` + `type_hash` + `activity_id`
value: empty
Purpose: Token transfers and other activities

CF 30: `stats`
Key: prefix(1B) + variable
Sub-namespaces:

- 0x01 Daily chain stats, 0x02 Hourly stats, 0x03 Epoch stats
- 0x04 Miner stats, 0x05 Block time dist, 0x06 Epoch time dist
- 0x07 Daily block stats, 0x08 DAO daily snapshots
- 0x0B HODL wave, 0x0F Script daily deltas
- 0x10 Token daily deltas, 0x11 Cluster daily deltas
- 0x12 Spore daily deltas, 0x15 NFT daily deltas
- 0x21 Script info (code_hash -> ScriptInfo)
- 0xF0 Sync meta (tip, status, runtime, reorg, hodl_tracker)  
  Purpose: All time-series, chart data, script info, sync metadata
