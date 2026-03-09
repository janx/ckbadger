# Activity Breakdown V2 — Design

## Goal

Two improvements to the homepage Activity Breakdown pie chart:

1. **Exclude coinbase** from the asset type breakdown pie chart (coinbase txns are noise for asset analysis)
2. **Add scripts breakdown pie chart** showing activity counts per script code_hash (both lock and type scripts)

## Principle Alignment

- **CKB Native**: Scripts are a core CKB concept — showing script usage makes CKB's programmability tangible
- **Local First**: Stats computed during indexing, no external dependencies
- **Agent Friendly**: Structured API with both hex and name for script identification

## Change 1: Exclude coinbase from asset type pie chart

Frontend-only. Remove `Coinbase` slice from `buildChartData()` and exclude `coinbaseCount` from `totalActivities` in `frontend/components/activity-breakdown.tsx`.

## Change 2: Scripts breakdown pie chart

### Store — DailyActivityStats

Add field to `DailyActivityStats` in `crates/ckbadger-store/src/types.rs`:

```rust
#[serde(default)]
pub script_counts: HashMap<String, u32>,  // hex code_hash -> activity count
```

Re-sync required (schema change). Acceptable per dev status.

### Activity builder — activities.rs

- Add `lock_code_hash: Vec<u8>` to `InputCellView` (source: `LiveCellInfo.lock_code_hash`)
- Add `involved_scripts: HashSet<Vec<u8>>` to `OwnerAccum` — collects lock_code_hash + all type_code_hashes
- Change return type of `build_activities_for_block` to `Vec<(Vec<u8>, Vec<Vec<u8>>, ActivityEntry)>` — `(lock_hash, script_code_hashes, entry)`

### Batch.rs — both bulk and live sync paths

Pass script code_hashes to `accumulate_activity_stats` at both call sites (~line 4746 and ~line 5917).

### Accumulation — statistics.rs

```rust
pub fn accumulate_activity_stats(
    entry: &ActivityEntry,
    scripts: &[Vec<u8>],  // NEW: involved script code_hashes
    stats: &mut DailyActivityStats,
)
```

Each hex code_hash in `scripts` gets +1 in `stats.script_counts`.

### API — statistics.rs

New response struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCountEntry {
    pub code_hash: String,   // "0x9bd7..."
    pub name: Option<String>, // "secp256k1-blake160"
    pub count: u32,
}
```

Add `script_counts: Vec<ScriptCountEntry>` to `DailyActivityStatsResponse`. Names resolved from existing `CF_SCRIPT_INFO` via `get_script_info()`.

### Frontend — activity-breakdown.tsx

- Second `PieChart` showing script usage
- Script name as label, fallback to truncated hex if unnamed
- Same data source (getDailyActivityStats), no new API call

## Counting rules

- Each activity involves exactly 1 lock script + 0..N type scripts
- Each distinct code_hash gets +1 per activity where it appears
- Example: DAO deposit via secp256k1 → secp256k1 +1, DAO +1
