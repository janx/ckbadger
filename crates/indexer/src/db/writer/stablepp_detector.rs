//! Stable++ protocol detector: classifies each transaction's CDP effect from
//! TX-LEVEL facts — which vault / intent / pool cells it consumes and creates,
//! and which way it moves RUSD supply.
//!
//! Stable++ actions describe what a TRANSACTION did, so they are computed once
//! from the transaction and are independent of which participant is being
//! indexed. Deriving them per-owner previously let one transaction claim to be
//! a borrow, an adjust and a repay at the same time, and let a participant's
//! private RUSD balance change masquerade as a protocol event.

use std::cmp::Ordering;

use ckbadger_store::types::{ItemDelta, LockCallEntry, ProtocolAction, TypeCallEntry};

use crate::parser::stablepp::{
    is_stablepp_asset, is_stablepp_intent_lock, is_stablepp_pool, is_stablepp_vault_lock,
};
use crate::parser::udt::UdtParser;

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

pub(crate) struct StableppDetector;

/// Arbitrary-width unsigned sum stored as little-endian 64-bit limbs. RUSD
/// supply is compared as two exact totals, so a transaction whose inflow or
/// outflow exceeds `u128` still classifies exactly.
#[derive(Default, Eq, PartialEq)]
struct ExactMagnitude {
    limbs: Vec<u64>,
}

impl ExactMagnitude {
    fn add_u128(&mut self, value: u128) {
        self.add_limb(0, value as u64);
        self.add_limb(1, (value >> 64) as u64);
    }

    fn add_limb(&mut self, mut index: usize, mut value: u64) {
        while value != 0 {
            if self.limbs.len() <= index {
                self.limbs.resize(index + 1, 0);
            }
            let (sum, carry) = self.limbs[index].overflowing_add(value);
            self.limbs[index] = sum;
            value = u64::from(carry);
            index += 1;
        }
    }
}

impl Ord for ExactMagnitude {
    fn cmp(&self, other: &Self) -> Ordering {
        self.limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }
}

impl PartialOrd for ExactMagnitude {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Which way a transaction moved total RUSD supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupplyDirection {
    /// RUSD was created — new debt was drawn.
    Mint,
    /// RUSD was destroyed — debt was retired.
    Burn,
    /// Supply unchanged; RUSD only moved between holders.
    Flat,
}

impl SupplyDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mint => "mint",
            Self::Burn => "burn",
            Self::Flat => "flat",
        }
    }
}

/// The transaction-level facts a Stable++ classification is derived from.
struct TxFacts {
    /// Vault cells consumed.
    vault_in: usize,
    /// Vault cells created.
    vault_out: usize,
    /// Stable++ intent cells consumed.
    intent_in: usize,
    /// Whether any Stable++ *structural* cell (vault, intent or pool) takes
    /// part. RUSD cells alone do NOT count: moving the token is not a protocol
    /// event, it is an ordinary token transfer that the token tag already
    /// describes.
    structure_involved: bool,
    /// Exact total RUSD entering the transaction.
    supply_in: ExactMagnitude,
    /// Exact total RUSD leaving the transaction.
    supply_out: ExactMagnitude,
}

impl TxFacts {
    fn from_tx(tx: &TxView<'_>) -> Self {
        let mut facts = Self {
            vault_in: 0,
            vault_out: 0,
            intent_in: 0,
            structure_involved: false,
            supply_in: ExactMagnitude::default(),
            supply_out: ExactMagnitude::default(),
        };

        for input in &tx.inputs {
            if is_stablepp_vault_lock(input.lock_code_hash) {
                facts.vault_in += 1;
                facts.structure_involved = true;
            }
            if is_stablepp_intent_lock(input.lock_code_hash) {
                facts.intent_in += 1;
                facts.structure_involved = true;
            }
            if input.type_code_hash.is_some_and(is_stablepp_pool) {
                facts.structure_involved = true;
            }
            if input.type_code_hash.is_some_and(is_stablepp_asset) {
                // Consumed cells carry no raw data on either sync path, so the
                // amount must come from the pre-resolved `udt_amount` — the
                // same value `classify_input` uses.
                facts
                    .supply_in
                    .add_u128(input.udt_amount.unwrap_or_else(|| {
                        tracing::warn!(
                            tx_hash = %hex::encode(tx.tx_hash),
                            block_number = tx.block_number,
                            "consumed Stable++ asset cell has no resolved xUDT amount; \
                             treating it as zero for supply accounting"
                        );
                        0
                    }));
            }
        }

        for output in &tx.outputs {
            if is_stablepp_vault_lock(output.lock_code_hash) {
                facts.vault_out += 1;
                facts.structure_involved = true;
            }
            if is_stablepp_intent_lock(output.lock_code_hash) {
                facts.structure_involved = true;
            }
            if output.type_code_hash.is_some_and(is_stablepp_pool) {
                facts.structure_involved = true;
            }
            if output.type_code_hash.is_some_and(is_stablepp_asset) {
                // Created cells do carry their raw data on both sync paths.
                facts
                    .supply_out
                    .add_u128(stablepp_asset_amount(output.data, tx));
            }
        }

        facts
    }

    fn supply_direction(&self) -> SupplyDirection {
        match self.supply_out.cmp(&self.supply_in) {
            Ordering::Greater => SupplyDirection::Mint,
            Ordering::Less => SupplyDirection::Burn,
            Ordering::Equal => SupplyDirection::Flat,
        }
    }

    /// Number of distinct vault positions the transaction acts on.
    fn vault_count(&self) -> usize {
        std::cmp::max(self.vault_in, self.vault_out)
    }

    /// The single Stable++ action this transaction represents, or `None` when it
    /// is not a Stable++ protocol event at all.
    ///
    /// Note there is deliberately no `liquidation` outcome. Every one of the 68
    /// vault-closing transactions in mainnet history consumes an intent cell
    /// belonging to the same owner whose vault closes and pays that owner out —
    /// i.e. all closes are owner-initiated. No chain-observable discriminator
    /// for a forced close exists, so emitting "liquidation" would be inventing
    /// a label rather than reporting one.
    fn classify(&self) -> Option<&'static str> {
        if !self.structure_involved {
            return None;
        }

        Some(match (self.vault_in > 0, self.vault_out > 0) {
            // A vault appeared where none was consumed.
            (false, true) => "open_vault",
            // A vault was consumed and not recreated.
            (true, false) => "close_vault",
            // The vault survives: the debt direction names the action.
            (true, true) => match self.supply_direction() {
                SupplyDirection::Mint => "borrow",
                SupplyDirection::Burn => "repay",
                SupplyDirection::Flat => "adjust",
            },
            // No vault changed hands. Redemption is the only claim we can make
            // here, and only when RUSD was actually destroyed against a
            // consumed intent — a transaction that merely returns RUSD to its
            // owner has redeemed nothing.
            (false, false) => {
                if self.intent_in > 0 && self.supply_direction() == SupplyDirection::Burn {
                    "redemption"
                } else {
                    "interaction"
                }
            }
        })
    }
}

/// Amount held by a newly created Stable++ asset cell.
///
/// The Stable++ asset is xUDT-compatible, so a cell carrying that type script
/// always holds a 16-byte little-endian amount; a cell that does not is a
/// protocol violation the on-chain script would have rejected. It is reported
/// loudly rather than silently distorting the supply totals.
fn stablepp_asset_amount(data: &[u8], tx: &TxView<'_>) -> u128 {
    match UdtParser::parse_amount(data) {
        Some(amount) => amount,
        None => {
            tracing::warn!(
                tx_hash = %hex::encode(tx.tx_hash),
                block_number = tx.block_number,
                data_len = data.len(),
                "created Stable++ asset cell carries no readable xUDT amount; \
                 treating it as zero for supply accounting"
            );
            0
        }
    }
}

impl StableppDetector {
    pub fn new(_is_mainnet: bool) -> Self {
        Self
    }
}

impl ProtocolDetector for StableppDetector {
    fn might_apply_batch(
        &self,
        lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
        type_code_hashes: &std::collections::HashSet<[u8; 32]>,
    ) -> bool {
        lock_code_hashes
            .iter()
            .any(|h| is_stablepp_vault_lock(h) || is_stablepp_intent_lock(h))
            || type_code_hashes.iter().any(|h| is_stablepp_pool(h))
    }

    fn might_apply(&self, tx: &TxView<'_>) -> bool {
        tx.inputs.iter().any(|input| {
            is_stablepp_vault_lock(input.lock_code_hash)
                || is_stablepp_intent_lock(input.lock_code_hash)
                || input.type_code_hash.is_some_and(is_stablepp_pool)
        }) || tx.outputs.iter().any(|output| {
            is_stablepp_vault_lock(output.lock_code_hash)
                || is_stablepp_intent_lock(output.lock_code_hash)
                || output.type_code_hash.is_some_and(is_stablepp_pool)
        })
    }

    /// Stable++ actions are transaction-level facts. The same action is
    /// returned for every participant of the transaction and the writer's
    /// dedup collapses them into the single action the transaction represents.
    fn detect(
        &self,
        tx: &TxView<'_>,
        _owner_lock_hash: &[u8],
        _accum: &OwnerAccum<'_>,
        _item_deltas: &[ItemDelta],
        _type_calls: &[TypeCallEntry],
        _lock_calls: &[LockCallEntry],
    ) -> anyhow::Result<Vec<ProtocolAction>> {
        let facts = TxFacts::from_tx(tx);
        let Some(action) = facts.classify() else {
            return Ok(vec![]);
        };

        let metadata = serde_json::json!({
            "hasIntent": facts.intent_in > 0,
            "vaultCount": facts.vault_count(),
            "supplyDirection": facts.supply_direction().as_str(),
        });

        Ok(vec![ProtocolAction::new("stablepp", action, metadata)])
    }
}
#[cfg(test)]
#[allow(clippy::useless_vec)]
mod tests {
    use super::*;
    use crate::db::writer::activities::{build_tx_actions_for_block, OutputCellView, TxView};
    use crate::rpc::parse_hex_to_bytes;

    // --- Stable++ mainnet script identities -------------------------------

    /// Corrected Stable++ vault lock code_hash (registry-mapped). The parser const
    /// VAULT_LOCK_CODE_HASH_MAINNET still holds the old pool-guard value (0xff35…),
    /// which detection no longer treats as a vault.
    const SPP_VAULT_LOCK: &str =
        "0x4ed68fcb7eaa4ff78d46a2fad88a32ce9caffd4b96a0a4bba96ff4871f018675";
    const SPP_INTENT_LOCK: &str =
        "0x56fb632a13abdad7308d2e034baae1cb049e8e8ff23cc7c0b69449f617549733";
    /// The pool cell's lock (a guard lock, not itself a Stable++ identity); the
    /// pool is identified by its TYPE script below.
    const SPP_POOL_GUARD_LOCK: &str =
        "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67";
    const SPP_POOL_TYPE: &str =
        "0x26622198b66240e437e323e0fecf1c26ba3c8c28a45f03ed3ebb9f7f2bdc0055";
    /// RUSD — the Stable++ debt asset (xUDT-compatible).
    const RUSD_TYPE: &str = "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b";

    // Non-Stable++ scripts appearing in the captured fixtures.
    const VAULT_DATA_TYPE: &str =
        "0xbf47c98d8beb7c745999b6fdb77612808878d58f875e3820d5057a50ea95cb62";
    const INTENT_DATA_TYPE: &str =
        "0x42a0b2aacc836c0fc2bbd421a9020de42b8411584190f30be547fdf54214acc3";
    const INTENT_REQ_TYPE: &str =
        "0x24b5173b731c24302be73db39dc0a62397f6b92ba17d8f15b920795f0a4f3b75";
    const SECP_LOCK: &str = "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
    const JOYID_LOCK: &str = "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323";
    const UTXOSWAP_INTENT_LOCK: &str =
        "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e";
    const OTHER_LOCK_A: &str = "0x7ef6e226ca9a3514ac76759f0b1550e70c9aa10aff89111fedf2c9d800d256f7";
    const OTHER_LOCK_B: &str = "0x393df3359e33f85010cd65a3c4a4268f72d95ec6b049781a916c680b31ea9a88";
    const OTHER_TYPE_A: &str = "0xc70a8b00526419826023bcf196852eecdc87406cdff7366234f6387265413c98";

    // --- Fixture harness ---------------------------------------------------

    struct OwnedInput {
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        udt_amount: Option<u128>,
        capacity: i64,
        data: Vec<u8>,
    }

    impl OwnedInput {
        fn view(&self) -> crate::db::writer::activities::InputCellView<'_> {
            crate::db::writer::activities::InputCellView {
                previous_tx_hash: &[0u8; 32],
                previous_output_index: 0,
                lock_script_hash: &self.lock_script_hash,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                capacity: self.capacity,
                occupied_capacity: 61_00000000,
                type_code_hash: self.type_code_hash.as_deref(),
                type_hash_type: Some(1),
                type_script_hash: self.type_script_hash.as_deref(),
                type_args: self.type_args.as_deref(),
                udt_amount: self.udt_amount,
                bit_cell_identity_id: None,
                data: &self.data,
                is_dao_withdraw_request: false,
                dao_compensation: None,
            }
        }
    }

    struct OwnedOutput {
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        capacity: i64,
        data: Vec<u8>,
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
                type_hash_type: Some(1),
                type_args: self.type_args.as_deref(),
                type_script_hash: self.type_script_hash.as_deref(),
                data_hash: &[],
                data_size: self.data.len() as i32,
                data: &self.data,
            }
        }
    }

    /// One cell of a captured transaction.
    struct FixtureCell {
        is_input: bool,
        lock_code_hash: &'static str,
        lock_args: &'static str,
        capacity: i64,
        type_code_hash: Option<&'static str>,
        udt_amount: Option<u128>,
    }

    fn cell(
        is_input: bool,
        lock_code_hash: &'static str,
        lock_args: &'static str,
        capacity: i64,
        type_code_hash: Option<&'static str>,
        udt_amount: Option<u128>,
    ) -> FixtureCell {
        FixtureCell {
            is_input,
            lock_code_hash,
            lock_args,
            capacity,
            type_code_hash,
            udt_amount,
        }
    }

    /// Build a `TxView` from captured cells and return the Stable++ protocol
    /// actions the detector emits, as `(action, metadata)` pairs.
    ///
    /// Real 32-byte lock/type script hashes are not part of the capture, so each
    /// distinct `(lock_code_hash, lock_args)` pair is given a unique synthetic
    /// script hash. Owner grouping only needs identity, not the real hash.
    fn stablepp_actions(cells: &[FixtureCell]) -> Vec<(String, serde_json::Value)> {
        let mut lock_keys: Vec<(&str, &str)> = Vec::new();
        let mut type_keys: Vec<&str> = Vec::new();
        for c in cells {
            let key = (c.lock_code_hash, c.lock_args);
            if !lock_keys.contains(&key) {
                lock_keys.push(key);
            }
            if let Some(t) = c.type_code_hash {
                if !type_keys.contains(&t) {
                    type_keys.push(t);
                }
            }
        }
        assert!(lock_keys.len() < 200, "fixture has too many distinct locks");

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for c in cells {
            let lock_idx = lock_keys
                .iter()
                .position(|k| *k == (c.lock_code_hash, c.lock_args))
                .expect("lock key was collected above");
            let lock_script_hash = vec![lock_idx as u8 + 1; 32];
            let type_script_hash = c.type_code_hash.map(|t| {
                let i = type_keys
                    .iter()
                    .position(|k| *k == t)
                    .expect("type key was collected above");
                vec![i as u8 + 0x40; 32]
            });
            // UDT cells carry a 16-byte little-endian amount in their data.
            let data = c
                .udt_amount
                .map(|a| a.to_le_bytes().to_vec())
                .unwrap_or_default();
            let type_args = c.type_code_hash.map(|_| Vec::new());

            if c.is_input {
                inputs.push(OwnedInput {
                    lock_script_hash,
                    lock_code_hash: parse_hex_to_bytes(c.lock_code_hash),
                    lock_args: parse_hex_to_bytes(c.lock_args),
                    type_code_hash: c.type_code_hash.map(parse_hex_to_bytes),
                    type_script_hash,
                    type_args,
                    udt_amount: c.udt_amount,
                    capacity: c.capacity,
                    // Consumed cells reach the detector WITHOUT their raw data
                    // on both sync paths (`batch.rs` supplies only a resolved
                    // .bit account id, `bulk_build` supplies `&[]`). Mirroring
                    // that here keeps the fixtures honest: any attempt to read a
                    // consumed cell's amount out of `data` reads nothing and the
                    // supply-direction assertions fail.
                    data: Vec::new(),
                });
            } else {
                outputs.push(OwnedOutput {
                    lock_script_hash,
                    lock_code_hash: parse_hex_to_bytes(c.lock_code_hash),
                    lock_args: parse_hex_to_bytes(c.lock_args),
                    type_code_hash: c.type_code_hash.map(parse_hex_to_bytes),
                    type_script_hash,
                    type_args,
                    capacity: c.capacity,
                    data,
                });
            }
        }

        let tx = TxView {
            tx_hash: &[0x9A; 32],
            block_hash: &[0x9B; 32],
            tx_index: 1,
            block_number: 14_000_000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();
        assert_eq!(actions_list.len(), 1);
        actions_list[0]
            .protocol_actions
            .iter()
            .filter(|a| a.protocol == "stablepp")
            .map(|a| (a.action.clone(), a.metadata_value().unwrap()))
            .collect()
    }

    fn only_action(cells: &[FixtureCell]) -> (String, serde_json::Value) {
        let actions = stablepp_actions(cells);
        assert_eq!(
            actions.len(),
            1,
            "expected exactly one tx-level stablepp action, got {:?}",
            actions.iter().map(|(a, _)| a).collect::<Vec<_>>()
        );
        actions.into_iter().next().unwrap()
    }

    // =====================================================================
    // Captured mainnet transactions.
    //
    // Every fixture below is a byte-faithful capture of a real mainnet tx
    // (cells resolved through the node). Stable++ actions are TX-LEVEL facts:
    // one transaction yields at most one Stable++ action, derived from what the
    // transaction does to vault / intent / pool cells and to RUSD supply — never
    // from one participant's private view of it.
    // =====================================================================

    /// tx 0x43b21951c54c6aece9af6a21b7a326f63f7e4ff533a62ef03f91cf4e244620b0
    /// (block 14,355,116). Chain truth: 4 vaults in, 3 vaults out, RUSD supply
    /// mints 30,635,316,966. The per-owner detector labelled this borrow AND
    /// adjust AND repay simultaneously.
    fn tx_multi_vault_mint() -> Vec<FixtureCell> {
        vec![
            cell(
                true,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                181700000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                true,
                SPP_VAULT_LOCK,
                "0x00018b1db3cf3c744bbf0138548cd89d1aa86a5e2432",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_VAULT_LOCK,
                "0x00019ec33bf4568d5df59aca404af0ed3b621a7e7d17",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_VAULT_LOCK,
                "0x0001ae019b886c53df96d0d991ad0aa6f2f60f973a91",
                51400000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_VAULT_LOCK,
                "0x0001c6f59a2c7b37b9fbc1155eb83cf5dc1674bc741a",
                51400000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0xd8fadd4e4b46852ae9a74c02722ad4926d9401af0d979aef68f43a297f470bb8",
                1052490000000,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0xd8fadd4e4b46852ae9a74c02722ad4926d9401af0d979aef68f43a297f470bb8",
                15400000000,
                Some(RUSD_TYPE),
                Some(1000000000),
            ),
            cell(
                true,
                SECP_LOCK,
                "0xe7582665f3c045e5bd42ee75b7e12b0c3cb3a2e3",
                139199991449,
                None,
                None,
            ),
            cell(
                false,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                174300000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                false,
                SPP_VAULT_LOCK,
                "0x00019ec33bf4568d5df59aca404af0ed3b621a7e7d17",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SPP_INTENT_LOCK,
                "0xacb3869a352d98a5241b44ba4810f08d699345b2385d9c13bdf18a90859f5ac5",
                978739537037,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SPP_VAULT_LOCK,
                "0x0001ae019b886c53df96d0d991ad0aa6f2f60f973a91",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SPP_INTENT_LOCK,
                "0xa5b73f70370dbb90088832946d9731b91f5fef2ecf0f8126fa92859304525f86",
                49163039680,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SPP_VAULT_LOCK,
                "0x00018b1db3cf3c744bbf0138548cd89d1aa86a5e2432",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SPP_INTENT_LOCK,
                "0xc53b6204458e8af8c20e09efa67f44da309c27ab69096eac7eb8518545a434c2",
                24587423282,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x0001572d2aa3e716b98f0caa6883328c8cebbdd766d5",
                14400000000,
                Some(RUSD_TYPE),
                Some(31238290576),
            ),
            cell(
                false,
                SECP_LOCK,
                "0xe7582665f3c045e5bd42ee75b7e12b0c3cb3a2e3",
                14200000000,
                Some(RUSD_TYPE),
                Some(397026390),
            ),
            cell(
                false,
                SECP_LOCK,
                "0xe7582665f3c045e5bd42ee75b7e12b0c3cb3a2e3",
                178399979642,
                None,
                None,
            ),
        ]
    }

    /// tx 0x8ba080d7a97c5987dad91a8305b0b65358f4ca3304aea797102bb90714d7c9ce
    /// (block 14,387,443). One vault consumed, none created, RUSD burns.
    /// The old truth table labelled every such close a liquidation too (68/68).
    fn tx_close_vault() -> Vec<FixtureCell> {
        vec![
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x00018b1db3cf3c744bbf0138548cd89d1aa86a5e2432",
                60500000000,
                Some(INTENT_REQ_TYPE),
                None,
            ),
            cell(
                true,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                218700000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                true,
                SPP_VAULT_LOCK,
                "0x00018b1db3cf3c744bbf0138548cd89d1aa86a5e2432",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0xc53b6204458e8af8c20e09efa67f44da309c27ab69096eac7eb8518545a434c2",
                100000000000000,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0xc53b6204458e8af8c20e09efa67f44da309c27ab69096eac7eb8518545a434c2",
                24015775285,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0xc53b6204458e8af8c20e09efa67f44da309c27ab69096eac7eb8518545a434c2",
                24587423282,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0xc53b6204458e8af8c20e09efa67f44da309c27ab69096eac7eb8518545a434c2",
                15400000000,
                Some(RUSD_TYPE),
                Some(1000000000),
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x00018b1db3cf3c744bbf0138548cd89d1aa86a5e2432",
                14400000000,
                Some(RUSD_TYPE),
                Some(506718271345),
            ),
            cell(
                true,
                SECP_LOCK,
                "0x39f845ecc469baad4f1e8c2318c322a4d4538e84",
                7300299479888,
                None,
                None,
            ),
            cell(
                false,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                211300000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x0001572d2aa3e716b98f0caa6883328c8cebbdd766d5",
                14400000000,
                Some(RUSD_TYPE),
                Some(450010922),
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x00018b1db3cf3c744bbf0138548cd89d1aa86a5e2432",
                100048603198567,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SECP_LOCK,
                "0x39f845ecc469baad4f1e8c2318c322a4d4538e84",
                7441399472312,
                None,
                None,
            ),
        ]
    }

    /// tx 0x6873c64cfd7fe533744d1d794129aa8c36dafb4c8b5aed544823556bfd80092b
    /// (block 14,998,432). Vault in AND out, RUSD supply mints 80,165,867 — a
    /// borrow. The borrower is a pure receiver (no input capacity), so the old
    /// per-owner `input_capacity == 0` early return skipped them and the tx was
    /// labelled "adjust".
    fn tx_borrow() -> Vec<FixtureCell> {
        vec![
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x0001fd17037d3dc5491e61c84caae4ed071af96b9010",
                60600000000,
                Some(INTENT_REQ_TYPE),
                None,
            ),
            cell(
                true,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                277900000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                true,
                SPP_VAULT_LOCK,
                "0x0001fd17037d3dc5491e61c84caae4ed071af96b9010",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x0001fd17037d3dc5491e61c84caae4ed071af96b9010",
                26000000000000,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SECP_LOCK,
                "0x39f845ecc469baad4f1e8c2318c322a4d4538e84",
                12540597852762,
                None,
                None,
            ),
            cell(
                false,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                277900000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                false,
                SPP_VAULT_LOCK,
                "0x0001fd17037d3dc5491e61c84caae4ed071af96b9010",
                57800000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SPP_INTENT_LOCK,
                "0x3f9c6ada395d47f78d659f06cc299f95c3865671618d47557e5e0e8f2cabecc0",
                26000000000000,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x0001572d2aa3e716b98f0caa6883328c8cebbdd766d5",
                14400000000,
                Some(RUSD_TYPE),
                Some(80165867),
            ),
            cell(
                false,
                SECP_LOCK,
                "0x39f845ecc469baad4f1e8c2318c322a4d4538e84",
                12586797842958,
                None,
                None,
            ),
        ]
    }

    /// tx 0x678d39ace3f562f7ea2d34b83852ff48b9d3b1ca318e98cb45727b115d01487a
    /// (block 14,689,021). No vault consumed, one created — an open.
    fn tx_open_vault() -> Vec<FixtureCell> {
        vec![
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x0001032f3c7bb2c752dab577333839a29a0cf1132000",
                60400000000,
                Some(INTENT_REQ_TYPE),
                None,
            ),
            cell(
                true,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                248300000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x0001032f3c7bb2c752dab577333839a29a0cf1132000",
                187181800000000,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                true,
                SECP_LOCK,
                "0x39f845ecc469baad4f1e8c2318c322a4d4538e84",
                8887199036172,
                None,
                None,
            ),
            cell(
                false,
                SPP_POOL_GUARD_LOCK,
                "0x4e456f35ce02e25c0c58a7b6483309c691d86cd3d6d5b5e325ee81324d1a8eec",
                255700000000,
                Some(SPP_POOL_TYPE),
                None,
            ),
            cell(
                false,
                SPP_VAULT_LOCK,
                "0x0001032f3c7bb2c752dab577333839a29a0cf1132000",
                51400000000,
                Some(VAULT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x0001032f3c7bb2c752dab577333839a29a0cf1132000",
                14400000000,
                Some(RUSD_TYPE),
                Some(1400000000000),
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x0001572d2aa3e716b98f0caa6883328c8cebbdd766d5",
                14400000000,
                Some(RUSD_TYPE),
                Some(14000000000),
            ),
            cell(
                false,
                SPP_INTENT_LOCK,
                "0x824e847743afcae14bd0663e8e80cdb624e08f97b58490c640c02d1c2a102649",
                15400000000,
                Some(RUSD_TYPE),
                Some(1000000000),
            ),
            cell(
                false,
                SPP_INTENT_LOCK,
                "0x824e847743afcae14bd0663e8e80cdb624e08f97b58490c640c02d1c2a102649",
                187181800000000,
                Some(INTENT_DATA_TYPE),
                None,
            ),
            cell(
                false,
                SECP_LOCK,
                "0x39f845ecc469baad4f1e8c2318c322a4d4538e84",
                8844599026176,
                None,
                None,
            ),
        ]
    }

    /// tx 0xb86857056821a53aa88dd54a7dbb586ae926fb4de95e9812f7cd3bb561f44a7a
    /// (block 19,991,764). A plain RUSD transfer between two JoyID addresses.
    /// No vault, intent or pool cell — not a Stable++ protocol event at all.
    /// The old detector labelled this (and ~10,700 like it) "redemption".
    fn tx_plain_rusd_transfer() -> Vec<FixtureCell> {
        vec![
            cell(
                true,
                JOYID_LOCK,
                "0x0001572d2aa3e716b98f0caa6883328c8cebbdd766d5",
                14400000000,
                Some(RUSD_TYPE),
                Some(12817746),
            ),
            cell(
                true,
                JOYID_LOCK,
                "0x0001572d2aa3e716b98f0caa6883328c8cebbdd766d5",
                14400000000,
                Some(RUSD_TYPE),
                Some(15428221375),
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x0001f2efa1840e4ab531dcab4024b3d1159a93fc1fd4",
                14400000000,
                Some(RUSD_TYPE),
                Some(15441039121),
            ),
            cell(
                false,
                JOYID_LOCK,
                "0x0001572d2aa3e716b98f0caa6883328c8cebbdd766d5",
                14399997978,
                None,
                None,
            ),
        ]
    }

    /// tx 0x7da5a5ea71a1fe3343fff0e2d7165d9b5f0723451faa9d63a260ba27bc9a310b
    /// (block 19,494,320). A UTXOSwap swap settlement that moves RUSD. It is a
    /// UTXOSwap event, not a Stable++ one — no Stable++ structural cell appears.
    fn tx_utxoswap_swap_with_rusd() -> Vec<FixtureCell> {
        vec![
            cell(true, UTXOSWAP_INTENT_LOCK, "0xf2c152e06e8b9b55e357ed0e996ebf8c9b8102b5f6808f0469c0834edf88744574c159d06ff1e1e700000000000000000000000000000000040100e1f505000000000000000000000000f5df0100000000000000000000000000", 21200000000, Some(RUSD_TYPE), Some(122869)),
            cell(true, OTHER_LOCK_A, "0x", 24300000000, Some(OTHER_TYPE_A), None),
            cell(true, OTHER_LOCK_B, "0xf6808f0469c0834edf88744574c159d06ff1e1e7a3f39482799dc454952dda3b", 4675103593763316, None, None),
            cell(true, OTHER_LOCK_B, "0xf6808f0469c0834edf88744574c159d06ff1e1e7a3f39482799dc454952dda3b", 15400000000, Some(RUSD_TYPE), Some(5698497854664)),
            cell(true, SECP_LOCK, "0x42e11d5e294aa6901d30f0d4fb50fb30222b7f54", 99944963895, None, None),
            cell(false, JOYID_LOCK, "0x0001407c4c5a5c7b76ca3534462d06f699ccbd65b2b5", 14400000000, Some(RUSD_TYPE), Some(611)),
            cell(false, JOYID_LOCK, "0x0001407c4c5a5c7b76ca3534462d06f699ccbd65b2b5", 6900000000, None, None),
            cell(false, OTHER_LOCK_A, "0x", 24300000000, Some(OTHER_TYPE_A), None),
            cell(false, OTHER_LOCK_B, "0xf6808f0469c0834edf88744574c159d06ff1e1e7a3f39482799dc454952dda3b", 4675103493763316, None, None),
            cell(false, OTHER_LOCK_B, "0xf6808f0469c0834edf88744574c159d06ff1e1e7a3f39482799dc454952dda3b", 15400000000, Some(RUSD_TYPE), Some(5698497976922)),
            cell(false, SECP_LOCK, "0x42e11d5e294aa6901d30f0d4fb50fb30222b7f54", 99944954680, None, None),
        ]
    }

    /// tx 0x1f67ff64fa379e9c99ce4e8c847541a7d69ee75b2481901e12d38ec384a177f9
    /// (block 19,494,320). The matching UTXOSwap swap submission.
    fn tx_utxoswap_submit_with_rusd() -> Vec<FixtureCell> {
        vec![
            cell(true, JOYID_LOCK, "0x0001407c4c5a5c7b76ca3534462d06f699ccbd65b2b5", 14400000000, Some(RUSD_TYPE), Some(90924453)),
            cell(true, JOYID_LOCK, "0x0001407c4c5a5c7b76ca3534462d06f699ccbd65b2b5", 6900000000, None, None),
            cell(true, JOYID_LOCK, "0x0001407c4c5a5c7b76ca3534462d06f699ccbd65b2b5", 21764453552, None, None),
            cell(false, UTXOSWAP_INTENT_LOCK, "0xf2c152e06e8b9b55e357ed0e996ebf8c9b8102b5f6808f0469c0834edf88744574c159d06ff1e1e700000000000000000000000000000000040100e1f505000000000000000000000000f5df0100000000000000000000000000", 21200000000, Some(RUSD_TYPE), Some(122869)),
            cell(false, JOYID_LOCK, "0x0001407c4c5a5c7b76ca3534462d06f699ccbd65b2b5", 14400000000, Some(RUSD_TYPE), Some(90801584)),
            cell(false, JOYID_LOCK, "0x0001407c4c5a5c7b76ca3534462d06f699ccbd65b2b5", 7464447622, None, None),
        ]
    }

    /// tx 0x7f1a5e6aef2e5fd396e0c685865d24022f6fbce23c4edbb34233afb0aa949c2a
    /// (block 17,507,836). The owner reclaims their own Stable++ intent cells
    /// back to their plain lock. RUSD supply is unchanged (246,700,000,000 in
    /// and out, same owner), so no debt was redeemed — the old detector called
    /// this "redemption" purely because one participant's private RUSD delta
    /// was non-zero.
    fn tx_intent_reclaim_with_rusd() -> Vec<FixtureCell> {
        vec![
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x90701463ce244572d5237c7d81c9e03e4289c12b",
                60100000000,
                Some(INTENT_REQ_TYPE),
                None,
            ),
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x90701463ce244572d5237c7d81c9e03e4289c12b",
                14200000000,
                Some(RUSD_TYPE),
                Some(246700000000),
            ),
            cell(
                true,
                SECP_LOCK,
                "0x90701463ce244572d5237c7d81c9e03e4289c12b",
                59370416115960,
                None,
                None,
            ),
            cell(
                false,
                SECP_LOCK,
                "0x90701463ce244572d5237c7d81c9e03e4289c12b",
                60100000000,
                None,
                None,
            ),
            cell(
                false,
                SECP_LOCK,
                "0x90701463ce244572d5237c7d81c9e03e4289c12b",
                14200000000,
                Some(RUSD_TYPE),
                Some(246700000000),
            ),
            cell(
                false,
                SECP_LOCK,
                "0x90701463ce244572d5237c7d81c9e03e4289c12b",
                59370416110659,
                None,
                None,
            ),
        ]
    }

    /// tx 0x2cc346ede08ae710bdacaadfc7d88c0e3698bf3e7652ad5644d107a098530de1
    /// (block 18,371,688). Intent reclaim with no RUSD involved at all.
    fn tx_intent_reclaim_no_rusd() -> Vec<FixtureCell> {
        vec![
            cell(
                true,
                SPP_INTENT_LOCK,
                "0x89b9076d37b6abef990b04431059f57356fd1f7f",
                60100000000,
                Some(INTENT_REQ_TYPE),
                None,
            ),
            cell(
                true,
                SECP_LOCK,
                "0x89b9076d37b6abef990b04431059f57356fd1f7f",
                60000000000000,
                None,
                None,
            ),
            cell(
                false,
                SECP_LOCK,
                "0x89b9076d37b6abef990b04431059f57356fd1f7f",
                60100000000,
                None,
                None,
            ),
            cell(
                false,
                SECP_LOCK,
                "0x89b9076d37b6abef990b04431059f57356fd1f7f",
                59999999998635,
                None,
                None,
            ),
        ]
    }

    // --- Rule 1: RUSD movement alone is not a Stable++ event ---------------

    #[test]
    fn plain_rusd_transfer_emits_no_stablepp_action() {
        assert!(
            stablepp_actions(&tx_plain_rusd_transfer()).is_empty(),
            "a plain RUSD transfer touches no vault/intent/pool cell and is not \
             a Stable++ protocol event"
        );
    }

    #[test]
    fn utxoswap_trades_of_rusd_emit_no_stablepp_action() {
        assert!(
            stablepp_actions(&tx_utxoswap_swap_with_rusd()).is_empty(),
            "a UTXOSwap settlement that happens to move RUSD is not a Stable++ event"
        );
        assert!(
            stablepp_actions(&tx_utxoswap_submit_with_rusd()).is_empty(),
            "a UTXOSwap submission that happens to move RUSD is not a Stable++ event"
        );
    }

    // --- Rules 2-4: vault lifecycle from tx-level facts --------------------

    #[test]
    fn vault_created_only_is_open_vault() {
        let (action, meta) = only_action(&tx_open_vault());
        assert_eq!(action, "open_vault");
        assert_eq!(meta["vaultCount"], 1);
    }

    #[test]
    fn vault_consumed_only_is_close_vault_and_never_liquidation() {
        let (action, meta) = only_action(&tx_close_vault());
        assert_eq!(action, "close_vault");
        assert_eq!(meta["vaultCount"], 1);
        assert_eq!(meta["hasIntent"], true);
        assert_eq!(meta["supplyDirection"], "burn");
    }

    #[test]
    fn vault_in_and_out_with_supply_mint_is_borrow() {
        // The borrower receives RUSD without funding the tx; tx-level supply
        // accounting sees the mint regardless of whose balance moved.
        let (action, meta) = only_action(&tx_borrow());
        assert_eq!(action, "borrow");
        assert_eq!(meta["supplyDirection"], "mint");
        assert_eq!(meta["vaultCount"], 1);
    }

    #[test]
    fn multi_vault_tx_yields_exactly_one_truthful_action() {
        // 4 vaults in / 3 out with a net RUSD mint: one tx, one fact.
        let (action, meta) = only_action(&tx_multi_vault_mint());
        assert_eq!(action, "borrow");
        assert_eq!(meta["supplyDirection"], "mint");
        assert_eq!(meta["vaultCount"], 4);
    }

    #[test]
    fn contradictory_labels_are_impossible_for_one_tx() {
        for fixture in [
            tx_multi_vault_mint(),
            tx_close_vault(),
            tx_borrow(),
            tx_open_vault(),
        ] {
            let actions = stablepp_actions(&fixture);
            assert_eq!(
                actions.len(),
                1,
                "a transaction must carry at most one Stable++ action, got {:?}",
                actions.iter().map(|(a, _)| a).collect::<Vec<_>>()
            );
        }
    }

    /// No mainnet close is a liquidation: all 68 vault-closing transactions in
    /// chain history consume an intent belonging to the very owner whose vault
    /// closes, and pay that owner out. Without a discriminator that fires on
    /// real data, "liquidation" is never emitted.
    #[test]
    fn liquidation_is_never_emitted() {
        for fixture in [
            tx_multi_vault_mint(),
            tx_close_vault(),
            tx_borrow(),
            tx_open_vault(),
            tx_intent_reclaim_with_rusd(),
            tx_intent_reclaim_no_rusd(),
        ] {
            for (action, _) in stablepp_actions(&fixture) {
                assert_ne!(
                    action, "liquidation",
                    "liquidation has no chain discriminator and must not be fabricated"
                );
            }
        }
    }

    // --- Rule 5: redemption requires burning RUSD against an intent --------

    #[test]
    fn intent_reclaim_is_interaction_not_redemption() {
        let (action, meta) = only_action(&tx_intent_reclaim_with_rusd());
        assert_eq!(
            action, "interaction",
            "RUSD returning to its own owner is not a redemption: supply is unchanged"
        );
        assert_eq!(meta["supplyDirection"], "flat");
        assert_eq!(meta["vaultCount"], 0);
        assert_eq!(meta["hasIntent"], true);

        let (action, _) = only_action(&tx_intent_reclaim_no_rusd());
        assert_eq!(action, "interaction");
    }

    /// A redemption destroys RUSD against the protocol. Synthesised from the
    /// reclaim fixture by burning the reclaimed RUSD instead of returning it:
    /// intent consumed, no vault touched, supply falls.
    #[test]
    fn intent_consumed_with_supply_burn_is_redemption() {
        let mut cells = tx_intent_reclaim_with_rusd();
        cells.retain(|c| c.is_input || c.udt_amount.is_none());
        let (action, meta) = only_action(&cells);
        assert_eq!(action, "redemption");
        assert_eq!(meta["supplyDirection"], "burn");
        assert_eq!(meta["vaultCount"], 0);
        assert_eq!(meta["hasIntent"], true);
    }

    // --- Exact supply arithmetic ------------------------------------------

    #[test]
    fn stablepp_token_delta_sign_is_exact_above_i128_range() {
        let mut positive = ExactMagnitude::default();
        let mut negative = ExactMagnitude::default();
        positive.add_u128(u128::MAX);
        negative.add_u128(u128::MAX - 1);
        assert_eq!(positive.cmp(&negative), Ordering::Greater);
    }

    #[test]
    fn stablepp_token_delta_sign_handles_sums_larger_than_u128() {
        let mut positive = ExactMagnitude::default();
        let mut negative = ExactMagnitude::default();
        positive.add_u128(u128::MAX);
        positive.add_u128(u128::MAX);
        negative.add_u128(u128::MAX);
        assert_eq!(positive.cmp(&negative), Ordering::Greater);
    }

    /// RUSD supply is summed across the whole transaction with arbitrary-width
    /// arithmetic, so a tx whose totals exceed `u128` still classifies exactly.
    #[test]
    fn supply_direction_is_exact_beyond_u128_totals() {
        let facts = TxFacts {
            vault_in: 1,
            vault_out: 1,
            intent_in: 0,
            structure_involved: true,
            supply_in: {
                let mut m = ExactMagnitude::default();
                m.add_u128(u128::MAX);
                m
            },
            supply_out: {
                let mut m = ExactMagnitude::default();
                m.add_u128(u128::MAX);
                m.add_u128(1);
                m
            },
        };
        assert_eq!(facts.supply_direction(), SupplyDirection::Mint);
        assert_eq!(facts.classify(), Some("borrow"));
    }

    // --- The classification table itself ----------------------------------

    #[test]
    fn classification_table_matches_the_spec() {
        let facts = |vault_in, vault_out, intent_in, structure_involved, delta: i64| {
            let mut supply_in = ExactMagnitude::default();
            let mut supply_out = ExactMagnitude::default();
            if delta > 0 {
                supply_out.add_u128(delta as u128);
            } else if delta < 0 {
                supply_in.add_u128(delta.unsigned_abs() as u128);
            }
            TxFacts {
                vault_in,
                vault_out,
                intent_in,
                structure_involved,
                supply_in,
                supply_out,
            }
        };

        // No Stable++ structure at all -> not a protocol event, whatever RUSD did.
        assert_eq!(facts(0, 0, 0, false, 0).classify(), None);
        assert_eq!(facts(0, 0, 0, false, 500).classify(), None);
        assert_eq!(facts(0, 0, 0, false, -500).classify(), None);

        // Vault lifecycle is decided by the vault cells, not by supply.
        assert_eq!(facts(0, 1, 0, true, 0).classify(), Some("open_vault"));
        assert_eq!(facts(0, 1, 0, true, 500).classify(), Some("open_vault"));
        assert_eq!(facts(0, 2, 0, true, -500).classify(), Some("open_vault"));
        assert_eq!(facts(1, 0, 0, true, 0).classify(), Some("close_vault"));
        assert_eq!(facts(1, 0, 0, true, -500).classify(), Some("close_vault"));
        assert_eq!(facts(1, 0, 0, true, 500).classify(), Some("close_vault"));

        // A surviving vault: the debt direction names the action.
        assert_eq!(facts(1, 1, 0, true, 500).classify(), Some("borrow"));
        assert_eq!(facts(1, 1, 0, true, -500).classify(), Some("repay"));
        assert_eq!(facts(1, 1, 0, true, 0).classify(), Some("adjust"));

        // No vault: only a burn against a consumed intent is a redemption.
        assert_eq!(facts(0, 0, 1, true, -500).classify(), Some("redemption"));
        assert_eq!(facts(0, 0, 1, true, 0).classify(), Some("interaction"));
        assert_eq!(facts(0, 0, 1, true, 500).classify(), Some("interaction"));
        // A burn with no intent consumed is not a redemption.
        assert_eq!(facts(0, 0, 0, true, -500).classify(), Some("interaction"));
    }
}
