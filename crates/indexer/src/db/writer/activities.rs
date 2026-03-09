//! Activity builder: derives per-owner position changes from parsed block data.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use ckbadger_store::types::{ActivityEntry, AssetAction, AssetChange};

use crate::parser::cell::ParsedCell;
use crate::parser::udt::UdtParser;

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
    lookup: HashMap<Vec<u8>, AssetKind>,
}

impl CodeHashes {
    fn new() -> Self {
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

        let entries: &[(&str, AssetKind)] = &[
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

        let lookup = entries
            .iter()
            .map(|(hex, kind)| (parse_hex_to_bytes(hex), *kind))
            .collect();

        Self { lookup }
    }

    fn classify(&self, code_hash: &[u8]) -> Option<AssetKind> {
        self.lookup.get(code_hash).copied()
    }
}

/// Input cell info needed for activity building.
#[derive(Clone)]
pub struct InputCellView {
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub capacity: i64,
    pub occupied_capacity: i64,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_args: Option<Vec<u8>>,
    pub udt_amount: Option<u128>,
    pub data: Vec<u8>,
    pub is_dao_withdraw_request: bool,
}

/// Transaction data needed for activity building.
pub struct TxView<'a> {
    pub tx_hash: &'a [u8],
    pub block_hash: &'a [u8],
    pub tx_index: i32,
    pub block_number: i64,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub inputs: Vec<InputCellView>,
    pub outputs: &'a [ParsedCell],
}

/// `(lock_hash, involved_script_code_hashes, ActivityEntry)` — one per owner per transaction.
pub type OwnerActivity = (Vec<u8>, Vec<Vec<u8>>, ActivityEntry);

/// Build activities for all transactions in a block.
///
/// Returns `OwnerActivity` triples — one per owner per transaction.
pub fn build_activities_for_block(
    txs: &[TxView<'_>],
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<OwnerActivity> {
    let hashes = code_hashes();
    let mut all_activities = Vec::new();

    for tx in txs {
        let activities = build_tx_activities(tx, hashes, token_info_cache);
        all_activities.extend(activities);
    }

    all_activities
}

/// Accumulator for per-owner position within one transaction.
#[derive(Default)]
struct OwnerAccum {
    input_capacity: i128,
    output_capacity: i128,
    input_occupied: i64,
    output_occupied: i64,
    /// UDT: type_script_hash -> (input_amount, output_amount)
    udt_deltas: HashMap<Vec<u8>, (i128, i128)>,
    /// DAO deposits (output cells with DAO type and data == 0x00..00)
    dao_deposits: Vec<i64>,
    /// DAO withdraw requests (output cells with DAO type and non-zero deposit block)
    dao_withdraw_requests: Vec<(i64, i64)>,
    /// DAO withdraw completes (input cells with DAO withdraw request consumed without DAO output)
    dao_withdraw_completes: Vec<i64>,
    /// Spore/DOB IDs seen as inputs
    spore_inputs: Vec<Vec<u8>>,
    /// Spore/DOB IDs seen as outputs
    spore_outputs: Vec<Vec<u8>>,
    /// mNFT IDs seen as inputs
    nft_inputs: Vec<Vec<u8>>,
    /// mNFT IDs seen as outputs
    nft_outputs: Vec<Vec<u8>>,
    /// DotBit IDs seen as inputs
    dotbit_inputs: Vec<Vec<u8>>,
    /// DotBit IDs seen as outputs
    dotbit_outputs: Vec<Vec<u8>>,
    /// did:ckb IDs seen as inputs
    did_ckb_inputs: Vec<Vec<u8>>,
    /// did:ckb IDs seen as outputs
    did_ckb_outputs: Vec<Vec<u8>>,
    /// Distinct script code_hashes involved (lock + type)
    involved_scripts: HashSet<Vec<u8>>,
}

const DOTBIT_TYPE_ARGS_LEN: usize = 20;
const DOTBIT_DATA_HASH_PREFIX_LEN: usize = 32;

fn build_tx_activities(
    tx: &TxView<'_>,
    hashes: &CodeHashes,
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<OwnerActivity> {
    let mut owners: HashMap<Vec<u8>, OwnerAccum> = HashMap::new();

    // Process inputs (skip inputs with unknown cell info — empty lock_script_hash)
    for input in &tx.inputs {
        if input.lock_script_hash.len() < 32 {
            continue;
        }
        let accum = owners.entry(input.lock_script_hash.clone()).or_default();
        accum.involved_scripts.insert(input.lock_code_hash.clone());
        accum.input_capacity += input.capacity as i128;
        accum.input_occupied += input.occupied_capacity;

        if let Some(ref type_code_hash) = input.type_code_hash {
            accum.involved_scripts.insert(type_code_hash.clone());
            classify_input(
                accum,
                type_code_hash,
                input.type_script_hash.as_deref(),
                input.type_args.as_deref(),
                input.udt_amount,
                &input.data,
                input.is_dao_withdraw_request,
                hashes,
                input.capacity,
            );
        }
    }

    // Process outputs
    for cell in tx.outputs {
        let accum = owners.entry(cell.lock_script_hash.clone()).or_default();
        accum.involved_scripts.insert(cell.lock_code_hash.clone());
        accum.output_capacity += cell.capacity as i128;

        // Compute occupied for output
        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
        let type_script_size = cell
            .type_args
            .as_ref()
            .map(|args| 32 + 1 + args.len() as i64)
            .unwrap_or(0);
        let occupied =
            (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
        accum.output_occupied += occupied;

        if let Some(ref type_code_hash) = cell.type_code_hash {
            accum.involved_scripts.insert(type_code_hash.clone());
            classify_output(
                accum,
                type_code_hash,
                cell.type_script_hash.as_deref(),
                cell.type_args.as_deref(),
                &cell.data,
                hashes,
                cell.capacity,
            );
        }
    }

    let mut result = Vec::with_capacity(owners.len());

    for (lock_hash, accum) in &owners {
        let ckb_delta = accum.output_capacity - accum.input_capacity;
        let occupied_delta = accum.output_occupied - accum.input_occupied;

        // Peers = all other lock_hashes in this tx
        let peers: Vec<Vec<u8>> = owners
            .keys()
            .filter(|h| h.as_slice() != lock_hash.as_slice())
            .cloned()
            .collect();

        // Build asset changes
        let mut asset_changes = Vec::new();

        // UDT changes
        for (type_script_hash, (input_amt, output_amt)) in &accum.udt_deltas {
            let delta = *output_amt - *input_amt;
            if delta != 0 {
                let (symbol, decimals) = token_info_cache
                    .get(type_script_hash)
                    .cloned()
                    .unwrap_or((None, None));
                asset_changes.push(AssetChange::Token {
                    type_script_hash: type_script_hash.clone(),
                    delta,
                    symbol,
                    decimals,
                });
            }
        }

        // DAO deposits
        for capacity in &accum.dao_deposits {
            asset_changes.push(AssetChange::DaoDeposit {
                capacity: *capacity,
            });
        }

        // DAO withdraw requests
        for (capacity, deposit_block) in &accum.dao_withdraw_requests {
            asset_changes.push(AssetChange::DaoWithdrawRequest {
                capacity: *capacity,
                deposit_block: *deposit_block,
            });
        }

        // DAO withdraw completes
        for capacity in &accum.dao_withdraw_completes {
            asset_changes.push(AssetChange::DaoWithdrawComplete {
                capacity: *capacity,
                compensation: 0,
            });
        }

        // Spore/DOB changes
        emit_nft_changes(
            &accum.spore_inputs,
            &accum.spore_outputs,
            "spore",
            true,
            &mut asset_changes,
        );

        // mNFT changes
        emit_nft_changes(
            &accum.nft_inputs,
            &accum.nft_outputs,
            "m-nft",
            false,
            &mut asset_changes,
        );

        // DotBit changes
        emit_nft_changes(
            &accum.dotbit_inputs,
            &accum.dotbit_outputs,
            "dotbit",
            false,
            &mut asset_changes,
        );

        // did:ckb changes
        emit_nft_changes(
            &accum.did_ckb_inputs,
            &accum.did_ckb_outputs,
            "did_ckb",
            true,
            &mut asset_changes,
        );

        let entry = ActivityEntry {
            tx_hash: tx.tx_hash.to_vec(),
            block_hash: tx.block_hash.to_vec(),
            block_number: tx.block_number,
            tx_index: tx.tx_index,
            timestamp: tx.timestamp,
            ckb_delta,
            occupied_delta,
            is_cellbase: tx.is_cellbase,
            asset_changes,
            peers,
        };

        let scripts: Vec<Vec<u8>> = accum.involved_scripts.iter().cloned().collect();
        result.push((lock_hash.clone(), scripts, entry));
    }

    result
}

fn classify_input(
    accum: &mut OwnerAccum,
    type_code_hash: &[u8],
    type_script_hash: Option<&[u8]>,
    type_args: Option<&[u8]>,
    udt_amount: Option<u128>,
    data: &[u8],
    is_dao_withdraw_request: bool,
    hashes: &CodeHashes,
    capacity: i64,
) {
    match hashes.classify(type_code_hash) {
        Some(AssetKind::Udt) => {
            if let Some(tsh) = type_script_hash {
                if let Some(amount) = udt_amount.or_else(|| UdtParser::parse_amount(data)) {
                    let entry = accum.udt_deltas.entry(tsh.to_vec()).or_insert((0, 0));
                    entry.0 += amount as i128;
                }
            }
        }
        Some(AssetKind::Dao) => {
            if is_dao_withdraw_request {
                accum.dao_withdraw_completes.push(capacity);
            }
        }
        Some(AssetKind::SporeDid) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.did_ckb_inputs.push(args.to_vec());
                }
            }
        }
        Some(AssetKind::Spore | AssetKind::Cluster) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.spore_inputs.push(args.to_vec());
                }
            }
        }
        Some(AssetKind::MnftToken) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.nft_inputs.push(args.to_vec());
                }
            }
        }
        Some(AssetKind::Dotbit) => {
            if let Some(account_id) = resolve_dotbit_account_id(type_args, data) {
                accum.dotbit_inputs.push(account_id);
            }
        }
        None => {}
    }
}

fn classify_output(
    accum: &mut OwnerAccum,
    type_code_hash: &[u8],
    type_script_hash: Option<&[u8]>,
    type_args: Option<&[u8]>,
    cell_data: &[u8],
    hashes: &CodeHashes,
    capacity: i64,
) {
    match hashes.classify(type_code_hash) {
        Some(AssetKind::Udt) => {
            if let Some(tsh) = type_script_hash {
                if let Some(amount) = UdtParser::parse_amount(cell_data) {
                    let entry = accum.udt_deltas.entry(tsh.to_vec()).or_insert((0, 0));
                    entry.1 += amount as i128;
                }
            }
        }
        Some(AssetKind::Dao) => {
            if cell_data.len() == 8 {
                let bytes: [u8; 8] = cell_data.try_into().unwrap_or_else(|_| {
                    panic!(
                        "failed to decode DAO output data while classifying activity: len={}",
                        cell_data.len()
                    )
                });
                let deposit_block = u64::from_le_bytes(bytes);
                if deposit_block == 0 {
                    accum.dao_deposits.push(capacity);
                } else {
                    accum
                        .dao_withdraw_requests
                        .push((capacity, deposit_block as i64));
                }
            }
        }
        Some(AssetKind::SporeDid) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.did_ckb_outputs.push(args.to_vec());
                }
            }
        }
        Some(AssetKind::Spore | AssetKind::Cluster) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.spore_outputs.push(args.to_vec());
                }
            }
        }
        Some(AssetKind::MnftToken) => {
            if let Some(args) = type_args {
                if !args.is_empty() {
                    accum.nft_outputs.push(args.to_vec());
                }
            }
        }
        Some(AssetKind::Dotbit) => {
            if let Some(account_id) = resolve_dotbit_account_id(type_args, cell_data) {
                accum.dotbit_outputs.push(account_id);
            }
        }
        None => {}
    }
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
    let min_len = DOTBIT_DATA_HASH_PREFIX_LEN + DOTBIT_TYPE_ARGS_LEN;
    if cell_data.len() < min_len {
        return None;
    }
    let account_id = cell_data
        [DOTBIT_DATA_HASH_PREFIX_LEN..DOTBIT_DATA_HASH_PREFIX_LEN + DOTBIT_TYPE_ARGS_LEN]
        .to_vec();
    if account_id.iter().all(|&b| b == 0) {
        return None;
    }
    Some(account_id)
}

/// Emit DOB/NFT asset changes by comparing input vs output ID sets.
fn emit_nft_changes(
    inputs: &[Vec<u8>],
    outputs: &[Vec<u8>],
    standard: &str,
    is_dob: bool,
    asset_changes: &mut Vec<AssetChange>,
) {
    // IDs only in outputs = Mint
    for id in outputs {
        let in_inputs = inputs.iter().any(|i| i == id);
        let action = if in_inputs {
            AssetAction::Transfer
        } else {
            AssetAction::Mint
        };
        if is_dob {
            asset_changes.push(AssetChange::Dob {
                dob_id: id.clone(),
                standard: standard.to_string(),
                action,
            });
        } else {
            asset_changes.push(AssetChange::Nft {
                nft_id: id.clone(),
                standard: standard.to_string(),
                action,
            });
        }
    }
    // IDs only in inputs = Burn
    for id in inputs {
        let in_outputs = outputs.iter().any(|o| o == id);
        if !in_outputs {
            let action = AssetAction::Burn;
            if is_dob {
                asset_changes.push(AssetChange::Dob {
                    dob_id: id.clone(),
                    standard: standard.to_string(),
                    action,
                });
            } else {
                asset_changes.push(AssetChange::Nft {
                    nft_id: id.clone(),
                    standard: standard.to_string(),
                    action,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_output(
        lock_hash_byte: u8,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        data: Vec<u8>,
    ) -> ParsedCell {
        let data_size = data.len() as i32;
        ParsedCell {
            capacity,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![lock_hash_byte; 32],
            type_code_hash,
            type_hash_type: None,
            type_args,
            type_script_hash,
            data_hash: vec![0; 32],
            data_size,
            data,
        }
    }

    fn make_input(lock_hash_byte: u8, capacity: i64, occupied: i64) -> InputCellView {
        InputCellView {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash: vec![0x11; 32],
            capacity,
            occupied_capacity: occupied,
            type_code_hash: None,
            type_script_hash: None,
            type_args: None,
            udt_amount: None,
            data: vec![],
            is_dao_withdraw_request: false,
        }
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

        let tx = TxView {
            tx_hash: &[0x01; 32],
            block_hash: &[0xA1; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 300_00000000, 61_00000000)],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 2);

        let alice_act = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![alice; 32])
            .map(|(_, _, e)| e)
            .unwrap();
        assert_eq!(alice_act.ckb_delta, -100_00000000);
        assert_eq!(alice_act.peers.len(), 1);
        assert_eq!(alice_act.peers[0], vec![bob; 32]);
        assert!(!alice_act.is_cellbase);

        let bob_act = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![bob; 32])
            .map(|(_, _, e)| e)
            .unwrap();
        assert_eq!(bob_act.ckb_delta, 100_00000000);
        assert_eq!(bob_act.peers.len(), 1);
        assert_eq!(bob_act.peers[0], vec![alice; 32]);
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
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 1);
        let (lock_hash, _, entry) = &activities[0];
        assert_eq!(lock_hash, &vec![miner; 32]);
        assert_eq!(entry.ckb_delta, 5000_00000000);
        assert!(entry.is_cellbase);
        assert!(entry.peers.is_empty());
    }

    #[test]
    fn test_occupied_delta_computed() {
        let alice = 0xAA;
        let outputs = vec![make_output(
            alice,
            100_00000000,
            None,
            None,
            None,
            vec![0u8; 100], // 100 bytes of data
        )];

        let tx = TxView {
            tx_hash: &[0x03; 32],
            block_hash: &[0xA3; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 100_00000000, 61_00000000)],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 1);
        let (_, _, entry) = &activities[0];
        assert_eq!(entry.ckb_delta, 0);
        // Output occupied = (8 + (32+1+20) + 0 + 100) * 100_000_000 = 16_100_000_000
        // occupied_delta = 16_100_000_000 - 6_100_000_000 = 10_000_000_000
        assert_eq!(entry.occupied_delta, 100_00000000);
    }

    #[test]
    fn test_three_party_peers() {
        let alice = 0xAA;
        let bob = 0xBB;
        let carol = 0xCC;

        let outputs = vec![
            make_output(bob, 100_00000000, None, None, None, vec![]),
            make_output(carol, 100_00000000, None, None, None, vec![]),
            make_output(alice, 100_00000000, None, None, None, vec![]),
        ];

        let tx = TxView {
            tx_hash: &[0x04; 32],
            block_hash: &[0xA4; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 300_00000000, 61_00000000)],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 3);

        for (lock_hash, _, entry) in &activities {
            assert_eq!(entry.peers.len(), 2);
            assert!(!entry.peers.contains(lock_hash));
        }
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
            outputs: &outputs1,
        };

        let outputs2 = vec![make_output(bob, 200_00000000, None, None, None, vec![])];

        let tx2 = TxView {
            tx_hash: &[0x02; 32],
            block_hash: &[0xB1; 32],
            tx_index: 1,
            block_number: 100,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 200_00000000, 61_00000000)],
            outputs: &outputs2,
        };

        let activities = build_activities_for_block(&[tx1, tx2], &HashMap::new());
        assert_eq!(activities.len(), 3);

        let alice_entries: Vec<_> = activities
            .iter()
            .filter(|(lh, _, _)| lh == &vec![alice; 32])
            .collect();
        assert_eq!(alice_entries.len(), 2);
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
        alice_input.data = 5000u128.to_le_bytes().to_vec();

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
            inputs: vec![alice_input],
            outputs: &outputs,
        };

        let mut token_cache = HashMap::new();
        token_cache.insert(
            type_script_hash.clone(),
            (Some("SEAL".to_string()), Some(8u8)),
        );

        let activities = build_activities_for_block(&[tx], &token_cache);

        let alice_act = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![alice; 32])
            .map(|(_, _, e)| e)
            .unwrap();
        let token_change = alice_act
            .asset_changes
            .iter()
            .find(|c| matches!(c, AssetChange::Token { .. }))
            .unwrap();
        match token_change {
            AssetChange::Token {
                delta,
                symbol,
                decimals,
                ..
            } => {
                assert_eq!(*delta, -1000);
                assert_eq!(symbol.as_deref(), Some("SEAL"));
                assert_eq!(*decimals, Some(8));
            }
            _ => unreachable!(),
        }

        let bob_act = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![bob; 32])
            .map(|(_, _, e)| e)
            .unwrap();
        let token_change = bob_act
            .asset_changes
            .iter()
            .find(|c| matches!(c, AssetChange::Token { .. }))
            .unwrap();
        match token_change {
            AssetChange::Token { delta, .. } => {
                assert_eq!(*delta, 1000);
            }
            _ => unreachable!(),
        }
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
            inputs: vec![alice_input],
            outputs: &outputs,
        };

        let mut token_cache = HashMap::new();
        token_cache.insert(
            type_script_hash.clone(),
            (Some("SEAL".to_string()), Some(8u8)),
        );

        let activities = build_activities_for_block(&[tx], &token_cache);
        let alice_act = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![alice; 32])
            .map(|(_, _, e)| e)
            .unwrap();
        let token_change = alice_act
            .asset_changes
            .iter()
            .find(|c| matches!(c, AssetChange::Token { .. }))
            .unwrap();
        match token_change {
            AssetChange::Token { delta, .. } => assert_eq!(*delta, -1000),
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_no_activities_for_empty_block() {
        let activities = build_activities_for_block(&[], &HashMap::new());
        assert!(activities.is_empty());
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
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 1);
        let (_, _, entry) = &activities[0];

        let dotbit_change = entry
            .asset_changes
            .iter()
            .find(|c| matches!(c, AssetChange::Nft { standard, .. } if standard == "dotbit"))
            .expect("dotbit nft change should be present");
        match dotbit_change {
            AssetChange::Nft {
                nft_id,
                standard,
                action,
            } => {
                assert_eq!(nft_id, &account_id);
                assert_eq!(standard, "dotbit");
                assert!(matches!(action, AssetAction::Mint));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_did_ckb_changes_are_labeled_as_did_ckb_dob() {
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
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 1);
        let (_, _, entry) = &activities[0];

        let did_change = entry
            .asset_changes
            .iter()
            .find(|c| matches!(c, AssetChange::Dob { standard, .. } if standard == "did_ckb"))
            .expect("did_ckb dob change should be present");
        match did_change {
            AssetChange::Dob {
                dob_id,
                standard,
                action,
            } => {
                assert_eq!(dob_id, &did_id);
                assert_eq!(standard, "did_ckb");
                assert!(matches!(action, AssetAction::Mint));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_dao_withdraw_complete_is_classified_from_input_view_flag() {
        let owner = 0xAA;
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);

        let mut dao_input = make_input(owner, 102_00000000, 102_00000000);
        dao_input.type_code_hash = Some(dao_code_hash);
        dao_input.is_dao_withdraw_request = true;

        let outputs: Vec<ParsedCell> = vec![];

        let tx = TxView {
            tx_hash: &[0x09; 32],
            block_hash: &[0xA9; 32],
            tx_index: 1,
            block_number: 200,
            timestamp: 1_700_000_200,
            is_cellbase: false,
            inputs: vec![dao_input],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 1);
        let (_, _, entry) = &activities[0];
        assert!(entry
            .asset_changes
            .iter()
            .any(|change| matches!(change, AssetChange::DaoWithdrawComplete { capacity, .. } if *capacity == 102_00000000)));
    }

    #[test]
    fn test_scripts_tracked_for_transfer() {
        let alice = 0xAA;
        let bob = 0xBB;

        let outputs = vec![
            make_output(bob, 100_00000000, None, None, None, vec![]),
            make_output(alice, 200_00000000, None, None, None, vec![]),
        ];

        let tx = TxView {
            tx_hash: &[0x01; 32],
            block_hash: &[0xA1; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 300_00000000, 61_00000000)],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        let (_, alice_scripts, _) = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![alice; 32])
            .unwrap();
        assert!(alice_scripts.contains(&vec![0x11; 32]));

        let (_, bob_scripts, _) = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![bob; 32])
            .unwrap();
        assert!(bob_scripts.contains(&vec![0x11; 32]));
    }
}
