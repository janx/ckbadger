//! Activity builder: derives per-owner position changes from parsed block data.
//!
//! Produces `TxActions` — one per transaction — containing protocol actions,
//! type/lock calls, and per-participant deltas (CKB, items, tags).

use anyhow::{anyhow, bail, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use ckbadger_store::types::{
    ItemDelta, LockCallEntry, ParticipantDelta, ProtocolAction, TxActions, TypeCallEntry,
    ITEM_KIND_IDENTITY, ITEM_KIND_OBJECT, ITEM_KIND_TOKEN, TAG_CELLBASE, TAG_DAO, TAG_IDENTITY,
    TAG_LOCK_CALL, TAG_OBJECT, TAG_PROTOCOL, TAG_TOKEN, TAG_TYPE_CALL,
};

use crate::parser::udt::UdtParser;

mod bundled_udt {
    pub const EXTRA_UDT_CODE_HASHES: &[u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/bundled_udt_script_code_hashes.json"
    ));
}

static CODE_HASHES: OnceLock<CodeHashes> = OnceLock::new();

fn code_hashes() -> &'static CodeHashes {
    CODE_HASHES.get_or_init(CodeHashes::new)
}

/// Asset kind for single-lookup classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    Udt,
    Dao,
    SporeDid,
    Spore,
    Cluster,
    MnftToken,
    Dotbit,
}

/// Pre-computed code hashes for asset detection via HashMap lookup.
struct CodeHashes {
    type_lookup: HashMap<Vec<u8>, AssetKind>,
    standard_locks: HashSet<Vec<u8>>,
}

impl CodeHashes {
    fn new() -> Self {
        use crate::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
        use crate::rpc::parse_hex_to_bytes;

        // Build the type-script → AssetKind lookup from the bundled protocol
        // registry (a network-agnostic union of mainnet + testnet code_hashes),
        // replacing the previous hardcoded parser-const list. This is what lets
        // testnet assets (e.g. testnet sUDT / mNFT token) classify in activities.
        //
        // Coverage is intentionally identical to the old const map: only the
        // asset-bearing protocols below receive an AssetKind. Every other
        // registry protocol — mNFT issuer/class, all locks, and the
        // fiber/stablepp/utxoswap scripts — is skipped via the `_` arm, exactly
        // as the old `entries` array omitted them. (Stable++/ccBTC style
        // xudt_compatible assets are still picked up below via EXTRA_UDT.)
        let mut type_lookup: HashMap<Vec<u8>, AssetKind> = HashMap::new();
        for (code_hash, protocol) in PROTOCOL_REGISTRY.iter() {
            let kind = match protocol {
                ProtocolScript::Sudt | ProtocolScript::Xudt => AssetKind::Udt,
                ProtocolScript::Dao => AssetKind::Dao,
                ProtocolScript::SporeDid => AssetKind::SporeDid,
                ProtocolScript::SporeNft => AssetKind::Spore,
                ProtocolScript::Cluster => AssetKind::Cluster,
                ProtocolScript::MnftToken => AssetKind::MnftToken,
                ProtocolScript::DotbitAccount => AssetKind::Dotbit,
                _ => continue,
            };
            type_lookup.insert(code_hash.clone(), kind);
        }

        // Extend with xudt_compatible scripts from bundled script labels (decoderType "udt").
        let extra: Vec<String> = serde_json::from_slice(bundled_udt::EXTRA_UDT_CODE_HASHES)
            .expect("bundled UDT script code hashes JSON is invalid — build.rs bug");
        for hex_str in &extra {
            let bytes = parse_hex_to_bytes(hex_str);
            type_lookup.entry(bytes).or_insert(AssetKind::Udt);
        }

        // Standard lock code_hashes (access-control only, no protocol semantics)
        let standard_lock_hashes: &[&str] = &[
            // secp256k1-blake160
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            // secp256k1-multisig (mainnet)
            "0x5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8",
            // secp256k1-multisig (mainnet legacy)
            "0xd1a9f877aed3f5e07cb9c52b61ab96d06f250ae6883cc7f0a2423db0976fc821",
            // secp256k1-multisig (testnet)
            "0x765b3ed6ae264b335d07e73ac332bf2c0f38f8d3340ed521cb447b4c42dd5f09",
            // secp256k1-multisig (v2)
            "0x36c971b8d41fbd94aabca77dc75e826729ac98447b46f91e00796155dddb0d29",
            // anyone-can-pay (mainnet)
            "0xd369597ff47f29fbc0d47d2e3775370d1250b85140c670e4718af712983a2354",
            // anyone-can-pay (testnet)
            "0x3419a1c09eb2567f6552ee7a8ecffd64155cffe0f1796e6e61ec088d740c1356",
            // anyone-can-pay (testnet deprecated)
            "0x86a1c6987a4acbe1a887cca4c9dd2ac9fcb07405bbeda51b861b18bbf7492c4b",
            // omni-lock v2 (mainnet)
            "0x9b819793a64463aed77c615d6cb226eea5487ccfc0783043a587254cda2b6f26",
            // omni-lock v1 (mainnet)
            "0xa4398768d87bd17aea1361edc3accd6a0117774dc4ebc813bfa173e8ac0d086d",
            // omni-lock v2 (testnet)
            "0xf329effd1c475a2978453c8600e1eaf0bc2087ee093c3ee64cc96ec6847752cb",
            // omni-lock v1 (testnet)
            "0x79f90bb5e892d80dd213439eeab551120eb417678824f282b4ffb5f21bad2e1e",
            // PW-lock (mainnet)
            "0xbf43c3602455798c1a61a596e0d95278864c552fafe231c063b3fabf97a8febc",
            // PW-lock (testnet)
            "0x58c5f491aba6d61678b7cf7edf4910b1f5e00ec0cde2f42e0abb4fd9aff25a63",
            // JoyID (mainnet)
            "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            // JoyID (testnet)
            "0xd23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac",
        ];

        let standard_locks: HashSet<Vec<u8>> = standard_lock_hashes
            .iter()
            .map(|hex| parse_hex_to_bytes(hex))
            .collect();

        Self {
            type_lookup,
            standard_locks,
        }
    }

    fn classify(&self, code_hash: &[u8]) -> Option<AssetKind> {
        self.type_lookup.get(code_hash).copied()
    }

    fn is_standard_lock(&self, code_hash: &[u8]) -> bool {
        self.standard_locks.contains(code_hash)
    }
}

/// Input cell info needed for activity building.
#[derive(Clone, Copy)]
pub struct InputCellView<'a> {
    pub lock_script_hash: &'a [u8],
    pub lock_code_hash: &'a [u8],
    pub lock_hash_type: i16,
    pub lock_args: &'a [u8],
    pub capacity: i64,
    pub occupied_capacity: i64,
    pub type_code_hash: Option<&'a [u8]>,
    pub type_hash_type: Option<i16>,
    pub type_script_hash: Option<&'a [u8]>,
    pub type_args: Option<&'a [u8]>,
    pub udt_amount: Option<u128>,
    pub data: &'a [u8],
    pub is_dao_withdraw_request: bool,
    pub dao_compensation: Option<i64>,
}

/// Output cell info needed for activity building (borrowed from facts or ParsedCell).
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct OutputCellView<'a> {
    pub capacity: i64,
    pub lock_code_hash: &'a [u8],
    pub lock_hash_type: i16,
    pub lock_args: &'a [u8],
    pub lock_script_hash: &'a [u8],
    pub type_code_hash: Option<&'a [u8]>,
    pub type_hash_type: Option<i16>,
    pub type_args: Option<&'a [u8]>,
    pub type_script_hash: Option<&'a [u8]>,
    pub data_hash: &'a [u8],
    pub data_size: i32,
    pub data: &'a [u8],
}

/// Transaction data needed for activity building.
pub struct TxView<'a> {
    pub tx_hash: &'a [u8],
    pub block_hash: &'a [u8],
    pub tx_index: i32,
    pub block_number: i64,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub inputs: Vec<InputCellView<'a>>,
    pub outputs: Vec<OutputCellView<'a>>,
}

/// Detects protocol-level actions by analyzing cross-layer signals.
pub(crate) trait ProtocolDetector: Send + Sync {
    /// Batch-level pre-filter: returns false if no code_hash in the entire batch
    /// matches this detector. Called once per batch (not per TX).
    /// Default implementation returns true (opt-in optimization).
    fn might_apply_batch(
        &self,
        _lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
        _type_code_hashes: &std::collections::HashSet<[u8; 32]>,
    ) -> bool {
        true // default: don't skip
    }

    /// Fast pre-filter: returns false if this detector definitely has no match for the tx.
    /// Called once per tx (not per owner). Implementations should check lock/type code_hashes
    /// without allocating or building data structures.
    fn might_apply(&self, tx: &TxView<'_>) -> bool;

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        accum: &OwnerAccum<'_>,
        item_deltas: &[ItemDelta],
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction>;
}

/// Build `TxActions` for all transactions in a block (no protocol detectors).
#[cfg(test)]
pub fn build_tx_actions_for_block_no_detectors(txs: &[TxView<'_>]) -> Result<Vec<TxActions>> {
    build_tx_actions_for_block(txs, &[])
}

/// Build `TxActions` for all transactions in a block with protocol detectors.
pub fn build_tx_actions_for_block(
    txs: &[TxView<'_>],
    detectors: &[Box<dyn ProtocolDetector>],
) -> Result<Vec<TxActions>> {
    let hashes = code_hashes();
    txs.iter()
        .map(|tx| build_tx_actions(tx, hashes, detectors))
        .collect()
}

/// Accumulator for per-owner position within one transaction.
#[derive(Default)]
pub(crate) struct OwnerAccum<'a> {
    pub(crate) lock_code_hash: Option<&'a [u8]>,
    pub(crate) lock_hash_type: Option<i16>,
    pub(crate) lock_args: Option<&'a [u8]>,
    pub(crate) input_capacity: i128,
    pub(crate) output_capacity: i128,
    pub(crate) input_used: i64,
    pub(crate) output_used: i64,
    /// UDT: type_script_hash -> (input_amount, output_amount). u128 amounts (per the
    /// sUDT/xUDT standard); the signed net delta is derived at emit time.
    pub(crate) udt_deltas: HashMap<&'a [u8], (u128, u128)>,
    /// DAO deposits (output cells with DAO type and data == 0x00..00)
    pub(crate) dao_deposits: Vec<i64>,
    /// DAO withdraw requests (output cells with DAO type and non-zero deposit block)
    pub(crate) dao_withdraw_requests: Vec<(i64, i64)>,
    /// DAO withdraw completes (input cells with DAO withdraw request consumed without DAO output)
    /// Each entry is (capacity, compensation).
    pub(crate) dao_withdraw_completes: Vec<(i64, i64)>,
    /// Spore/DOB IDs seen as inputs
    pub(crate) spore_inputs: Vec<&'a [u8]>,
    /// Spore/DOB IDs seen as outputs
    pub(crate) spore_outputs: Vec<&'a [u8]>,
    /// mNFT IDs seen as inputs
    pub(crate) nft_inputs: Vec<&'a [u8]>,
    /// mNFT IDs seen as outputs
    pub(crate) nft_outputs: Vec<&'a [u8]>,
    /// DotBit IDs seen as inputs
    pub(crate) dotbit_inputs: Vec<Vec<u8>>,
    /// DotBit IDs seen as outputs
    pub(crate) dotbit_outputs: Vec<Vec<u8>>,
    /// did:ckb IDs seen as inputs
    pub(crate) did_ckb_inputs: Vec<&'a [u8]>,
    /// did:ckb IDs seen as outputs
    pub(crate) did_ckb_outputs: Vec<&'a [u8]>,
    /// Distinct script code_hashes involved (lock + type)
    pub(crate) involved_scripts: BTreeSet<&'a [u8]>,
    /// Whether any cell for this owner has a type script
    pub(crate) has_type_script: bool,
    /// Unrecognized type script instances keyed by (code_hash, hash_type, args)
    pub(crate) unrecognized_type_calls: BTreeSet<(&'a [u8], i16, &'a [u8])>,
    /// Non-standard lock scripts seen on output cells in this tx, keyed by (code_hash, hash_type, args)
    pub(crate) unrecognized_lock_calls: BTreeSet<(&'a [u8], i16, &'a [u8])>,
}

const DOTBIT_TYPE_ARGS_LEN: usize = 20;
const DOTBIT_DATA_HASH_PREFIX_LEN: usize = 32;

fn record_owner_lock_script<'a>(
    accum: &mut OwnerAccum<'a>,
    lock_code_hash: &'a [u8],
    lock_hash_type: i16,
    lock_args: &'a [u8],
) -> Result<()> {
    match (accum.lock_code_hash, accum.lock_hash_type, accum.lock_args) {
        (Some(existing_code_hash), Some(existing_hash_type), Some(existing_args)) => {
            if existing_code_hash != lock_code_hash {
                bail!(
                    "owner lock_code_hash mismatch for same lock hash: existing=0x{}, new=0x{}",
                    hex::encode(existing_code_hash),
                    hex::encode(lock_code_hash)
                );
            }
            if existing_hash_type != lock_hash_type {
                bail!(
                    "owner lock_hash_type mismatch for same lock hash: existing={}, new={}, lock_code_hash=0x{}",
                    existing_hash_type,
                    lock_hash_type,
                    hex::encode(lock_code_hash)
                );
            }
            if existing_args != lock_args {
                bail!(
                    "owner lock_args mismatch for same lock hash: existing_len={}, new_len={}, lock_code_hash=0x{}",
                    existing_args.len(),
                    lock_args.len(),
                    hex::encode(lock_code_hash)
                );
            }
        }
        (None, None, None) => {
            accum.lock_code_hash = Some(lock_code_hash);
            accum.lock_hash_type = Some(lock_hash_type);
            accum.lock_args = Some(lock_args);
        }
        _ => bail!(
            "owner lock script state partially initialized: code_hash={}, hash_type={}, args={}",
            accum.lock_code_hash.is_some(),
            accum.lock_hash_type.is_some(),
            accum.lock_args.is_some()
        ),
    }
    Ok(())
}

fn build_tx_actions<'a>(
    tx: &TxView<'a>,
    hashes: &CodeHashes,
    detectors: &[Box<dyn ProtocolDetector>],
) -> Result<TxActions> {
    let mut owners: HashMap<&'a [u8], OwnerAccum<'a>> = HashMap::new();

    // Process inputs — lock_script_hash must always be exactly 32 bytes
    for (input_idx, input) in tx.inputs.iter().enumerate() {
        if input.lock_script_hash.len() != 32 {
            bail!(
                "input lock_script_hash has invalid length: len={}, expected=32, \
                 tx_hash={}, input_idx={}, block_number={}",
                input.lock_script_hash.len(),
                hex::encode(tx.tx_hash),
                input_idx,
                tx.block_number,
            );
        }
        let accum = owners.entry(input.lock_script_hash).or_default();
        record_owner_lock_script(
            accum,
            input.lock_code_hash,
            input.lock_hash_type,
            input.lock_args,
        )?;
        accum.involved_scripts.insert(input.lock_code_hash);
        accum.input_capacity += input.capacity as i128;
        accum.input_used += input.occupied_capacity;

        if let Some(type_code_hash) = input.type_code_hash {
            accum.involved_scripts.insert(type_code_hash);
            classify_input(
                accum,
                type_code_hash,
                input.type_hash_type,
                input.type_script_hash,
                input.type_args,
                input.udt_amount,
                input.data,
                input.is_dao_withdraw_request,
                input.dao_compensation,
                hashes,
                input.capacity,
            )?;
        }
    }

    // Process outputs
    for cell in &tx.outputs {
        let accum = owners.entry(cell.lock_script_hash).or_default();
        record_owner_lock_script(
            accum,
            cell.lock_code_hash,
            cell.lock_hash_type,
            cell.lock_args,
        )?;
        accum.involved_scripts.insert(cell.lock_code_hash);
        accum.output_capacity += cell.capacity as i128;

        // Compute occupied for output
        let type_args_len = if cell.type_code_hash.is_some() {
            Some(cell.type_args.map(|a| a.len()).unwrap_or(0))
        } else {
            None
        };
        let occupied = super::cells::compute_occupied_capacity_shannons(
            cell.data_size as usize,
            cell.lock_args.len(),
            type_args_len,
        )?;
        accum.output_used += occupied;

        if let Some(type_code_hash) = cell.type_code_hash {
            accum.involved_scripts.insert(type_code_hash);
            classify_output(
                accum,
                type_code_hash,
                cell.type_hash_type,
                cell.type_script_hash,
                cell.type_args,
                cell.data,
                hashes,
                cell.capacity,
            )?;
        }
    }

    // Detect non-standard output locks and record as lock_calls for sending owners
    for cell in &tx.outputs {
        if !hashes.is_standard_lock(cell.lock_code_hash) {
            // Record on all owners who are sending CKB (input_capacity > 0)
            // and who are NOT the output cell's owner
            for (lock_hash, accum) in owners.iter_mut() {
                if *lock_hash != cell.lock_script_hash && accum.input_capacity > 0 {
                    accum.unrecognized_lock_calls.insert((
                        cell.lock_code_hash,
                        cell.lock_hash_type,
                        cell.lock_args,
                    ));
                }
            }
        }
    }

    let mut owner_hashes: Vec<&[u8]> = owners.keys().copied().collect();
    owner_hashes.sort();

    // Pre-filter detectors once per tx (not per owner)
    let applicable_detectors: Vec<&dyn ProtocolDetector> = detectors
        .iter()
        .filter(|d| d.might_apply(tx))
        .map(|d| d.as_ref())
        .collect();

    // --- Phase 1: Per-owner item deltas and DAO protocol actions ---
    let mut all_protocol_actions: Vec<ProtocolAction> = Vec::new();
    let mut tx_type_calls: BTreeSet<(&[u8], i16, &[u8])> = BTreeSet::new();
    let mut tx_lock_calls: BTreeSet<(&[u8], i16, &[u8])> = BTreeSet::new();
    let mut participants = Vec::with_capacity(owner_hashes.len());

    for lock_hash in &owner_hashes {
        let accum = owners
            .get(lock_hash)
            .expect("owner hash collected from owners map must exist");
        let ckb_delta = accum.output_capacity - accum.input_capacity;
        let used_delta = accum.output_used - accum.input_used;

        // Build item deltas
        let mut item_deltas = Vec::new();

        // UDT changes → ItemDelta (token). Net-difference (u128, no intermediate overflow).
        for (type_script_hash, (input_amt, output_amt)) in &accum.udt_deltas {
            if output_amt == input_amt {
                continue;
            }
            let (magnitude, negative) = if output_amt >= input_amt {
                (output_amt - input_amt, false)
            } else {
                (input_amt - output_amt, true)
            };
            item_deltas.push(ItemDelta {
                item_id: type_script_hash.to_vec(),
                kind: ITEM_KIND_TOKEN,
                magnitude,
                negative,
            });
        }

        // Spore/DOB changes → ItemDelta (object, +1/-1)
        emit_object_item_deltas(&accum.spore_inputs, &accum.spore_outputs, &mut item_deltas);

        // mNFT changes → ItemDelta (object, +1/-1)
        emit_object_item_deltas(&accum.nft_inputs, &accum.nft_outputs, &mut item_deltas);

        // DotBit changes → ItemDelta (identity, +1/-1)
        emit_identity_item_deltas(
            &accum.dotbit_inputs,
            &accum.dotbit_outputs,
            &mut item_deltas,
        );

        // did:ckb changes → ItemDelta (identity, +1/-1)
        emit_identity_item_deltas(
            &accum.did_ckb_inputs,
            &accum.did_ckb_outputs,
            &mut item_deltas,
        );

        // DAO → ProtocolAction (collected at TX level)
        let has_dao = !accum.dao_deposits.is_empty()
            || !accum.dao_withdraw_requests.is_empty()
            || !accum.dao_withdraw_completes.is_empty();

        for capacity in &accum.dao_deposits {
            all_protocol_actions.push(ProtocolAction::new(
                "dao",
                "deposit",
                serde_json::json!({
                    "capacity": *capacity,
                    "lockHash": hex::encode(lock_hash),
                }),
            ));
        }

        for (capacity, deposit_block) in &accum.dao_withdraw_requests {
            all_protocol_actions.push(ProtocolAction::new(
                "dao",
                "withdraw_request",
                serde_json::json!({
                    "capacity": *capacity,
                    "depositBlock": *deposit_block,
                    "lockHash": hex::encode(lock_hash),
                }),
            ));
        }

        for (capacity, compensation) in &accum.dao_withdraw_completes {
            all_protocol_actions.push(ProtocolAction::new(
                "dao",
                "withdraw_complete",
                serde_json::json!({
                    "capacity": *capacity,
                    "compensation": *compensation,
                    "lockHash": hex::encode(lock_hash),
                }),
            ));
        }

        // Collect type_calls and lock_calls for TX-level dedup
        for entry in &accum.unrecognized_type_calls {
            tx_type_calls.insert(*entry);
        }
        for entry in &accum.unrecognized_lock_calls {
            tx_lock_calls.insert(*entry);
        }

        // Per-owner type/lock call refs for detector compatibility
        let owner_type_calls: Vec<TypeCallEntry> = accum
            .unrecognized_type_calls
            .iter()
            .map(|(code_hash, hash_type, args)| TypeCallEntry {
                type_code_hash: code_hash.to_vec(),
                type_hash_type: *hash_type,
                type_args: args.to_vec(),
            })
            .collect();

        let owner_lock_calls: Vec<LockCallEntry> = accum
            .unrecognized_lock_calls
            .iter()
            .map(|(code_hash, hash_type, args)| LockCallEntry {
                lock_code_hash: code_hash.to_vec(),
                lock_hash_type: *hash_type,
                lock_args: args.to_vec(),
            })
            .collect();

        // Run detectors per-owner, collect at TX level
        let detector_actions: Vec<ProtocolAction> = applicable_detectors
            .iter()
            .flat_map(|d| {
                d.detect(
                    tx,
                    lock_hash,
                    accum,
                    &item_deltas,
                    &owner_type_calls,
                    &owner_lock_calls,
                )
            })
            .collect();
        all_protocol_actions.extend(detector_actions);

        // Compute tags bitmask
        let mut tags: u16 = 0;
        if item_deltas.iter().any(|d| d.kind == ITEM_KIND_TOKEN) {
            tags |= TAG_TOKEN;
        }
        if item_deltas.iter().any(|d| d.kind == ITEM_KIND_OBJECT) {
            tags |= TAG_OBJECT;
        }
        if item_deltas.iter().any(|d| d.kind == ITEM_KIND_IDENTITY) {
            tags |= TAG_IDENTITY;
        }
        if has_dao {
            tags |= TAG_DAO;
        }
        if tx.is_cellbase {
            tags |= TAG_CELLBASE;
        }
        if !accum.unrecognized_type_calls.is_empty() {
            tags |= TAG_TYPE_CALL;
        }
        if !accum.unrecognized_lock_calls.is_empty() {
            tags |= TAG_LOCK_CALL;
        }

        participants.push(ParticipantDelta {
            lock_hash: lock_hash.to_vec(),
            ckb_delta,
            used_delta,
            item_deltas,
            tags,
        });
    }

    // Deduplicate protocol actions by (protocol, action, metadata)
    dedup_protocol_actions(&mut all_protocol_actions);

    // If any protocol_actions exist, set TAG_PROTOCOL on all participants
    if !all_protocol_actions.is_empty() {
        for p in &mut participants {
            p.tags |= TAG_PROTOCOL;
        }
    }

    // Build TX-level deduped type_calls and lock_calls
    let type_calls: Vec<TypeCallEntry> = tx_type_calls
        .iter()
        .map(|(code_hash, hash_type, args)| TypeCallEntry {
            type_code_hash: code_hash.to_vec(),
            type_hash_type: *hash_type,
            type_args: args.to_vec(),
        })
        .collect();

    let lock_calls: Vec<LockCallEntry> = tx_lock_calls
        .iter()
        .map(|(code_hash, hash_type, args)| LockCallEntry {
            lock_code_hash: code_hash.to_vec(),
            lock_hash_type: *hash_type,
            lock_args: args.to_vec(),
        })
        .collect();

    Ok(TxActions {
        tx_hash: tx.tx_hash.to_vec(),
        block_hash: tx.block_hash.to_vec(),
        block_number: tx.block_number,
        tx_index: tx.tx_index,
        timestamp: tx.timestamp,
        is_cellbase: tx.is_cellbase,
        protocol_actions: all_protocol_actions,
        type_calls,
        lock_calls,
        participants,
    })
}

/// Deduplicate protocol actions by (protocol, action, metadata_raw).
fn dedup_protocol_actions(actions: &mut Vec<ProtocolAction>) {
    let mut seen = HashSet::new();
    actions.retain(|a| {
        let key = (
            a.protocol.clone(),
            a.action.clone(),
            a.metadata.raw().to_string(),
        );
        seen.insert(key)
    });
}

fn classify_input<'a>(
    accum: &mut OwnerAccum<'a>,
    type_code_hash: &'a [u8],
    type_hash_type: Option<i16>,
    type_script_hash: Option<&'a [u8]>,
    type_args: Option<&'a [u8]>,
    udt_amount: Option<u128>,
    data: &'a [u8],
    is_dao_withdraw_request: bool,
    dao_compensation: Option<i64>,
    hashes: &CodeHashes,
    capacity: i64,
) -> Result<()> {
    accum.has_type_script = true;
    match hashes.classify(type_code_hash) {
        Some(AssetKind::Udt) => {
            if let Some(tsh) = type_script_hash {
                if let Some(amount) = udt_amount {
                    let entry = accum.udt_deltas.entry(tsh).or_insert((0, 0));
                    entry.0 = entry.0.checked_add(amount).ok_or_else(|| {
                        anyhow::anyhow!(
                            "udt input delta overflow: type_hash=0x{}",
                            hex::encode(tsh)
                        )
                    })?;
                }
            }
        }
        Some(AssetKind::Dao) => {
            if is_dao_withdraw_request {
                let compensation = dao_compensation.ok_or_else(|| {
                    anyhow!(
                        "DAO withdraw-complete input has is_dao_withdraw_request=true but \
                         dao_compensation is None (pre-computation failed or missing)"
                    )
                })?;
                accum.dao_withdraw_completes.push((capacity, compensation));
            }
        }
        Some(AssetKind::SporeDid) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.did_ckb_inputs.push(args);
                }
            }
        }
        Some(AssetKind::Spore | AssetKind::Cluster) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.spore_inputs.push(args);
                }
            }
        }
        Some(AssetKind::MnftToken) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.nft_inputs.push(args);
                }
            }
        }
        Some(AssetKind::Dotbit) => {
            if let Some(account_id) = resolve_dotbit_account_id(type_args, data) {
                accum.dotbit_inputs.push(account_id);
            }
        }
        None => {
            record_script_call(accum, type_code_hash, type_hash_type, type_args)?;
        }
    }
    Ok(())
}

fn classify_output<'a>(
    accum: &mut OwnerAccum<'a>,
    type_code_hash: &'a [u8],
    type_hash_type: Option<i16>,
    type_script_hash: Option<&'a [u8]>,
    type_args: Option<&'a [u8]>,
    cell_data: &'a [u8],
    hashes: &CodeHashes,
    capacity: i64,
) -> Result<()> {
    accum.has_type_script = true;
    match hashes.classify(type_code_hash) {
        Some(AssetKind::Udt) => {
            if let Some(tsh) = type_script_hash {
                if let Some(amount) = UdtParser::parse_amount(cell_data) {
                    let entry = accum.udt_deltas.entry(tsh).or_insert((0, 0));
                    entry.1 = entry.1.checked_add(amount).ok_or_else(|| {
                        anyhow::anyhow!(
                            "udt output delta overflow: type_hash=0x{}",
                            hex::encode(tsh)
                        )
                    })?;
                }
            }
        }
        Some(AssetKind::Dao) => {
            if cell_data.len() == 8 {
                let bytes: [u8; 8] = cell_data[..8].try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "failed to decode DAO output data while classifying activity: len={}",
                        cell_data.len()
                    )
                })?;
                let deposit_block = u64::from_le_bytes(bytes);
                if deposit_block == 0 {
                    accum.dao_deposits.push(capacity);
                } else {
                    let deposit_block_i64 = i64::try_from(deposit_block).map_err(|_| {
                        anyhow!(
                            "DAO deposit block number exceeds i64 range: deposit_block={}",
                            deposit_block
                        )
                    })?;
                    accum
                        .dao_withdraw_requests
                        .push((capacity, deposit_block_i64));
                }
            }
        }
        Some(AssetKind::SporeDid) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.did_ckb_outputs.push(args);
                }
            }
        }
        Some(AssetKind::Spore | AssetKind::Cluster) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.spore_outputs.push(args);
                }
            }
        }
        Some(AssetKind::MnftToken) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.nft_outputs.push(args);
                }
            }
        }
        Some(AssetKind::Dotbit) => {
            if let Some(account_id) = resolve_dotbit_account_id(type_args, cell_data) {
                accum.dotbit_outputs.push(account_id);
            }
        }
        None => {
            record_script_call(accum, type_code_hash, type_hash_type, type_args)?;
        }
    }
    Ok(())
}

fn record_script_call<'a>(
    accum: &mut OwnerAccum<'a>,
    type_code_hash: &'a [u8],
    type_hash_type: Option<i16>,
    type_args: Option<&'a [u8]>,
) -> Result<()> {
    let hash_type = type_hash_type.ok_or_else(|| {
        anyhow::anyhow!(
            "missing type_hash_type while recording script call: type_code_hash=0x{}",
            hex::encode(type_code_hash)
        )
    })?;
    let args = type_args.ok_or_else(|| {
        anyhow::anyhow!(
            "missing type_args while recording script call: type_code_hash=0x{}",
            hex::encode(type_code_hash)
        )
    })?;
    accum
        .unrecognized_type_calls
        .insert((type_code_hash, hash_type, args));
    Ok(())
}

fn resolve_dotbit_account_id(type_args: Option<&[u8]>, cell_data: &[u8]) -> Option<Vec<u8>> {
    if let Some(args) = type_args {
        // Normal case: .bit account_id comes from type args.
        if args.len() == DOTBIT_TYPE_ARGS_LEN && !args.iter().all(|&b| b == 0) {
            return Some(args.to_vec());
        }
        if !args.is_empty() {
            return Some(args.to_vec());
        }
    }

    // Compatibility path for old .bit layouts: account_id in cell data.
    // Accepts both full cell data (≥52 bytes: 32-byte prefix + 20-byte id)
    // and a pre-resolved account_id (exactly 20 bytes, from DB lookup for
    // consumed inputs whose raw cell data is unavailable).
    let min_len = DOTBIT_DATA_HASH_PREFIX_LEN + DOTBIT_TYPE_ARGS_LEN;
    let account_id = if cell_data.len() >= min_len {
        &cell_data[DOTBIT_DATA_HASH_PREFIX_LEN..DOTBIT_DATA_HASH_PREFIX_LEN + DOTBIT_TYPE_ARGS_LEN]
    } else if cell_data.len() == DOTBIT_TYPE_ARGS_LEN {
        cell_data
    } else {
        return None;
    };
    if account_id.iter().all(|&b| b == 0) {
        return None;
    }
    Some(account_id.to_vec())
}

/// Emit object ItemDeltas by comparing input vs output ID sets.
///
/// - Output-only IDs → delta +1 (arrived)
/// - Input-only IDs → delta -1 (departed)
/// - IDs in both → skipped (same owner, data change is Layer 3)
fn emit_object_item_deltas<T: AsRef<[u8]>>(
    inputs: &[T],
    outputs: &[T],
    item_deltas: &mut Vec<ItemDelta>,
) {
    // Output-only → +1
    for id in outputs {
        let id = id.as_ref();
        if !inputs.iter().any(|i| i.as_ref() == id) {
            item_deltas.push(ItemDelta {
                item_id: id.to_vec(),
                kind: ITEM_KIND_OBJECT,
                magnitude: 1,
                negative: false,
            });
        }
    }
    // Input-only → -1
    for id in inputs {
        let id = id.as_ref();
        if !outputs.iter().any(|o| o.as_ref() == id) {
            item_deltas.push(ItemDelta {
                item_id: id.to_vec(),
                kind: ITEM_KIND_OBJECT,
                magnitude: 1,
                negative: true,
            });
        }
    }
}

/// Emit identity ItemDeltas by comparing input vs output ID sets.
///
/// Same +1/-1 logic as objects.
fn emit_identity_item_deltas<T: AsRef<[u8]>>(
    inputs: &[T],
    outputs: &[T],
    item_deltas: &mut Vec<ItemDelta>,
) {
    // Output-only → +1
    for id in outputs {
        let id = id.as_ref();
        if !inputs.iter().any(|i| i.as_ref() == id) {
            item_deltas.push(ItemDelta {
                item_id: id.to_vec(),
                kind: ITEM_KIND_IDENTITY,
                magnitude: 1,
                negative: false,
            });
        }
    }
    // Input-only → -1
    for id in inputs {
        let id = id.as_ref();
        if !outputs.iter().any(|o| o.as_ref() == id) {
            item_deltas.push(ItemDelta {
                item_id: id.to_vec(),
                kind: ITEM_KIND_IDENTITY,
                magnitude: 1,
                negative: true,
            });
        }
    }
}

// Tests rewritten for TxActions/ItemDelta model.
#[cfg(test)]
#[allow(clippy::useless_vec)]
mod tests {
    use super::*;

    /// Owned data for constructing test OutputCellView instances.
    struct OwnedOutput {
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_hash_type: Option<i16>,
        type_script_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        data: Vec<u8>,
        capacity: i64,
    }

    impl OwnedOutput {
        fn view(&self) -> OutputCellView<'_> {
            OutputCellView {
                capacity: self.capacity,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                lock_script_hash: &self.lock_script_hash,
                type_code_hash: self.type_code_hash.as_deref(),
                type_hash_type: self.type_hash_type,
                type_args: self.type_args.as_deref(),
                type_script_hash: self.type_script_hash.as_deref(),
                data_hash: &[],
                data_size: self.data.len() as i32,
                data: &self.data,
            }
        }
    }

    fn make_output(
        lock_hash_byte: u8,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        data: Vec<u8>,
    ) -> OwnedOutput {
        OwnedOutput {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash: vec![0x11; 32],
            lock_args: vec![0x22; 20],
            type_code_hash,
            type_hash_type: None,
            type_script_hash,
            type_args,
            data,
            capacity,
        }
    }

    /// Owned data for constructing test InputCellView instances.
    struct OwnedInput {
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        udt_amount: Option<u128>,
        data: Vec<u8>,
        capacity: i64,
        occupied_capacity: i64,
        type_hash_type: Option<i16>,
        is_dao_withdraw_request: bool,
        dao_compensation: Option<i64>,
    }

    impl OwnedInput {
        fn view(&self) -> InputCellView<'_> {
            InputCellView {
                lock_script_hash: &self.lock_script_hash,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                capacity: self.capacity,
                occupied_capacity: self.occupied_capacity,
                type_code_hash: self.type_code_hash.as_deref(),
                type_hash_type: self.type_hash_type,
                type_script_hash: self.type_script_hash.as_deref(),
                type_args: self.type_args.as_deref(),
                udt_amount: self.udt_amount,
                data: &self.data,
                is_dao_withdraw_request: self.is_dao_withdraw_request,
                dao_compensation: self.dao_compensation,
            }
        }
    }

    fn make_input(lock_hash_byte: u8, capacity: i64, occupied: i64) -> OwnedInput {
        OwnedInput {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash: vec![0x11; 32],
            lock_args: vec![0x22; 20],
            type_code_hash: None,
            type_script_hash: None,
            type_args: None,
            udt_amount: None,
            data: vec![],
            capacity,
            occupied_capacity: occupied,
            type_hash_type: None,
            is_dao_withdraw_request: false,
            dao_compensation: None,
        }
    }

    /// Helper: find participant by lock_hash byte pattern in a TxActions.
    fn find_participant(actions: &TxActions, lock_byte: u8) -> &ParticipantDelta {
        actions
            .participants
            .iter()
            .find(|p| p.lock_hash == vec![lock_byte; 32])
            .unwrap_or_else(|| panic!("participant 0x{:02x} not found", lock_byte))
    }

    #[test]
    fn test_build_tx_actions_preserves_participant_lock_hash() {
        let owner = 0xAA;
        let input = make_input(owner, 100_00000000, 61_00000000);

        let tx = TxView {
            tx_hash: &[0x21; 32],
            block_hash: &[0xA1; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: vec![],
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        assert_eq!(actions_list[0].participants.len(), 1);
        assert_eq!(actions_list[0].participants[0].lock_hash, vec![owner; 32]);
    }

    #[test]
    fn test_build_tx_actions_sorts_participants_by_lock_hash() {
        let alice = 0xAA;
        let bob = 0xBB;
        let outputs = vec![
            make_output(alice, 100_00000000, None, None, None, vec![]),
            make_output(bob, 100_00000000, None, None, None, vec![]),
        ];

        let bob_input = make_input(bob, 200_00000000, 61_00000000);
        let tx = TxView {
            tx_hash: &[0x22; 32],
            block_hash: &[0xA2; 32],
            tx_index: 1,
            block_number: 1001,
            timestamp: 1_700_000_010,
            is_cellbase: false,
            inputs: vec![bob_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        let hashes: Vec<Vec<u8>> = actions_list[0]
            .participants
            .iter()
            .map(|p| p.lock_hash.clone())
            .collect();
        assert_eq!(hashes, vec![vec![alice; 32], vec![bob; 32]]);
    }

    #[test]
    fn test_simple_ckb_transfer() {
        // Alice sends 100 CKB to Bob
        let alice = 0xAA;
        let bob = 0xBB;

        let outputs = vec![
            make_output(bob, 100_00000000, None, None, None, vec![]),
            make_output(alice, 200_00000000, None, None, None, vec![]),
        ];

        let alice_input_owned = make_input(alice, 300_00000000, 61_00000000);
        let tx = TxView {
            tx_hash: &[0x01; 32],
            block_hash: &[0xA1; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input_owned.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];
        assert_eq!(actions.participants.len(), 2);

        let alice_p = find_participant(actions, alice);
        assert_eq!(alice_p.ckb_delta, -100_00000000);
        assert!(!actions.is_cellbase);

        let bob_p = find_participant(actions, bob);
        assert_eq!(bob_p.ckb_delta, 100_00000000);
    }

    #[test]
    fn test_cellbase_reward() {
        let miner = 0xCC;
        let outputs = vec![make_output(miner, 5000_00000000, None, None, None, vec![])];

        let tx = TxView {
            tx_hash: &[0x02; 32],
            block_hash: &[0xA2; 32],
            tx_index: 0,
            block_number: 500,
            timestamp: 1_700_000_000,
            is_cellbase: true,
            inputs: vec![],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];
        assert!(actions.is_cellbase);
        assert_eq!(actions.participants.len(), 1);
        let miner_p = find_participant(actions, miner);
        assert_eq!(miner_p.ckb_delta, 5000_00000000);
        assert!(miner_p.tags & TAG_CELLBASE != 0);
    }

    #[test]
    fn test_used_delta_computed() {
        let alice = 0xAA;
        let outputs = vec![make_output(
            alice,
            100_00000000,
            None,
            None,
            None,
            vec![0u8; 100], // 100 bytes of data
        )];

        let alice_input_owned = make_input(alice, 100_00000000, 61_00000000);
        let tx = TxView {
            tx_hash: &[0x03; 32],
            block_hash: &[0xA3; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input_owned.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        let alice_p = find_participant(&actions_list[0], alice);
        assert_eq!(alice_p.ckb_delta, 0);
        // Output occupied = (8 + (32+1+20) + 0 + 100) * 100_000_000 = 16_100_000_000
        // used_delta = 16_100_000_000 - 6_100_000_000 = 10_000_000_000
        assert_eq!(alice_p.used_delta, 100_00000000);
    }

    #[test]
    fn test_three_party_participants() {
        let alice = 0xAA;
        let bob = 0xBB;
        let carol = 0xCC;

        let outputs = vec![
            make_output(bob, 100_00000000, None, None, None, vec![]),
            make_output(carol, 100_00000000, None, None, None, vec![]),
            make_output(alice, 100_00000000, None, None, None, vec![]),
        ];

        let alice_input_owned = make_input(alice, 300_00000000, 61_00000000);
        let tx = TxView {
            tx_hash: &[0x04; 32],
            block_hash: &[0xA4; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input_owned.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        assert_eq!(actions_list[0].participants.len(), 3);
    }

    #[test]
    fn test_multiple_txs_in_block() {
        let alice = 0xAA;
        let bob = 0xBB;

        let outputs1 = vec![make_output(alice, 500_00000000, None, None, None, vec![])];

        let tx1 = TxView {
            tx_hash: &[0x01; 32],
            block_hash: &[0xB1; 32],
            tx_index: 0,
            block_number: 100,
            timestamp: 1_700_000_000,
            is_cellbase: true,
            inputs: vec![],
            outputs: outputs1.iter().map(|o| o.view()).collect(),
        };

        let outputs2 = vec![make_output(bob, 200_00000000, None, None, None, vec![])];

        let alice_input_owned = make_input(alice, 200_00000000, 61_00000000);
        let tx2 = TxView {
            tx_hash: &[0x02; 32],
            block_hash: &[0xB1; 32],
            tx_index: 1,
            block_number: 100,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input_owned.view()],
            outputs: outputs2.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx1, tx2]).unwrap();
        // One TxActions per transaction
        assert_eq!(actions_list.len(), 2);
        // First tx: cellbase with alice
        assert_eq!(actions_list[0].participants.len(), 1);
        // Second tx: alice + bob
        assert_eq!(actions_list[1].participants.len(), 2);
    }

    #[test]
    fn test_udt_token_transfer() {
        let alice = 0xAA;
        let bob = 0xBB;
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let type_script_hash = vec![0xDD; 32];

        let mut alice_input = make_input(alice, 200_00000000, 61_00000000);
        alice_input.type_code_hash = Some(sudt_code_hash.clone());
        alice_input.type_script_hash = Some(type_script_hash.clone());
        alice_input.udt_amount = Some(5000);

        let outputs = vec![
            make_output(
                bob,
                142_00000000,
                Some(sudt_code_hash.clone()),
                Some(type_script_hash.clone()),
                Some(vec![0xEE; 20]),
                1000u128.to_le_bytes().to_vec(),
            ),
            make_output(
                alice,
                58_00000000,
                Some(sudt_code_hash),
                Some(type_script_hash.clone()),
                Some(vec![0xEE; 20]),
                4000u128.to_le_bytes().to_vec(),
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x05; 32],
            block_hash: &[0xA5; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];

        let alice_p = find_participant(actions, alice);
        assert!(alice_p.tags & TAG_TOKEN != 0);
        let alice_token = alice_p
            .item_deltas
            .iter()
            .find(|d| d.kind == ITEM_KIND_TOKEN)
            .expect("alice should have token item delta");
        assert_eq!(alice_token.magnitude, 1000);
        assert!(alice_token.negative);
        assert_eq!(alice_token.item_id, type_script_hash);

        let bob_p = find_participant(actions, bob);
        let bob_token = bob_p
            .item_deltas
            .iter()
            .find(|d| d.kind == ITEM_KIND_TOKEN)
            .expect("bob should have token item delta");
        assert_eq!(bob_token.magnitude, 1000);
        assert!(!bob_token.negative);
    }

    #[test]
    fn udt_item_delta_above_i128_max_is_not_wrapped() {
        // Regression: an activity-feed token delta for a valid sUDT amount > i128::MAX must
        // not wrap. On-chain: block 4743232 sUDT amount 0x00…704ea6403c0ca7 (LE) = 2.22e38.
        // Under the old `delta: i128` / `amount as i128` code this stored a wrapped-negative
        // delta (~-1.18e38); now it must be magnitude = big, negative = false.
        let big: u128 = 222_044_604_925_031_325_468_940_491_728_862_838_784;
        let bob = 0xBB;
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let type_script_hash = vec![0xDD; 32];

        // Plain (non-UDT) funding input; a single sUDT mint output of `big` to bob.
        let alice_input = make_input(0xAA, 300_00000000, 61_00000000);
        let outputs = vec![make_output(
            bob,
            142_00000000,
            Some(sudt_code_hash),
            Some(type_script_hash.clone()),
            Some(vec![0xEE; 20]),
            big.to_le_bytes().to_vec(),
        )];

        let tx = TxView {
            tx_hash: &[0x06; 32],
            block_hash: &[0xA6; 32],
            tx_index: 1,
            block_number: 4_743_232,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];
        let bob_p = find_participant(actions, bob);
        let bob_token = bob_p
            .item_deltas
            .iter()
            .find(|d| d.kind == ITEM_KIND_TOKEN)
            .expect("bob should have token item delta");
        assert_eq!(bob_token.magnitude, big);
        assert!(!bob_token.negative);
    }

    #[test]
    fn test_udt_token_transfer_uses_prefetched_input_amount_when_input_data_is_empty() {
        let alice = 0xAA;
        let bob = 0xBB;
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let type_script_hash = vec![0xDD; 32];

        let mut alice_input = make_input(alice, 200_00000000, 61_00000000);
        alice_input.type_code_hash = Some(sudt_code_hash.clone());
        alice_input.type_script_hash = Some(type_script_hash.clone());
        alice_input.udt_amount = Some(5000);
        // Real sync path does not populate input raw data.
        alice_input.data = vec![];

        let outputs = vec![
            make_output(
                bob,
                142_00000000,
                Some(sudt_code_hash.clone()),
                Some(type_script_hash.clone()),
                Some(vec![0xEE; 20]),
                1000u128.to_le_bytes().to_vec(),
            ),
            make_output(
                alice,
                58_00000000,
                Some(sudt_code_hash),
                Some(type_script_hash.clone()),
                Some(vec![0xEE; 20]),
                4000u128.to_le_bytes().to_vec(),
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x06; 32],
            block_hash: &[0xA6; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let alice_p = find_participant(&actions_list[0], alice);
        let alice_token = alice_p
            .item_deltas
            .iter()
            .find(|d| d.kind == ITEM_KIND_TOKEN)
            .expect("alice should have token item delta");
        assert_eq!(alice_token.magnitude, 1000);
        assert!(alice_token.negative);
    }

    #[test]
    fn test_no_actions_for_empty_block() {
        let actions_list = build_tx_actions_for_block_no_detectors(&[]).unwrap();
        assert!(actions_list.is_empty());
    }

    #[test]
    fn test_dotbit_output_falls_back_to_account_id_in_cell_data_when_type_args_missing() {
        let owner = 0xAA;
        let dotbit_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID);
        let account_id = vec![0x5a; 20];

        let mut dotbit_data = vec![0x00; 32];
        dotbit_data.extend_from_slice(&account_id);

        let outputs = vec![make_output(
            owner,
            100_00000000,
            Some(dotbit_code_hash),
            Some(vec![0x11; 32]),
            None,
            dotbit_data,
        )];

        let tx = TxView {
            tx_hash: &[0x07; 32],
            block_hash: &[0xA7; 32],
            tx_index: 0,
            block_number: 123,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        let owner_p = find_participant(&actions_list[0], owner);
        assert!(owner_p.tags & TAG_IDENTITY != 0);
        let identity_delta = owner_p
            .item_deltas
            .iter()
            .find(|d| d.kind == ITEM_KIND_IDENTITY)
            .expect("dotbit identity item delta should be present");
        assert_eq!(identity_delta.item_id, account_id);
        assert_eq!(identity_delta.magnitude, 1); // output-only = +1
        assert!(!identity_delta.negative);
    }

    #[test]
    fn test_did_ckb_changes_are_labeled_as_identity() {
        let owner = 0xBB;
        let did_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::spore::SPORE_CODE_HASH_MAINNET_DID);
        let did_id = vec![0x6b; 32];

        let outputs = vec![make_output(
            owner,
            100_00000000,
            Some(did_code_hash),
            Some(vec![0x22; 32]),
            Some(did_id.clone()),
            vec![0u8; 16],
        )];

        let tx = TxView {
            tx_hash: &[0x08; 32],
            block_hash: &[0xA8; 32],
            tx_index: 0,
            block_number: 124,
            timestamp: 1_700_000_100,
            is_cellbase: false,
            inputs: vec![],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        let owner_p = find_participant(&actions_list[0], owner);
        assert!(owner_p.tags & TAG_IDENTITY != 0);
        let identity_delta = owner_p
            .item_deltas
            .iter()
            .find(|d| d.kind == ITEM_KIND_IDENTITY)
            .expect("did_ckb identity item delta should be present");
        assert_eq!(identity_delta.item_id, did_id);
        assert_eq!(identity_delta.magnitude, 1); // output-only = +1
        assert!(!identity_delta.negative);
    }

    #[test]
    fn test_dao_withdraw_complete_produces_protocol_action() {
        let owner = 0xAA;
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);

        let mut dao_input = make_input(owner, 102_00000000, 102_00000000);
        dao_input.type_code_hash = Some(dao_code_hash);
        dao_input.is_dao_withdraw_request = true;
        dao_input.dao_compensation = Some(5_00000000);

        let outputs: Vec<OwnedOutput> = vec![];

        let tx = TxView {
            tx_hash: &[0x09; 32],
            block_hash: &[0xA9; 32],
            tx_index: 1,
            block_number: 200,
            timestamp: 1_700_000_200,
            is_cellbase: false,
            inputs: vec![dao_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];
        let owner_p = find_participant(actions, owner);
        assert!(owner_p.tags & TAG_DAO != 0);
        // DAO withdraw_complete is now a TX-level protocol_action
        let dao_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "dao" && a.action == "withdraw_complete")
            .expect("should have dao withdraw_complete protocol action");
        let meta = dao_action.metadata_value().unwrap();
        assert_eq!(meta["capacity"], 102_00000000i64);
        assert_eq!(meta["compensation"], 5_00000000i64);
    }

    #[test]
    fn test_unrecognized_type_script_produces_type_call() {
        let alice = 0xAA;
        let bob = 0xBB;
        let unknown_code_hash = vec![0xFF; 32];
        let alice_type_args = vec![0xAB; 20];
        let bob_type_args = vec![0xEE; 20];

        let mut alice_input = make_input(alice, 200_00000000, 61_00000000);
        alice_input.type_code_hash = Some(unknown_code_hash.clone());
        alice_input.type_script_hash = Some(vec![0xDD; 32]);
        alice_input.type_hash_type = Some(1);
        alice_input.type_args = Some(alice_type_args.clone());

        let outputs = vec![make_output(
            bob,
            200_00000000,
            Some(unknown_code_hash.clone()),
            Some(vec![0xDD; 32]),
            Some(bob_type_args.clone()),
            vec![],
        )];
        let mut outputs = outputs;
        outputs[0].type_hash_type = Some(1);

        let tx = TxView {
            tx_hash: &[0x0A; 32],
            block_hash: &[0xAA; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];

        // Type calls are now TX-level (deduplicated)
        // Both alice_type_args and bob_type_args should appear since they differ
        assert!(!actions.type_calls.is_empty());
        assert!(actions
            .type_calls
            .iter()
            .any(|tc| tc.type_code_hash == vec![0xFF; 32]));

        // Participants should have TYPE_CALL tag
        let alice_p = find_participant(actions, alice);
        assert!(alice_p.tags & TAG_TYPE_CALL != 0);
        assert!(alice_p.item_deltas.is_empty());

        let bob_p = find_participant(actions, bob);
        assert!(bob_p.tags & TAG_TYPE_CALL != 0);
        assert!(bob_p.item_deltas.is_empty());
    }

    #[test]
    fn test_pure_ckb_transfer_has_no_asset_tags() {
        let alice = 0xAA;
        let bob = 0xBB;

        let outputs = vec![
            make_output(bob, 100_00000000, None, None, None, vec![]),
            make_output(alice, 200_00000000, None, None, None, vec![]),
        ];

        let alice_input_owned = make_input(alice, 300_00000000, 61_00000000);
        let tx = TxView {
            tx_hash: &[0x0B; 32],
            block_hash: &[0xAB; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input_owned.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];
        for p in &actions.participants {
            // Pure CKB: no token/object/identity/dao/type_call
            // (lock_call may be set because test helper uses non-standard lock code_hash 0x11..11)
            let asset_mask = TAG_TOKEN | TAG_OBJECT | TAG_IDENTITY | TAG_DAO | TAG_TYPE_CALL;
            assert_eq!(p.tags & asset_mask, 0);
            assert!(p.item_deltas.is_empty());
        }
    }

    #[test]
    fn test_mixed_known_and_unknown_scripts_in_same_tx() {
        let alice = 0xAA;
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let unknown_code_hash = vec![0xFF; 32];
        let type_script_hash = vec![0xDD; 32];

        let mut udt_input = make_input(alice, 200_00000000, 61_00000000);
        udt_input.type_code_hash = Some(sudt_code_hash.clone());
        udt_input.type_script_hash = Some(type_script_hash.clone());
        udt_input.data = 5000u128.to_le_bytes().to_vec();

        let outputs = vec![
            make_output(
                alice,
                100_00000000,
                Some(sudt_code_hash),
                Some(type_script_hash),
                Some(vec![0xEE; 20]),
                3000u128.to_le_bytes().to_vec(),
            ),
            make_output(
                alice,
                100_00000000,
                Some(unknown_code_hash.clone()),
                Some(vec![0xCC; 32]),
                Some(vec![0xEE; 20]),
                vec![],
            ),
        ];
        let mut outputs = outputs;
        outputs[1].type_hash_type = Some(1);

        let tx = TxView {
            tx_hash: &[0x0C; 32],
            block_hash: &[0xAC; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![udt_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];
        let alice_p = find_participant(actions, alice);
        // Has token item delta
        assert!(alice_p.tags & TAG_TOKEN != 0);
        assert!(alice_p
            .item_deltas
            .iter()
            .any(|d| d.kind == ITEM_KIND_TOKEN));
        // Type calls at TX level
        assert_eq!(actions.type_calls.len(), 1);
        assert_eq!(actions.type_calls[0].type_code_hash, unknown_code_hash);
        assert_eq!(actions.type_calls[0].type_hash_type, 1);
        assert_eq!(actions.type_calls[0].type_args, vec![0xEE; 20]);
    }

    #[test]
    fn test_xudt_compatible_code_hash_classified_as_udt() {
        use crate::rpc::parse_hex_to_bytes;
        let hashes = CodeHashes::new();

        // Stable++ Asset (mainnet) — xudt_compatible, decoderType "udt" in script labels
        let stablepp = parse_hex_to_bytes(
            "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b",
        );
        assert_eq!(
            hashes.classify(&stablepp),
            Some(AssetKind::Udt),
            "Stable++ Asset should be classified as Udt"
        );

        // ccBTC Asset (mainnet)
        let ccbtc = parse_hex_to_bytes(
            "0x092c2c4a26ea475a8e860c29cf00502103add677705e2ccd8d6fe5af3caa5ae3",
        );
        assert_eq!(
            hashes.classify(&ccbtc),
            Some(AssetKind::Udt),
            "ccBTC Asset should be classified as Udt"
        );

        // Random unknown code_hash should still be None
        assert_eq!(hashes.classify(&[0x99; 32]), None);
    }

    #[test]
    fn test_registry_preserves_exact_old_asset_coverage() {
        // Regression guard: every code_hash the pre-registry const `entries`
        // array covered must still classify to the *exact same* AssetKind after
        // the migration to PROTOCOL_REGISTRY. If the bundled registry ever drops
        // one of these hashes (or a slug remaps), this fails loudly instead of
        // silently regressing activity classification.
        use crate::parser::dao::DAO_CODE_HASH;
        use crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID;
        use crate::parser::mnft::MNFT_TOKEN_CODE_HASH;
        use crate::parser::spore::{
            CLUSTER_CODE_HASH_MAINNET_V2, CLUSTER_CODE_HASH_TESTNET_V1,
            CLUSTER_CODE_HASH_TESTNET_V2, SPORE_CODE_HASH_MAINNET_DID, SPORE_CODE_HASH_MAINNET_V2,
            SPORE_CODE_HASH_TESTNET_V1, SPORE_CODE_HASH_TESTNET_V2,
        };
        use crate::parser::udt::{SUDT_CODE_HASH, XUDT_CODE_HASH_DATA1, XUDT_CODE_HASH_TYPE};
        use crate::rpc::parse_hex_to_bytes;

        let hashes = CodeHashes::new();
        let expected: &[(&str, AssetKind)] = &[
            (SUDT_CODE_HASH, AssetKind::Udt),
            (XUDT_CODE_HASH_DATA1, AssetKind::Udt),
            (XUDT_CODE_HASH_TYPE, AssetKind::Udt),
            (DAO_CODE_HASH, AssetKind::Dao),
            (SPORE_CODE_HASH_MAINNET_DID, AssetKind::SporeDid),
            (SPORE_CODE_HASH_MAINNET_V2, AssetKind::Spore),
            (SPORE_CODE_HASH_TESTNET_V2, AssetKind::Spore),
            (SPORE_CODE_HASH_TESTNET_V1, AssetKind::Spore),
            (CLUSTER_CODE_HASH_MAINNET_V2, AssetKind::Cluster),
            (CLUSTER_CODE_HASH_TESTNET_V2, AssetKind::Cluster),
            (CLUSTER_CODE_HASH_TESTNET_V1, AssetKind::Cluster),
            (MNFT_TOKEN_CODE_HASH, AssetKind::MnftToken),
            (DOTBIT_ACCOUNT_CELL_TYPE_ID, AssetKind::Dotbit),
        ];
        for (hex, kind) in expected {
            assert_eq!(
                hashes.classify(&parse_hex_to_bytes(hex)),
                Some(*kind),
                "old-const code_hash {hex} must still classify as {kind:?}"
            );
        }

        // Negative coverage must also hold: mNFT issuer/class and lock scripts
        // were NOT asset-classified by the old map and must stay unclassified.
        assert_eq!(
            hashes.classify(&parse_hex_to_bytes(
                crate::parser::mnft::MNFT_ISSUER_CODE_HASH
            )),
            None,
            "mNFT issuer must not be classified as an asset"
        );
        assert_eq!(
            hashes.classify(&parse_hex_to_bytes(
                crate::parser::mnft::MNFT_CLASS_CODE_HASH
            )),
            None,
            "mNFT class must not be classified as an asset"
        );
    }

    #[test]
    fn test_testnet_sudt_classifies_as_udt() {
        use crate::rpc::parse_hex_to_bytes;
        let hashes = CodeHashes::new();
        // Testnet sUDT code_hash — absent from the old mainnet-only const map,
        // now classified via the network-agnostic ProtocolRegistry.
        let testnet_sudt = parse_hex_to_bytes(
            "0xc5e5dcf215925f7ef4dfaf5f4b4f105bc321c02776d6e7d52a1db3fcd9d011a4",
        );
        assert_eq!(
            hashes.classify(&testnet_sudt),
            Some(AssetKind::Udt),
            "testnet sUDT should classify as Udt via the registry"
        );
        // Sanity: this is the testnet hash, distinct from the mainnet sUDT const.
        assert_ne!(
            testnet_sudt,
            parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH),
            "testnet sUDT must differ from mainnet sUDT"
        );
    }

    #[test]
    fn test_testnet_mnft_token_classifies_as_mnft_token() {
        use crate::rpc::parse_hex_to_bytes;
        let hashes = CodeHashes::new();
        // Testnet mNFT token code_hash — mainnet-only in the old const map,
        // now classified via the registry.
        let testnet_mnft = parse_hex_to_bytes(
            "0xb1837b5ad01a88558731953062d1f5cb547adf89ece01e8934a9f0aeed2d959f",
        );
        assert_eq!(
            hashes.classify(&testnet_mnft),
            Some(AssetKind::MnftToken),
            "testnet mNFT token should classify as MnftToken via the registry"
        );
    }

    #[test]
    fn test_xudt_compatible_produces_token_item_delta_not_type_call() {
        use crate::rpc::parse_hex_to_bytes;

        let alice = 0xAA;
        let bob = 0xBB;

        // Stable++ Asset (mainnet) code_hash
        let stablepp_code_hash = parse_hex_to_bytes(
            "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b",
        );
        let type_script_hash = vec![0x71; 32];
        let type_args = vec![0x36; 32];

        // Alice has 100 tokens, sends to Bob
        let amount: u128 = 100_00000000;

        let mut alice_input = make_input(alice, 200_00000000, 102_00000000);
        alice_input.type_code_hash = Some(stablepp_code_hash.clone());
        alice_input.type_script_hash = Some(type_script_hash.clone());
        alice_input.type_hash_type = Some(1);
        alice_input.type_args = Some(type_args.clone());
        alice_input.udt_amount = Some(amount);

        let mut bob_output = make_output(
            bob,
            200_00000000,
            Some(stablepp_code_hash),
            Some(type_script_hash),
            Some(type_args),
            amount.to_le_bytes().to_vec(),
        );
        bob_output.type_hash_type = Some(1);

        let outputs = vec![bob_output];
        let tx = TxView {
            tx_hash: &[0x0B; 32],
            block_hash: &[0xBB; 32],
            tx_index: 1,
            block_number: 2000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];

        // No type_calls at TX level (xudt is recognized, not an unknown script)
        assert!(
            actions.type_calls.is_empty(),
            "xudt_compatible should not produce type_calls"
        );

        // Alice should have a token item delta
        let alice_p = find_participant(actions, alice);
        assert!(
            alice_p
                .item_deltas
                .iter()
                .any(|d| d.kind == ITEM_KIND_TOKEN),
            "xudt_compatible should produce Token item delta"
        );
    }

    #[test]
    fn test_non_standard_output_lock_recorded_as_lock_call() {
        // Alice (standard lock) sends CKB to an output with a non-standard lock
        let alice = 0xAA;
        let non_standard_lock_code_hash = vec![0xDD; 32]; // not in standard_locks
        let non_standard_lock_args = vec![0x01, 0x02, 0x03];

        let mut output = make_output(0xCC, 50_00000000, None, None, None, vec![]);
        output.lock_code_hash = non_standard_lock_code_hash.clone();
        output.lock_args = non_standard_lock_args.clone();

        let outputs = vec![output];

        let alice_input_owned = make_input(alice, 100_00000000, 61_00000000);
        let tx = TxView {
            tx_hash: &[0x30; 32],
            block_hash: &[0xB0; 32],
            tx_index: 0,
            block_number: 2000,
            timestamp: 1_700_100_000,
            is_cellbase: false,
            inputs: vec![alice_input_owned.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];

        // Lock calls are TX-level
        assert_eq!(actions.lock_calls.len(), 1);
        assert_eq!(
            actions.lock_calls[0].lock_code_hash,
            non_standard_lock_code_hash
        );
        assert_eq!(actions.lock_calls[0].lock_hash_type, 1);
        assert_eq!(actions.lock_calls[0].lock_args, non_standard_lock_args);

        // Alice should have LOCK_CALL tag
        let alice_p = find_participant(actions, alice);
        assert!(alice_p.tags & TAG_LOCK_CALL != 0);
    }

    #[test]
    fn test_standard_lock_output_not_recorded_as_lock_call() {
        // Alice sends to Bob who uses standard secp256k1 lock — no lock_call
        let alice = 0xAA;
        let secp_code_hash = crate::rpc::parse_hex_to_bytes(
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
        );

        let mut output = make_output(0xBB, 50_00000000, None, None, None, vec![]);
        output.lock_code_hash = secp_code_hash;

        let outputs = vec![output];

        let alice_input_owned = make_input(alice, 100_00000000, 61_00000000);
        let tx = TxView {
            tx_hash: &[0x31; 32],
            block_hash: &[0xB1; 32],
            tx_index: 0,
            block_number: 2001,
            timestamp: 1_700_100_010,
            is_cellbase: false,
            inputs: vec![alice_input_owned.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions_list = build_tx_actions_for_block_no_detectors(&[tx]).unwrap();
        let actions = &actions_list[0];
        assert!(actions.lock_calls.is_empty());
    }

    // ---- RGB++ detector tests ----

    use crate::db::writer::rgbpp_detector::RgbppDetector;
    use crate::parser::rgbpp::RGBPP_LOCK_CODE_HASH_MAINNET;

    /// Build rgbpp lock args from an out_index and btc_txid hex string.
    fn make_rgbpp_lock_args(out_index: u32, btc_txid_hex: &str) -> Vec<u8> {
        let mut args = Vec::with_capacity(36);
        args.extend_from_slice(&out_index.to_le_bytes());
        let mut txid_bytes = hex::decode(btc_txid_hex).expect("valid hex for btc txid");
        txid_bytes.reverse(); // BTC txid is stored reversed
        args.extend_from_slice(&txid_bytes);
        args
    }

    fn make_input_with_lock(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> OwnedInput {
        OwnedInput {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_args,
            capacity,
            occupied_capacity: 61_00000000,
            type_code_hash,
            type_hash_type: Some(1),
            type_script_hash: None,
            type_args,
            udt_amount: None,
            data: vec![],
            is_dao_withdraw_request: false,
            dao_compensation: None,
        }
    }

    fn make_output_with_lock(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> OwnedOutput {
        OwnedOutput {
            capacity,
            lock_code_hash,
            lock_args,
            lock_script_hash: vec![lock_hash_byte; 32],
            type_code_hash,
            type_hash_type: Some(1),
            type_args,
            type_script_hash: None,
            data: vec![],
        }
    }

    #[test]
    fn test_rgbpp_leap_to_ckb() {
        // Input cell with rgbpp lock + xUDT type, output cell with standard lock + same type
        let rgbpp_owner = 0xAA; // owner with rgbpp lock
        let ckb_owner = 0xBB; // owner with standard CKB lock

        let rgbpp_code_hash = crate::rpc::parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];
        let xudt_code_hash = vec![0x77; 32]; // some type script
        let type_args = vec![0x88; 32];

        let rgbpp_args = make_rgbpp_lock_args(
            0,
            "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd",
        );

        let input = make_input_with_lock(
            rgbpp_owner,
            rgbpp_code_hash,
            rgbpp_args,
            200_00000000,
            Some(xudt_code_hash.clone()),
            Some(type_args.clone()),
        );

        let outputs = vec![make_output_with_lock(
            ckb_owner,
            standard_lock,
            vec![0x22; 20],
            200_00000000,
            Some(xudt_code_hash),
            Some(type_args),
        )];

        let tx = TxView {
            tx_hash: &[0x40; 32],
            block_hash: &[0xC0; 32],
            tx_index: 1,
            block_number: 5000,
            timestamp: 1_700_200_000,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(RgbppDetector::new())];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        // Protocol actions are TX-level; check for leap_to_ckb
        assert!(actions
            .protocol_actions
            .iter()
            .any(|a| a.protocol == "rgbpp" && a.action == "leap_to_ckb"));

        // Both participants should have TAG_PROTOCOL
        let ckb_p = find_participant(actions, ckb_owner);
        assert!(ckb_p.tags & TAG_PROTOCOL != 0);

        let rgbpp_p = find_participant(actions, rgbpp_owner);
        assert!(rgbpp_p.tags & TAG_PROTOCOL != 0);
    }

    #[test]
    fn test_rgbpp_transfer() {
        // Input and output both have rgbpp lock + same type but different BTC UTXO args.
        // Different lock_args means different lock_script_hash, so we use different owner bytes.
        let input_owner = 0xAA;
        let output_owner = 0xAB;

        let rgbpp_code_hash = crate::rpc::parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_MAINNET);
        let xudt_code_hash = vec![0x77; 32];
        let type_args = vec![0x88; 32];

        let input_args = make_rgbpp_lock_args(
            0,
            "1111111111111111111111111111111111111111111111111111111111111111",
        );
        let output_args = make_rgbpp_lock_args(
            1,
            "2222222222222222222222222222222222222222222222222222222222222222",
        );

        let input = make_input_with_lock(
            input_owner,
            rgbpp_code_hash.clone(),
            input_args,
            200_00000000,
            Some(xudt_code_hash.clone()),
            Some(type_args.clone()),
        );

        // Output with different rgbpp lock args (different BTC UTXO) = different lock_script_hash
        let outputs = vec![make_output_with_lock(
            output_owner,
            rgbpp_code_hash,
            output_args,
            200_00000000,
            Some(xudt_code_hash),
            Some(type_args),
        )];

        let tx = TxView {
            tx_hash: &[0x41; 32],
            block_hash: &[0xC1; 32],
            tx_index: 1,
            block_number: 5001,
            timestamp: 1_700_200_010,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(RgbppDetector::new())];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        // Protocol actions are TX-level; check for transfer
        let transfer_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "rgbpp" && a.action == "transfer")
            .expect("should have rgbpp transfer action");

        // Both participants should have TAG_PROTOCOL
        let input_p = find_participant(actions, input_owner);
        assert!(input_p.tags & TAG_PROTOCOL != 0);

        let output_p = find_participant(actions, output_owner);
        assert!(output_p.tags & TAG_PROTOCOL != 0);

        // Verify metadata contains btcTxid
        let metadata = transfer_action.metadata_value().unwrap();
        assert!(metadata.get("btcTxid").is_some());
        assert!(metadata.get("outIndex").is_some());
    }

    #[test]
    fn test_no_rgbpp_action_for_standard_locks() {
        // No rgbpp locks in tx — expect no protocol actions
        let alice = 0xAA;
        let bob = 0xBB;
        let standard_lock = vec![0x11; 32];
        let xudt_code_hash = vec![0x77; 32];
        let type_args = vec![0x88; 32];

        let input = make_input_with_lock(
            alice,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            Some(xudt_code_hash.clone()),
            Some(type_args.clone()),
        );

        let outputs = vec![make_output_with_lock(
            bob,
            standard_lock,
            vec![0x33; 20],
            200_00000000,
            Some(xudt_code_hash),
            Some(type_args),
        )];

        let tx = TxView {
            tx_hash: &[0x42; 32],
            block_hash: &[0xC2; 32],
            tx_index: 1,
            block_number: 5002,
            timestamp: 1_700_200_020,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(RgbppDetector::new())];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        assert!(
            actions_list[0].protocol_actions.is_empty(),
            "no rgbpp actions expected for standard-lock-only tx"
        );
    }

    #[test]
    fn test_protocol_detector_might_apply_filters_irrelevant_tx() {
        use super::super::fiber_detector::FiberDetector;
        use super::super::rgbpp_detector::RgbppDetector;
        use super::super::stablepp_detector::StableppDetector;
        use super::super::utxoswap_detector::UtxoSwapDetector;

        let plain_lock = vec![0u8; 32];
        let input_lock_hash = vec![0xAA; 32];
        let input_lock_args = vec![0x22; 20];
        let input_data: Vec<u8> = vec![];
        let output_lock_args = vec![0x33; 20];
        let output_lock_hash = vec![0xBB; 32];
        let output_data: Vec<u8> = vec![];

        // Build a TxView with plain lock code_hash on input and output (no protocol scripts)
        let input = InputCellView {
            lock_script_hash: &input_lock_hash,
            lock_code_hash: &plain_lock,
            lock_hash_type: 1,
            lock_args: &input_lock_args,
            capacity: 200_00000000,
            occupied_capacity: 61_00000000,
            type_code_hash: None,
            type_hash_type: None,
            type_script_hash: None,
            type_args: None,
            udt_amount: None,
            data: &input_data,
            is_dao_withdraw_request: false,
            dao_compensation: None,
        };

        let output = OutputCellView {
            capacity: 200_00000000,
            lock_code_hash: &plain_lock,
            lock_hash_type: 1,
            lock_args: &output_lock_args,
            lock_script_hash: &output_lock_hash,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: &[],
            data_size: 0,
            data: &output_data,
        };

        let tx = TxView {
            tx_hash: &[0x99; 32],
            block_hash: &[0xCC; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_100_000,
            is_cellbase: false,
            inputs: vec![input],
            outputs: vec![output],
        };

        let rgbpp = RgbppDetector::new();
        let fiber = FiberDetector::new(true);
        let stablepp = StableppDetector::new(true);
        let utxoswap = UtxoSwapDetector::new(true);

        assert!(
            !rgbpp.might_apply(&tx),
            "rgbpp should not apply to plain tx"
        );
        assert!(
            !fiber.might_apply(&tx),
            "fiber should not apply to plain tx"
        );
        assert!(
            !stablepp.might_apply(&tx),
            "stablepp should not apply to plain tx"
        );
        assert!(
            !utxoswap.might_apply(&tx),
            "utxoswap should not apply to plain tx"
        );
    }

    #[test]
    fn test_might_apply_batch_empty_sets_returns_false() {
        use super::super::fiber_detector::FiberDetector;
        use super::super::rgbpp_detector::RgbppDetector;
        use super::super::stablepp_detector::StableppDetector;
        use super::super::utxoswap_detector::UtxoSwapDetector;
        use std::collections::HashSet;

        let empty_locks: HashSet<[u8; 32]> = HashSet::new();
        let empty_types: HashSet<[u8; 32]> = HashSet::new();

        let rgbpp = RgbppDetector::new();
        let fiber = FiberDetector::new(true);
        let stablepp = StableppDetector::new(true);
        let utxoswap = UtxoSwapDetector::new(true);

        assert!(!rgbpp.might_apply_batch(&empty_locks, &empty_types));
        assert!(!fiber.might_apply_batch(&empty_locks, &empty_types));
        assert!(!stablepp.might_apply_batch(&empty_locks, &empty_types));
        assert!(!utxoswap.might_apply_batch(&empty_locks, &empty_types));
    }

    #[test]
    fn test_might_apply_batch_with_matching_code_hash() {
        use super::super::fiber_detector::FiberDetector;
        use super::super::rgbpp_detector::RgbppDetector;
        use super::super::stablepp_detector::StableppDetector;
        use super::super::utxoswap_detector::UtxoSwapDetector;
        use crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET;
        use crate::parser::rgbpp::RGBPP_LOCK_CODE_HASH_MAINNET;
        use crate::parser::utxoswap::INTENT_LOCK_CODE_HASH_MAINNET;
        use crate::rpc::parse_hex_to_bytes;
        use std::collections::HashSet;

        let empty_types: HashSet<[u8; 32]> = HashSet::new();

        // RgbppDetector should match when rgbpp lock code_hash is in the set
        let rgbpp_hash = parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_MAINNET);
        let mut locks: HashSet<[u8; 32]> = HashSet::new();
        let mut h = [0u8; 32];
        h.copy_from_slice(&rgbpp_hash);
        locks.insert(h);
        let rgbpp = RgbppDetector::new();
        assert!(rgbpp.might_apply_batch(&locks, &empty_types));

        // FiberDetector should match when funding lock code_hash is in the set
        let fiber_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let mut locks: HashSet<[u8; 32]> = HashSet::new();
        let mut h = [0u8; 32];
        h.copy_from_slice(&fiber_hash);
        locks.insert(h);
        let fiber = FiberDetector::new(true);
        assert!(fiber.might_apply_batch(&locks, &empty_types));

        // StableppDetector should match when the corrected vault lock code_hash is in the set
        let stablepp_hash = parse_hex_to_bytes(
            "0x4ed68fcb7eaa4ff78d46a2fad88a32ce9caffd4b96a0a4bba96ff4871f018675",
        );
        let mut locks: HashSet<[u8; 32]> = HashSet::new();
        let mut h = [0u8; 32];
        h.copy_from_slice(&stablepp_hash);
        locks.insert(h);
        let stablepp = StableppDetector::new(true);
        assert!(stablepp.might_apply_batch(&locks, &empty_types));

        // UtxoSwapDetector should match when intent lock code_hash is in the set
        let utxoswap_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let mut locks: HashSet<[u8; 32]> = HashSet::new();
        let mut h = [0u8; 32];
        h.copy_from_slice(&utxoswap_hash);
        locks.insert(h);
        let utxoswap = UtxoSwapDetector::new(true);
        assert!(utxoswap.might_apply_batch(&locks, &empty_types));
    }

    #[test]
    fn test_might_apply_batch_unrelated_code_hash_returns_false() {
        use super::super::fiber_detector::FiberDetector;
        use super::super::rgbpp_detector::RgbppDetector;
        use super::super::stablepp_detector::StableppDetector;
        use super::super::utxoswap_detector::UtxoSwapDetector;
        use std::collections::HashSet;

        // An unrelated code_hash should not trigger any detector
        let unrelated = [0xFFu8; 32];
        let mut locks: HashSet<[u8; 32]> = HashSet::new();
        locks.insert(unrelated);
        let mut types: HashSet<[u8; 32]> = HashSet::new();
        types.insert(unrelated);

        assert!(!RgbppDetector::new().might_apply_batch(&locks, &types));
        assert!(!FiberDetector::new(true).might_apply_batch(&locks, &types));
        assert!(!StableppDetector::new(true).might_apply_batch(&locks, &types));
        assert!(!UtxoSwapDetector::new(true).might_apply_batch(&locks, &types));
    }
}
