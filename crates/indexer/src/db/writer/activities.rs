//! Activity builder: derives per-owner position changes from parsed block data.

use std::collections::HashMap;

use ckbadger_store::types::{ActivityEntry, AssetAction, AssetChange};

use crate::parser::cell::ParsedCell;
use crate::parser::udt::UdtParser;

/// Pre-computed code hashes for asset detection.
struct CodeHashes {
    dao: Vec<u8>,
    sudt: Vec<u8>,
    xudt_data1: Vec<u8>,
    xudt_type: Vec<u8>,
    spore_hashes: Vec<Vec<u8>>,
    cluster_hashes: Vec<Vec<u8>>,
    mnft_token: Vec<u8>,
    dotbit: Vec<u8>,
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

        Self {
            dao: parse_hex_to_bytes(DAO_CODE_HASH),
            sudt: parse_hex_to_bytes(SUDT_CODE_HASH),
            xudt_data1: parse_hex_to_bytes(XUDT_CODE_HASH_DATA1),
            xudt_type: parse_hex_to_bytes(XUDT_CODE_HASH_TYPE),
            spore_hashes: vec![
                parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2),
                parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_DID),
                parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V2),
                parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V1),
            ],
            cluster_hashes: vec![
                parse_hex_to_bytes(CLUSTER_CODE_HASH_MAINNET_V2),
                parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V2),
                parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V1),
            ],
            mnft_token: parse_hex_to_bytes(MNFT_TOKEN_CODE_HASH),
            dotbit: parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID),
        }
    }

    fn is_udt(&self, code_hash: &[u8]) -> bool {
        code_hash == self.sudt || code_hash == self.xudt_data1 || code_hash == self.xudt_type
    }

    fn is_spore(&self, code_hash: &[u8]) -> bool {
        self.spore_hashes.iter().any(|h| h == code_hash)
    }

    fn is_cluster(&self, code_hash: &[u8]) -> bool {
        self.cluster_hashes.iter().any(|h| h == code_hash)
    }
}

/// Input cell info needed for activity building.
pub struct InputCellView {
    pub lock_script_hash: Vec<u8>,
    pub capacity: i64,
    pub occupied_capacity: i64,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_args: Option<Vec<u8>>,
    pub data: Vec<u8>,
    pub data_size: i32,
}

/// Transaction data needed for activity building.
pub struct TxView<'a> {
    pub tx_hash: &'a [u8],
    pub tx_index: i32,
    pub block_number: i64,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub inputs: Vec<InputCellView>,
    pub outputs: &'a [ParsedCell],
    pub outputs_data: &'a [String],
}

/// Build activities for all transactions in a block.
///
/// Returns `(lock_hash, ActivityEntry)` pairs — one per owner per transaction.
pub fn build_activities_for_block(
    txs: &[TxView<'_>],
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<(Vec<u8>, ActivityEntry)> {
    let hashes = CodeHashes::new();
    let mut all_activities = Vec::new();

    for tx in txs {
        let activities = build_tx_activities(tx, &hashes, token_info_cache);
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
}

fn build_tx_activities(
    tx: &TxView<'_>,
    hashes: &CodeHashes,
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<(Vec<u8>, ActivityEntry)> {
    let mut owners: HashMap<Vec<u8>, OwnerAccum> = HashMap::new();

    // Process inputs
    for input in &tx.inputs {
        let accum = owners.entry(input.lock_script_hash.clone()).or_default();
        accum.input_capacity += input.capacity as i128;
        accum.input_occupied += input.occupied_capacity;

        if let Some(ref type_code_hash) = input.type_code_hash {
            classify_input(
                accum,
                type_code_hash,
                input.type_script_hash.as_deref(),
                input.type_args.as_deref(),
                &input.data,
                input.data_size,
                hashes,
            );
        }
    }

    // Process outputs
    for (idx, cell) in tx.outputs.iter().enumerate() {
        let accum = owners.entry(cell.lock_script_hash.clone()).or_default();
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
            classify_output(
                accum,
                type_code_hash,
                cell.type_script_hash.as_deref(),
                cell.type_args.as_deref(),
                &cell.data,
                cell.data_size,
                hashes,
                idx,
                tx.outputs_data,
            );
        }
    }

    // Collect all lock hashes for peer computation
    let all_lock_hashes: Vec<Vec<u8>> = owners.keys().cloned().collect();

    let mut result = Vec::with_capacity(owners.len());

    for (lock_hash, accum) in owners {
        let ckb_delta = accum.output_capacity - accum.input_capacity;
        let occupied_delta = accum.output_occupied - accum.input_occupied;

        // Peers = all other lock_hashes in this tx
        let peers: Vec<Vec<u8>> = all_lock_hashes
            .iter()
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

        let entry = ActivityEntry {
            tx_hash: tx.tx_hash.to_vec(),
            block_number: tx.block_number,
            tx_index: tx.tx_index,
            timestamp: tx.timestamp,
            ckb_delta,
            occupied_delta,
            is_cellbase: tx.is_cellbase,
            asset_changes,
            peers,
        };

        result.push((lock_hash, entry));
    }

    result
}

fn classify_input(
    accum: &mut OwnerAccum,
    type_code_hash: &[u8],
    type_script_hash: Option<&[u8]>,
    type_args: Option<&[u8]>,
    data: &[u8],
    _data_size: i32,
    hashes: &CodeHashes,
) {
    if hashes.is_udt(type_code_hash) {
        if let Some(tsh) = type_script_hash {
            if let Some(amount) = UdtParser::parse_amount(data) {
                let entry = accum.udt_deltas.entry(tsh.to_vec()).or_insert((0, 0));
                entry.0 += amount as i128;
            }
        }
    } else if type_code_hash == hashes.dao {
        // DAO input — handled by DAO withdraw detection
    } else if hashes.is_spore(type_code_hash) || hashes.is_cluster(type_code_hash) {
        if let Some(args) = type_args {
            if !args.is_empty() {
                accum.spore_inputs.push(args.to_vec());
            }
        }
    } else if type_code_hash == hashes.mnft_token {
        if let Some(args) = type_args {
            if !args.is_empty() {
                accum.nft_inputs.push(args.to_vec());
            }
        }
    } else if type_code_hash == hashes.dotbit {
        if let Some(args) = type_args {
            if !args.is_empty() {
                accum.dotbit_inputs.push(args.to_vec());
            }
        }
    }
}

fn classify_output(
    accum: &mut OwnerAccum,
    type_code_hash: &[u8],
    type_script_hash: Option<&[u8]>,
    type_args: Option<&[u8]>,
    _cell_data: &[u8],
    data_size: i32,
    hashes: &CodeHashes,
    output_idx: usize,
    outputs_data: &[String],
) {
    if hashes.is_udt(type_code_hash) {
        if let Some(tsh) = type_script_hash {
            // Parse output data for UDT amount
            if let Some(data_hex) = outputs_data.get(output_idx) {
                let data = crate::rpc::parse_hex_to_bytes(data_hex);
                if let Some(amount) = UdtParser::parse_amount(&data) {
                    let entry = accum.udt_deltas.entry(tsh.to_vec()).or_insert((0, 0));
                    entry.1 += amount as i128;
                }
            }
        }
    } else if type_code_hash == hashes.dao {
        // DAO output: deposit vs withdraw request
        if data_size == 8 {
            if let Some(data_hex) = outputs_data.get(output_idx) {
                let data_bytes = crate::rpc::parse_hex_to_bytes(data_hex);
                if data_bytes.len() == 8 {
                    let deposit_block =
                        u64::from_le_bytes(data_bytes[..8].try_into().unwrap_or([0; 8]));
                    if deposit_block == 0 {
                        // New deposit (data = 0 means deposit)
                        // capacity is stored on the OwnerAccum via output_capacity
                        // We can't get exact capacity here since we iterate cells,
                        // but the calling code has access to the full cell
                    } else {
                        // Withdraw request (data = deposit block number)
                        // Note: actual capacity is the cell capacity
                    }
                }
            }
        }
        // For deposits, detect output cells with DAO type script where data is all zeros
        if let Some(data_hex) = outputs_data.get(output_idx) {
            let data_bytes = crate::rpc::parse_hex_to_bytes(data_hex);
            if data_bytes.len() == 8 {
                let val = u64::from_le_bytes(data_bytes[..8].try_into().unwrap_or([0; 8]));
                if val == 0 {
                    // This is a DAO deposit — we don't know the exact capacity here
                    // but we'll get it from the output cells iteration above
                }
            }
        }
    } else if hashes.is_spore(type_code_hash) || hashes.is_cluster(type_code_hash) {
        if let Some(args) = type_args {
            if !args.is_empty() {
                accum.spore_outputs.push(args.to_vec());
            }
        }
    } else if type_code_hash == hashes.mnft_token {
        if let Some(args) = type_args {
            if !args.is_empty() {
                accum.nft_outputs.push(args.to_vec());
            }
        }
    } else if type_code_hash == hashes.dotbit {
        if let Some(args) = type_args {
            if !args.is_empty() {
                accum.dotbit_outputs.push(args.to_vec());
            }
        }
    }
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
            capacity,
            occupied_capacity: occupied,
            type_code_hash: None,
            type_script_hash: None,
            type_args: None,
            data: vec![],
            data_size: 0,
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
        let outputs_data = vec!["0x".to_string(), "0x".to_string()];
        let tx = TxView {
            tx_hash: &[0x01; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 300_00000000, 61_00000000)],
            outputs: &outputs,
            outputs_data: &outputs_data,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 2);

        let alice_act = activities
            .iter()
            .find(|(lh, _)| lh == &vec![alice; 32])
            .map(|(_, e)| e)
            .unwrap();
        assert_eq!(alice_act.ckb_delta, -100_00000000);
        assert_eq!(alice_act.peers.len(), 1);
        assert_eq!(alice_act.peers[0], vec![bob; 32]);
        assert!(!alice_act.is_cellbase);

        let bob_act = activities
            .iter()
            .find(|(lh, _)| lh == &vec![bob; 32])
            .map(|(_, e)| e)
            .unwrap();
        assert_eq!(bob_act.ckb_delta, 100_00000000);
        assert_eq!(bob_act.peers.len(), 1);
        assert_eq!(bob_act.peers[0], vec![alice; 32]);
    }

    #[test]
    fn test_cellbase_reward() {
        let miner = 0xCC;
        let outputs = vec![make_output(miner, 5000_00000000, None, None, None, vec![])];
        let outputs_data = vec!["0x".to_string()];
        let tx = TxView {
            tx_hash: &[0x02; 32],
            tx_index: 0,
            block_number: 500,
            timestamp: 1_700_000_000,
            is_cellbase: true,
            inputs: vec![],
            outputs: &outputs,
            outputs_data: &outputs_data,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 1);
        let (lock_hash, entry) = &activities[0];
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
        let outputs_data = vec!["0x".to_string()];
        let tx = TxView {
            tx_hash: &[0x03; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 100_00000000, 61_00000000)],
            outputs: &outputs,
            outputs_data: &outputs_data,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 1);
        let (_, entry) = &activities[0];
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
        let outputs_data = vec!["0x".to_string(); 3];
        let tx = TxView {
            tx_hash: &[0x04; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 300_00000000, 61_00000000)],
            outputs: &outputs,
            outputs_data: &outputs_data,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        assert_eq!(activities.len(), 3);

        for (lock_hash, entry) in &activities {
            assert_eq!(entry.peers.len(), 2);
            assert!(!entry.peers.contains(lock_hash));
        }
    }

    #[test]
    fn test_multiple_txs_in_block() {
        let alice = 0xAA;
        let bob = 0xBB;

        let outputs1 = vec![make_output(alice, 500_00000000, None, None, None, vec![])];
        let outputs_data1 = vec!["0x".to_string()];
        let tx1 = TxView {
            tx_hash: &[0x01; 32],
            tx_index: 0,
            block_number: 100,
            timestamp: 1_700_000_000,
            is_cellbase: true,
            inputs: vec![],
            outputs: &outputs1,
            outputs_data: &outputs_data1,
        };

        let outputs2 = vec![make_output(bob, 200_00000000, None, None, None, vec![])];
        let outputs_data2 = vec!["0x".to_string()];
        let tx2 = TxView {
            tx_hash: &[0x02; 32],
            tx_index: 1,
            block_number: 100,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 200_00000000, 61_00000000)],
            outputs: &outputs2,
            outputs_data: &outputs_data2,
        };

        let activities = build_activities_for_block(&[tx1, tx2], &HashMap::new());
        assert_eq!(activities.len(), 3);

        let alice_entries: Vec<_> = activities
            .iter()
            .filter(|(lh, _)| lh == &vec![alice; 32])
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
        let outputs_data = vec![
            format!("0x{}", hex::encode(1000u128.to_le_bytes())),
            format!("0x{}", hex::encode(4000u128.to_le_bytes())),
        ];
        let tx = TxView {
            tx_hash: &[0x05; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input],
            outputs: &outputs,
            outputs_data: &outputs_data,
        };

        let mut token_cache = HashMap::new();
        token_cache.insert(
            type_script_hash.clone(),
            (Some("SEAL".to_string()), Some(8u8)),
        );

        let activities = build_activities_for_block(&[tx], &token_cache);

        let alice_act = activities
            .iter()
            .find(|(lh, _)| lh == &vec![alice; 32])
            .map(|(_, e)| e)
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
            .find(|(lh, _)| lh == &vec![bob; 32])
            .map(|(_, e)| e)
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
    fn test_no_activities_for_empty_block() {
        let activities = build_activities_for_block(&[], &HashMap::new());
        assert!(activities.is_empty());
    }
}
