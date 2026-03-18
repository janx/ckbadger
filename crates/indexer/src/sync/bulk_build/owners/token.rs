use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::{
    CkbadgerStore, TokenInfo, CF_ADDR_TOKENS_BY_BALANCE, CF_TOKENS, CF_TOKEN_HOLDERS,
    CF_TOKEN_HOLDERS_BY_BALANCE,
};
use rocksdb::IteratorMode;

use super::{BulkReducer, ReducerContext};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{
    CellFacts, CellSemanticTag, ResolvedInputFacts, ResolvedTxFacts,
};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct TokenOwner {
    tokens: HashMap<Vec<u8>, TokenAccum>,
}

impl BulkReducer for TokenOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts, ctx: &ReducerContext<'_>) -> Result<()> {
        for input in &tx.resolved_inputs {
            let Some(view) = TokenCellView::from_input(input, ctx, tx)? else {
                continue;
            };
            let token = self
                .tokens
                .entry(view.type_hash.clone())
                .or_insert_with(|| TokenAccum::from_view(&view, tx.block_number));
            token.apply_input(&view, tx)?;
        }

        for cell in &tx.cells {
            let Some(view) = TokenCellView::from_output(cell, ctx, tx)? else {
                continue;
            };
            let token = self
                .tokens
                .entry(view.type_hash.clone())
                .or_insert_with(|| TokenAccum::from_view(&view, tx.block_number));
            token.apply_output(&view, tx)?;
        }

        Ok(())
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        let mut rows = Vec::new();
        let mut type_hashes: Vec<&Vec<u8>> = self.tokens.keys().collect();
        type_hashes.sort();

        for type_hash in type_hashes {
            let token = self
                .tokens
                .get(type_hash)
                .expect("sorted token type hash must exist");
            rows.push(MaterializedRow::new(
                CF_TOKENS,
                type_hash.clone(),
                bincode::serialize(&token.to_info(type_hash.clone()))?,
            ));

            let mut holders: Vec<(&Vec<u8>, &i128)> = token
                .holders
                .iter()
                .filter(|(_, balance)| **balance > 0)
                .collect();
            holders.sort_by(|(lock_a, _), (lock_b, _)| lock_a.cmp(lock_b));

            for (lock_hash, balance) in holders {
                rows.push(MaterializedRow::new(
                    CF_TOKEN_HOLDERS,
                    keys::encode_token_holder_key(type_hash, lock_hash).to_vec(),
                    balance.to_le_bytes().to_vec(),
                ));
                rows.push(MaterializedRow::new(
                    CF_TOKEN_HOLDERS_BY_BALANCE,
                    keys::encode_token_holder_balance_key(type_hash, *balance, lock_hash).to_vec(),
                    Vec::new(),
                ));
                rows.push(MaterializedRow::new(
                    CF_ADDR_TOKENS_BY_BALANCE,
                    keys::encode_addr_token_balance_key(lock_hash, *balance, type_hash).to_vec(),
                    Vec::new(),
                ));
            }
        }

        materializer.materialize_final_snapshot(&rows)
    }
}

#[derive(Debug, Clone)]
struct TokenAccum {
    type_code_hash: Vec<u8>,
    hash_type: u8,
    type_args: Vec<u8>,
    standard: &'static str,
    first_seen_block: i64,
    live_supply: i128,
    holders: HashMap<Vec<u8>, i128>,
}

impl TokenAccum {
    fn from_view(view: &TokenCellView, first_seen_block: i64) -> Self {
        Self {
            type_code_hash: view.type_code_hash.clone(),
            hash_type: view.type_hash_type as u8,
            type_args: view.type_args.clone(),
            standard: view.standard,
            first_seen_block,
            live_supply: 0,
            holders: HashMap::new(),
        }
    }

    fn apply_input(&mut self, view: &TokenCellView, tx: &ResolvedTxFacts) -> Result<()> {
        self.ensure_metadata(view, tx)?;
        self.live_supply = checked_next_i128(
            self.live_supply,
            -(view.amount as i128),
            "token live_supply",
            &view.type_hash,
            tx,
        )?;

        let current = *self.holders.get(&view.lock_hash).unwrap_or(&0);
        let next = checked_next_i128(
            current,
            -(view.amount as i128),
            "token holder balance",
            &view.type_hash,
            tx,
        )?;
        if next == 0 {
            self.holders.remove(&view.lock_hash);
        } else {
            self.holders.insert(view.lock_hash.clone(), next);
        }

        Ok(())
    }

    fn apply_output(&mut self, view: &TokenCellView, tx: &ResolvedTxFacts) -> Result<()> {
        self.ensure_metadata(view, tx)?;
        if tx.block_number < self.first_seen_block {
            self.first_seen_block = tx.block_number;
        }
        self.live_supply = checked_next_i128(
            self.live_supply,
            view.amount as i128,
            "token live_supply",
            &view.type_hash,
            tx,
        )?;

        let current = *self.holders.get(&view.lock_hash).unwrap_or(&0);
        let next = checked_next_i128(
            current,
            view.amount as i128,
            "token holder balance",
            &view.type_hash,
            tx,
        )?;
        self.holders.insert(view.lock_hash.clone(), next);
        Ok(())
    }

    fn ensure_metadata(&self, view: &TokenCellView, tx: &ResolvedTxFacts) -> Result<()> {
        if self.type_code_hash != view.type_code_hash
            || self.hash_type != view.type_hash_type as u8
            || self.type_args != view.type_args
            || self.standard != view.standard
        {
            bail!(
                "token reducer metadata mismatch: type_hash=0x{}, block={}, tx=0x{}, tx_index={}",
                hex::encode(&view.type_hash),
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            );
        }
        Ok(())
    }

    fn to_info(&self, _type_hash: Vec<u8>) -> TokenInfo {
        TokenInfo {
            type_code_hash: self.type_code_hash.clone(),
            hash_type: self.hash_type,
            type_args: self.type_args.clone(),
            standard: self.standard.to_string(),
            name: None,
            symbol: None,
            decimals: None,
            total_supply: Some(self.live_supply),
            max_supply: None,
            holders_count: i64::try_from(self.holders.len()).expect("holders count exceeds i64"),
            first_seen_block: self.first_seen_block,
            icon_url: None,
            description: None,
            transfers_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct TokenCellView {
    type_hash: Vec<u8>,
    type_code_hash: Vec<u8>,
    type_hash_type: i16,
    type_args: Vec<u8>,
    lock_hash: Vec<u8>,
    amount: u128,
    standard: &'static str,
}

impl TokenCellView {
    fn from_output(
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts,
    ) -> Result<Option<Self>> {
        Self::from_parts(
            cell.semantic_tag,
            cell.type_script_hash_id,
            cell.type_code_hash_id,
            cell.type_hash_type,
            cell.type_args_id,
            cell.lock_script_hash_id,
            cell.udt_amount,
            ctx,
            tx,
            format!(
                "output outpoint=0x{}:{}",
                hex::encode(cell.outpoint.tx_hash),
                cell.outpoint.index
            ),
        )
    }

    fn from_input(
        input: &ResolvedInputFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts,
    ) -> Result<Option<Self>> {
        Self::from_parts(
            input.semantic_tag,
            input.type_script_hash_id,
            input.type_code_hash_id,
            input.type_hash_type,
            input.type_args_id,
            input.lock_script_hash_id,
            input.udt_amount,
            ctx,
            tx,
            format!(
                "input outpoint=0x{}:{}",
                hex::encode(input.outpoint.tx_hash),
                input.outpoint.index
            ),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        semantic_tag: CellSemanticTag,
        type_script_hash_id: Option<crate::sync::types::InternId>,
        type_code_hash_id: Option<crate::sync::types::InternId>,
        type_hash_type: Option<i16>,
        type_args_id: Option<crate::sync::types::InternId>,
        lock_script_hash_id: crate::sync::types::InternId,
        udt_amount: Option<u128>,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts,
        location: String,
    ) -> Result<Option<Self>> {
        let standard = match semantic_tag {
            CellSemanticTag::Sudt => "sudt",
            CellSemanticTag::Xudt => "xudt",
            _ => return Ok(None),
        };

        let Some(amount) = udt_amount else {
            if matches!(semantic_tag, CellSemanticTag::Xudt) {
                return Ok(None);
            }
            bail!(
                "missing UDT amount for fungible token cell: block={}, tx=0x{}, tx_index={}, {}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                location
            );
        };

        let type_hash = ctx
            .resolve_identity(type_script_hash_id.ok_or_else(|| {
                anyhow!(
                    "missing type_script_hash_id for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    location
                )
            })?)
            .to_vec();
        let type_code_hash = ctx
            .resolve_identity(type_code_hash_id.ok_or_else(|| {
                anyhow!(
                    "missing type_code_hash_id for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    location
                )
            })?)
            .to_vec();
        let type_hash_type = type_hash_type.ok_or_else(|| {
            anyhow!(
                "missing type_hash_type for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                location
            )
        })?;
        let type_args = ctx
            .resolve_identity(type_args_id.ok_or_else(|| {
                anyhow!(
                    "missing type_args_id for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    location
                )
            })?)
            .to_vec();
        let lock_hash = ctx.resolve_identity(lock_script_hash_id).to_vec();

        Ok(Some(Self {
            type_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            lock_hash,
            amount,
            standard,
        }))
    }
}

fn checked_next_i128(
    current: i128,
    delta: i128,
    metric: &str,
    type_hash: &[u8],
    tx: &ResolvedTxFacts,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: type_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(type_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: type_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(type_hash),
            current,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }
    Ok(next)
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct TokenStateSnapshot {
    pub tokens: HashMap<Vec<u8>, TokenInfo>,
    pub token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>>,
    pub addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>>,
}

#[doc(hidden)]
pub(crate) fn materialize_token_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<TokenStateSnapshot> {
    let mut interner = IdentityInterner::default();
    let arena = build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let ctx = ReducerContext::new(&interner);
    let mut owner = TokenOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = unique_temp_test_dir("bulk-build-token-owner");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer)?;
        let _ = materializer.finish();

        let tokens = domain_store
            .list_tokens()?
            .into_iter()
            .collect::<HashMap<_, _>>();

        let mut token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
        for (type_hash, _info) in &tokens {
            let holders = domain_store
                .list_token_holders(type_hash, usize::MAX)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            token_holders.insert(type_hash.clone(), holders);
        }

        let mut addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
        let iter = domain_store.iterator_cf(
            domain_store.cf_addr_tokens_by_balance(),
            IteratorMode::Start,
        );
        for item in iter {
            let (key, value) = item?;
            if !value.is_empty() {
                bail!(
                    "addr_tokens_by_balance value must be empty in token snapshot helper: value_len={}",
                    value.len()
                );
            }
            let (lock_hash, balance, type_hash) = keys::decode_addr_token_balance_key(&key);
            addr_tokens
                .entry(lock_hash)
                .or_default()
                .insert(type_hash, balance);
        }

        TokenStateSnapshot {
            tokens,
            token_holders,
            addr_tokens,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

fn unique_temp_test_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ckbadger-{}-{}-{}",
        prefix,
        std::process::id(),
        nanos
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{CellFacts, OutPointKey, ResolvedInputFacts};
    use crate::sync::types::InternId;

    #[test]
    fn token_owner_reduces_live_supply_and_holder_removal() {
        let mut interner = IdentityInterner::default();
        let lock_a = interner.intern_bytes(vec![0xaa; 32]);
        let lock_b = interner.intern_bytes(vec![0xbb; 32]);
        let type_hash_id = interner.intern_bytes(vec![0xcc; 32]);
        let type_code_hash_id =
            interner.intern_bytes(hex::decode(&crate::parser::udt::SUDT_CODE_HASH[2..]).unwrap());
        let type_args_id = interner.intern_bytes(vec![0x12; 32]);
        let ctx = ReducerContext::new(&interner);

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x41; 32],
            block_number: 100,
            block_hash: [0x03; 32],
            timestamp_ms: 1_700_000_000_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x41; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(99),
                lock_hash_type: 1,
                lock_args_id: InternId::new(98),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                occupied_capacity: 142_00000000,
                data_size: 16,
                data: Vec::new(),
                data_hash: None,
                udt_amount: Some(1000),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                protocol_facts: None,
            }],
        };
        let tx1 = ResolvedTxFacts {
            tx_hash: [0x42; 32],
            block_number: 100,
            block_hash: [0x03; 32],
            timestamp_ms: 1_700_000_000_001,
            block_dao_ar: 1,
            tx_index: 1,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x41; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                udt_amount: Some(1000),
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(99),
                lock_hash_type: 1,
                lock_args_id: InternId::new(98),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                protocol_facts: None,
            }],
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x42; 32], 0),
                    created_at_block: 100,
                    capacity: 200_00000000,
                    lock_script_hash_id: lock_b,
                    lock_code_hash_id: InternId::new(97),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(96),
                    type_script_hash_id: Some(type_hash_id),
                    type_code_hash_id: Some(type_code_hash_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(type_args_id),
                    occupied_capacity: 142_00000000,
                    data_size: 16,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: Some(600),
                    semantic_tag: CellSemanticTag::Sudt,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x42; 32], 1),
                    created_at_block: 100,
                    capacity: 200_00000000,
                    lock_script_hash_id: lock_a,
                    lock_code_hash_id: InternId::new(95),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(94),
                    type_script_hash_id: Some(type_hash_id),
                    type_code_hash_id: Some(type_code_hash_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(type_args_id),
                    occupied_capacity: 142_00000000,
                    data_size: 16,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: Some(400),
                    semantic_tag: CellSemanticTag::Sudt,
                    dao_state: None,
                    protocol_facts: None,
                },
            ],
        };
        let tx2 = ResolvedTxFacts {
            tx_hash: [0x43; 32],
            block_number: 100,
            block_hash: [0x03; 32],
            timestamp_ms: 1_700_000_000_002,
            block_dao_ar: 1,
            tx_index: 2,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x42; 32], 1),
                created_at_block: 100,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                udt_amount: Some(400),
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(95),
                lock_hash_type: 1,
                lock_args_id: InternId::new(94),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                protocol_facts: None,
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x43; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                lock_script_hash_id: lock_b,
                lock_code_hash_id: InternId::new(93),
                lock_hash_type: 1,
                lock_args_id: InternId::new(92),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                occupied_capacity: 142_00000000,
                data_size: 16,
                data: Vec::new(),
                data_hash: None,
                udt_amount: Some(400),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                protocol_facts: None,
            }],
        };

        let mut owner = TokenOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply tx0");
        owner.apply_tx(&tx1, &ctx).expect("apply tx1");
        owner.apply_tx(&tx2, &ctx).expect("apply tx2");

        let token = owner.tokens.get(&vec![0xcc; 32]).expect("token");
        assert_eq!(token.live_supply, 1000);
        assert_eq!(token.holders.len(), 1);
        assert_eq!(token.holders.get(&vec![0xbb; 32]), Some(&1000));
        assert!(!token.holders.contains_key(&vec![0xaa; 32]));
    }
}
