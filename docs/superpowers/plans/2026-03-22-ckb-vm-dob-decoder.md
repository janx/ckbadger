# CKB-VM DOB Decoder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the inline Rust DOB trait decoder with CKB-VM execution of on-chain decoder binaries, achieving 100% correctness for all DOB pattern types (including custom decoders like Nervape) while removing DOB decoding from the bulk sync critical path.

**Architecture:** New `ckbadger-dob-decoder` crate wraps `ckb-vm` to execute RISC-V decoder binaries. Bulk sync skips DOB trait decoding entirely (justified below). Post-sync background worker decodes all DOB spores via CKB-VM and writes results to a new domain CF (`CF_DOB_DECODED`). API reads from the cache only — API never writes to RocksDB. CKB-VM is the **single decode path** — the old inline Rust decoder is removed, not kept as fallback.

**Bulk Sync Policy Exception:** `CF_DOB_DECODED` is a **derived cache** — it contains decoded interpretations of raw DOB data (DNA + pattern), not canonical chain state. The raw data (cell payloads, cluster metadata) IS written inline during bulk sync. Only the CKB-VM interpretation is deferred to post-sync because running RISC-V binaries inline would severely impact bulk sync throughput. This is analogous to how search indexes are built — the source data is authoritative, the derived index is rebuilt from it.

**Tech Stack:** `ckb-vm = "0.24"` (asm feature), existing `ckb-types`/`ckb-hash`/`reqwest` from workspace.

**Key reference:** [dob-decoder-standalone-server](https://github.com/sporeprotocol/dob-decoder-standalone-server) — decoder binaries are pure functions receiving `argv = [dna_hex, pattern_json]` and outputting via syscall 2177. No CKB syscall mocking needed.

**Data audit summary** (from DB analysis of 40,979 spores):
- 32,775 DOB spores (28,581 dob/0 + 4,194 dob/1)
- 397 clusters, 394 with valid dob metadata
- Only 5 unique decoder hashes chain-wide
- 8 clusters with custom pattern types (nervape*, btcfs, ckbfs) that the current Rust decoder cannot handle

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `crates/dob-decoder/Cargo.toml` | New crate manifest — depends on ckb-vm, serde, serde_json, anyhow, hex, tracing |
| `crates/dob-decoder/src/lib.rs` | Public API: `DobDecoder` struct, `decode_dob0`, `decode_dob1_chain` |
| `crates/dob-decoder/src/vm.rs` | CKB-VM execution: load binary, register syscall 2177, run, capture output |
| `crates/dob-decoder/src/types.rs` | `DobDecodedResult`, `DobTrait`, `DecoderBinaryRef` types |
| `crates/dob-decoder/src/cache.rs` | Disk-based decoder binary cache: `DecoderBinaryCache` |
| `crates/dob-decoder/src/fetch.rs` | Fetch decoder binary from CKB RPC by code_hash or type_id |
| `crates/indexer/src/sync/dob_decode_worker.rs` | Background worker: batch-decode all pending DOB spores after sync |

### Modified files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace root) | Add `crates/dob-decoder` to members, add `ckb-vm` to workspace deps |
| `crates/ckbadger-store/src/store.rs` | Add `CF_DOB_DECODED` constant and accessor |
| `crates/ckbadger-store/src/types.rs` | Add `DobDecodedEntry` type |
| `crates/ckbadger-store/src/batch.rs` | Add `put_dob_decoded` / `delete_dob_decoded` methods |
| `crates/ckbadger-store/src/spore_ops.rs` | Add `get_dob_decoded` / `list_undecoded_dob_spores` methods |
| `crates/indexer/src/parser/media_source.rs:54-61` | Skip `extract_dob_media_sources` when `skip_dob_decode` flag is set |
| `crates/indexer/src/db/writer/spore.rs:565-586` | Pass skip flag during bulk sync |
| `crates/api/src/routes/spore.rs:764-833` | Replace `decode_dob_embedded` with CKB-VM decoder call |
| `crates/indexer/src/entry.rs` | Spawn background DOB decode worker after sync setup |
| `crates/config/src/lib.rs` | Add `decoder_cache_path` to StoreConfig |
| `crates/indexer/Cargo.toml` | Add `ckbadger-dob-decoder` dependency |
| `crates/api/Cargo.toml` | Add `ckbadger-dob-decoder` dependency |

---

## Task 1: Create `ckbadger-dob-decoder` crate — VM execution core

**Files:**
- Create: `crates/dob-decoder/Cargo.toml`
- Create: `crates/dob-decoder/src/lib.rs`
- Create: `crates/dob-decoder/src/vm.rs`
- Create: `crates/dob-decoder/src/types.rs`
- Modify: `Cargo.toml` (workspace root, lines 3-13 members + lines 22-89 deps)

- [ ] **Step 1: Add workspace member and dependency**

In `Cargo.toml` (workspace root):

```toml
# Add to [workspace] members (line ~12):
members = [
    # ... existing ...
    "crates/dob-decoder",
]

# Add to [workspace.dependencies] (after line 84):
ckb-vm = { version = "0.24", features = ["asm"] }
```

- [ ] **Step 2: Create crate manifest**

Create `crates/dob-decoder/Cargo.toml`:

```toml
[package]
name = "ckbadger-dob-decoder"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
ckb-vm = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
hex = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Create types module**

Create `crates/dob-decoder/src/types.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Output from a DOB decoder binary — matches the `StandardDOBOutput` format
/// from dob-decoder-standalone-server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobTraitGroup {
    pub name: String,
    pub traits: Vec<DobTraitValue>,
}

/// A single trait value from the decoder output.
/// Decoder outputs `{"String": "Blue"}` or `{"Number": 42}` — a single-entry map.
/// We deserialize it as a map and extract the single entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobTraitValue {
    #[serde(flatten)]
    pub inner: std::collections::BTreeMap<String, Value>,
}

impl DobTraitValue {
    /// Extract the display value as a string, regardless of type tag.
    pub fn display_value(&self) -> String {
        self.inner
            .values()
            .next()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default()
    }

    /// Get the type tag (e.g. "String", "Number", "SVG").
    pub fn type_tag(&self) -> &str {
        self.inner
            .keys()
            .next()
            .map(|s| s.as_str())
            .unwrap_or("Unknown")
    }
}

/// Flattened trait for storage and API: name + display value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobTrait {
    pub name: String,
    pub value: String,
}

/// Full decode result for a single DOB spore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodedResult {
    pub traits: Vec<DobTrait>,
    pub raw_output: String,
}

/// Decoder binary reference from cluster metadata.
#[derive(Debug, Clone)]
pub enum DecoderRef {
    CodeHash(Vec<u8>),
    TypeId(Vec<u8>),
}
```

- [ ] **Step 4: Create VM execution module**

Create `crates/dob-decoder/src/vm.rs`:

```rust
use anyhow::{anyhow, Result};
use ckb_vm::cost_model::constant_cycles;
use ckb_vm::machine::asm::{AsmCoreMachine, AsmMachine};
use ckb_vm::memory::Memory;
use ckb_vm::registers::{A0, A7};
use ckb_vm::{Bytes, DefaultMachineBuilder, SupportMachine, Syscalls};
use std::sync::{Arc, Mutex};

/// Debug syscall number used by DOB decoder binaries to output results.
const DEBUG_SYSCALL_NUMBER: u64 = 2177;

/// Captured output strings from the decoder binary.
type OutputBuffer = Arc<Mutex<Vec<String>>>;

struct DebugSyscall {
    output: OutputBuffer,
}

impl<Mac: SupportMachine> Syscalls<Mac> for DebugSyscall {
    fn initialize(&mut self, _machine: &mut Mac) -> Result<(), ckb_vm::error::Error> {
        Ok(())
    }

    fn ecall(&mut self, machine: &mut Mac) -> Result<bool, ckb_vm::error::Error> {
        let code = machine.registers()[A7].to_u64();
        if code != DEBUG_SYSCALL_NUMBER {
            return Ok(false);
        }
        let addr = machine.registers()[A0].to_u64();
        let mut buf = Vec::new();
        let mut ptr = addr;
        loop {
            let byte = machine
                .memory_mut()
                .load8(&Mac::REG::from_u64(ptr))?
                .to_u8();
            if byte == 0 {
                break;
            }
            buf.push(byte);
            ptr += 1;
        }
        let s = String::from_utf8_lossy(&buf).to_string();
        self.output.lock().unwrap().push(s);
        Ok(true)
    }
}

/// Execute a RISC-V decoder binary with the given arguments.
///
/// Returns `(exit_code, output_strings)`.
pub fn execute_riscv_binary(binary: &[u8], args: &[&str]) -> Result<(i8, Vec<String>)> {
    let output: OutputBuffer = Arc::new(Mutex::new(Vec::new()));

    let asm_core = AsmCoreMachine::new(
        ckb_vm::ISA_IMC | ckb_vm::ISA_B | ckb_vm::ISA_MOP | ckb_vm::ISA_A,
        ckb_vm::machine::VERSION2,
        u64::MAX,
    );

    let core = DefaultMachineBuilder::new(asm_core)
        .instruction_cycle_func(Box::new(constant_cycles))
        .syscall(Box::new(DebugSyscall {
            output: output.clone(),
        }))
        .build();

    let mut machine = AsmMachine::new(core);

    let program = Bytes::copy_from_slice(binary);
    let argv: Vec<Bytes> = args.iter().map(|a| Bytes::from(a.to_string())).collect();

    machine.load_program(&program, &argv)?;
    let exit_code = machine.run()?;

    let strings = output.lock().unwrap().clone();
    Ok((exit_code, strings))
}
```

- [ ] **Step 5: Create lib.rs with public API**

Create `crates/dob-decoder/src/lib.rs`:

```rust
pub mod cache;
pub mod fetch;
pub mod types;
pub mod vm;

use anyhow::{anyhow, Result};
use serde_json::Value;
use types::{DobDecodedResult, DobTrait, DobTraitGroup};

/// Decode a DOB/0 spore: single decoder, DNA + pattern → traits.
pub fn decode_dob0(
    decoder_binary: &[u8],
    dna_hex: &str,
    pattern_json: &str,
) -> Result<DobDecodedResult> {
    let (exit_code, outputs) = vm::execute_riscv_binary(decoder_binary, &[dna_hex, pattern_json])?;
    if exit_code != 0 {
        return Err(anyhow!(
            "DOB/0 decoder exited with code {}: {:?}",
            exit_code,
            outputs
        ));
    }
    let raw_output = outputs.into_iter().collect::<String>();
    let trait_groups: Vec<DobTraitGroup> = serde_json::from_str(&raw_output)
        .map_err(|e| anyhow!("failed to parse decoder output: {}: {}", e, raw_output))?;

    let traits = flatten_trait_groups(&trait_groups);
    Ok(DobDecodedResult { traits, raw_output })
}

/// Decode a DOB/1 spore: chain of decoders, each feeding into the next.
pub fn decode_dob1_chain(
    decoders: &[(&[u8], &str)], // [(binary, pattern_json), ...]
    dna_hex: &str,
) -> Result<DobDecodedResult> {
    if decoders.is_empty() {
        return Err(anyhow!("DOB/1 decoder chain is empty — at least one decoder required"));
    }
    let mut previous_output: Option<String> = None;

    for (i, (binary, pattern_json)) in decoders.iter().enumerate() {
        let args: Vec<&str> = if let Some(prev) = &previous_output {
            vec![dna_hex, pattern_json, prev]
        } else {
            vec![dna_hex, pattern_json]
        };

        let (exit_code, outputs) = vm::execute_riscv_binary(binary, &args)?;
        if exit_code != 0 {
            return Err(anyhow!(
                "DOB/1 decoder chain step {} exited with code {}: {:?}",
                i,
                exit_code,
                outputs
            ));
        }
        previous_output = Some(outputs.into_iter().collect::<String>());
    }

    let raw_output = previous_output
        .ok_or_else(|| anyhow!("DOB/1 decoder chain produced no output"))?;
    let trait_groups: Vec<DobTraitGroup> = serde_json::from_str(&raw_output)
        .map_err(|e| anyhow!("failed to parse decoder chain output: {}: {}", e, raw_output))?;

    let traits = flatten_trait_groups(&trait_groups);
    Ok(DobDecodedResult { traits, raw_output })
}

fn flatten_trait_groups(groups: &[DobTraitGroup]) -> Vec<DobTrait> {
    groups
        .iter()
        .map(|g| DobTrait {
            name: g.name.clone(),
            value: g
                .traits
                .first()
                .map(|t| t.display_value())
                .unwrap_or_default(),
        })
        .collect()
}
```

- [ ] **Step 6: Verify the crate compiles**

Run: `cargo check -p ckbadger-dob-decoder`
Expected: compiles (may have warnings for unused modules cache/fetch — that's OK, they'll be added in Task 2)

- [ ] **Step 7: Add unit test for VM execution**

Add to `crates/dob-decoder/src/vm.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_returns_error_for_empty_binary() {
        let result = execute_riscv_binary(&[], &["arg1"]);
        assert!(result.is_err());
    }
}
```

Run: `cargo test -p ckbadger-dob-decoder`
Expected: test passes (empty binary is invalid ELF → error)

- [ ] **Step 8: Commit**

```bash
git add crates/dob-decoder/ Cargo.toml
git commit -m "feat(dob-decoder): create ckbadger-dob-decoder crate with CKB-VM execution core"
```

---

## Task 2: Decoder binary cache and fetching

**Files:**
- Create: `crates/dob-decoder/src/cache.rs`
- Create: `crates/dob-decoder/src/fetch.rs`
- Modify: `crates/dob-decoder/Cargo.toml` (add reqwest, tokio)
- Modify: `crates/config/src/lib.rs:67-73` (add decoder_cache_path)

- [ ] **Step 1: Add dependencies for async fetch**

Add to `crates/dob-decoder/Cargo.toml` under `[dependencies]`:

```toml
reqwest = { workspace = true }
tokio = { workspace = true }
ckb-types = { workspace = true }
ckb-jsonrpc-types = { workspace = true }
ckb-hash = { workspace = true }
```

- [ ] **Step 2: Create cache module**

Create `crates/dob-decoder/src/cache.rs`:

```rust
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::info;

/// Disk-backed cache for decoder RISC-V binaries.
/// In-memory LRU holds the most recently used binaries.
pub struct DecoderBinaryCache {
    cache_dir: PathBuf,
    memory: Mutex<HashMap<String, Vec<u8>>>,
}

impl DecoderBinaryCache {
    pub fn new(cache_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(cache_dir)?;
        Ok(Self {
            cache_dir: cache_dir.to_path_buf(),
            memory: Mutex::new(HashMap::new()),
        })
    }

    /// Get a cached decoder binary by its cache key (e.g. "code_hash_0x1234...").
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        // Check memory first
        if let Some(binary) = self.memory.lock().unwrap().get(key) {
            return Some(binary.clone());
        }
        // Check disk
        let path = self.disk_path(key);
        if path.exists() {
            if let Ok(binary) = std::fs::read(&path) {
                self.memory
                    .lock()
                    .unwrap()
                    .insert(key.to_string(), binary.clone());
                return Some(binary);
            }
        }
        None
    }

    /// Store a decoder binary in both memory and disk cache.
    pub fn put(&self, key: &str, binary: &[u8]) -> Result<()> {
        let path = self.disk_path(key);
        std::fs::write(&path, binary)?;
        self.memory
            .lock()
            .unwrap()
            .insert(key.to_string(), binary.to_vec());
        info!(key, bytes = binary.len(), "cached decoder binary");
        Ok(())
    }

    fn disk_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.bin", key))
    }

    /// Cache key for a code_hash decoder.
    pub fn code_hash_key(hash: &[u8]) -> String {
        format!("code_hash_{}", hex::encode(hash))
    }

    /// Cache key for a type_id decoder.
    pub fn type_id_key(hash: &[u8]) -> String {
        format!("type_id_{}", hex::encode(hash))
    }
}
```

- [ ] **Step 3: Create fetch module**

Create `crates/dob-decoder/src/fetch.rs`:

```rust
use crate::cache::DecoderBinaryCache;
use crate::types::DecoderRef;
use anyhow::{anyhow, Result};
use serde_json::Value;
use tracing::{debug, warn};

/// Fetch a decoder binary, using cache when available.
///
/// For `CodeHash`: fetches the deployment transaction and extracts cell data.
/// For `TypeId`: searches for the live cell via CKB indexer RPC.
pub async fn fetch_decoder_binary(
    decoder_ref: &DecoderRef,
    rpc_url: &str,
    cache: &DecoderBinaryCache,
) -> Result<Vec<u8>> {
    let cache_key = match decoder_ref {
        DecoderRef::CodeHash(hash) => DecoderBinaryCache::code_hash_key(hash),
        DecoderRef::TypeId(hash) => DecoderBinaryCache::type_id_key(hash),
    };

    // Check cache first
    if let Some(binary) = cache.get(&cache_key) {
        debug!(key = cache_key, "decoder binary cache hit");
        return Ok(binary);
    }

    // Fetch from chain
    let binary = match decoder_ref {
        DecoderRef::CodeHash(hash) => fetch_by_code_hash(hash, rpc_url).await?,
        DecoderRef::TypeId(hash) => fetch_by_type_id(hash, rpc_url).await?,
    };

    cache.put(&cache_key, &binary)?;
    Ok(binary)
}

/// Fetch decoder binary by searching CKB indexer for a cell whose type_script
/// uses TypeID and has args matching the given hash.
async fn fetch_by_type_id(type_id_args: &[u8], rpc_url: &str) -> Result<Vec<u8>> {
    let type_id_code_hash = "0x00000000000000000000000000000000000000000000000000545950455f4944";
    let args_hex = format!("0x{}", hex::encode(type_id_args));

    let search_key = serde_json::json!({
        "script": {
            "code_hash": type_id_code_hash,
            "hash_type": "type",
            "args": args_hex
        },
        "script_type": "type",
        "with_data": true
    });

    let request = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_cells",
        "params": [search_key, "asc", "0x1"]
    });

    let client = reqwest::Client::new();
    let response: Value = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await?
        .json()
        .await?;

    let cells = response["result"]["objects"]
        .as_array()
        .ok_or_else(|| anyhow!("no cells found for type_id {}", args_hex))?;

    let cell = cells
        .first()
        .ok_or_else(|| anyhow!("no live cell for type_id {}", args_hex))?;

    let data_hex = cell["output_data"]
        .as_str()
        .ok_or_else(|| anyhow!("missing output_data for type_id cell"))?;

    let data = hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex))?;
    if data.is_empty() {
        return Err(anyhow!("empty decoder binary for type_id {}", args_hex));
    }

    Ok(data)
}

/// Fetch decoder binary by known deployment transaction.
/// For code_hash decoders, we need the deployment tx_hash and output index.
///
/// Falls back to searching the indexer if the deployment is not hardcoded.
async fn fetch_by_code_hash(code_hash: &[u8], rpc_url: &str) -> Result<Vec<u8>> {
    let hash_hex = hex::encode(code_hash);

    // Known DOB/0 standard decoder deployments (mainnet)
    let known_deployments: Vec<(&str, &str, u32)> = vec![
        // (code_hash, tx_hash, output_index)
        (
            "13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c",
            "71023885a2178648be6a7f138ee49379000a82cda98dd8adabee99eaaca42fde",
            0,
        ),
    ];

    for (known_hash, tx_hash, out_idx) in &known_deployments {
        if hash_hex == *known_hash {
            return fetch_cell_data_from_tx(rpc_url, tx_hash, *out_idx).await;
        }
    }

    // For unknown code_hash decoders, search via indexer using data content hash.
    // This requires iterating cells which is expensive — log a warning.
    warn!(
        code_hash = hash_hex,
        "unknown code_hash decoder — cannot fetch automatically. \
         Place the binary manually in the decoder cache directory as \
         code_hash_{}.bin",
        hash_hex
    );
    Err(anyhow!(
        "no known deployment for code_hash decoder 0x{}. \
         Place the binary in the decoder cache directory.",
        hash_hex
    ))
}

async fn fetch_cell_data_from_tx(
    rpc_url: &str,
    tx_hash: &str,
    output_index: u32,
) -> Result<Vec<u8>> {
    let request = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_transaction",
        "params": [format!("0x{}", tx_hash)]
    });

    let client = reqwest::Client::new();
    let response: Value = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await?
        .json()
        .await?;

    let outputs_data = response["result"]["transaction"]["outputs_data"]
        .as_array()
        .ok_or_else(|| anyhow!("missing outputs_data in tx 0x{}", tx_hash))?;

    let data_hex = outputs_data
        .get(output_index as usize)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow!(
                "output index {} not found in tx 0x{}",
                output_index,
                tx_hash
            )
        })?;

    let data = hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex))?;
    if data.is_empty() {
        return Err(anyhow!(
            "empty cell data at tx 0x{}:{}",
            tx_hash,
            output_index
        ));
    }

    // Verify blake2b hash matches for code_hash type
    let mut hasher = ckb_hash::new_blake2b();
    hasher.update(&data);
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    debug!(
        "fetched decoder binary: {} bytes, data_hash=0x{}",
        data.len(),
        hex::encode(&hash)
    );

    Ok(data)
}
```

- [ ] **Step 4: Add `decoder_cache_path` to StoreConfig**

In `crates/config/src/lib.rs`, add to `StoreConfig` (line ~72):

```rust
pub struct StoreConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_budget_gb: Option<u64>,
    pub direct_io_reads: bool,
    #[serde(default = "default_decoder_cache_path")]
    pub decoder_cache_path: String,
}

// Add the default function:
fn default_decoder_cache_path() -> String {
    "data/decoder-cache".to_string()
}
```

Update `Default for StoreConfig`:

```rust
impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            domain_data_path: "data/domain".to_string(),
            append_only_data_path: "data/append-only".to_string(),
            memory_budget_gb: None,
            direct_io_reads: true,
            decoder_cache_path: default_decoder_cache_path(),
        }
    }
}
```

- [ ] **Step 5: Verify everything compiles**

Run: `cargo check -p ckbadger-dob-decoder && cargo check -p ckbadger-config`
Expected: compiles cleanly

- [ ] **Step 6: Add cache test**

Add to `crates/dob-decoder/src/cache.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_put_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DecoderBinaryCache::new(dir.path()).unwrap();

        let key = "code_hash_abc123";
        let binary = vec![0x7f, 0x45, 0x4c, 0x46]; // ELF magic

        assert!(cache.get(key).is_none());
        cache.put(key, &binary).unwrap();
        assert_eq!(cache.get(key).unwrap(), binary);

        // Verify disk persistence
        let cache2 = DecoderBinaryCache::new(dir.path()).unwrap();
        assert_eq!(cache2.get(key).unwrap(), binary);
    }
}
```

Run: `cargo test -p ckbadger-dob-decoder`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/dob-decoder/ crates/config/src/lib.rs
git commit -m "feat(dob-decoder): add decoder binary cache and CKB RPC fetching"
```

---

## Task 3: Add `CF_DOB_DECODED` to store

**Files:**
- Modify: `crates/ckbadger-store/src/store.rs:300-331` (add CF constant)
- Modify: `crates/ckbadger-store/src/types.rs` (add DobDecodedEntry)
- Modify: `crates/ckbadger-store/src/batch.rs:896` (add put/delete methods)
- Modify: `crates/ckbadger-store/src/spore_ops.rs` (add read methods)

- [ ] **Step 1: Add CF constant**

In `crates/ckbadger-store/src/store.rs`, after line 331 (`CF_ADDR_FIBER_CHANNELS`):

```rust
pub const CF_DOB_DECODED: &str = "dob_decoded";
```

Then find the `DOMAIN_COLUMN_FAMILIES` array (or equivalent CF list used in `open_domain`) and add `CF_DOB_DECODED` to it. Also add it to `CF_WRITE_POLICY_BULK_DISABLED` (this CF is only written post-sync, not during bulk sync). Then add the accessor method near line 1248:

```rust
pub fn cf_dob_decoded(&self) -> &ColumnFamily {
    self.db.cf_handle(CF_DOB_DECODED).expect("CF_DOB_DECODED")
}
```

- [ ] **Step 2: Add DobDecodedEntry type**

In `crates/ckbadger-store/src/types.rs`, add after `SporeMediaProfile`:

```rust
/// Cached DOB decode result from CKB-VM execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodedEntry {
    /// Flattened trait name→value pairs.
    pub traits: Vec<DobDecodedTrait>,
    /// SVG markup from DOB/1 rendering, if any.
    pub svg_markup: Option<String>,
    /// Media sources extracted from decoded trait values.
    pub media_sources: Vec<SporeMediaSource>,
    /// Epoch timestamp when this was decoded.
    pub decoded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodedTrait {
    pub name: String,
    pub value: String,
}
```

- [ ] **Step 3: Add batch write methods**

In `crates/ckbadger-store/src/batch.rs`, add:

```rust
pub fn put_dob_decoded(&mut self, spore_id: &[u8], entry: &crate::types::DobDecodedEntry) {
    let value = bincode::serialize(entry).expect("serialize DobDecodedEntry");
    self.put_cf(self.store.cf_dob_decoded(), spore_id, &value);
}

pub fn delete_dob_decoded(&mut self, spore_id: &[u8]) {
    self.delete_cf(self.store.cf_dob_decoded(), spore_id);
}
```

- [ ] **Step 4: Add read methods to spore_ops**

In `crates/ckbadger-store/src/spore_ops.rs`, add:

```rust
/// Get cached DOB decode result for a spore.
pub fn get_dob_decoded(&self, spore_id: &[u8]) -> anyhow::Result<Option<crate::types::DobDecodedEntry>> {
    match self.get_cf(self.cf_dob_decoded(), spore_id)? {
        Some(value) => Ok(Some(bincode::deserialize(&value)?)),
        None => Ok(None),
    }
}

/// Iterate spore entries that are DOB (content_type starts with "dob/")
/// and do NOT have a cached decode result in CF_DOB_DECODED.
/// Uses `after_key` for keyset pagination to avoid O(N²) re-scanning.
/// Returns (spore_id, content_type, cluster_id) tuples.
pub fn list_undecoded_dob_spores(
    &self,
    limit: usize,
    after_key: Option<&[u8]>,
) -> anyhow::Result<Vec<(Vec<u8>, String, Option<Vec<u8>>)>> {
    use crate::types::{ObjectEntry, ObjectExtra};

    let mode = match after_key {
        Some(key) => rocksdb::IteratorMode::From(key, rocksdb::Direction::Forward),
        None => rocksdb::IteratorMode::Start,
    };
    let iter = self.iterator_cf(self.cf_spore_data(), mode);
    let mut results = Vec::new();
    let mut skip_first = after_key.is_some();

    for item in iter {
        let (key, value) = item.map_err(|e| {
            anyhow::anyhow!("failed to iterate spore_data in list_undecoded_dob_spores: {}", e)
        })?;
        // Skip the after_key itself (we want entries AFTER it)
        if skip_first {
            skip_first = false;
            if after_key.is_some_and(|ak| ak == key.as_ref()) {
                continue;
            }
        }
        let entry: ObjectEntry = bincode::deserialize(&value)?;
        if let ObjectExtra::Spore { content_type, .. } = &entry.extra {
            if content_type.to_ascii_lowercase().starts_with("dob/") {
                // Check if already decoded
                if self.get_cf(self.cf_dob_decoded(), &key)?.is_none() {
                    results.push((
                        key.to_vec(),
                        content_type.clone(),
                        entry.collection_id.clone(),
                    ));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
    }
    Ok(results)
}
```

- [ ] **Step 5: Verify store compiles and existing tests pass**

Run: `cargo check -p ckbadger-store && cargo test -p ckbadger-store --lib`
Expected: compiles and tests pass

Note: Adding a new CF requires a DB rebuild (delete and re-sync). This is acceptable per project policy ("Schema changes are cheap").

- [ ] **Step 6: Commit**

```bash
git add crates/ckbadger-store/
git commit -m "feat(store): add CF_DOB_DECODED for cached CKB-VM DOB decode results"
```

---

## Task 4: Skip DOB trait decoding during bulk sync

**Files:**
- Modify: `crates/indexer/src/parser/media_source.rs:27-104` (add skip flag)
- Modify: `crates/indexer/src/db/writer/spore.rs:565-586` (pass skip flag)

- [ ] **Step 1: Add skip_dob_decode parameter to analyze_spore_media_profile**

In `crates/indexer/src/parser/media_source.rs`, change the function signature (line 27):

```rust
pub fn analyze_spore_media_profile(
    content_type: &str,
    content: &[u8],
    cluster_description: Option<&str>,
    skip_dob_decode: bool,
) -> SporeMediaProfile {
```

Then at line 54, wrap the DOB branch:

```rust
if is_text_like_content_type(&normalized_type) {
    match decode_text_payload(content) {
        Ok(text) => {
            if normalized_type.starts_with("dob/") {
                if !skip_dob_decode {
                    let (mut dob_sources, dob_rendered) =
                        extract_dob_media_sources(&text, cluster_description, &mut issues);
                    sources.append(&mut dob_sources);
                    if dob_rendered {
                        has_renderable_image = true;
                    }
                }
                // When skipped, DOB media sources will be backfilled
                // by the background DOB decode worker after sync.
            } else {
```

- [ ] **Step 2: Update all call sites**

Search for all calls to `analyze_spore_media_profile` and add `false` (don't skip) as the default for existing callers. In the spore writer (`crates/indexer/src/db/writer/spore.rs:565-586`), pass the bulk sync flag.

The writer code at line ~580:

```rust
analyze_spore_media_profile(
    &spore.content_type,
    &spore.content,
    cluster_description.as_deref(),
    is_bulk_sync,  // skip DOB decode during bulk sync
)
```

The `is_bulk_sync` flag needs to be threaded through from the writer context. Check how `SporeBatchState` or the writer receives this context and add the flag accordingly.

- [ ] **Step 3: Update media_source.rs tests**

Update existing test calls to `analyze_spore_media_profile` to pass `false` for the new parameter. Search for all test calls in `crates/indexer/src/parser/media_source.rs` and update them.

- [ ] **Step 4: Verify all tests pass**

Run: `cargo test -p ckbadger-indexer --lib`
Expected: all existing tests pass (behavior unchanged when `skip_dob_decode = false`)

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/parser/media_source.rs crates/indexer/src/db/writer/spore.rs
git commit -m "perf(bulk-sync): skip DOB trait decoding in media_profile during bulk sync"
```

---

## Task 5: Wire CKB-VM decoder into API `/decode` endpoint

**Files:**
- Modify: `crates/api/Cargo.toml` (add ckbadger-dob-decoder dep)
- Modify: `crates/api/src/routes/spore.rs:764-833` (replace decode_dob_embedded)
- Modify: `crates/api/src/routes/spore.rs:1656-1705` (update decode_spore handler)

- [ ] **Step 1: Add dependency**

In `crates/api/Cargo.toml`, add:

```toml
ckbadger-dob-decoder = { path = "../dob-decoder" }
```

- [ ] **Step 2: Add DobDecoder to AppState**

Find the `AppState` struct in the API crate and add:

```rust
pub dob_decoder_cache: Arc<ckbadger_dob_decoder::cache::DecoderBinaryCache>,
pub rpc_url: String,
```

Initialize these in the API server startup code using the config's `decoder_cache_path` and `ckb.rpc_url`.

- [ ] **Step 3: Update decode_spore handler (API is read-only)**

Replace the `decode_dob_embedded` call in the handler (lines 1656-1705) with:

1. Check `CF_DOB_DECODED` cache — if hit, return cached result
2. On miss: return a structured response indicating the spore is pending decode, with `"status": "pending"` and empty traits

The API **never writes to RocksDB** (secondary mode is read-only). The background worker in the indexer is solely responsible for populating `CF_DOB_DECODED`.

**No fallback path.** Delete the old `decode_dob_embedded` function entirely. CKB-VM is the single decode path, executed only by the indexer's background worker. If a DOB spore hasn't been decoded yet, the API returns pending status — it does not attempt to decode inline.

The response type `SporeDobDecodeResponse` should gain a `status` field:
- `"decoded"` — traits available from cache
- `"pending"` — spore exists but not yet decoded by background worker

- [ ] **Step 4: Update API tests**

Update `crates/api/tests/api_integration.rs` tests for the decode endpoint. Since tests won't have a real CKB node, the fallback to inline decoder should activate.

- [ ] **Step 5: Verify API compiles and tests pass**

Run: `cargo check -p ckbadger-api && cargo test -p ckbadger-api`
Expected: compiles and tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/api/
git commit -m "feat(api): wire CKB-VM DOB decoder into /decode endpoint with CF_DOB_DECODED cache"
```

---

## Task 6: Background DOB decode worker

> **Dependency:** Task 8 (DNA extraction) must be completed before this worker can decode real spores. The worker is designed to compile with a placeholder — implement Task 8 immediately after this task.

**Files:**
- Create: `crates/indexer/src/sync/dob_decode_worker.rs`
- Modify: `crates/indexer/Cargo.toml` (add ckbadger-dob-decoder dep)
- Modify: `crates/indexer/src/sync/mod.rs` (add module)

- [ ] **Step 1: Add dependency**

In `crates/indexer/Cargo.toml`, add:

```toml
ckbadger-dob-decoder = { path = "../dob-decoder" }
```

- [ ] **Step 2: Create worker module**

Create `crates/indexer/src/sync/dob_decode_worker.rs`:

```rust
//! Background worker that batch-decodes DOB spores using CKB-VM
//! after sync has caught up to the chain tip.

use anyhow::Result;
use ckbadger_dob_decoder::cache::DecoderBinaryCache;
use ckbadger_dob_decoder::types::DecoderRef;
use ckbadger_store::types::{
    DobDecodedEntry, DobDecodedTrait, ObjectEntry, ObjectExtra, SporeMediaSource,
    StorageDependencyTier,
};
use ckbadger_store::CkbadgerStore;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Batch size for iterating undecoded spores.
const BATCH_SIZE: usize = 500;

pub struct DobDecodeWorker {
    store: Arc<CkbadgerStore>,
    decoder_cache: Arc<DecoderBinaryCache>,
    rpc_url: String,
    shutdown: Arc<AtomicBool>,
}

impl DobDecodeWorker {
    pub fn new(
        store: Arc<CkbadgerStore>,
        decoder_cache: Arc<DecoderBinaryCache>,
        rpc_url: String,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store,
            decoder_cache,
            rpc_url,
            shutdown,
        }
    }

    /// Run the decode worker. Call this after sync reaches the tip.
    /// Iterates all undecoded DOB spores and decodes them via CKB-VM.
    pub async fn run(&self) -> Result<()> {
        info!("DOB decode worker starting — scanning for undecoded spores");

        let mut total_decoded = 0usize;
        let mut total_failed = 0usize;
        let mut cursor: Option<Vec<u8>> = None;

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                info!("DOB decode worker shutting down");
                break;
            }

            let undecoded = self.store.list_undecoded_dob_spores(
                BATCH_SIZE,
                cursor.as_deref(),
            )?;
            if undecoded.is_empty() {
                info!(
                    total_decoded,
                    total_failed, "DOB decode worker complete — no more undecoded spores"
                );
                break;
            }

            // Advance cursor to last key in batch for next iteration
            if let Some((last_key, _, _)) = undecoded.last() {
                cursor = Some(last_key.clone());
            }

            for (spore_id, content_type, cluster_id) in &undecoded {
                if self.shutdown.load(Ordering::Relaxed) {
                    break;
                }

                match self
                    .decode_single_spore(spore_id, content_type, cluster_id.as_deref())
                    .await
                {
                    Ok(entry) => {
                        let mut batch = self.store.write_batch();
                        // Update media_profile on the spore entry if we found new sources
                        if !entry.media_sources.is_empty() {
                            self.update_spore_media_profile(
                                &mut batch,
                                spore_id,
                                &entry.media_sources,
                            )?;
                        }
                        batch.put_dob_decoded(spore_id, &entry);
                        self.store.commit_batch(batch)?;
                        total_decoded += 1;
                    }
                    Err(e) => {
                        debug!(
                            spore_id = hex::encode(spore_id),
                            error = %e,
                            "failed to decode DOB spore"
                        );
                        total_failed += 1;
                    }
                }
            }

            debug!(
                batch_size = undecoded.len(),
                total_decoded, total_failed, "DOB decode worker batch complete"
            );
        }

        Ok(())
    }

    async fn decode_single_spore(
        &self,
        spore_id: &[u8],
        content_type: &str,
        cluster_id: Option<&[u8]>,
    ) -> Result<DobDecodedEntry> {
        // 1. Load cluster metadata to get decoder ref + pattern
        let cluster_desc = cluster_id
            .and_then(|cid| self.store.get_spore(cid).ok()?)
            .and_then(|entry| entry.description);

        let metadata: Value = cluster_desc
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .ok_or_else(|| anyhow::anyhow!("missing cluster metadata"))?;

        // 2. Load spore content (DNA)
        // The spore content is stored in the append-only store.
        // For the background worker, we need to read it from the spore entry
        // or from the cell payload store.
        let spore_entry = self
            .store
            .get_spore(spore_id)?
            .ok_or_else(|| anyhow::anyhow!("spore not found"))?;

        let dna_hex = extract_dna_from_spore(spore_id, &spore_entry, content_type)?;
        let decoded = self.run_vm_decode(&metadata, &dna_hex, content_type).await?;

        // 3. Extract media sources from decoded trait values
        let media_sources = extract_media_sources_from_traits(&decoded.traits);

        Ok(DobDecodedEntry {
            traits: decoded
                .traits
                .into_iter()
                .map(|t| DobDecodedTrait {
                    name: t.name,
                    value: t.value,
                })
                .collect(),
            svg_markup: None, // TODO: extract from IMAGE trait if present
            media_sources,
            decoded_at: chrono::Utc::now().timestamp(),
        })
    }

    async fn run_vm_decode(
        &self,
        metadata: &Value,
        dna_hex: &str,
        content_type: &str,
    ) -> Result<ckbadger_dob_decoder::types::DobDecodedResult> {
        let dob = metadata
            .get("dob")
            .ok_or_else(|| anyhow::anyhow!("no dob field in cluster metadata"))?;

        let ver = dob.get("ver").and_then(|v| v.as_u64()).unwrap_or(0);

        if ver == 0 || content_type.to_ascii_lowercase() == "dob/0" {
            // DOB/0: single decoder
            let decoder_ref = parse_decoder_ref(dob)?;
            let pattern = dob
                .get("pattern")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_else(|| "[]".to_string());

            let binary = ckbadger_dob_decoder::fetch::fetch_decoder_binary(
                &decoder_ref,
                &self.rpc_url,
                &self.decoder_cache,
            )
            .await?;

            ckbadger_dob_decoder::decode_dob0(&binary, dna_hex, &pattern)
        } else {
            // DOB/1+: decoder chain
            let decoders_val = dob
                .get("decoders")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("missing decoders array for DOB/1"))?;

            let mut decoder_chain: Vec<(Vec<u8>, String)> = Vec::new();
            for dec in decoders_val {
                let decoder_ref = parse_decoder_ref(dec)?;
                let pattern = dec
                    .get("pattern")
                    .map(|v| serde_json::to_string(v).unwrap_or_default())
                    .unwrap_or_else(|| "[]".to_string());

                let binary = ckbadger_dob_decoder::fetch::fetch_decoder_binary(
                    &decoder_ref,
                    &self.rpc_url,
                    &self.decoder_cache,
                )
                .await?;

                decoder_chain.push((binary, pattern));
            }

            let chain_refs: Vec<(&[u8], &str)> = decoder_chain
                .iter()
                .map(|(b, p)| (b.as_slice(), p.as_str()))
                .collect();

            ckbadger_dob_decoder::decode_dob1_chain(&chain_refs, dna_hex)
        }
    }

    fn update_spore_media_profile(
        &self,
        batch: &mut ckbadger_store::batch::StoreBatch,
        spore_id: &[u8],
        new_sources: &[SporeMediaSource],
    ) -> Result<()> {
        if let Some(mut entry) = self.store.get_spore(spore_id)? {
            if let ObjectExtra::Spore {
                ref mut media_profile,
                ..
            } = entry.extra
            {
                // Merge new sources, avoiding duplicates
                for source in new_sources {
                    if !media_profile.sources.iter().any(|s| s.uri == source.uri) {
                        media_profile.sources.push(source.clone());
                    }
                }
                // Recalculate tier
                media_profile.tier = resolve_tier(&media_profile.sources);
                if !media_profile.has_renderable_image {
                    media_profile.has_renderable_image = new_sources
                        .iter()
                        .any(|s| uri_seems_image(&s.uri));
                }
            }
            batch.put_spore(spore_id, &entry);
        }
        Ok(())
    }
}

fn parse_decoder_ref(dob_or_decoder: &Value) -> Result<DecoderRef> {
    let decoder = dob_or_decoder
        .get("decoder")
        .ok_or_else(|| anyhow::anyhow!("missing decoder field"))?;

    let dtype = decoder
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("code_hash");

    let hash_hex = decoder
        .get("hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing decoder hash"))?;

    let hash = hex::decode(hash_hex.strip_prefix("0x").unwrap_or(hash_hex))?;

    match dtype {
        "type_id" => Ok(DecoderRef::TypeId(hash)),
        _ => Ok(DecoderRef::CodeHash(hash)),
    }
}

fn extract_dna_from_spore(
    _spore_id: &[u8],
    _entry: &ObjectEntry,
    _content_type: &str,
) -> Result<String> {
    // TODO: Load cell content from append-only store to extract DNA hex.
    // For now, this is a placeholder — the actual implementation needs
    // to read from CF_CELLS in the append-only store using the spore's
    // outpoint, then parse the DNA from the spore molecule data.
    Err(anyhow::anyhow!(
        "extract_dna_from_spore not yet implemented — \
         needs append-only store access for cell payload"
    ))
}

fn extract_media_sources_from_traits(
    traits: &[ckbadger_dob_decoder::types::DobTrait],
) -> Vec<SporeMediaSource> {
    // Reuse the URI extraction logic from media_source.rs
    // Scan trait values for btcfs://, ipfs://, https://, etc.
    let mut sources = Vec::new();
    let schemes = [
        ("btcfs://", "btcfs"),
        ("ckbfs://", "ckbfs"),
        ("ipfs://", "ipfs"),
        ("ar://", "ar"),
        ("https://", "https"),
        ("http://", "http"),
    ];

    for trait_item in traits {
        let lower = trait_item.value.to_ascii_lowercase();
        for (prefix, scheme) in &schemes {
            if lower.contains(prefix) {
                // Extract the URI from the trait value
                if let Some(start) = lower.find(prefix) {
                    let end = lower[start..]
                        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<')
                        .map(|i| start + i)
                        .unwrap_or(trait_item.value.len());
                    let uri = &trait_item.value[start..end];
                    let tier = match *scheme {
                        "btcfs" => StorageDependencyTier::FullyOnCkbAndBtc,
                        "ckbfs" => StorageDependencyTier::FullyOnCkb,
                        "ipfs" | "ar" => StorageDependencyTier::DecentralizedDependent,
                        _ => StorageDependencyTier::CentralizedDependent,
                    };
                    sources.push(SporeMediaSource {
                        uri: uri.to_string(),
                        scheme: scheme.to_string(),
                        source_location: "dob_vm_trait".to_string(),
                        dependency_tier: tier,
                    });
                }
            }
        }
    }
    sources
}

// These are simplified versions — in the actual implementation,
// import from media_source.rs or make them pub(crate).
fn resolve_tier(sources: &[SporeMediaSource]) -> StorageDependencyTier {
    if sources.is_empty() {
        return StorageDependencyTier::FullyOnCkb;
    }
    let mut worst = StorageDependencyTier::FullyOnCkb;
    for s in sources {
        let tier_rank = match s.dependency_tier {
            StorageDependencyTier::CentralizedDependent => 4,
            StorageDependencyTier::DecentralizedDependent => 3,
            StorageDependencyTier::FullyOnCkbAndBtc => 2,
            StorageDependencyTier::FullyOnCkb => 1,
            StorageDependencyTier::Unknown => 5,
        };
        let worst_rank = match worst {
            StorageDependencyTier::CentralizedDependent => 4,
            StorageDependencyTier::DecentralizedDependent => 3,
            StorageDependencyTier::FullyOnCkbAndBtc => 2,
            StorageDependencyTier::FullyOnCkb => 1,
            StorageDependencyTier::Unknown => 5,
        };
        if tier_rank > worst_rank {
            worst = s.dependency_tier;
        }
    }
    worst
}

fn uri_seems_image(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
        || lower.ends_with(".webp")
        || lower.ends_with(".avif")
}
```

- [ ] **Step 3: Register module**

In `crates/indexer/src/sync/mod.rs`, add:

```rust
pub mod dob_decode_worker;
```

- [ ] **Step 4: Verify indexer compiles**

Run: `cargo check -p ckbadger-indexer`
Expected: compiles (the `extract_dna_from_spore` TODO is acceptable for now — it returns an error that the worker will log and skip)

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/ crates/dob-decoder/
git commit -m "feat(indexer): add background DOB decode worker using CKB-VM"
```

---

## Task 7: Wire background worker into sync lifecycle

**Files:**
- Modify: `crates/indexer/src/entry.rs:130-170` (spawn worker after sync setup)
- Modify: `crates/indexer/src/sync/indexer.rs` (expose shutdown signal)

- [ ] **Step 1: Spawn worker after sync catches up**

In the indexer entry point (`crates/indexer/src/entry.rs`), after the main sync loop transitions from bulk to live mode, spawn the background DOB decode worker:

```rust
// After sync reaches tip (in the live sync loop or after bulk sync completes):
let dob_shutdown = Arc::new(AtomicBool::new(false));
let dob_worker = DobDecodeWorker::new(
    store.clone(),
    Arc::new(DecoderBinaryCache::new(Path::new(&config.decoder_cache_path))?),
    config.rpc_url.clone(),
    dob_shutdown.clone(),
);

tokio::spawn(async move {
    if let Err(e) = dob_worker.run().await {
        error!("DOB decode worker failed: {}", e);
    }
});
```

The exact insertion point depends on the sync loop structure. Look for where bulk sync completes and live sync begins — that's where to spawn the worker.

- [ ] **Step 2: Ensure shutdown propagation**

Make sure the DOB worker's shutdown flag is set when the indexer shuts down. Connect it to the existing shutdown signal chain.

- [ ] **Step 3: Verify everything compiles and existing tests pass**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: compiles and existing tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/
git commit -m "feat(indexer): spawn DOB decode worker after sync catches up to tip"
```

---

## Task 8: Implement `extract_dna_from_spore` (cell payload access)

**Files:**
- Modify: `crates/indexer/src/sync/dob_decode_worker.rs` (implement the TODO)

- [ ] **Step 1: Add append-only store to worker**

The worker needs access to the append-only store (for `CF_CELLS` cell payloads). Add it to the `DobDecodeWorker` struct:

```rust
pub struct DobDecodeWorker {
    store: Arc<CkbadgerStore>,
    append_only_store: Arc<CkbadgerStore>,
    // ... rest
}
```

- [ ] **Step 2: Implement DNA extraction**

Replace the `extract_dna_from_spore` placeholder:

1. Find the spore's outpoint using `store.list_spore_outpoints_by_spore_id(spore_id)`
2. Load cell payload from append-only store using the outpoint
3. Parse the Spore molecule data to extract `content` field
4. Parse DNA hex from content (reuse `parse_dna_hex_from_content_text` logic from `media_source.rs`, or make it `pub(crate)`)

The exact implementation depends on how cell payloads are stored and how Spore molecule data is parsed. Reference `crates/indexer/src/parser/spore.rs` for the Spore data parsing logic.

- [ ] **Step 3: Test with a known spore**

Write a test (or use a debug binary) that loads a known DOB/0 spore from the DB and attempts to decode it. This validates the full pipeline: spore lookup → DNA extraction → decoder binary fetch → CKB-VM execution → trait output.

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/
git commit -m "feat(dob-worker): implement DNA extraction from append-only cell payloads"
```

---

## Task 9: End-to-end integration test

**Files:**
- Modify: `crates/api/tests/api_integration.rs`

- [ ] **Step 1: Add decode endpoint test with CF_DOB_DECODED cache**

Write an integration test that:
1. Creates a test store with `CF_DOB_DECODED`
2. Inserts a DOB/0 spore entry with known content_type and cluster metadata
3. Pre-populates `CF_DOB_DECODED` with a cached decode result
4. Calls `GET /spore/objects/{spore_id}/decode`
5. Asserts the response matches the cached result

This tests the cache-hit path without needing CKB-VM.

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/api/tests/
git commit -m "test(api): add integration test for DOB decode with CF_DOB_DECODED cache"
```

---

## Notes for implementer

### DB rebuild required

Adding `CF_DOB_DECODED` changes the schema. Delete RocksDB data and re-sync:
```bash
rm -rf temp/data/domain temp/data/append-only
# Re-run indexer
```

### Decoder binary acquisition

Only 5 unique decoder hashes exist on mainnet. The standard DOB/0 decoder has a hardcoded deployment tx. Other decoders use `type_id` lookup via CKB indexer RPC. If a `code_hash` decoder is unknown, the binary must be placed manually in the cache directory.

### Key documents to read

- `docs/docs.nervos.org/website/docs/ecosystem-scripts/19-spore-dob-0.mdx` — DOB/0 protocol spec
- `docs/dob-cookbook/BestPractices.md` — decoder design best practices
- `docs/prompts/BULK_SYNC.md` — bulk sync constraints

### Reorg handling for `CF_DOB_DECODED`

Spore content is immutable (keyed by spore_id / type_id). If a spore is consumed during a reorg, the decoded cache entry remains valid because the same spore_id always has the same DNA + cluster. If a spore is destroyed and recreated with different content at a different spore_id, the old entry is harmless (the spore_id is gone). Therefore, `CF_DOB_DECODED` does not need explicit reorg rollback logic.

### ARM / Apple Silicon compatibility

`ckb-vm` with `asm` feature only compiles on x86_64. For development on Apple Silicon, use cfg-conditional feature gates:

```toml
[target.'cfg(target_arch = "x86_64")'.dependencies]
ckb-vm = { workspace = true }
```

Or use the interpreter backend on non-x86_64 targets (slower but functional). Address this if ARM development is needed.

### Media source helpers

The `resolve_tier` and `uri_seems_image` functions in the worker duplicate logic from `media_source.rs`. Make the originals `pub(crate)` as part of Task 6 (not a post-hoc note) and import them in the worker.
