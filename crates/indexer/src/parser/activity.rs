use std::collections::HashMap;

use ckbadger_common::{ActivityCategory, ActivityMetadata, ActivityType};

use crate::rpc::TransactionView;

use super::cell::ParsedCell;

#[derive(Debug, Clone)]
pub struct ParsedActivity {
    pub activity_id: Vec<u8>,
    pub activity_type: ActivityType,
    pub activity_category: ActivityCategory,
    pub tx_hash: Vec<u8>,
    pub tx_index: i32,
    pub activity_index: i16,
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Option<Vec<u8>>,
    pub amount: String,
    pub asset_id: Option<Vec<u8>>,
    pub metadata: serde_json::Value,
}

impl ParsedActivity {
    pub fn compute_activity_id(
        tx_hash: &[u8],
        activity_type: &ActivityType,
        index: i16,
    ) -> Vec<u8> {
        use ckb_hash::new_blake2b;
        let mut hasher = new_blake2b();
        hasher.update(tx_hash);
        hasher.update(activity_type.as_str().as_bytes());
        hasher.update(&index.to_le_bytes());
        let mut result = vec![0u8; 32];
        hasher.finalize(&mut result);
        result
    }
}

pub struct ActivityParser;

impl ActivityParser {
    /// Parse CKB transfer activities from a transaction.
    ///
    /// This uses a flow-based algorithm to properly attribute transfers:
    /// 1. Calculate net balance change for each address (output - input)
    /// 2. Addresses with positive net change are receivers
    /// 3. Addresses with negative net change are senders
    /// 4. Match senders to receivers using a greedy flow algorithm
    ///
    /// The algorithm ensures:
    /// - Total sent by all senders = Total received by all receivers (conservation)
    /// - Each transfer has a specific from/to pair with exact amount
    /// - No O(N²) explosion - at most O(senders × receivers) activities, but with
    ///   amount tracking to prevent duplicate counting
    pub fn parse_ckb_transfers(
        tx: &TransactionView,
        tx_hash: &[u8],
        tx_index: i32,
        output_cells: &[ParsedCell],
        input_cells: &[ParsedCell],
    ) -> Vec<ParsedActivity> {
        if Self::is_cellbase(tx) {
            return vec![];
        }

        // Calculate total input capacity per address
        let mut input_capacity: HashMap<Vec<u8>, i128> = HashMap::new();
        for cell in input_cells {
            *input_capacity
                .entry(cell.lock_script_hash.clone())
                .or_insert(0) += cell.capacity as i128;
        }

        // Calculate total output capacity per address
        let mut output_capacity: HashMap<Vec<u8>, i128> = HashMap::new();
        for cell in output_cells {
            *output_capacity
                .entry(cell.lock_script_hash.clone())
                .or_insert(0) += cell.capacity as i128;
        }

        // Calculate net balance change for each address
        // Positive = net receiver, Negative = net sender
        let all_addresses: std::collections::HashSet<Vec<u8>> = input_capacity
            .keys()
            .chain(output_capacity.keys())
            .cloned()
            .collect();

        // Collect senders (net negative) and receivers (net positive)
        let mut senders: Vec<(Vec<u8>, i128)> = Vec::new();
        let mut receivers: Vec<(Vec<u8>, i128)> = Vec::new();

        for addr in all_addresses {
            let input = *input_capacity.get(&addr).unwrap_or(&0);
            let output = *output_capacity.get(&addr).unwrap_or(&0);
            let net_change = output - input;

            if net_change > 0 {
                receivers.push((addr, net_change));
            } else if net_change < 0 {
                senders.push((addr, -net_change)); // Store as positive amount
            }
            // net_change == 0 means no transfer for this address (e.g., just reorganizing cells)
        }

        // If no senders or no receivers, no transfers occurred
        // (This handles self-transfers where all outputs go back to sender)
        if senders.is_empty() || receivers.is_empty() {
            return vec![];
        }

        // Sort by amount descending for more intuitive pairing (largest transfers first)
        senders.sort_by(|a, b| b.1.cmp(&a.1));
        receivers.sort_by(|a, b| b.1.cmp(&a.1));

        // Greedy flow algorithm: match senders to receivers
        // This produces at most (senders + receivers - 1) activities
        let mut activities = Vec::new();
        let mut activity_index: i16 = 0;

        let mut sender_remaining: Vec<i128> = senders.iter().map(|(_, amt)| *amt).collect();
        let mut receiver_remaining: Vec<i128> = receivers.iter().map(|(_, amt)| *amt).collect();

        let mut sender_idx = 0;
        let mut receiver_idx = 0;

        while sender_idx < senders.len() && receiver_idx < receivers.len() {
            let send_amt = sender_remaining[sender_idx];
            let recv_amt = receiver_remaining[receiver_idx];

            if send_amt == 0 {
                sender_idx += 1;
                continue;
            }
            if recv_amt == 0 {
                receiver_idx += 1;
                continue;
            }

            // Transfer the minimum of what sender can send and receiver can receive
            let transfer_amount = std::cmp::min(send_amt, recv_amt);

            let activity_id = ParsedActivity::compute_activity_id(
                tx_hash,
                &ActivityType::CkbTransfer,
                activity_index,
            );

            activities.push(ParsedActivity {
                activity_id,
                activity_type: ActivityType::CkbTransfer,
                activity_category: ActivityCategory::Ckb,
                tx_hash: tx_hash.to_vec(),
                tx_index,
                activity_index,
                from_lock_hash: Some(senders[sender_idx].0.clone()),
                to_lock_hash: Some(receivers[receiver_idx].0.clone()),
                amount: transfer_amount.to_string(),
                asset_id: None,
                metadata: ActivityMetadata::CkbTransfer {}.to_json(),
            });

            activity_index += 1;

            sender_remaining[sender_idx] -= transfer_amount;
            receiver_remaining[receiver_idx] -= transfer_amount;

            if sender_remaining[sender_idx] == 0 {
                sender_idx += 1;
            }
            if receiver_remaining[receiver_idx] == 0 {
                receiver_idx += 1;
            }
        }

        activities
    }

    pub fn parse_cellbase_reward(
        tx: &TransactionView,
        tx_hash: &[u8],
        tx_index: i32,
        output_cells: &[ParsedCell],
        block_reward: u64,
        proposal_reward: u64,
    ) -> Option<ParsedActivity> {
        if !Self::is_cellbase(tx) {
            return None;
        }

        if output_cells.is_empty() {
            return None;
        }

        let miner_lock_hash = output_cells[0].lock_script_hash.clone();
        let total_reward: u64 = output_cells.iter().map(|c| c.capacity as u64).sum();

        let activity_id =
            ParsedActivity::compute_activity_id(tx_hash, &ActivityType::CellbaseReward, 0);

        let metadata = ActivityMetadata::CellbaseReward {
            total_reward: total_reward.to_string(),
            block_reward: block_reward.to_string(),
            proposal_reward: proposal_reward.to_string(),
        };

        Some(ParsedActivity {
            activity_id,
            activity_type: ActivityType::CellbaseReward,
            activity_category: ActivityCategory::Cellbase,
            tx_hash: tx_hash.to_vec(),
            tx_index,
            activity_index: 0,
            from_lock_hash: None,
            to_lock_hash: Some(miner_lock_hash),
            amount: total_reward.to_string(),
            asset_id: None,
            metadata: metadata.to_json(),
        })
    }

    pub fn parse_token_activities(
        tx_hash: &[u8],
        tx_index: i32,
        udt_transfers: &[super::udt::ParsedUdtTransfer],
        activity_index_start: i16,
    ) -> Vec<ParsedActivity> {
        let mut activities = Vec::new();
        let mut activity_index = activity_index_start;

        for transfer in udt_transfers {
            if transfer.amount == 0 {
                continue;
            }

            let (activity_type, from_lock_hash, to_lock_hash) = if transfer.is_mint {
                (
                    ActivityType::TokenMint,
                    None,
                    Some(transfer.to_lock_hash.clone()),
                )
            } else if transfer.is_burn {
                (
                    ActivityType::TokenBurn,
                    transfer.from_lock_hash.clone(),
                    None,
                )
            } else {
                (
                    ActivityType::TokenTransfer,
                    transfer.from_lock_hash.clone(),
                    Some(transfer.to_lock_hash.clone()),
                )
            };

            let activity_id =
                ParsedActivity::compute_activity_id(tx_hash, &activity_type, activity_index);

            let metadata = ActivityMetadata::Token {
                symbol: None,
                decimals: 0,
                token_type_hash: format!("0x{}", hex::encode(&transfer.type_script_hash)),
            };

            activities.push(ParsedActivity {
                activity_id,
                activity_type,
                activity_category: ActivityCategory::Token,
                tx_hash: tx_hash.to_vec(),
                tx_index,
                activity_index,
                from_lock_hash,
                to_lock_hash,
                amount: transfer.amount.to_string(),
                asset_id: Some(transfer.type_script_hash.clone()),
                metadata: metadata.to_json(),
            });

            activity_index += 1;
        }

        activities
    }

    pub fn parse_dob_activities(
        tx_hash: &[u8],
        tx_index: i32,
        output_spores: &[super::spore::ParsedSporeCell],
        input_spores: &[super::spore::ParsedSporeCell],
        activity_index_start: i16,
    ) -> Vec<ParsedActivity> {
        let mut activities = Vec::new();
        let mut activity_index = activity_index_start;

        let input_by_spore_id: std::collections::HashMap<&[u8], &super::spore::ParsedSporeCell> =
            input_spores
                .iter()
                .map(|s| (s.spore_id.as_slice(), s))
                .collect();

        let output_by_spore_id: std::collections::HashMap<&[u8], &super::spore::ParsedSporeCell> =
            output_spores
                .iter()
                .map(|s| (s.spore_id.as_slice(), s))
                .collect();

        for out_spore in output_spores {
            let input_spore = input_by_spore_id.get(out_spore.spore_id.as_slice());

            let (activity_type, from_lock_hash) = if let Some(inp) = input_spore {
                (ActivityType::DobTransfer, Some(inp.owner_lock_hash.clone()))
            } else {
                (ActivityType::DobMint, None)
            };

            let activity_id =
                ParsedActivity::compute_activity_id(tx_hash, &activity_type, activity_index);

            let metadata = ActivityMetadata::Dob {
                cluster_id: out_spore
                    .cluster_id
                    .as_ref()
                    .map(|c| format!("0x{}", hex::encode(c))),
                content_type: out_spore.content_type.clone(),
                spore_id: format!("0x{}", hex::encode(&out_spore.spore_id)),
            };

            activities.push(ParsedActivity {
                activity_id,
                activity_type,
                activity_category: ActivityCategory::Dob,
                tx_hash: tx_hash.to_vec(),
                tx_index,
                activity_index,
                from_lock_hash,
                to_lock_hash: Some(out_spore.owner_lock_hash.clone()),
                amount: "1".to_string(),
                asset_id: Some(out_spore.spore_id.clone()),
                metadata: metadata.to_json(),
            });

            activity_index += 1;
        }

        for inp_spore in input_spores {
            let has_output = output_by_spore_id.contains_key(inp_spore.spore_id.as_slice());

            if !has_output {
                let activity_id = ParsedActivity::compute_activity_id(
                    tx_hash,
                    &ActivityType::DobBurn,
                    activity_index,
                );

                let metadata = ActivityMetadata::Dob {
                    cluster_id: inp_spore
                        .cluster_id
                        .as_ref()
                        .map(|c| format!("0x{}", hex::encode(c))),
                    content_type: inp_spore.content_type.clone(),
                    spore_id: format!("0x{}", hex::encode(&inp_spore.spore_id)),
                };

                activities.push(ParsedActivity {
                    activity_id,
                    activity_type: ActivityType::DobBurn,
                    activity_category: ActivityCategory::Dob,
                    tx_hash: tx_hash.to_vec(),
                    tx_index,
                    activity_index,
                    from_lock_hash: Some(inp_spore.owner_lock_hash.clone()),
                    to_lock_hash: None,
                    amount: "1".to_string(),
                    asset_id: Some(inp_spore.spore_id.clone()),
                    metadata: metadata.to_json(),
                });

                activity_index += 1;
            }
        }

        activities
    }

    pub fn parse_nft_activities(
        tx_hash: &[u8],
        tx_index: i32,
        output_mnfts: &[super::mnft::ParsedMnftToken],
        input_mnfts: &[super::mnft::ParsedMnftToken],
        output_dotbits: &[super::dotbit::ParsedDotbitAccount],
        input_dotbits: &[super::dotbit::ParsedDotbitAccount],
        activity_index_start: i16,
    ) -> Vec<ParsedActivity> {
        let mut activities = Vec::new();
        let mut activity_index = activity_index_start;

        let input_mnft_by_id: std::collections::HashMap<&[u8], &super::mnft::ParsedMnftToken> =
            input_mnfts
                .iter()
                .map(|t| (t.token_id.as_slice(), t))
                .collect();

        for out_token in output_mnfts {
            let input_token = input_mnft_by_id.get(out_token.token_id.as_slice());

            let (activity_type, from_lock_hash) = if let Some(inp) = input_token {
                (ActivityType::NftTransfer, Some(inp.owner_lock_hash.clone()))
            } else {
                (ActivityType::NftMint, None)
            };

            let activity_id =
                ParsedActivity::compute_activity_id(tx_hash, &activity_type, activity_index);

            let metadata = ActivityMetadata::Nft {
                nft_type: "mnft".to_string(),
                nft_id: format!("0x{}", hex::encode(&out_token.token_id)),
                name: None,
            };

            activities.push(ParsedActivity {
                activity_id,
                activity_type,
                activity_category: ActivityCategory::Nft,
                tx_hash: tx_hash.to_vec(),
                tx_index,
                activity_index,
                from_lock_hash,
                to_lock_hash: Some(out_token.owner_lock_hash.clone()),
                amount: "1".to_string(),
                asset_id: Some(out_token.token_id.clone()),
                metadata: metadata.to_json(),
            });

            activity_index += 1;
        }

        let input_dotbit_by_id: std::collections::HashMap<
            &[u8],
            &super::dotbit::ParsedDotbitAccount,
        > = input_dotbits
            .iter()
            .map(|a| (a.account_id.as_slice(), a))
            .collect();

        for out_account in output_dotbits {
            let input_account = input_dotbit_by_id.get(out_account.account_id.as_slice());

            let (activity_type, from_lock_hash) = if let Some(inp) = input_account {
                (ActivityType::NftTransfer, Some(inp.owner_lock_hash.clone()))
            } else {
                (ActivityType::NftMint, None)
            };

            let activity_id =
                ParsedActivity::compute_activity_id(tx_hash, &activity_type, activity_index);

            let metadata = ActivityMetadata::Nft {
                nft_type: "dotbit".to_string(),
                nft_id: format!("0x{}", hex::encode(&out_account.account_id)),
                name: None,
            };

            activities.push(ParsedActivity {
                activity_id,
                activity_type,
                activity_category: ActivityCategory::Nft,
                tx_hash: tx_hash.to_vec(),
                tx_index,
                activity_index,
                from_lock_hash,
                to_lock_hash: Some(out_account.owner_lock_hash.clone()),
                amount: "1".to_string(),
                asset_id: Some(out_account.account_id.clone()),
                metadata: metadata.to_json(),
            });

            activity_index += 1;
        }

        activities
    }

    pub fn parse_dao_activities(
        tx_hash: &[u8],
        tx_index: i32,
        output_dao_cells: &[super::dao::ParsedDaoCell],
        input_dao_cells: &[super::dao::ParsedDaoCell],
        activity_index_start: i16,
    ) -> Vec<ParsedActivity> {
        use super::dao::DaoState;

        let mut activities = Vec::new();
        let mut activity_index = activity_index_start;

        for dao_cell in output_dao_cells {
            let (activity_type, from_lock_hash, deposit_block_number) = match dao_cell.state {
                DaoState::Deposit => (ActivityType::DaoDeposit, None, None),
                DaoState::WithdrawRequest => {
                    let from_lock = input_dao_cells
                        .iter()
                        .find(|inp| inp.state == DaoState::Deposit)
                        .map(|inp| inp.lock_script_hash.clone());
                    (
                        ActivityType::DaoWithdrawRequest,
                        from_lock,
                        dao_cell.deposit_block_number,
                    )
                }
            };

            let activity_id =
                ParsedActivity::compute_activity_id(tx_hash, &activity_type, activity_index);

            let metadata = ActivityMetadata::Dao {
                deposit_ar: None,
                withdraw_ar: None,
                compensation: None,
            };

            activities.push(ParsedActivity {
                activity_id,
                activity_type,
                activity_category: ActivityCategory::Dao,
                tx_hash: tx_hash.to_vec(),
                tx_index,
                activity_index,
                from_lock_hash,
                to_lock_hash: Some(dao_cell.lock_script_hash.clone()),
                amount: (dao_cell.capacity as u64).to_string(),
                asset_id: deposit_block_number.map(|bn| bn.to_le_bytes().to_vec()),
                metadata: metadata.to_json(),
            });

            activity_index += 1;
        }

        for inp_dao_cell in input_dao_cells {
            if inp_dao_cell.state != DaoState::WithdrawRequest {
                continue;
            }

            let has_dao_output = !output_dao_cells.is_empty();

            if !has_dao_output {
                let activity_id = ParsedActivity::compute_activity_id(
                    tx_hash,
                    &ActivityType::DaoWithdrawComplete,
                    activity_index,
                );

                let metadata = ActivityMetadata::Dao {
                    deposit_ar: None,
                    withdraw_ar: None,
                    compensation: None,
                };

                activities.push(ParsedActivity {
                    activity_id,
                    activity_type: ActivityType::DaoWithdrawComplete,
                    activity_category: ActivityCategory::Dao,
                    tx_hash: tx_hash.to_vec(),
                    tx_index,
                    activity_index,
                    from_lock_hash: Some(inp_dao_cell.lock_script_hash.clone()),
                    to_lock_hash: None,
                    amount: (inp_dao_cell.capacity as u64).to_string(),
                    asset_id: inp_dao_cell
                        .deposit_block_number
                        .map(|bn| bn.to_le_bytes().to_vec()),
                    metadata: metadata.to_json(),
                });

                activity_index += 1;
            }
        }

        activities
    }

    /// Parse script deployment activities.
    ///
    /// A script deployment is detected when a cell:
    /// 1. Has a type script (typically TYPE_ID pattern for unique code_hash)
    /// 2. Contains non-trivial data (the script code itself)
    ///
    /// The type_script_hash becomes the code_hash that other scripts reference
    /// with hash_type = "type".
    ///
    /// For data hash deployments (hash_type = "data/data1/data2"), the data_hash
    /// is the code_hash. We detect these by cells with significant data (> 32 bytes)
    /// that have a type script indicating it's meant to be referenced as code.
    pub fn parse_script_deployments(
        tx_hash: &[u8],
        tx_index: i32,
        output_cells: &[ParsedCell],
        activity_index_start: i16,
    ) -> Vec<ParsedActivity> {
        let mut activities = Vec::new();
        let mut activity_index = activity_index_start;

        // TYPE_ID code_hash (used for unique script deployments)
        const TYPE_ID_CODE_HASH: &str =
            "0x00000000000000000000000000000000000000000000000000545950455f4944";

        for cell in output_cells {
            // Skip cells without data (no code to deploy)
            if cell.data_size == 0 {
                continue;
            }

            // Skip cells without type script (can't be uniquely identified as code)
            let type_code_hash = match &cell.type_code_hash {
                Some(hash) => hash,
                None => continue,
            };

            let type_script_hash = match &cell.type_script_hash {
                Some(hash) => hash,
                None => continue,
            };

            // Check if this is a TYPE_ID deployment (most common for scripts)
            let type_code_hash_hex = format!("0x{}", hex::encode(type_code_hash));
            let is_type_id = type_code_hash_hex == TYPE_ID_CODE_HASH;

            // Also check for significant data size (actual code, not just small metadata)
            // Scripts are typically > 100 bytes (even small ones)
            // But we use a lower threshold to catch all deployments
            let is_significant_data = cell.data_size > 32;

            // A cell is considered a script deployment if:
            // 1. It uses TYPE_ID (most reliable indicator), OR
            // 2. It has significant data with a type script (potential code cell)
            if !is_type_id && !is_significant_data {
                continue;
            }

            // The code_hash for hash_type="type" is the type_script_hash
            // For data/data1/data2, it would be the data_hash, but we use type_script_hash
            // as the canonical identifier since it's immutable for TYPE_ID scripts
            let code_hash = format!("0x{}", hex::encode(type_script_hash));

            let activity_id = ParsedActivity::compute_activity_id(
                tx_hash,
                &ActivityType::ScriptDeploy,
                activity_index,
            );

            let metadata = ActivityMetadata::Script {
                code_hash: code_hash.clone(),
                data_size: cell.data_size as u64,
            };

            // to_lock_hash is the deployer (who owns this code cell)
            activities.push(ParsedActivity {
                activity_id,
                activity_type: ActivityType::ScriptDeploy,
                activity_category: ActivityCategory::Script,
                tx_hash: tx_hash.to_vec(),
                tx_index,
                activity_index,
                from_lock_hash: None,
                to_lock_hash: Some(cell.lock_script_hash.clone()),
                amount: cell.data_size.to_string(),
                asset_id: Some(type_script_hash.clone()),
                metadata: metadata.to_json(),
            });

            activity_index += 1;
        }

        activities
    }

    /// Parse RGB++ activities from a transaction.
    ///
    /// RGB++ uses isomorphic bindings between Bitcoin UTXOs and CKB cells.
    /// Activity types:
    /// - RGBPP_TRANSFER: L1 transfer (RGBPP_lock input → RGBPP_lock output only)
    /// - RGBPP_LEAP_IN: BTC → CKB (RGBPP_lock input → BTC_TIME_lock output)
    /// - RGBPP_LEAP_OUT: CKB → BTC (non-RGBPP input → RGBPP_lock output)
    /// - RGBPP_ISSUANCE: New asset creation with RGBPP_lock
    ///
    /// Only cells with type_script are considered "asset cells" for RGB++ detection.
    pub fn parse_rgbpp_activities(
        tx_hash: &[u8],
        tx_index: i32,
        output_cells: &[ParsedCell],
        input_cells: &[ParsedCell],
        is_mainnet: bool,
        activity_index_start: i16,
    ) -> Vec<ParsedActivity> {
        use super::rgbpp::{RgbppLockType, RgbppParser};

        let mut activities = Vec::new();
        let mut activity_index = activity_index_start;

        // Filter to only typed cells (cells with type_script = asset cells)
        let typed_inputs: Vec<_> = input_cells
            .iter()
            .filter(|c| c.type_script_hash.is_some())
            .collect();
        let typed_outputs: Vec<_> = output_cells
            .iter()
            .filter(|c| c.type_script_hash.is_some())
            .collect();

        // If no typed cells, no RGB++ activity possible
        if typed_inputs.is_empty() && typed_outputs.is_empty() {
            return activities;
        }

        // Count lock types in typed inputs
        let mut input_rgbpp_count = 0;
        let mut input_btc_time_count = 0;
        let mut input_rgbpp_cells: Vec<&ParsedCell> = Vec::new();
        let mut input_btc_time_cells: Vec<&ParsedCell> = Vec::new();

        for cell in &typed_inputs {
            let lock_type = RgbppParser::detect_lock_type(&cell.lock_code_hash, is_mainnet);
            match lock_type {
                RgbppLockType::RgbppLock => {
                    input_rgbpp_count += 1;
                    input_rgbpp_cells.push(cell);
                }
                RgbppLockType::BtcTimeLock => {
                    input_btc_time_count += 1;
                    input_btc_time_cells.push(cell);
                }
                RgbppLockType::Other => {}
            }
        }

        // Count lock types in typed outputs
        let mut output_rgbpp_count = 0;
        let mut output_btc_time_count = 0;
        let mut output_rgbpp_cells: Vec<&ParsedCell> = Vec::new();
        let mut output_btc_time_cells: Vec<&ParsedCell> = Vec::new();

        for cell in &typed_outputs {
            let lock_type = RgbppParser::detect_lock_type(&cell.lock_code_hash, is_mainnet);
            match lock_type {
                RgbppLockType::RgbppLock => {
                    output_rgbpp_count += 1;
                    output_rgbpp_cells.push(cell);
                }
                RgbppLockType::BtcTimeLock => {
                    output_btc_time_count += 1;
                    output_btc_time_cells.push(cell);
                }
                RgbppLockType::Other => {}
            }
        }

        // If no RGB++ related cells, return early
        if input_rgbpp_count == 0
            && input_btc_time_count == 0
            && output_rgbpp_count == 0
            && output_btc_time_count == 0
        {
            return activities;
        }

        // Collect type_script_hashes from inputs to detect issuance (new types)
        let input_type_hashes: std::collections::HashSet<Vec<u8>> = typed_inputs
            .iter()
            .filter_map(|c| c.type_script_hash.clone())
            .collect();

        // Detect activity type based on input/output patterns
        // Priority order: LeapIn > Transfer > LeapOut/Issuance

        if input_rgbpp_count > 0 {
            // Has RGBPP_lock inputs

            if output_btc_time_count > 0 {
                // RGBPP_LEAP_IN: RGBPP_lock input → BTC_TIME_lock output
                // Create one activity per BTC_TIME_lock output (the leap destination)
                for output_cell in &output_btc_time_cells {
                    let btc_txid = RgbppParser::extract_btc_txid_from_btc_time_lock_args(
                        &output_cell.lock_args,
                    );

                    let activity_id = ParsedActivity::compute_activity_id(
                        tx_hash,
                        &ActivityType::RgbppLeapIn,
                        activity_index,
                    );

                    // from: first RGBPP_lock input, to: BTC_TIME_lock cell owner
                    let from_lock_hash = input_rgbpp_cells
                        .first()
                        .map(|c| c.lock_script_hash.clone());

                    let metadata = ActivityMetadata::Rgbpp {
                        btc_txid,
                        commitment: None,
                        asset_id: output_cell
                            .type_script_hash
                            .as_ref()
                            .map(|h| format!("0x{}", hex::encode(h))),
                    };

                    activities.push(ParsedActivity {
                        activity_id,
                        activity_type: ActivityType::RgbppLeapIn,
                        activity_category: ActivityCategory::Rgbpp,
                        tx_hash: tx_hash.to_vec(),
                        tx_index,
                        activity_index,
                        from_lock_hash,
                        to_lock_hash: Some(output_cell.lock_script_hash.clone()),
                        amount: (output_cell.capacity as u64).to_string(),
                        asset_id: output_cell.type_script_hash.clone(),
                        metadata: metadata.to_json(),
                    });

                    activity_index += 1;
                }
            } else if output_rgbpp_count > 0 {
                // RGBPP_TRANSFER: RGBPP_lock input → RGBPP_lock output only
                // Create one activity per unique output RGBPP_lock cell
                for output_cell in &output_rgbpp_cells {
                    let btc_txid = RgbppParser::parse_rgbpp_lock_args(&output_cell.lock_args)
                        .map(|args| args.btc_txid);

                    let activity_id = ParsedActivity::compute_activity_id(
                        tx_hash,
                        &ActivityType::RgbppTransfer,
                        activity_index,
                    );

                    let from_lock_hash = input_rgbpp_cells
                        .first()
                        .map(|c| c.lock_script_hash.clone());

                    let metadata = ActivityMetadata::Rgbpp {
                        btc_txid,
                        commitment: None,
                        asset_id: output_cell
                            .type_script_hash
                            .as_ref()
                            .map(|h| format!("0x{}", hex::encode(h))),
                    };

                    activities.push(ParsedActivity {
                        activity_id,
                        activity_type: ActivityType::RgbppTransfer,
                        activity_category: ActivityCategory::Rgbpp,
                        tx_hash: tx_hash.to_vec(),
                        tx_index,
                        activity_index,
                        from_lock_hash,
                        to_lock_hash: Some(output_cell.lock_script_hash.clone()),
                        amount: (output_cell.capacity as u64).to_string(),
                        asset_id: output_cell.type_script_hash.clone(),
                        metadata: metadata.to_json(),
                    });

                    activity_index += 1;
                }
            }
        } else if output_rgbpp_count > 0 {
            // No RGBPP_lock in typed inputs, but RGBPP_lock in outputs
            // Could be LEAP_OUT or ISSUANCE

            for output_cell in &output_rgbpp_cells {
                // Check if this is a new asset type (not seen in inputs)
                let is_new_type = output_cell
                    .type_script_hash
                    .as_ref()
                    .is_some_and(|h| !input_type_hashes.contains(h));

                let activity_type = if is_new_type {
                    ActivityType::RgbppIssuance
                } else {
                    ActivityType::RgbppLeapOut
                };

                let btc_txid = RgbppParser::parse_rgbpp_lock_args(&output_cell.lock_args)
                    .map(|args| args.btc_txid);

                let activity_id =
                    ParsedActivity::compute_activity_id(tx_hash, &activity_type, activity_index);

                // For leap-out/issuance, from is the CKB address (non-RGBPP typed input)
                let from_lock_hash = typed_inputs.first().map(|c| c.lock_script_hash.clone());

                let metadata = ActivityMetadata::Rgbpp {
                    btc_txid,
                    commitment: None,
                    asset_id: output_cell
                        .type_script_hash
                        .as_ref()
                        .map(|h| format!("0x{}", hex::encode(h))),
                };

                activities.push(ParsedActivity {
                    activity_id,
                    activity_type,
                    activity_category: ActivityCategory::Rgbpp,
                    tx_hash: tx_hash.to_vec(),
                    tx_index,
                    activity_index,
                    from_lock_hash,
                    to_lock_hash: Some(output_cell.lock_script_hash.clone()),
                    amount: (output_cell.capacity as u64).to_string(),
                    asset_id: output_cell.type_script_hash.clone(),
                    metadata: metadata.to_json(),
                });

                activity_index += 1;
            }
        }

        activities
    }

    fn is_cellbase(tx: &TransactionView) -> bool {
        tx.inputs.first().is_some_and(|input| {
            input.previous_output.tx_hash
                == "0x0000000000000000000000000000000000000000000000000000000000000000"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::script::ScriptParser;
    use crate::parser::udt::{ParsedUdtTransfer, UdtStandard};
    use crate::rpc::{CellInput, CellOutput, OutPoint, Script, TransactionView};

    const SECP256K1_CODE_HASH: &str =
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";

    fn create_lock_script(args: &str) -> Script {
        Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: args.to_string(),
        }
    }

    fn create_output(capacity: &str, lock_args: &str) -> CellOutput {
        CellOutput {
            capacity: capacity.to_string(),
            lock: create_lock_script(lock_args),
            type_: None,
        }
    }

    fn create_parsed_cell(capacity: i64, lock_args: &str) -> ParsedCell {
        let lock = create_lock_script(lock_args);
        let lock_script_hash = ScriptParser::compute_script_hash(&lock);
        ParsedCell {
            capacity,
            lock_code_hash: crate::rpc::parse_hex_to_bytes(SECP256K1_CODE_HASH),
            lock_hash_type: 1,
            lock_args: crate::rpc::parse_hex_to_bytes(lock_args),
            lock_script_hash,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0u8; 32],
            data_size: 0,
            data: vec![],
        }
    }

    fn create_cellbase_tx() -> TransactionView {
        TransactionView {
            hash: "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                previous_output: OutPoint {
                    tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000"
                        .to_string(),
                    index: "0xffffffff".to_string(),
                },
                since: "0x0".to_string(),
            }],
            outputs: vec![create_output("0x2540be400", "0xabc123")], // 10 CKB
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        }
    }

    fn create_regular_tx() -> TransactionView {
        TransactionView {
            hash: "0xfedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                previous_output: OutPoint {
                    tx_hash: "0x1111111111111111111111111111111111111111111111111111111111111111"
                        .to_string(),
                    index: "0x0".to_string(),
                },
                since: "0x0".to_string(),
            }],
            outputs: vec![
                create_output("0x4a817c800", "0xdef456"), // 20 CKB to receiver
                create_output("0x12a05f200", "0xabc123"), // 5 CKB back to sender (change)
            ],
            outputs_data: vec!["0x".to_string(), "0x".to_string()],
            witnesses: vec!["0x".to_string()],
        }
    }

    #[test]
    fn test_parse_cellbase_reward() {
        let tx = create_cellbase_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);
        let output_cells = vec![create_parsed_cell(10_00000000, "0xabc123")];

        let activity = ActivityParser::parse_cellbase_reward(
            &tx,
            &tx_hash,
            0,
            &output_cells,
            8_00000000,
            2_00000000,
        );

        assert!(activity.is_some());
        let activity = activity.unwrap();
        assert_eq!(activity.activity_type, ActivityType::CellbaseReward);
        assert_eq!(activity.activity_category, ActivityCategory::Cellbase);
        assert_eq!(activity.amount, "1000000000");
        assert!(activity.from_lock_hash.is_none());
        assert!(activity.to_lock_hash.is_some());

        let metadata: serde_json::Value = activity.metadata;
        assert_eq!(metadata["totalReward"], "1000000000");
        assert_eq!(metadata["blockReward"], "800000000");
        assert_eq!(metadata["proposalReward"], "200000000");
    }

    #[test]
    fn test_parse_cellbase_reward_not_cellbase() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);
        let output_cells = vec![create_parsed_cell(20_00000000, "0xdef456")];

        let activity = ActivityParser::parse_cellbase_reward(
            &tx,
            &tx_hash,
            0,
            &output_cells,
            8_00000000,
            2_00000000,
        );

        assert!(activity.is_none());
    }

    #[test]
    fn test_parse_ckb_transfer_simple() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);

        let input_cells = vec![create_parsed_cell(25_00000000, "0xabc123")]; // 25 CKB from sender

        let output_cells = vec![
            create_parsed_cell(20_00000000, "0xdef456"), // 20 CKB to receiver
            create_parsed_cell(5_00000000, "0xabc123"),  // 5 CKB change to sender
        ];

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 1, &output_cells, &input_cells);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::CkbTransfer);
        assert_eq!(activity.activity_category, ActivityCategory::Ckb);
        assert_eq!(activity.amount, "2000000000"); // 20 CKB transferred
        assert!(activity.from_lock_hash.is_some());
        assert!(activity.to_lock_hash.is_some());
    }

    #[test]
    fn test_parse_ckb_transfer_cellbase_returns_empty() {
        let tx = create_cellbase_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);
        let input_cells: Vec<ParsedCell> = vec![];
        let output_cells = vec![create_parsed_cell(10_00000000, "0xabc123")];

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 0, &output_cells, &input_cells);

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_ckb_transfer_multi_output() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);

        let input_cells = vec![create_parsed_cell(100_00000000, "0xabc123")]; // 100 CKB from sender

        let output_cells = vec![
            create_parsed_cell(30_00000000, "0xdef456"), // 30 CKB to receiver1
            create_parsed_cell(40_00000000, "0x789abc"), // 40 CKB to receiver2
            create_parsed_cell(30_00000000, "0xabc123"), // 30 CKB change to sender
        ];

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 1, &output_cells, &input_cells);

        assert_eq!(activities.len(), 2); // Two transfers to two different recipients

        let total_transferred: i128 = activities
            .iter()
            .map(|a| a.amount.parse::<i128>().unwrap())
            .sum();
        assert_eq!(total_transferred, 70_00000000); // 30 + 40 CKB
    }

    #[test]
    fn test_parse_ckb_transfer_self_transfer_excluded() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);

        let input_cells = vec![create_parsed_cell(100_00000000, "0xabc123")];
        let output_cells = vec![
            create_parsed_cell(100_00000000, "0xabc123"), // All back to sender (self-transfer)
        ];

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 1, &output_cells, &input_cells);

        assert!(activities.is_empty()); // No net transfer
    }

    #[test]
    fn test_parse_ckb_transfer_many_to_many() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);

        // 3 senders, 2 receivers
        let input_cells = vec![
            create_parsed_cell(50_00000000, "0xaa0001"),
            create_parsed_cell(30_00000000, "0xaa0002"),
            create_parsed_cell(20_00000000, "0xaa0003"),
        ];

        let output_cells = vec![
            create_parsed_cell(60_00000000, "0xbb0001"),
            create_parsed_cell(40_00000000, "0xbb0002"),
        ];

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 1, &output_cells, &input_cells);

        // Flow algorithm: at most (3 senders + 2 receivers - 1) = 4 activities
        assert!(activities.len() <= 4);

        let total_transferred: i128 = activities
            .iter()
            .map(|a| a.amount.parse::<i128>().unwrap())
            .sum();
        assert_eq!(total_transferred, 100_00000000);

        for activity in &activities {
            assert!(activity.from_lock_hash.is_some());
            assert!(activity.to_lock_hash.is_some());
            assert!(activity.amount.parse::<i128>().unwrap() > 0);
        }
    }

    #[test]
    fn test_parse_ckb_transfer_partial_change() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);

        // Sender sends 100, gets 30 back, net transfer = 70
        let input_cells = vec![create_parsed_cell(100_00000000, "0xcc0001")];

        let output_cells = vec![
            create_parsed_cell(70_00000000, "0xdd0001"),
            create_parsed_cell(30_00000000, "0xcc0001"),
        ];

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 1, &output_cells, &input_cells);

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].amount, "7000000000");
    }

    #[test]
    fn test_parse_ckb_transfer_exact_amounts_conserved() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);

        // Multiple senders and receivers: net sent = 250, net received = 250
        let input_cells = vec![
            create_parsed_cell(100_00000000, "0xee0001"),
            create_parsed_cell(200_00000000, "0xee0002"),
        ];

        let output_cells = vec![
            create_parsed_cell(150_00000000, "0xff0001"),
            create_parsed_cell(100_00000000, "0xff0002"),
            create_parsed_cell(50_00000000, "0xee0001"),
        ];

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 1, &output_cells, &input_cells);

        let total_transferred: i128 = activities
            .iter()
            .map(|a| a.amount.parse::<i128>().unwrap())
            .sum();
        assert_eq!(total_transferred, 250_00000000);
    }

    #[test]
    fn test_parse_ckb_transfer_stress_many_addresses() {
        let tx = create_regular_tx();
        let tx_hash = crate::rpc::parse_hex_to_bytes(&tx.hash);

        // 10 senders × 10 receivers - old O(N²) would create 100 activities
        let mut input_cells = Vec::new();
        for i in 0..10u8 {
            input_cells.push(create_parsed_cell(10_00000000, &format!("0x1100{:02x}", i)));
        }

        let mut output_cells = Vec::new();
        for i in 0..10u8 {
            output_cells.push(create_parsed_cell(10_00000000, &format!("0x2200{:02x}", i)));
        }

        let activities =
            ActivityParser::parse_ckb_transfers(&tx, &tx_hash, 1, &output_cells, &input_cells);

        // Flow algorithm: at most (10 + 10 - 1) = 19 activities
        assert!(
            activities.len() <= 19,
            "Expected at most 19 activities, got {}",
            activities.len()
        );

        let total_transferred: i128 = activities
            .iter()
            .map(|a| a.amount.parse::<i128>().unwrap())
            .sum();
        assert_eq!(total_transferred, 100_00000000);
    }

    #[test]
    fn test_activity_id_deterministic() {
        let tx_hash = vec![1u8; 32];
        let id1 = ParsedActivity::compute_activity_id(&tx_hash, &ActivityType::CkbTransfer, 0);
        let id2 = ParsedActivity::compute_activity_id(&tx_hash, &ActivityType::CkbTransfer, 0);

        assert_eq!(id1, id2);

        let id3 = ParsedActivity::compute_activity_id(&tx_hash, &ActivityType::CkbTransfer, 1);
        assert_ne!(id1, id3);

        let id4 = ParsedActivity::compute_activity_id(&tx_hash, &ActivityType::TokenMint, 0);
        assert_ne!(id1, id4);
    }

    fn create_udt_transfer(
        is_mint: bool,
        is_burn: bool,
        amount: u128,
        from_lock_hash: Option<Vec<u8>>,
        to_lock_hash: Vec<u8>,
    ) -> ParsedUdtTransfer {
        ParsedUdtTransfer {
            type_script_hash: vec![0xaa; 32],
            type_code_hash: vec![0xbb; 32],
            type_hash_type: 1,
            type_args: vec![0xcc; 20],
            from_lock_hash,
            to_lock_hash,
            amount,
            standard: UdtStandard::Sudt,
            is_mint,
            is_burn,
        }
    }

    #[test]
    fn test_parse_token_mint() {
        let tx_hash = vec![1u8; 32];
        let to_lock_hash = vec![0x11; 32];

        let transfers = vec![create_udt_transfer(
            true,
            false,
            1_000_000,
            None,
            to_lock_hash.clone(),
        )];

        let activities = ActivityParser::parse_token_activities(&tx_hash, 0, &transfers, 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::TokenMint);
        assert_eq!(activity.activity_category, ActivityCategory::Token);
        assert_eq!(activity.amount, "1000000");
        assert!(activity.from_lock_hash.is_none());
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
        assert!(activity.asset_id.is_some());
    }

    #[test]
    fn test_parse_token_transfer() {
        let tx_hash = vec![2u8; 32];
        let from_lock_hash = vec![0x22; 32];
        let to_lock_hash = vec![0x33; 32];

        let transfers = vec![create_udt_transfer(
            false,
            false,
            5_000_000,
            Some(from_lock_hash.clone()),
            to_lock_hash.clone(),
        )];

        let activities = ActivityParser::parse_token_activities(&tx_hash, 1, &transfers, 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::TokenTransfer);
        assert_eq!(activity.amount, "5000000");
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
    }

    #[test]
    fn test_parse_token_burn() {
        let tx_hash = vec![3u8; 32];
        let from_lock_hash = vec![0x44; 32];

        let transfers = vec![create_udt_transfer(
            false,
            true,
            2_000_000,
            Some(from_lock_hash.clone()),
            vec![],
        )];

        let activities = ActivityParser::parse_token_activities(&tx_hash, 2, &transfers, 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::TokenBurn);
        assert_eq!(activity.amount, "2000000");
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert!(activity.to_lock_hash.is_none());
    }

    #[test]
    fn test_parse_token_zero_amount_skipped() {
        let tx_hash = vec![4u8; 32];
        let transfers = vec![create_udt_transfer(true, false, 0, None, vec![0x55; 32])];

        let activities = ActivityParser::parse_token_activities(&tx_hash, 0, &transfers, 0);

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_token_multiple_transfers() {
        let tx_hash = vec![5u8; 32];
        let transfers = vec![
            create_udt_transfer(true, false, 1000, None, vec![0x66; 32]),
            create_udt_transfer(false, false, 2000, Some(vec![0x77; 32]), vec![0x88; 32]),
            create_udt_transfer(false, true, 500, Some(vec![0x99; 32]), vec![]),
        ];

        let activities = ActivityParser::parse_token_activities(&tx_hash, 0, &transfers, 0);

        assert_eq!(activities.len(), 3);
        assert_eq!(activities[0].activity_type, ActivityType::TokenMint);
        assert_eq!(activities[0].activity_index, 0);
        assert_eq!(activities[1].activity_type, ActivityType::TokenTransfer);
        assert_eq!(activities[1].activity_index, 1);
        assert_eq!(activities[2].activity_type, ActivityType::TokenBurn);
        assert_eq!(activities[2].activity_index, 2);
    }

    #[test]
    fn test_parse_token_activity_index_start() {
        let tx_hash = vec![6u8; 32];
        let transfers = vec![create_udt_transfer(true, false, 1000, None, vec![0xaa; 32])];

        let activities = ActivityParser::parse_token_activities(&tx_hash, 0, &transfers, 5);

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].activity_index, 5);
    }

    #[test]
    fn test_parse_token_metadata_contains_type_hash() {
        let tx_hash = vec![7u8; 32];
        let transfers = vec![create_udt_transfer(true, false, 1000, None, vec![0xbb; 32])];

        let activities = ActivityParser::parse_token_activities(&tx_hash, 0, &transfers, 0);

        let metadata = &activities[0].metadata;
        assert!(metadata["tokenTypeHash"].is_string());
        assert!(metadata["tokenTypeHash"]
            .as_str()
            .unwrap()
            .starts_with("0x"));
    }

    fn create_parsed_spore(
        spore_id: Vec<u8>,
        owner_lock_hash: Vec<u8>,
        content_type: &str,
        cluster_id: Option<Vec<u8>>,
    ) -> crate::parser::spore::ParsedSporeCell {
        crate::parser::spore::ParsedSporeCell {
            spore_id,
            type_script_hash: vec![0xdd; 32],
            content_type: content_type.to_string(),
            content: vec![],
            cluster_id,
            owner_lock_hash,
        }
    }

    #[test]
    fn test_parse_dob_mint() {
        let tx_hash = vec![10u8; 32];
        let spore_id = vec![0x11; 32];
        let to_lock_hash = vec![0x22; 32];

        let output_spores = vec![create_parsed_spore(
            spore_id.clone(),
            to_lock_hash.clone(),
            "image/png",
            None,
        )];
        let input_spores: Vec<crate::parser::spore::ParsedSporeCell> = vec![];

        let activities =
            ActivityParser::parse_dob_activities(&tx_hash, 0, &output_spores, &input_spores, 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::DobMint);
        assert_eq!(activity.activity_category, ActivityCategory::Dob);
        assert_eq!(activity.amount, "1");
        assert!(activity.from_lock_hash.is_none());
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
        assert_eq!(activity.asset_id, Some(spore_id));
    }

    #[test]
    fn test_parse_dob_transfer() {
        let tx_hash = vec![11u8; 32];
        let spore_id = vec![0x33; 32];
        let from_lock_hash = vec![0x44; 32];
        let to_lock_hash = vec![0x55; 32];

        let input_spores = vec![create_parsed_spore(
            spore_id.clone(),
            from_lock_hash.clone(),
            "image/jpeg",
            None,
        )];
        let output_spores = vec![create_parsed_spore(
            spore_id.clone(),
            to_lock_hash.clone(),
            "image/jpeg",
            None,
        )];

        let activities =
            ActivityParser::parse_dob_activities(&tx_hash, 1, &output_spores, &input_spores, 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::DobTransfer);
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
    }

    #[test]
    fn test_parse_dob_burn() {
        let tx_hash = vec![12u8; 32];
        let spore_id = vec![0x66; 32];
        let from_lock_hash = vec![0x77; 32];

        let input_spores = vec![create_parsed_spore(
            spore_id.clone(),
            from_lock_hash.clone(),
            "text/plain",
            None,
        )];
        let output_spores: Vec<crate::parser::spore::ParsedSporeCell> = vec![];

        let activities =
            ActivityParser::parse_dob_activities(&tx_hash, 2, &output_spores, &input_spores, 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::DobBurn);
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert!(activity.to_lock_hash.is_none());
    }

    #[test]
    fn test_parse_dob_metadata_contains_spore_id() {
        let tx_hash = vec![13u8; 32];
        let spore_id = vec![0x88; 32];
        let cluster_id = vec![0x99; 32];

        let output_spores = vec![create_parsed_spore(
            spore_id,
            vec![0xaa; 32],
            "application/json",
            Some(cluster_id),
        )];
        let input_spores: Vec<crate::parser::spore::ParsedSporeCell> = vec![];

        let activities =
            ActivityParser::parse_dob_activities(&tx_hash, 0, &output_spores, &input_spores, 0);

        let metadata = &activities[0].metadata;
        assert_eq!(metadata["contentType"], "application/json");
        assert!(metadata["sporeId"].as_str().unwrap().starts_with("0x"));
        assert!(metadata["clusterId"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn test_parse_dob_multiple_operations() {
        let tx_hash = vec![14u8; 32];

        let mint_id = vec![0x01; 32];
        let transfer_id = vec![0x02; 32];
        let burn_id = vec![0x03; 32];

        let input_spores = vec![
            create_parsed_spore(transfer_id.clone(), vec![0xaa; 32], "image/png", None),
            create_parsed_spore(burn_id.clone(), vec![0xbb; 32], "image/gif", None),
        ];
        let output_spores = vec![
            create_parsed_spore(mint_id.clone(), vec![0xcc; 32], "image/webp", None),
            create_parsed_spore(transfer_id.clone(), vec![0xdd; 32], "image/png", None),
        ];

        let activities =
            ActivityParser::parse_dob_activities(&tx_hash, 0, &output_spores, &input_spores, 0);

        assert_eq!(activities.len(), 3);

        let mint = activities
            .iter()
            .find(|a| a.activity_type == ActivityType::DobMint)
            .unwrap();
        assert_eq!(mint.asset_id, Some(mint_id));

        let transfer = activities
            .iter()
            .find(|a| a.activity_type == ActivityType::DobTransfer)
            .unwrap();
        assert_eq!(transfer.asset_id, Some(transfer_id));

        let burn = activities
            .iter()
            .find(|a| a.activity_type == ActivityType::DobBurn)
            .unwrap();
        assert_eq!(burn.asset_id, Some(burn_id));
    }

    fn create_parsed_mnft_token(
        token_id: Vec<u8>,
        owner_lock_hash: Vec<u8>,
    ) -> crate::parser::mnft::ParsedMnftToken {
        crate::parser::mnft::ParsedMnftToken {
            token_id,
            type_script_hash: vec![0xee; 32],
            class_id: vec![0xff; 24],
            token_index: 1,
            characteristic: vec![0u8; 8],
            configure: 0,
            state: 0,
            owner_lock_hash,
        }
    }

    fn create_parsed_dotbit_account(
        account_id: Vec<u8>,
        owner_lock_hash: Vec<u8>,
    ) -> crate::parser::dotbit::ParsedDotbitAccount {
        crate::parser::dotbit::ParsedDotbitAccount {
            account_id,
            type_script_hash: vec![0xdd; 32],
            next_account_id: None,
            expired_at: Some(1735689600),
            owner_lock_hash,
        }
    }

    #[test]
    fn test_parse_nft_mnft_mint() {
        let tx_hash = vec![20u8; 32];
        let token_id = vec![0x11; 28];
        let to_lock_hash = vec![0x22; 32];

        let output_mnfts = vec![create_parsed_mnft_token(
            token_id.clone(),
            to_lock_hash.clone(),
        )];
        let input_mnfts: Vec<crate::parser::mnft::ParsedMnftToken> = vec![];
        let output_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];
        let input_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];

        let activities = ActivityParser::parse_nft_activities(
            &tx_hash,
            0,
            &output_mnfts,
            &input_mnfts,
            &output_dotbits,
            &input_dotbits,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::NftMint);
        assert_eq!(activity.activity_category, ActivityCategory::Nft);
        assert_eq!(activity.amount, "1");
        assert!(activity.from_lock_hash.is_none());
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
        assert_eq!(activity.asset_id, Some(token_id));

        let metadata = &activity.metadata;
        assert_eq!(metadata["nftType"], "mnft");
    }

    #[test]
    fn test_parse_nft_mnft_transfer() {
        let tx_hash = vec![21u8; 32];
        let token_id = vec![0x33; 28];
        let from_lock_hash = vec![0x44; 32];
        let to_lock_hash = vec![0x55; 32];

        let input_mnfts = vec![create_parsed_mnft_token(
            token_id.clone(),
            from_lock_hash.clone(),
        )];
        let output_mnfts = vec![create_parsed_mnft_token(
            token_id.clone(),
            to_lock_hash.clone(),
        )];
        let output_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];
        let input_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];

        let activities = ActivityParser::parse_nft_activities(
            &tx_hash,
            1,
            &output_mnfts,
            &input_mnfts,
            &output_dotbits,
            &input_dotbits,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::NftTransfer);
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
    }

    #[test]
    fn test_parse_nft_dotbit_mint() {
        let tx_hash = vec![22u8; 32];
        let account_id = vec![0x66; 20];
        let to_lock_hash = vec![0x77; 32];

        let output_mnfts: Vec<crate::parser::mnft::ParsedMnftToken> = vec![];
        let input_mnfts: Vec<crate::parser::mnft::ParsedMnftToken> = vec![];
        let output_dotbits = vec![create_parsed_dotbit_account(
            account_id.clone(),
            to_lock_hash.clone(),
        )];
        let input_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];

        let activities = ActivityParser::parse_nft_activities(
            &tx_hash,
            0,
            &output_mnfts,
            &input_mnfts,
            &output_dotbits,
            &input_dotbits,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::NftMint);
        assert!(activity.from_lock_hash.is_none());
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
        assert_eq!(activity.asset_id, Some(account_id));

        let metadata = &activity.metadata;
        assert_eq!(metadata["nftType"], "dotbit");
    }

    #[test]
    fn test_parse_nft_dotbit_transfer() {
        let tx_hash = vec![23u8; 32];
        let account_id = vec![0x88; 20];
        let from_lock_hash = vec![0x99; 32];
        let to_lock_hash = vec![0xaa; 32];

        let output_mnfts: Vec<crate::parser::mnft::ParsedMnftToken> = vec![];
        let input_mnfts: Vec<crate::parser::mnft::ParsedMnftToken> = vec![];
        let input_dotbits = vec![create_parsed_dotbit_account(
            account_id.clone(),
            from_lock_hash.clone(),
        )];
        let output_dotbits = vec![create_parsed_dotbit_account(
            account_id.clone(),
            to_lock_hash.clone(),
        )];

        let activities = ActivityParser::parse_nft_activities(
            &tx_hash,
            1,
            &output_mnfts,
            &input_mnfts,
            &output_dotbits,
            &input_dotbits,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::NftTransfer);
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
    }

    #[test]
    fn test_parse_nft_mixed_operations() {
        let tx_hash = vec![24u8; 32];

        let mnft_mint_id = vec![0x01; 28];
        let mnft_transfer_id = vec![0x02; 28];
        let dotbit_mint_id = vec![0x03; 20];

        let input_mnfts = vec![create_parsed_mnft_token(
            mnft_transfer_id.clone(),
            vec![0xaa; 32],
        )];
        let output_mnfts = vec![
            create_parsed_mnft_token(mnft_mint_id.clone(), vec![0xbb; 32]),
            create_parsed_mnft_token(mnft_transfer_id.clone(), vec![0xcc; 32]),
        ];
        let input_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];
        let output_dotbits = vec![create_parsed_dotbit_account(
            dotbit_mint_id.clone(),
            vec![0xdd; 32],
        )];

        let activities = ActivityParser::parse_nft_activities(
            &tx_hash,
            0,
            &output_mnfts,
            &input_mnfts,
            &output_dotbits,
            &input_dotbits,
            0,
        );

        assert_eq!(activities.len(), 3);

        let mnft_mints: Vec<_> = activities
            .iter()
            .filter(|a| {
                a.activity_type == ActivityType::NftMint
                    && a.metadata["nftType"].as_str() == Some("mnft")
            })
            .collect();
        assert_eq!(mnft_mints.len(), 1);
        assert_eq!(mnft_mints[0].asset_id, Some(mnft_mint_id));

        let mnft_transfers: Vec<_> = activities
            .iter()
            .filter(|a| a.activity_type == ActivityType::NftTransfer)
            .collect();
        assert_eq!(mnft_transfers.len(), 1);
        assert_eq!(mnft_transfers[0].asset_id, Some(mnft_transfer_id));

        let dotbit_mints: Vec<_> = activities
            .iter()
            .filter(|a| {
                a.activity_type == ActivityType::NftMint
                    && a.metadata["nftType"].as_str() == Some("dotbit")
            })
            .collect();
        assert_eq!(dotbit_mints.len(), 1);
        assert_eq!(dotbit_mints[0].asset_id, Some(dotbit_mint_id));
    }

    #[test]
    fn test_parse_nft_activity_index_start() {
        let tx_hash = vec![25u8; 32];
        let token_id = vec![0xee; 28];

        let output_mnfts = vec![create_parsed_mnft_token(token_id, vec![0xff; 32])];
        let input_mnfts: Vec<crate::parser::mnft::ParsedMnftToken> = vec![];
        let output_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];
        let input_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];

        let activities = ActivityParser::parse_nft_activities(
            &tx_hash,
            0,
            &output_mnfts,
            &input_mnfts,
            &output_dotbits,
            &input_dotbits,
            10,
        );

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].activity_index, 10);
    }

    #[test]
    fn test_parse_nft_metadata_contains_nft_id() {
        let tx_hash = vec![26u8; 32];
        let token_id = vec![0xab; 28];

        let output_mnfts = vec![create_parsed_mnft_token(token_id, vec![0xcd; 32])];
        let input_mnfts: Vec<crate::parser::mnft::ParsedMnftToken> = vec![];
        let output_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];
        let input_dotbits: Vec<crate::parser::dotbit::ParsedDotbitAccount> = vec![];

        let activities = ActivityParser::parse_nft_activities(
            &tx_hash,
            0,
            &output_mnfts,
            &input_mnfts,
            &output_dotbits,
            &input_dotbits,
            0,
        );

        let metadata = &activities[0].metadata;
        assert!(metadata["nftId"].as_str().unwrap().starts_with("0x"));
        assert_eq!(metadata["nftType"], "mnft");
    }

    fn create_parsed_dao_cell(
        capacity: i64,
        lock_script_hash: Vec<u8>,
        state: crate::parser::dao::DaoState,
        deposit_block_number: Option<u64>,
    ) -> crate::parser::dao::ParsedDaoCell {
        crate::parser::dao::ParsedDaoCell {
            lock_script_hash,
            capacity,
            state,
            deposit_block_number,
        }
    }

    #[test]
    fn test_parse_dao_deposit() {
        use crate::parser::dao::DaoState;

        let tx_hash = vec![30u8; 32];
        let to_lock_hash = vec![0x11; 32];

        let output_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            to_lock_hash.clone(),
            DaoState::Deposit,
            None,
        )];
        let input_dao_cells: Vec<crate::parser::dao::ParsedDaoCell> = vec![];

        let activities = ActivityParser::parse_dao_activities(
            &tx_hash,
            0,
            &output_dao_cells,
            &input_dao_cells,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::DaoDeposit);
        assert_eq!(activity.activity_category, ActivityCategory::Dao);
        assert_eq!(activity.amount, "20000000000");
        assert!(activity.from_lock_hash.is_none());
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));
        assert!(activity.asset_id.is_none());
    }

    #[test]
    fn test_parse_dao_withdraw_request() {
        use crate::parser::dao::DaoState;

        let tx_hash = vec![31u8; 32];
        let from_lock_hash = vec![0x22; 32];
        let to_lock_hash = vec![0x22; 32];
        let deposit_block_number = 12345u64;

        let input_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            from_lock_hash.clone(),
            DaoState::Deposit,
            None,
        )];
        let output_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            to_lock_hash.clone(),
            DaoState::WithdrawRequest,
            Some(deposit_block_number),
        )];

        let activities = ActivityParser::parse_dao_activities(
            &tx_hash,
            1,
            &output_dao_cells,
            &input_dao_cells,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::DaoWithdrawRequest);
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert_eq!(activity.to_lock_hash, Some(to_lock_hash));

        let expected_asset_id = deposit_block_number.to_le_bytes().to_vec();
        assert_eq!(activity.asset_id, Some(expected_asset_id));
    }

    #[test]
    fn test_parse_dao_withdraw_complete() {
        use crate::parser::dao::DaoState;

        let tx_hash = vec![32u8; 32];
        let from_lock_hash = vec![0x33; 32];
        let deposit_block_number = 12345u64;

        let input_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            from_lock_hash.clone(),
            DaoState::WithdrawRequest,
            Some(deposit_block_number),
        )];
        let output_dao_cells: Vec<crate::parser::dao::ParsedDaoCell> = vec![];

        let activities = ActivityParser::parse_dao_activities(
            &tx_hash,
            2,
            &output_dao_cells,
            &input_dao_cells,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::DaoWithdrawComplete);
        assert_eq!(activity.from_lock_hash, Some(from_lock_hash));
        assert!(activity.to_lock_hash.is_none());
        assert_eq!(activity.amount, "20000000000");

        let expected_asset_id = deposit_block_number.to_le_bytes().to_vec();
        assert_eq!(activity.asset_id, Some(expected_asset_id));
    }

    #[test]
    fn test_parse_dao_deposit_not_completion() {
        use crate::parser::dao::DaoState;

        let tx_hash = vec![33u8; 32];
        let lock_hash = vec![0x44; 32];

        let input_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            lock_hash.clone(),
            DaoState::Deposit,
            None,
        )];
        let output_dao_cells: Vec<crate::parser::dao::ParsedDaoCell> = vec![];

        let activities = ActivityParser::parse_dao_activities(
            &tx_hash,
            0,
            &output_dao_cells,
            &input_dao_cells,
            0,
        );

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_dao_metadata_type() {
        use crate::parser::dao::DaoState;

        let tx_hash = vec![34u8; 32];
        let output_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            vec![0x55; 32],
            DaoState::Deposit,
            None,
        )];
        let input_dao_cells: Vec<crate::parser::dao::ParsedDaoCell> = vec![];

        let activities = ActivityParser::parse_dao_activities(
            &tx_hash,
            0,
            &output_dao_cells,
            &input_dao_cells,
            0,
        );

        let metadata = &activities[0].metadata;
        assert_eq!(metadata["type"], "dao");
    }

    #[test]
    fn test_parse_dao_activity_index_start() {
        use crate::parser::dao::DaoState;

        let tx_hash = vec![35u8; 32];
        let output_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            vec![0x66; 32],
            DaoState::Deposit,
            None,
        )];
        let input_dao_cells: Vec<crate::parser::dao::ParsedDaoCell> = vec![];

        let activities = ActivityParser::parse_dao_activities(
            &tx_hash,
            0,
            &output_dao_cells,
            &input_dao_cells,
            5,
        );

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].activity_index, 5);
    }

    #[test]
    fn test_parse_dao_multiple_operations() {
        use crate::parser::dao::DaoState;

        let tx_hash = vec![36u8; 32];

        let output_dao_cells = vec![
            create_parsed_dao_cell(100_00000000, vec![0x77; 32], DaoState::Deposit, None),
            create_parsed_dao_cell(
                200_00000000,
                vec![0x88; 32],
                DaoState::WithdrawRequest,
                Some(99999),
            ),
        ];
        let input_dao_cells = vec![create_parsed_dao_cell(
            200_00000000,
            vec![0x99; 32],
            DaoState::Deposit,
            None,
        )];

        let activities = ActivityParser::parse_dao_activities(
            &tx_hash,
            0,
            &output_dao_cells,
            &input_dao_cells,
            0,
        );

        assert_eq!(activities.len(), 2);

        let deposits: Vec<_> = activities
            .iter()
            .filter(|a| a.activity_type == ActivityType::DaoDeposit)
            .collect();
        assert_eq!(deposits.len(), 1);

        let requests: Vec<_> = activities
            .iter()
            .filter(|a| a.activity_type == ActivityType::DaoWithdrawRequest)
            .collect();
        assert_eq!(requests.len(), 1);
    }

    const TYPE_ID_CODE_HASH: &str =
        "0x00000000000000000000000000000000000000000000000000545950455f4944";

    fn create_parsed_cell_with_type(
        capacity: i64,
        lock_args: &str,
        type_code_hash: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        data_size: i32,
    ) -> ParsedCell {
        let lock = create_lock_script(lock_args);
        let lock_script_hash = ScriptParser::compute_script_hash(&lock);
        ParsedCell {
            capacity,
            lock_code_hash: crate::rpc::parse_hex_to_bytes(SECP256K1_CODE_HASH),
            lock_hash_type: 1,
            lock_args: crate::rpc::parse_hex_to_bytes(lock_args),
            lock_script_hash,
            type_code_hash,
            type_hash_type: Some(1),
            type_args: Some(vec![0x12; 32]),
            type_script_hash,
            data_hash: vec![0xaa; 32],
            data_size,
            data: vec![0u8; data_size as usize],
        }
    }

    #[test]
    fn test_parse_script_deploy_type_id() {
        let tx_hash = vec![40u8; 32];
        let type_code_hash = crate::rpc::parse_hex_to_bytes(TYPE_ID_CODE_HASH);
        let type_script_hash = vec![0xbb; 32];
        let deployer_lock_hash = vec![0xcc; 32];

        let mut cell = create_parsed_cell_with_type(
            500_00000000,
            "0xdeployer",
            Some(type_code_hash),
            Some(type_script_hash.clone()),
            1000,
        );
        cell.lock_script_hash = deployer_lock_hash.clone();

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell], 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::ScriptDeploy);
        assert_eq!(activity.activity_category, ActivityCategory::Script);
        assert_eq!(activity.amount, "1000");
        assert!(activity.from_lock_hash.is_none());
        assert_eq!(activity.to_lock_hash, Some(deployer_lock_hash));
        assert_eq!(activity.asset_id, Some(type_script_hash.clone()));

        let metadata = &activity.metadata;
        assert_eq!(metadata["type"], "script");
        assert!(metadata["codeHash"].as_str().unwrap().starts_with("0x"));
        assert_eq!(metadata["dataSize"], 1000);
    }

    #[test]
    fn test_parse_script_deploy_no_data() {
        let tx_hash = vec![41u8; 32];
        let type_code_hash = crate::rpc::parse_hex_to_bytes(TYPE_ID_CODE_HASH);
        let type_script_hash = vec![0xdd; 32];

        let cell = create_parsed_cell_with_type(
            500_00000000,
            "0xnodata",
            Some(type_code_hash),
            Some(type_script_hash),
            0,
        );

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell], 0);

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_script_deploy_no_type_script() {
        let tx_hash = vec![42u8; 32];

        let mut cell = create_parsed_cell(500_00000000, "0xnotype");
        cell.data_size = 1000;
        cell.data = vec![0u8; 1000];

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell], 0);

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_script_deploy_significant_data_non_type_id() {
        let tx_hash = vec![43u8; 32];
        let custom_type_code_hash = vec![0xee; 32];
        let type_script_hash = vec![0xff; 32];

        let cell = create_parsed_cell_with_type(
            500_00000000,
            "0xcustom",
            Some(custom_type_code_hash),
            Some(type_script_hash.clone()),
            500,
        );

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell], 0);

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::ScriptDeploy);
        assert_eq!(activity.asset_id, Some(type_script_hash));
    }

    #[test]
    fn test_parse_script_deploy_small_data_non_type_id_skipped() {
        let tx_hash = vec![44u8; 32];
        let custom_type_code_hash = vec![0x11; 32];
        let type_script_hash = vec![0x22; 32];

        let cell = create_parsed_cell_with_type(
            500_00000000,
            "0xsmall",
            Some(custom_type_code_hash),
            Some(type_script_hash),
            30,
        );

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell], 0);

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_script_deploy_multiple() {
        let tx_hash = vec![45u8; 32];
        let type_code_hash = crate::rpc::parse_hex_to_bytes(TYPE_ID_CODE_HASH);

        let cell1 = create_parsed_cell_with_type(
            500_00000000,
            "0xdeploy1",
            Some(type_code_hash.clone()),
            Some(vec![0x01; 32]),
            1000,
        );
        let cell2 = create_parsed_cell_with_type(
            500_00000000,
            "0xdeploy2",
            Some(type_code_hash.clone()),
            Some(vec![0x02; 32]),
            2000,
        );

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell1, cell2], 0);

        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].activity_index, 0);
        assert_eq!(activities[1].activity_index, 1);
        assert_eq!(activities[0].amount, "1000");
        assert_eq!(activities[1].amount, "2000");
    }

    #[test]
    fn test_parse_script_deploy_activity_index_start() {
        let tx_hash = vec![46u8; 32];
        let type_code_hash = crate::rpc::parse_hex_to_bytes(TYPE_ID_CODE_HASH);

        let cell = create_parsed_cell_with_type(
            500_00000000,
            "0xidxtest",
            Some(type_code_hash),
            Some(vec![0x33; 32]),
            1000,
        );

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell], 10);

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].activity_index, 10);
    }

    #[test]
    fn test_parse_script_deploy_metadata_format() {
        let tx_hash = vec![47u8; 32];
        let type_code_hash = crate::rpc::parse_hex_to_bytes(TYPE_ID_CODE_HASH);
        let type_script_hash = vec![0x44; 32];

        let cell = create_parsed_cell_with_type(
            500_00000000,
            "0xmetadata",
            Some(type_code_hash),
            Some(type_script_hash.clone()),
            5000,
        );

        let activities = ActivityParser::parse_script_deployments(&tx_hash, 0, &[cell], 0);

        let metadata = &activities[0].metadata;
        let expected_code_hash = format!("0x{}", hex::encode(&type_script_hash));
        assert_eq!(metadata["codeHash"], expected_code_hash);
        assert_eq!(metadata["dataSize"], 5000);
    }

    fn create_rgbpp_lock_cell(
        capacity: i64,
        type_script_hash: Vec<u8>,
        btc_out_index: u32,
        btc_txid: &[u8; 32],
        is_mainnet: bool,
    ) -> ParsedCell {
        use crate::parser::rgbpp::{RGBPP_LOCK_CODE_HASH_MAINNET, RGBPP_LOCK_CODE_HASH_TESTNET};

        let code_hash = if is_mainnet {
            RGBPP_LOCK_CODE_HASH_MAINNET
        } else {
            RGBPP_LOCK_CODE_HASH_TESTNET
        };

        let mut lock_args = btc_out_index.to_le_bytes().to_vec();
        let mut reversed_txid = *btc_txid;
        reversed_txid.reverse();
        lock_args.extend_from_slice(&reversed_txid);

        let lock_code_hash = crate::rpc::parse_hex_to_bytes(code_hash);
        let lock_script_hash = {
            use ckb_hash::new_blake2b;
            let mut hasher = new_blake2b();
            hasher.update(&lock_code_hash);
            hasher.update(&[1u8]);
            hasher.update(&lock_args);
            let mut hash = vec![0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };

        ParsedCell {
            capacity,
            lock_code_hash,
            lock_hash_type: 1,
            lock_args,
            lock_script_hash,
            type_code_hash: Some(vec![0xaa; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0xbb; 32]),
            type_script_hash: Some(type_script_hash),
            data_hash: vec![0xcc; 32],
            data_size: 16,
            data: vec![0u8; 16],
        }
    }

    fn create_btc_time_lock_cell(
        capacity: i64,
        type_script_hash: Vec<u8>,
        btc_txid: &[u8; 32],
        is_mainnet: bool,
    ) -> ParsedCell {
        use crate::parser::rgbpp::{
            BTC_TIME_LOCK_CODE_HASH_MAINNET, BTC_TIME_LOCK_CODE_HASH_TESTNET,
        };

        let code_hash = if is_mainnet {
            BTC_TIME_LOCK_CODE_HASH_MAINNET
        } else {
            BTC_TIME_LOCK_CODE_HASH_TESTNET
        };

        let mut lock_args = vec![0u8; 68];
        let mut reversed_txid = *btc_txid;
        reversed_txid.reverse();
        lock_args[36..68].copy_from_slice(&reversed_txid);

        let lock_code_hash = crate::rpc::parse_hex_to_bytes(code_hash);
        let lock_script_hash = {
            use ckb_hash::new_blake2b;
            let mut hasher = new_blake2b();
            hasher.update(&lock_code_hash);
            hasher.update(&[1u8]);
            hasher.update(&lock_args);
            let mut hash = vec![0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };

        ParsedCell {
            capacity,
            lock_code_hash,
            lock_hash_type: 1,
            lock_args,
            lock_script_hash,
            type_code_hash: Some(vec![0xdd; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0xee; 32]),
            type_script_hash: Some(type_script_hash),
            data_hash: vec![0xff; 32],
            data_size: 16,
            data: vec![0u8; 16],
        }
    }

    fn create_regular_typed_cell(
        capacity: i64,
        lock_args: &str,
        type_script_hash: Vec<u8>,
    ) -> ParsedCell {
        let lock = create_lock_script(lock_args);
        let lock_script_hash = ScriptParser::compute_script_hash(&lock);
        ParsedCell {
            capacity,
            lock_code_hash: crate::rpc::parse_hex_to_bytes(SECP256K1_CODE_HASH),
            lock_hash_type: 1,
            lock_args: crate::rpc::parse_hex_to_bytes(lock_args),
            lock_script_hash,
            type_code_hash: Some(vec![0x11; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x22; 32]),
            type_script_hash: Some(type_script_hash),
            data_hash: vec![0x33; 32],
            data_size: 16,
            data: vec![0u8; 16],
        }
    }

    #[test]
    fn test_parse_rgbpp_transfer() {
        let tx_hash = vec![50u8; 32];
        let type_script_hash = vec![0x01; 32];
        let btc_txid_in: [u8; 32] = [0xaa; 32];
        let btc_txid_out: [u8; 32] = [0xbb; 32];

        let input_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid_in,
            true,
        )];
        let output_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid_out,
            true,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            0,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::RgbppTransfer);
        assert_eq!(activity.activity_category, ActivityCategory::Rgbpp);
        assert_eq!(activity.amount, "10000000000");
        assert!(activity.from_lock_hash.is_some());
        assert!(activity.to_lock_hash.is_some());
        assert_eq!(activity.asset_id, Some(type_script_hash));

        let metadata = &activity.metadata;
        assert!(metadata["btcTxid"].is_string());
    }

    #[test]
    fn test_parse_rgbpp_leap_in() {
        let tx_hash = vec![51u8; 32];
        let type_script_hash = vec![0x02; 32];
        let btc_txid_in: [u8; 32] = [0xcc; 32];
        let btc_txid_out: [u8; 32] = [0xdd; 32];

        let input_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid_in,
            true,
        )];
        let output_cells = vec![create_btc_time_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            &btc_txid_out,
            true,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            1,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::RgbppLeapIn);
        assert_eq!(activity.activity_category, ActivityCategory::Rgbpp);
        assert!(activity.from_lock_hash.is_some());
        assert!(activity.to_lock_hash.is_some());
        assert_eq!(activity.asset_id, Some(type_script_hash));
    }

    #[test]
    fn test_parse_rgbpp_leap_out() {
        let tx_hash = vec![52u8; 32];
        let type_script_hash = vec![0x03; 32];
        let btc_txid_out: [u8; 32] = [0xee; 32];

        let input_cells = vec![create_regular_typed_cell(
            100_00000000,
            "0xsender",
            type_script_hash.clone(),
        )];
        let output_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid_out,
            true,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            2,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::RgbppLeapOut);
        assert_eq!(activity.activity_category, ActivityCategory::Rgbpp);
        assert!(activity.from_lock_hash.is_some());
        assert!(activity.to_lock_hash.is_some());
        assert_eq!(activity.asset_id, Some(type_script_hash));
    }

    #[test]
    fn test_parse_rgbpp_issuance() {
        let tx_hash = vec![53u8; 32];
        let input_type_hash = vec![0x04; 32];
        let output_type_hash = vec![0x05; 32];
        let btc_txid_out: [u8; 32] = [0xff; 32];

        let input_cells = vec![create_regular_typed_cell(
            100_00000000,
            "0xissuer",
            input_type_hash.clone(),
        )];
        let output_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            output_type_hash.clone(),
            0,
            &btc_txid_out,
            true,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            3,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity.activity_type, ActivityType::RgbppIssuance);
        assert_eq!(activity.activity_category, ActivityCategory::Rgbpp);
        assert_eq!(activity.asset_id, Some(output_type_hash));
    }

    #[test]
    fn test_parse_rgbpp_no_typed_cells() {
        let tx_hash = vec![54u8; 32];
        let input_cells = vec![create_parsed_cell(100_00000000, "0xsender")];
        let output_cells = vec![create_parsed_cell(100_00000000, "0xreceiver")];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            0,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_rgbpp_no_rgbpp_cells() {
        let tx_hash = vec![55u8; 32];
        let type_script_hash = vec![0x06; 32];

        let input_cells = vec![create_regular_typed_cell(
            100_00000000,
            "0xsender",
            type_script_hash.clone(),
        )];
        let output_cells = vec![create_regular_typed_cell(
            100_00000000,
            "0xreceiver",
            type_script_hash,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            0,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        assert!(activities.is_empty());
    }

    #[test]
    fn test_parse_rgbpp_testnet() {
        let tx_hash = vec![56u8; 32];
        let type_script_hash = vec![0x07; 32];
        let btc_txid: [u8; 32] = [0x11; 32];

        let input_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid,
            false,
        )];
        let output_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            1,
            &btc_txid,
            false,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            0,
            &output_cells,
            &input_cells,
            false,
            0,
        );

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].activity_type, ActivityType::RgbppTransfer);
    }

    #[test]
    fn test_parse_rgbpp_activity_index_start() {
        let tx_hash = vec![57u8; 32];
        let type_script_hash = vec![0x08; 32];
        let btc_txid: [u8; 32] = [0x22; 32];

        let input_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid,
            true,
        )];
        let output_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid,
            true,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            0,
            &output_cells,
            &input_cells,
            true,
            5,
        );

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].activity_index, 5);
    }

    #[test]
    fn test_parse_rgbpp_multiple_outputs() {
        let tx_hash = vec![58u8; 32];
        let type_script_hash_1 = vec![0x09; 32];
        let type_script_hash_2 = vec![0x0a; 32];
        let btc_txid: [u8; 32] = [0x33; 32];

        let input_cells = vec![create_rgbpp_lock_cell(
            200_00000000,
            type_script_hash_1.clone(),
            0,
            &btc_txid,
            true,
        )];
        let output_cells = vec![
            create_rgbpp_lock_cell(100_00000000, type_script_hash_1.clone(), 0, &btc_txid, true),
            create_rgbpp_lock_cell(100_00000000, type_script_hash_2.clone(), 1, &btc_txid, true),
        ];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            0,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].activity_type, ActivityType::RgbppTransfer);
        assert_eq!(activities[1].activity_type, ActivityType::RgbppTransfer);
        assert_eq!(activities[0].activity_index, 0);
        assert_eq!(activities[1].activity_index, 1);
    }

    #[test]
    fn test_parse_rgbpp_metadata_format() {
        let tx_hash = vec![59u8; 32];
        let type_script_hash = vec![0x0b; 32];
        let btc_txid: [u8; 32] = [0x44; 32];

        let input_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid,
            true,
        )];
        let output_cells = vec![create_rgbpp_lock_cell(
            100_00000000,
            type_script_hash.clone(),
            0,
            &btc_txid,
            true,
        )];

        let activities = ActivityParser::parse_rgbpp_activities(
            &tx_hash,
            0,
            &output_cells,
            &input_cells,
            true,
            0,
        );

        let metadata = &activities[0].metadata;
        assert_eq!(metadata["type"], "rgbpp");
        assert!(metadata["assetId"].as_str().unwrap().starts_with("0x"));
    }
}
