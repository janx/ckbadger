//! RGB++ protocol detector: identifies cross-chain actions by analyzing lock transitions.

use ckbadger_store::types::ProtocolAction;

use crate::parser::rgbpp::{RgbppLockType, RgbppParser};

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

/// Which side of a transaction a cell appears on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellSide {
    Input,
    Output,
}

/// Lock classification for a cell participating in an RGB++ type-group.
#[derive(Debug, Clone)]
struct TypeGroupCell {
    side: CellSide,
    lock_type: RgbppLockType,
    lock_script_hash: Vec<u8>,
    lock_args: Vec<u8>,
}

pub(crate) struct RgbppDetector {
    is_mainnet: bool,
}

impl RgbppDetector {
    pub fn new(is_mainnet: bool) -> Self {
        Self { is_mainnet }
    }

    /// Determine the RGB++ action from the lock transitions within a type-group,
    /// filtered to only produce actions relevant to the given owner.
    fn classify_action(
        &self,
        cells: &[TypeGroupCell],
        owner_lock_hash: &[u8],
    ) -> Vec<ProtocolAction> {
        // Separate into inputs and outputs
        let inputs: Vec<&TypeGroupCell> =
            cells.iter().filter(|c| c.side == CellSide::Input).collect();
        let outputs: Vec<&TypeGroupCell> = cells
            .iter()
            .filter(|c| c.side == CellSide::Output)
            .collect();

        // Check if owner is involved in any cell of this type-group
        let owner_in_inputs = inputs.iter().any(|c| c.lock_script_hash == owner_lock_hash);
        let owner_in_outputs = outputs
            .iter()
            .any(|c| c.lock_script_hash == owner_lock_hash);
        if !owner_in_inputs && !owner_in_outputs {
            return vec![];
        }

        // Check if any input/output has rgbpp or btc_time_lock
        let has_rgbpp_input = inputs
            .iter()
            .any(|c| c.lock_type == RgbppLockType::RgbppLock);
        let has_btc_time_input = inputs
            .iter()
            .any(|c| c.lock_type == RgbppLockType::BtcTimeLock);
        let has_rgbpp_output = outputs
            .iter()
            .any(|c| c.lock_type == RgbppLockType::RgbppLock);
        let has_btc_time_output = outputs
            .iter()
            .any(|c| c.lock_type == RgbppLockType::BtcTimeLock);
        let has_standard_input = inputs.iter().any(|c| c.lock_type == RgbppLockType::Other);
        let has_standard_output = outputs.iter().any(|c| c.lock_type == RgbppLockType::Other);

        // At least one side must have an rgbpp-family lock
        if !has_rgbpp_input && !has_btc_time_input && !has_rgbpp_output && !has_btc_time_output {
            return vec![];
        }

        // Determine action based on lock transitions
        let action = if has_rgbpp_input && has_rgbpp_output && !has_standard_output {
            // rgbpp -> rgbpp: transfer (different BTC UTXO implied by different args)
            "transfer"
        } else if (has_rgbpp_input || has_btc_time_input) && has_standard_output {
            // rgbpp/btc_time_lock -> standard CKB lock: leap to CKB
            "leap_to_ckb"
        } else if has_standard_input && has_rgbpp_output {
            // standard CKB lock -> rgbpp: leap to BTC
            "leap_to_btc"
        } else if has_rgbpp_input && has_btc_time_output {
            // rgbpp -> btc_time_lock: time-locked
            "btc_time_locked"
        } else if !has_rgbpp_input && !has_btc_time_input && has_rgbpp_output {
            // no matching input -> rgbpp output: receive (issuance on BTC side)
            "receive"
        } else {
            // No recognized pattern
            return vec![];
        };

        // Extract metadata from the first rgbpp-family output (preferred) or input
        let metadata = self.extract_metadata(&inputs, &outputs);

        vec![ProtocolAction::new("rgbpp", action, metadata)]
    }

    /// Extract BTC txid and output index from rgbpp lock args.
    fn extract_metadata(
        &self,
        inputs: &[&TypeGroupCell],
        outputs: &[&TypeGroupCell],
    ) -> serde_json::Value {
        // Prefer output rgbpp lock for metadata (represents destination)
        for cell in outputs.iter().chain(inputs.iter()) {
            match cell.lock_type {
                RgbppLockType::RgbppLock => {
                    if let Some(args) = RgbppParser::parse_rgbpp_lock_args(&cell.lock_args) {
                        return serde_json::json!({
                            "btcTxid": args.btc_txid,
                            "outIndex": args.out_index,
                        });
                    }
                }
                RgbppLockType::BtcTimeLock => {
                    if let Some(btc_txid) =
                        RgbppParser::extract_btc_txid_from_btc_time_lock_args(&cell.lock_args)
                    {
                        return serde_json::json!({
                            "btcTxid": btc_txid,
                        });
                    }
                }
                RgbppLockType::Other => {}
            }
        }
        serde_json::Value::Null
    }
}

impl ProtocolDetector for RgbppDetector {
    fn might_apply_batch(
        &self,
        lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
        _type_code_hashes: &std::collections::HashSet<[u8; 32]>,
    ) -> bool {
        lock_code_hashes
            .iter()
            .any(|h| RgbppParser::detect_lock_type(h, self.is_mainnet) != RgbppLockType::Other)
    }

    fn might_apply(&self, tx: &TxView<'_>) -> bool {
        tx.inputs.iter().any(|input| {
            RgbppParser::detect_lock_type(input.lock_code_hash, self.is_mainnet)
                != RgbppLockType::Other
        }) || tx.outputs.iter().any(|output| {
            RgbppParser::detect_lock_type(output.lock_code_hash, self.is_mainnet)
                != RgbppLockType::Other
        })
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        _accum: &OwnerAccum<'_>,
        _asset_changes: &[ckbadger_store::types::AssetChange],
        _type_calls: &[ckbadger_store::types::TypeCallEntry],
        _lock_calls: &[ckbadger_store::types::LockCallEntry],
    ) -> Vec<ProtocolAction> {
        // Group cells by type_script identity (type_code_hash + type_args).
        // Skip cells without type scripts.
        use std::collections::BTreeMap;

        // Key: (type_code_hash, type_args) — BTreeMap for deterministic iteration order
        let mut type_groups: BTreeMap<(Vec<u8>, Vec<u8>), Vec<TypeGroupCell>> = BTreeMap::new();

        // Process inputs
        for input in &tx.inputs {
            if let (Some(type_code_hash), Some(type_args)) = (input.type_code_hash, input.type_args)
            {
                let lock_type =
                    RgbppParser::detect_lock_type(input.lock_code_hash, self.is_mainnet);
                let key = (type_code_hash.to_vec(), type_args.to_vec());
                type_groups.entry(key).or_default().push(TypeGroupCell {
                    side: CellSide::Input,
                    lock_type,
                    lock_script_hash: input.lock_script_hash.to_vec(),
                    lock_args: input.lock_args.to_vec(),
                });
            }
        }

        // Process outputs
        for output in &tx.outputs {
            if let (Some(type_code_hash), Some(type_args)) =
                (output.type_code_hash, output.type_args)
            {
                let lock_type =
                    RgbppParser::detect_lock_type(output.lock_code_hash, self.is_mainnet);
                let key = (type_code_hash.to_vec(), type_args.to_vec());
                type_groups.entry(key).or_default().push(TypeGroupCell {
                    side: CellSide::Output,
                    lock_type,
                    lock_script_hash: output.lock_script_hash.to_vec(),
                    lock_args: output.lock_args.to_vec(),
                });
            }
        }

        // For each type-group, classify the action
        let mut actions: Vec<ProtocolAction> = Vec::new();
        for cells in type_groups.values() {
            actions.extend(self.classify_action(cells, owner_lock_hash));
        }

        actions
    }
}
