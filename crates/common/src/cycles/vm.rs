//! Exact in-process CKB script execution for committed transactions.
use super::{MockTransaction, ReplayFailure, VmActivationEpochs};
use crate::cycles_task::{ScriptGroupCycles, ScriptGroupKind, TransactionCycles};
use ckb_chain_spec::consensus::{Consensus, ConsensusBuilder};
use ckb_script::{ScriptGroupType, TransactionScriptsVerifier, TxVerifyEnv};
use ckb_traits::{CellDataProvider, ExtensionProvider, HeaderProvider};
use ckb_types::{
    bytes::Bytes,
    core::{
        cell::{
            resolve_transaction, CellMeta, CellMetaBuilder, CellProvider, CellStatus, HeaderChecker,
        },
        error::OutPointError,
        hardfork::{HardForks, CKB2021, CKB2023},
        HeaderView,
    },
    packed::{self, Byte32, OutPoint},
    prelude::*,
};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Default)]
struct Loader {
    cells: HashMap<OutPoint, Bytes>,
    metas: HashMap<OutPoint, CellMeta>,
    headers: HashMap<Byte32, HeaderView>,
    extensions: HashMap<Byte32, packed::Bytes>,
}
impl CellDataProvider for Loader {
    fn get_cell_data(&self, point: &OutPoint) -> Option<Bytes> {
        self.cells.get(point).cloned()
    }
    fn get_cell_data_hash(&self, point: &OutPoint) -> Option<Byte32> {
        self.cells
            .get(point)
            .map(|data| packed::CellOutput::calc_data_hash(data))
    }
}
impl CellProvider for Loader {
    fn cell(&self, point: &OutPoint, _eager_load: bool) -> CellStatus {
        self.metas
            .get(point)
            .cloned()
            .map(CellStatus::live_cell)
            .unwrap_or(CellStatus::Unknown)
    }
}
impl HeaderChecker for Loader {
    fn check_valid(&self, hash: &Byte32) -> Result<(), OutPointError> {
        if self.headers.contains_key(hash) {
            Ok(())
        } else {
            Err(OutPointError::InvalidHeader(hash.clone()))
        }
    }
}
impl HeaderProvider for Loader {
    fn get_header(&self, hash: &Byte32) -> Option<HeaderView> {
        self.headers.get(hash).cloned()
    }
}
impl ExtensionProvider for Loader {
    fn get_block_extension(&self, hash: &Byte32) -> Option<packed::Bytes> {
        self.extensions.get(hash).cloned()
    }
}

pub(super) fn consensus(epochs: VmActivationEpochs, max_cycles: u64) -> Result<Consensus, String> {
    // Script verification consults precisely RFC0032 and RFC0049. Other
    // hardfork switches belong to non-script consensus checks, not this VM.
    let forks = HardForks {
        ckb2021: CKB2021::new_dev_default()
            .as_builder()
            .rfc_0032(epochs.vm1)
            .build()?,
        ckb2023: CKB2023::new_dev_default()
            .as_builder()
            .rfc_0049(epochs.vm2)
            .build()?,
    };
    Ok(ConsensusBuilder::default()
        .hardfork_switch(forks)
        .max_block_cycles(max_cycles)
        .build())
}

pub(super) fn header(value: &impl serde::Serialize) -> Result<HeaderView, String> {
    let mut json = serde_json::to_value(value).map_err(|e| e.to_string())?;
    if json
        .get("extra_hash")
        .is_none_or(serde_json::Value::is_null)
    {
        let legacy = json
            .get("uncles_hash")
            .filter(|v| !v.is_null())
            .cloned()
            .ok_or_else(|| "header lacks consensus extra_hash".to_string())?;
        json["extra_hash"] = legacy;
    }
    let view: ckb_jsonrpc_types::HeaderView = serde_json::from_value(json)
        .map_err(|e| format!("invalid commit/dependency header: {e}"))?;
    let expected: Byte32 = view.hash.pack();
    let packed: packed::Header = view.inner.into();
    let header = packed.into_view();
    if header.hash() != expected {
        return Err(format!(
            "header hash mismatch: expected={expected:#x} actual={:#x}",
            header.hash()
        ));
    }
    Ok(header)
}

fn json_into<T: serde::de::DeserializeOwned>(value: &impl serde::Serialize) -> Result<T, String> {
    serde_json::from_value(serde_json::to_value(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub(super) fn execute(
    mock: MockTransaction,
    commit: HeaderView,
    epochs: VmActivationEpochs,
    max_cycles: u64,
    extensions: HashMap<Byte32, packed::Bytes>,
) -> Result<TransactionCycles, ReplayFailure> {
    let json_tx: ckb_jsonrpc_types::Transaction =
        json_into(&mock.tx).map_err(ReplayFailure::invalid_data)?;
    let tx: packed::Transaction = json_tx.into();
    let mut loader = Loader {
        extensions,
        ..Default::default()
    };
    for h in &mock.mock_info.header_deps {
        let view = header(h).map_err(ReplayFailure::invalid_data)?;
        loader.headers.insert(view.hash(), view);
    }
    let mut resolve = |output: &super::Output,
                       data: &str,
                       point: &super::OutPoint,
                       info: ckb_types::core::TransactionInfo|
     -> Result<CellMeta, String> {
        let output: ckb_jsonrpc_types::CellOutput = json_into(output)?;
        let point: ckb_jsonrpc_types::OutPoint = json_into(point)?;
        let point: OutPoint = point.into();
        let bytes = Bytes::from(
            hex::decode(data.strip_prefix("0x").unwrap_or(data)).map_err(|e| e.to_string())?,
        );
        loader.cells.insert(point.clone(), bytes.clone());
        Ok(CellMetaBuilder::from_cell_output(output.into(), bytes)
            .out_point(point)
            .transaction_info(info)
            .build())
    };
    let mut metas = Vec::new();
    for input in &mock.mock_info.inputs {
        metas.push(
            resolve(
                &input.output,
                &input.data,
                &input.input.previous_output,
                input.transaction_info.clone(),
            )
            .map_err(ReplayFailure::invalid_data)?,
        );
    }
    for dep in &mock.mock_info.cell_deps {
        metas.push(
            resolve(
                &dep.output,
                &dep.data,
                &dep.cell_dep.out_point,
                dep.transaction_info.clone(),
            )
            .map_err(ReplayFailure::invalid_data)?,
        );
    }
    loader.metas = metas
        .into_iter()
        .map(|cell| (cell.out_point.clone(), cell))
        .collect();
    let resolved = resolve_transaction(
        tx.into_view(),
        &mut std::collections::HashSet::<OutPoint>::new(),
        &loader,
        &loader,
    )
    .map_err(|e| ReplayFailure::DependencyResolution.logged(e))?;
    let verifier = TransactionScriptsVerifier::new(
        Arc::new(resolved),
        loader,
        Arc::new(consensus(epochs, max_cycles).map_err(ReplayFailure::invalid_data)?),
        Arc::new(TxVerifyEnv::new_commit(&commit)),
    );
    execute_groups(
        &verifier,
        max_cycles,
        commit
            .hash()
            .as_slice()
            .try_into()
            .map_err(|_| ReplayFailure::invalid_data("commit hash length"))?,
    )
}

fn execute_groups(
    verifier: &TransactionScriptsVerifier<Loader>,
    limit: u64,
    block_hash: [u8; 32],
) -> Result<TransactionCycles, ReplayFailure> {
    let mut groups = Vec::new();
    let mut total = 0_u64;
    for (kind, hash, _) in verifier.groups_with_type() {
        let remaining = limit
            .checked_sub(total)
            .ok_or(ReplayFailure::CyclesOverflow)?;
        let group_type = match kind {
            ScriptGroupType::Lock => ScriptGroupKind::Lock,
            ScriptGroupType::Type => ScriptGroupKind::Type,
        };
        let script_hash = hash
            .as_slice()
            .try_into()
            .map_err(|_| ReplayFailure::invalid_data("script hash length"))?;
        let cycles = verifier.verify_single(kind, hash, remaining).map_err(|e| {
            ReplayFailure::ScriptGroup {
                group_type,
                script_hash,
            }
            .logged(e)
        })?;
        total = total
            .checked_add(cycles)
            .ok_or(ReplayFailure::CyclesOverflow)?;
        groups.push(ScriptGroupCycles {
            script_hash,
            group_type,
            cycles: i64::try_from(cycles).map_err(|_| ReplayFailure::CyclesOverflow)?,
        });
    }
    if groups.is_empty() {
        return Err(ReplayFailure::NoScriptGroups);
    }
    groups.sort_by_key(|group| (group.group_type, group.script_hash));
    let result = TransactionCycles {
        block_hash,
        cycles: i64::try_from(total).map_err(|_| ReplayFailure::CyclesOverflow)?,
        groups,
    };
    result.validate().map_err(ReplayFailure::invalid_data)?;
    Ok(result)
}

#[cfg(test)]
pub(super) fn type_script_version_at_epoch(epoch: u64, epochs: VmActivationEpochs) -> u8 {
    use ckb_types::core::{
        cell::ResolvedTransaction, EpochNumberWithFraction, HeaderBuilder, ScriptHashType,
        TransactionBuilder,
    };
    let header = HeaderBuilder::default()
        .epoch(EpochNumberWithFraction::new(epoch, 0, 1).pack())
        .build();
    let resolved = ResolvedTransaction {
        transaction: TransactionBuilder::default().build(),
        resolved_inputs: vec![],
        resolved_cell_deps: vec![],
        resolved_dep_groups: vec![],
    };
    let verifier = TransactionScriptsVerifier::new(
        Arc::new(resolved),
        Loader::default(),
        Arc::new(consensus(epochs, 10_000_000).unwrap()),
        Arc::new(TxVerifyEnv::new_commit(&header)),
    );
    let script = packed::Script::new_builder()
        .hash_type(ScriptHashType::Type.into())
        .build();
    match verifier.select_version(&script).unwrap() {
        ckb_script::ScriptVersion::V0 => 0,
        ckb_script::ScriptVersion::V1 => 1,
        ckb_script::ScriptVersion::V2 => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_script::ScriptVersion;
    use ckb_types::core::{
        cell::ResolvedTransaction, EpochNumberWithFraction, ScriptHashType, TransactionBuilder,
        TransactionInfo,
    };

    // A complete RV64 ELF: return the requested exit code via Linux exit(93).
    fn program(exit: u8) -> Bytes {
        let mut elf = vec![0_u8; 132];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&2_u16.to_le_bytes());
        elf[18..20].copy_from_slice(&243_u16.to_le_bytes());
        elf[20..24].copy_from_slice(&1_u32.to_le_bytes());
        elf[24..32].copy_from_slice(&0x10078_u64.to_le_bytes());
        elf[32..40].copy_from_slice(&64_u64.to_le_bytes());
        elf[52..54].copy_from_slice(&64_u16.to_le_bytes());
        elf[54..56].copy_from_slice(&56_u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1_u16.to_le_bytes());
        elf[64..68].copy_from_slice(&1_u32.to_le_bytes());
        elf[68..72].copy_from_slice(&5_u32.to_le_bytes());
        elf[80..88].copy_from_slice(&0x10000_u64.to_le_bytes());
        elf[96..104].copy_from_slice(&132_u64.to_le_bytes());
        elf[104..112].copy_from_slice(&132_u64.to_le_bytes());
        elf[112..120].copy_from_slice(&4096_u64.to_le_bytes());
        elf[120..124].copy_from_slice(&(0x00000513_u32 | (u32::from(exit) << 20)).to_le_bytes());
        elf[124..128].copy_from_slice(&0x05d00893_u32.to_le_bytes());
        elf[128..132].copy_from_slice(&0x00000073_u32.to_le_bytes());
        elf.into()
    }

    fn verifier(
        epoch: u64,
        hash_type: ScriptHashType,
        exit: u8,
    ) -> TransactionScriptsVerifier<Loader> {
        let code = program(exit);
        let dep_type = packed::Script::new_builder()
            .args(Bytes::from_static(b"code type id").pack())
            .build();
        let code_hash = if hash_type == ScriptHashType::Type {
            dep_type.calc_script_hash()
        } else {
            packed::CellOutput::calc_data_hash(&code)
        };
        let script = packed::Script::new_builder()
            .code_hash(code_hash)
            .hash_type(hash_type.into())
            .build();
        let dep_point = OutPoint::new_builder().tx_hash([1_u8; 32].pack()).build();
        let input_point = OutPoint::new_builder().tx_hash([2_u8; 32].pack()).build();
        let output = packed::CellOutput::new_builder()
            .capacity(10_000_000_000_u64.pack())
            .lock(script.clone())
            .type_(Some(script).pack())
            .build();
        let header = ckb_types::core::HeaderBuilder::default()
            .number(10_u64.pack())
            .epoch(EpochNumberWithFraction::new(epoch, 0, 1).pack())
            .build();
        let info = TransactionInfo::new(
            9,
            EpochNumberWithFraction::new(epoch, 0, 1),
            [9_u8; 32].pack(),
            1,
        );
        let resolved = ResolvedTransaction {
            transaction: TransactionBuilder::default()
                .input(
                    packed::CellInput::new_builder()
                        .previous_output(input_point.clone())
                        .build(),
                )
                .output(output.clone())
                .output_data(Bytes::new().pack())
                .cell_dep(
                    packed::CellDep::new_builder()
                        .out_point(dep_point.clone())
                        .build(),
                )
                .build(),
            resolved_inputs: vec![CellMetaBuilder::from_cell_output(output, Bytes::new())
                .out_point(input_point)
                .transaction_info(info.clone())
                .build()],
            resolved_cell_deps: vec![CellMetaBuilder::from_cell_output(
                packed::CellOutput::new_builder()
                    .type_(Some(dep_type).pack())
                    .build(),
                code,
            )
            .out_point(dep_point)
            .transaction_info(info)
            .build()],
            resolved_dep_groups: vec![],
        };
        TransactionScriptsVerifier::new(
            Arc::new(resolved),
            Loader::default(),
            Arc::new(consensus(VmActivationEpochs { vm1: 100, vm2: 200 }, 10_000_000).unwrap()),
            Arc::new(TxVerifyEnv::new_commit(&header)),
        )
    }

    #[test]
    fn native_vm_retains_distinct_lock_and_type_group_cycles() {
        for epoch in [99, 100, 200] {
            let verifier = verifier(epoch, ScriptHashType::Type, 0);
            let result = execute_groups(&verifier, 10_000_000, [8; 32]).unwrap();
            assert_eq!(result.groups.len(), 2);
            assert_eq!(result.groups[0].script_hash, result.groups[1].script_hash);
            assert_ne!(result.groups[0].group_type, result.groups[1].group_type);
            assert!(result.groups.iter().all(|group| group.cycles > 0));
            assert_eq!(result.cycles, verifier.verify(10_000_000).unwrap() as i64);
            result.validate().unwrap();
        }
    }

    #[test]
    fn native_vm_uses_commit_epoch_and_hash_type_consensus_rules() {
        for (epoch, expected) in [
            (99, ScriptVersion::V0),
            (100, ScriptVersion::V1),
            (200, ScriptVersion::V2),
        ] {
            let verifier = verifier(epoch, ScriptHashType::Type, 0);
            for (_, _, group) in verifier.groups_with_type() {
                assert_eq!(verifier.select_version(&group.script).unwrap(), expected);
            }
        }
        for (kind, expected) in [
            (ScriptHashType::Data, ScriptVersion::V0),
            (ScriptHashType::Data1, ScriptVersion::V1),
            (ScriptHashType::Data2, ScriptVersion::V2),
        ] {
            let verifier = verifier(200, kind, 0);
            for (_, _, group) in verifier.groups_with_type() {
                assert_eq!(verifier.select_version(&group.script).unwrap(), expected);
            }
        }
        let early = verifier(99, ScriptHashType::Data1, 0);
        assert!(execute_groups(&early, 10_000_000, [8; 32]).is_err());
    }

    #[test]
    fn native_vm_rejects_failed_groups_and_cycle_budget_without_partial_result() {
        assert!(
            execute_groups(&verifier(200, ScriptHashType::Data, 1), 10_000_000, [8; 32])
                .unwrap_err()
                .to_string()
                .contains("failed")
        );
        assert!(execute_groups(&verifier(200, ScriptHashType::Data, 0), 1, [8; 32]).is_err());
    }

    #[test]
    fn native_vm_rejects_missing_groups() {
        let resolved = ResolvedTransaction {
            transaction: TransactionBuilder::default().build(),
            resolved_inputs: vec![],
            resolved_cell_deps: vec![],
            resolved_dep_groups: vec![],
        };
        let verifier = TransactionScriptsVerifier::new(
            Arc::new(resolved),
            Loader::default(),
            Arc::new(consensus(VmActivationEpochs { vm1: 0, vm2: 0 }, 10_000_000).unwrap()),
            Arc::new(TxVerifyEnv::new_commit(
                &ckb_types::core::HeaderBuilder::default().build(),
            )),
        );
        assert!(execute_groups(&verifier, 10_000_000, [8; 32])
            .unwrap_err()
            .to_string()
            .contains("no script groups"));
    }
}
