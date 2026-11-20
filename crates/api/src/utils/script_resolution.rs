use std::collections::{HashMap, HashSet};

use ckbadger_store::{
    types::{PositionedCellInfo, ScriptVersionInfo},
    CkbadgerStore, ReferenceCode,
};

const UNKNOWN_SCRIPT_NAME: &str = "unknown";

pub fn is_known_script_name(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(UNKNOWN_SCRIPT_NAME)
}

/// A script's hash type as stored (`i16`), checked to be one CKB defines
/// (data 0, type 1, data1 2, data2 4); anything else names the script.
pub fn script_hash_type(code_hash: &[u8], hash_type: i16) -> anyhow::Result<u8> {
    u8::try_from(hash_type)
        .ok()
        .filter(|byte| hash_type_to_string(*byte).is_some())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "script 0x{} has hash type {hash_type}, not a CKB hash type",
                hex::encode(code_hash)
            )
        })
}

/// The deployment hashes a script's pair names: by `type` the code hash is
/// the deployment's type hash, by `data` / `data1` / `data2` its data hash.
/// `(type hash, data hash)`.
pub fn pair_deployment_hashes(
    code_hash: &[u8],
    hash_type: u8,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    if hash_type == 1 {
        (Some(code_hash.to_vec()), None)
    } else {
        (None, Some(code_hash.to_vec()))
    }
}

pub fn hash_type_to_u8(hash_type: &str) -> Option<u8> {
    ckbadger_common::hash_type_from_label(hash_type)
}

pub fn hash_type_to_string(hash_type: u8) -> Option<&'static str> {
    ckbadger_common::hash_type_label(hash_type)
}

/// One live cell a type reference's resolution reads: its outpoint and its
/// binary (data hash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeReferenceLiveMatch {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    pub version_hash: Vec<u8>,
}

/// A type reference's live cells, read bounded ([`resolve_live_type_reference`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeReferenceLive {
    /// The type script under the hash is the consensus Type ID script.
    pub native_type_id: bool,
    /// A native Type ID reference: its one live Type ID cell. Any other: its
    /// live CODE cells, never its other live cells.
    pub matches: Vec<TypeReferenceLiveMatch>,
}

pub type VersionCodeCell = (Vec<u8>, i16, PositionedCellInfo, bool);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentScriptVersion {
    pub version_hash: Vec<u8>,
    pub version_info: Option<ScriptVersionInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousCurrentScriptVersion {
    pub version_hashes: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentScriptVersionResolution {
    NotFound,
    Resolved(Box<CurrentScriptVersion>),
    Ambiguous(Box<AmbiguousCurrentScriptVersion>),
}

/// Minimal cell metadata needed for per-transaction script resolution.
#[derive(Debug, Clone)]
pub struct DepCellInfo {
    pub type_script_hash: Option<Vec<u8>>,
    pub data_hash: Option<Vec<u8>>,
    /// Whether the cell's data is code (`keys::is_code_cell`, read from the
    /// code-cell index): only code is a binary version.
    pub is_code: bool,
}

/// Build a code_hash -> version_hash mapping from a transaction's resolved dep cells.
///
/// For type references: type_script_hash -> data_hash (the bytecode version).
/// For data references: data_hash -> data_hash (identity -- the code_hash IS the version).
///
/// First-seen wins for defensive correctness. In valid CKB transactions, each type_script_hash
/// appears at most once in cell_deps (CKB consensus rejects duplicates).
///
/// A dep cell whose data is not code (`secp256k1_data`, a config cell) is
/// not a binary version and maps nothing (finding 8).
pub fn build_dep_cell_mappings(dep_cells: &[DepCellInfo]) -> HashMap<Vec<u8>, Vec<u8>> {
    let mut mappings: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

    for cell in dep_cells.iter().filter(|cell| cell.is_code) {
        if let (Some(type_hash), Some(data_hash)) = (&cell.type_script_hash, &cell.data_hash) {
            mappings
                .entry(type_hash.clone())
                .or_insert_with(|| data_hash.clone());
        }
        if let Some(data_hash) = &cell.data_hash {
            mappings
                .entry(data_hash.clone())
                .or_insert_with(|| data_hash.clone());
        }
    }

    mappings
}

/// Read a dep cell's metadata from ckbadger's stores by outpoint: `None` when
/// the payload is not indexed yet (the node may be ahead of the store); a read
/// error propagates.
fn read_dep_cell_info(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    tx_hash: &[u8],
    output_index: i16,
) -> anyhow::Result<Option<DepCellInfo>> {
    let key = ckbadger_store::keys::encode_outpoint(tx_hash, output_index);
    let Some(info) = cells_store.get_cell_by_outpoint_key(&key)? else {
        return Ok(None);
    };
    // `cell_by_data_hash` has a row exactly for code data (`keys::is_code_cell`).
    let is_code = match info.data_hash {
        Some(data_hash) => store.is_code_data_hash(&data_hash)?,
        None => false,
    };
    Ok(Some(DepCellInfo {
        type_script_hash: info.type_script_hash().map(|hash| hash.to_vec()),
        data_hash: info.data_hash.map(|hash| hash.to_vec()),
        is_code,
    }))
}

/// Expand an on-chain transaction's `dep_type = dep_group` cell dep
/// `dep_tx_hash:dep_index` into its member out points.
///
/// The node accepted the transaction, so it resolved this dep group by the
/// consensus rule: the dep cell exists and its data decodes as a dep group
/// ([`decode_dep_group`]). Data that is missing or does not decode is a broken
/// invariant, not an empty member list (D-4a). `CkbChainReader::get_cell_data`
/// answers `None` for a cell the node does not store (for an on-chain dep, that
/// violation) and an error for a failed read or an undecodable row, which
/// propagates with the dep named.
fn expand_dep_group(
    ckb_store: &ckb_store_reader::CkbChainReader,
    tx_hash_hex: &str,
    dep_tx_hash: &[u8; 32],
    dep_index: u32,
) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
    let data = ckb_store
        .get_cell_data(dep_tx_hash, dep_index)
        .map_err(|e| {
            anyhow::anyhow!(
                "tx {tx_hash_hex}: dep_group cell dep 0x{}:{dep_index}: {e:#}",
                hex::encode(dep_tx_hash)
            )
        })?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "tx {tx_hash_hex}: dep_group cell dep 0x{}:{dep_index} has no data in the CKB store",
                hex::encode(dep_tx_hash)
            )
        })?;
    decode_dep_group(&data).map_err(|e| {
        anyhow::anyhow!(
            "tx {tx_hash_hex}: dep_group cell dep 0x{}:{dep_index}: {e}",
            hex::encode(dep_tx_hash)
        )
    })
}

/// The member out points of a dep group cell's data, by the consensus rule
/// (`ckb_types::core::cell::parse_dep_group_data`): non-empty data that is a
/// molecule `OutPointVec` with at least one member. The one structural rule
/// for every dep group decode in the API: a declared `dep_group` cell dep
/// (`decode_dep_group`, the tx page's resolved dependencies) and the cell
/// page's dep group classification.
pub(crate) fn dep_group_members(data: &[u8]) -> anyhow::Result<ckb_types::packed::OutPointVec> {
    use ckb_types::{packed, prelude::*};
    anyhow::ensure!(!data.is_empty(), "dep group data is empty");
    let members = packed::OutPointVec::from_slice(data).map_err(|e| {
        anyhow::anyhow!(
            "dep group data ({} bytes) is not an OutPointVec: {e}",
            data.len()
        )
    })?;
    anyhow::ensure!(!members.is_empty(), "dep group has no members");
    Ok(members)
}

/// Decode a dep group cell's data into its member out points by the consensus
/// rule ([`dep_group_members`]), as store keys. A member's index is an
/// output index, so it fits `i16` for any out point the chain resolved; one
/// that does not is an error naming the member.
fn decode_dep_group(data: &[u8]) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
    use ckb_types::prelude::*;
    dep_group_members(data)?
        .into_iter()
        .enumerate()
        .map(|(item, out_point)| {
            let tx_hash = out_point.tx_hash().raw_data().to_vec();
            let index: u32 = out_point.index().unpack();
            let index = i16::try_from(index).map_err(|_| {
                anyhow::anyhow!(
                    "dep group member {item} (0x{}:{index}) has an output index beyond i16",
                    hex::encode(&tx_hash)
                )
            })?;
            Ok((tx_hash, index))
        })
        .collect()
}

/// Resolve all dep cells for a transaction, building a code_hash -> version_hash mapping.
///
/// `Ok(None)` if the CKB store is unavailable or the transaction is not found.
/// A dep cell the store has not indexed yet is skipped (the node can be ahead
/// of the store; the caller falls back to global resolution for any code_hash
/// not in the mapping); a store read error propagates (finding 8), and so does
/// a `dep_group` dep that does not expand by the consensus rule (D-4a).
pub fn resolve_dep_cells_for_transaction(
    state: &crate::AppState,
    tx_hash_hex: &str,
) -> anyhow::Result<Option<HashMap<Vec<u8>, Vec<u8>>>> {
    let Some(ckb_store) = state.ckb_store.as_ref() else {
        return Ok(None);
    };
    let tx_hash_arr: [u8; 32] = hex::decode(tx_hash_hex.strip_prefix("0x").unwrap_or(tx_hash_hex))
        .map_err(|e| anyhow::anyhow!("tx hash {tx_hash_hex} is not hex: {e}"))?
        .try_into()
        .map_err(|bytes: Vec<u8>| {
            anyhow::anyhow!(
                "tx hash {tx_hash_hex} has {} bytes, expected 32",
                bytes.len()
            )
        })?;

    let Some(tx_view) = ckb_store.get_transaction(&tx_hash_arr)? else {
        return Ok(None);
    };
    let rpc_tx = ckb_store_reader::convert_transaction_view(&tx_view);

    let mut outpoints: Vec<(Vec<u8>, i16)> = Vec::new();

    for dep in &rpc_tx.cell_deps {
        let dep_tx_hash = hex::decode(
            dep.out_point
                .tx_hash
                .strip_prefix("0x")
                .unwrap_or(&dep.out_point.tx_hash),
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "tx {tx_hash_hex}: cell dep tx hash {} is not hex: {e}",
                dep.out_point.tx_hash
            )
        })?;
        let dep_index_str = dep
            .out_point
            .index
            .strip_prefix("0x")
            .unwrap_or(&dep.out_point.index);
        let dep_index = u32::from_str_radix(dep_index_str, 16).map_err(|e| {
            anyhow::anyhow!(
                "tx {tx_hash_hex}: cell dep index {} is not a hex u32: {e}",
                dep.out_point.index
            )
        })?;

        if dep.dep_type == "dep_group" {
            let dep_tx_arr: [u8; 32] = dep_tx_hash.as_slice().try_into().map_err(|_| {
                anyhow::anyhow!(
                    "tx {tx_hash_hex}: dep_group cell dep tx hash {} has {} bytes, expected 32",
                    dep.out_point.tx_hash,
                    dep_tx_hash.len()
                )
            })?;
            outpoints.extend(expand_dep_group(
                ckb_store,
                tx_hash_hex,
                &dep_tx_arr,
                dep_index,
            )?);
        } else {
            let index = i16::try_from(dep_index).map_err(|e| {
                anyhow::anyhow!("tx {tx_hash_hex}: cell dep index {dep_index} exceeds i16: {e}")
            })?;
            outpoints.push((dep_tx_hash, index));
        }
    }

    let mut dep_cells: Vec<DepCellInfo> = Vec::with_capacity(outpoints.len());
    for (dep_tx_hash, index) in &outpoints {
        let info = read_dep_cell_info(&state.store, &state.append_only_store, dep_tx_hash, *index)
            .map_err(|e| {
                anyhow::anyhow!(
                    "tx {tx_hash_hex}: failed to read cell dep 0x{}:{index}: {e}",
                    hex::encode(dep_tx_hash)
                )
            })?;
        dep_cells.extend(info);
    }

    Ok(Some(build_dep_cell_mappings(&dep_cells)))
}

/// What a type reference's live cells are, read bounded by its code cells
/// (resync fix round S3, the read the indexer's rollup makes): one prefix
/// seek over `live_code_cells_by_type` (the reference's live code cells under
/// a type script that is not Type ID), and, only when that holds none, the
/// reference's first live cell (one row of `cell_by_type`): every cell under
/// one type hash carries the one script, so that cell says whether the
/// reference is the consensus Type ID script (its one live cell) or holds no
/// code. Never a read of the type's whole live set: a populous type a
/// stranger names costs its code cells, which cost their owner capacity.
pub fn resolve_live_type_reference(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    reference_hash: &[u8],
) -> anyhow::Result<TypeReferenceLive> {
    let code = store.list_live_code_cells_by_type(reference_hash)?;
    if !code.is_empty() {
        return Ok(TypeReferenceLive {
            native_type_id: false,
            matches: code
                .into_iter()
                .map(|(tx_hash, output_index, binary)| TypeReferenceLiveMatch {
                    tx_hash,
                    output_index,
                    version_hash: binary.to_vec(),
                })
                .collect(),
        });
    }
    let first = store
        .list_cells_by_type(reference_hash, 1, None, cells_store)?
        .into_iter()
        .next();
    match first {
        Some((tx_hash, output_index, cell))
            if cell
                .type_code_hash
                .as_ref()
                .zip(cell.type_hash_type)
                .is_some_and(|(hash, kind)| {
                    ckbadger_store::keys::is_type_id_script(hash, kind)
                }) =>
        {
            let version_hash = cell
                .cell
                .data_hash
                .map(|hash| hash.to_vec())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                    "live Type ID cell is missing data_hash: reference_hash=0x{}, outpoint=0x{}:{}",
                    hex::encode(reference_hash),
                    hex::encode(&tx_hash),
                    output_index
                )
                })?;
            Ok(TypeReferenceLive {
                native_type_id: true,
                matches: vec![TypeReferenceLiveMatch {
                    tx_hash,
                    output_index,
                    version_hash,
                }],
            })
        }
        _ => Ok(TypeReferenceLive {
            native_type_id: false,
            matches: Vec::new(),
        }),
    }
}

/// Return the persisted reference->version mapping when it constitutes a real
/// resolution candidate.
///
/// Type mappings (hash_type=1) are always candidates: they are only written
/// from resolved dep cells (live path) or observed live code-cell versions
/// (bulk path).
///
/// Data-family mappings (hash_type 0/2/4) are self-mappings written on ANY
/// observed data-form usage — including junk locks that reuse a type-reference
/// hash with a data hash_type even though no binary with that data hash exists
/// on chain. Such a mapping only counts as a candidate when at least one code
/// cell (live or consumed) carries data whose hash equals the version hash,
/// read from the same CF_CELL_BY_DATA_HASH index the code-cells endpoint uses.
pub fn persisted_reference_version_candidate(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(version_hash) = store.get_script_reference_version_hash(hash_type, reference_hash)?
    else {
        return Ok(None);
    };
    if matches!(hash_type, 0 | 2 | 4)
        && store
            .find_any_cell_by_data_hash(&version_hash, cells_store)?
            .is_none()
    {
        return Ok(None);
    }
    Ok(Some(version_hash))
}

/// Build the observed Type ID reference set from already-listed version rows.
pub fn observed_type_id_references_from_versions(
    versions: &[(Vec<u8>, ScriptVersionInfo)],
) -> HashSet<Vec<u8>> {
    versions
        .iter()
        .flat_map(|(_, version)| {
            version
                .type_id_deployments
                .iter()
                .map(|(reference, _)| reference.clone())
        })
        .collect()
}

/// Observed native TypeID deployments retain provenance after retirement or
/// burn. For these references, only the indexer's canonical mapping identifies
/// a current executable; a live typed cell may now carry non-executable data.
///
/// Early-exits on the first matching deployment. Callers that already listed
/// versions should use [`observed_type_id_references_from_versions`] +
/// `.contains()` instead of calling this.
pub fn has_observed_type_id_lineage(
    store: &CkbadgerStore,
    reference_hash: &[u8],
) -> anyhow::Result<bool> {
    Ok(store.list_script_versions()?.iter().any(|(_, version)| {
        version
            .type_id_deployments
            .iter()
            .any(|(reference, _)| reference.as_slice() == reference_hash)
    }))
}

/// Resolve a reference's current binary candidate. Family routes pass this
/// result to the shared family-membership operator: a retired registered
/// reference may still belong to a family when its current binary is absent.
/// Type forms use the validated canonical mapping. Data-family forms also
/// recognize the reference hash itself when the caller admits that version
/// (self usage of a known version's binary).
pub fn reference_form_member_version(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
    is_allowed_version: &dyn Fn(&[u8]) -> bool,
) -> anyhow::Result<Option<Vec<u8>>> {
    Ok(reference_form_member_version_with_lineage(
        store,
        cells_store,
        hash_type,
        reference_hash,
        is_allowed_version,
        None,
    )?
    .current()
    .map(<[u8]>::to_vec))
}

/// What one reference form runs now ([`ReferenceCode`], the input of the one
/// family rule, `ScriptFamilyMembershipIndex::family_of_form`):
///
/// - a native Type ID reference (a live Type ID cell under it, or observed
///   lineage and no live cell): its binding, the indexer's canonical one, or
///   none when retired;
/// - a generic type reference: the distinct binaries its live code cells
///   hold, none, one or several (it reached `None` for both "no live cell"
///   and "several live binaries" before the lead's ruling; `Several` now
///   carries them);
/// - a data form: its own hash where a code cell carries it.
///
/// Callers that already loaded all version records provide the observed Type
/// ID set, avoiding another full version scan for each retired reference.
pub fn reference_form_member_version_with_lineage(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
    is_allowed_version: &dyn Fn(&[u8]) -> bool,
    observed_type_id: Option<bool>,
) -> anyhow::Result<ReferenceCode> {
    let one = |binary: Option<Vec<u8>>| binary.map_or(ReferenceCode::None, ReferenceCode::One);
    if hash_type == 1 {
        let live = resolve_live_type_reference(store, cells_store, reference_hash)?;
        if live.native_type_id
            || (live.matches.is_empty()
                && match observed_type_id {
                    Some(observed) => observed,
                    None => has_observed_type_id_lineage(store, reference_hash)?,
                })
        {
            return Ok(one(persisted_reference_version_candidate(
                store,
                cells_store,
                hash_type,
                reference_hash,
            )?));
        }
        // A generic type reference: the distinct binaries its live code
        // cells hold (the read carries code cells only).
        return Ok(ReferenceCode::of_live_binaries(
            live.matches
                .into_iter()
                .map(|candidate| candidate.version_hash)
                .collect(),
        ));
    }
    if let Some(version_hash) =
        persisted_reference_version_candidate(store, cells_store, hash_type, reference_hash)?
    {
        return Ok(ReferenceCode::One(version_hash));
    }
    if matches!(hash_type, 0 | 2 | 4) && is_allowed_version(reference_hash) {
        return Ok(ReferenceCode::One(reference_hash.to_vec()));
    }
    Ok(ReferenceCode::None)
}

/// Family charts and usage describe the whole indexed history of a reference.
/// A generic type reference with no current executable can still have a
/// persisted chain-observed binding to a family. That historical candidate is
/// never returned by current executable resolution.
pub fn historical_family_member_candidate(
    store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
    current_version_hash: Option<Vec<u8>>,
) -> anyhow::Result<Option<Vec<u8>>> {
    if hash_type == 1 && current_version_hash.is_none() {
        store.get_script_reference_version_hash(hash_type, reference_hash)
    } else {
        Ok(current_version_hash)
    }
}

/// Resolve ONE observed reference form -- the exact (reference_hash,
/// hash_type) pair -- to its version.
///
/// Unlike [`resolve_script_by_hash`], which answers "what does this hash mean
/// anywhere on chain" and therefore spans every form of a deployment, this
/// answers "what does this hash mean when used with this hash_type". It is the
/// resolution behind the code-cell endpoints when the caller supplies a
/// hash_type, so a junk data form that reuses a type reference's bytes cannot
/// borrow the real deployment's code cells.
///
/// Membership itself is not recomputed here: it reuses
/// [`reference_form_member_version`] (persisted candidate, then a data-family
/// self-reference) with "a code cell carries this bytecode" as the
/// admissible-version rule. Observed native TypeIDs use only the canonical
/// mapping; other type forms retain live typed-code discovery.
pub fn resolve_script_form_by_hash(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
) -> anyhow::Result<CurrentScriptVersionResolution> {
    if hash_type_to_string(hash_type).is_none() {
        anyhow::bail!(
            "cannot resolve a script reference form with unknown hash_type: reference_hash=0x{}, hash_type={}",
            hex::encode(reference_hash),
            hash_type
        );
    }

    let has_own_code_cell = store
        .find_any_cell_by_data_hash(reference_hash, cells_store)?
        .is_some();
    // Defer lineage: `reference_form_member_version_with_lineage` only scans
    // when live type matches are empty (passing None). The common cell-detail
    // path has live matches and must not pay a full versions table scan.
    // The one reading of what the form runs: one binary, several (a generic
    // type reference's live code cells), or none.
    match reference_form_member_version_with_lineage(
        store,
        cells_store,
        hash_type,
        reference_hash,
        &|hash: &[u8]| has_own_code_cell && hash == reference_hash,
        None,
    )? {
        ReferenceCode::One(version_hash) => {
            let version_info = store.get_script_version(&version_hash)?;
            Ok(CurrentScriptVersionResolution::Resolved(Box::new(
                CurrentScriptVersion {
                    version_hash,
                    version_info,
                },
            )))
        }
        ReferenceCode::Several(version_hashes) => Ok(CurrentScriptVersionResolution::Ambiguous(
            Box::new(AmbiguousCurrentScriptVersion { version_hashes }),
        )),
        ReferenceCode::None => Ok(CurrentScriptVersionResolution::NotFound),
    }
}

/// Resolve canonical bindings or a direct historical binary hash. Observed
/// native TypeIDs without a binding have no current executable; only other
/// type references use live typed-code discovery.
pub fn resolve_script_by_hash(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    reference_hash: &[u8],
) -> anyhow::Result<CurrentScriptVersionResolution> {
    let direct_version = store.get_script_version(reference_hash)?;
    let live = resolve_live_type_reference(store, cells_store, reference_hash)?;
    let native_type_id = live.native_type_id;
    let live_type_matches = live.matches;
    let retired_type_id =
        live_type_matches.is_empty() && has_observed_type_id_lineage(store, reference_hash)?;
    let persisted_versions = {
        let mut seen = HashSet::new();
        let mut versions = Vec::new();
        for hash_type in [0u8, 1u8, 2u8, 4u8] {
            // A generic type binding records historical use, not a unique
            // current executable. Its live cells decide current resolution.
            if hash_type == 1 && !native_type_id && !retired_type_id {
                continue;
            }
            if let Some(version_hash) = persisted_reference_version_candidate(
                store,
                cells_store,
                hash_type,
                reference_hash,
            )? {
                if seen.insert(version_hash.clone()) {
                    versions.push(version_hash);
                }
            }
        }
        versions
    };

    if persisted_versions.len() == 1 {
        let version_hash = persisted_versions[0].clone();
        let version_info = store.get_script_version(&version_hash)?;
        return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
            CurrentScriptVersion {
                version_hash,
                version_info,
            },
        )));
    }

    if persisted_versions.len() > 1 {
        return Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
            AmbiguousCurrentScriptVersion {
                version_hashes: persisted_versions,
            },
        )));
    }

    let type_matches = if native_type_id || retired_type_id {
        Vec::new()
    } else {
        live_type_matches
    };
    if let Some(version_info) = direct_version {
        let mut unique_versions: Vec<Vec<u8>> = {
            let mut seen = HashSet::new();
            type_matches
                .iter()
                .filter(|m| seen.insert(m.version_hash.clone()))
                .map(|m| m.version_hash.clone())
                .collect()
        };
        if unique_versions.is_empty() {
            return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
                CurrentScriptVersion {
                    version_hash: reference_hash.to_vec(),
                    version_info: Some(version_info),
                },
            )));
        }
        if unique_versions.len() == 1 && unique_versions[0] == reference_hash {
            return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
                CurrentScriptVersion {
                    version_hash: reference_hash.to_vec(),
                    version_info: Some(version_info),
                },
            )));
        }
        if !unique_versions.iter().any(|hash| hash == reference_hash) {
            unique_versions.push(reference_hash.to_vec());
        }
        unique_versions.sort();
        return Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
            AmbiguousCurrentScriptVersion {
                version_hashes: unique_versions,
            },
        )));
    }
    if !type_matches.is_empty() {
        let unique_versions: Vec<Vec<u8>> = {
            let mut seen = HashSet::new();
            type_matches
                .iter()
                .filter(|m| seen.insert(m.version_hash.clone()))
                .map(|m| m.version_hash.clone())
                .collect()
        };
        if unique_versions.len() > 1 {
            return Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
                AmbiguousCurrentScriptVersion {
                    version_hashes: unique_versions,
                },
            )));
        }
        let version_hash = unique_versions[0].clone();
        let version_info = store.get_script_version(&version_hash)?;
        return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
            CurrentScriptVersion {
                version_hash,
                version_info,
            },
        )));
    }

    Ok(CurrentScriptVersionResolution::NotFound)
}

pub fn list_version_code_cells(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    version_hash: &[u8],
) -> anyhow::Result<Vec<VersionCodeCell>> {
    let mut code_cells = store.list_all_cells_by_data_hash(version_hash, cells_store)?;
    code_cells.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| left.2.created_at_block.cmp(&right.2.created_at_block))
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    Ok(code_cells)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep_group_data(members: &[([u8; 32], u32)]) -> Vec<u8> {
        let mut data = (members.len() as u32).to_le_bytes().to_vec();
        for (tx_hash, index) in members {
            data.extend_from_slice(tx_hash);
            data.extend_from_slice(&index.to_le_bytes());
        }
        data
    }

    /// D-4a: a dep group decodes into its members in order.
    #[test]
    fn decode_dep_group_returns_every_member() {
        let data = dep_group_data(&[([0x01; 32], 0), ([0x02; 32], 300)]);
        assert_eq!(
            decode_dep_group(&data).unwrap(),
            vec![(vec![0x01; 32], 0), (vec![0x02; 32], 300)]
        );
    }

    /// D-4a: data the consensus rule does not resolve as a dep group (empty,
    /// not an `OutPointVec`, a truncated one, no members) is an error, never an
    /// empty member list; so is a member index beyond any output index.
    #[test]
    fn decode_dep_group_rejects_what_consensus_does_not_resolve() {
        let truncated = {
            let mut data = dep_group_data(&[([0x01; 32], 0), ([0x02; 32], 1)]);
            data.truncate(data.len() - 1);
            data
        };
        for (data, expected) in [
            (Vec::new(), "dep group data is empty"),
            (b"not an OutPointVec".to_vec(), "is not an OutPointVec"),
            (truncated, "is not an OutPointVec"),
            (dep_group_data(&[]), "dep group has no members"),
            (
                dep_group_data(&[([0x01; 32], 0), ([0x03; 32], 40_000)]),
                "member 1 (0x0303",
            ),
        ] {
            let error = decode_dep_group(&data).unwrap_err().to_string();
            assert!(error.contains(expected), "{expected}: {error}");
        }
    }

    #[test]
    fn historical_family_candidate_keeps_retired_type_provenance_out_of_current_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference = vec![0x34; 32];
        let version = vec![0x56; 32];
        store
            .put_script_reference_to_version_direct(1, &reference, &version)
            .unwrap();
        assert_eq!(
            reference_form_member_version(&store, &store, 1, &reference, &|_| true).unwrap(),
            None,
            "a retired generic type reference has no current executable"
        );
        assert_eq!(
            historical_family_member_candidate(&store, 1, &reference, None).unwrap(),
            Some(version),
            "historical chart membership retains its indexed binding"
        );
        assert_eq!(
            resolve_script_form_by_hash(&store, &store, 1, &reference).unwrap(),
            CurrentScriptVersionResolution::NotFound
        );
        assert_eq!(
            historical_family_member_candidate(&store, 0, &reference, None).unwrap(),
            None,
            "a junk data form cannot borrow type-form provenance"
        );
    }
    use ckbadger_store::{batch::StoreBatch, types::LiveCellInfo, CkbadgerStore};

    /// The type reference of `seed_live_generic_code_cell`'s cell: the
    /// derived hash of its type script (code `[0x13; 32]`, `type`, no args).
    fn generic_code_cell_reference() -> Vec<u8> {
        crate::utils::address::compute_script_hash(&[0x13; 32], 1, &[])
    }

    fn seed_live_generic_code_cell(store: &CkbadgerStore, reference: &[u8], binary: &[u8]) {
        assert_eq!(
            reference,
            generic_code_cell_reference(),
            "the fixture indexes the cell under its own type script hash"
        );
        let tx = vec![0xb1; 32];
        let mut batch = StoreBatch::new(store);
        batch.put_cell(
            &tx,
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                udt_amount: None,
                data_hash: Some(binary.to_vec().try_into().unwrap()),
            },
            10,
        );
        batch.put_cell_by_type(reference, 10, &tx, 0);
        // a code cell: filed in the code-cell index, as only code is a binary
        // (lane rd/api-lock-names, the lead's ruling), and by its type script
        // (resync fix round S3), as the indexer writes it
        batch.put_cell_by_data_hash(binary, 10, &tx, 0);
        batch.put_live_code_cell(reference, &tx, 0, binary);
        batch.commit().unwrap();
    }

    #[test]
    fn type_id_retirement_keeps_history_without_promoting_non_executable_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // The type-id reference is the derived hash of the cell's TYPE_ID script.
        let reference = crate::utils::address::compute_script_hash(
            &ckbadger_store::keys::TYPE_ID_CODE_HASH,
            1,
            &[0x13; 32],
        );
        let historical_binary = vec![0x41; 32];
        let replacement_data = [0x51; 32];
        let tx = vec![0x61; 32];
        let history = ScriptVersionInfo {
            version_hash: historical_binary.clone(),
            canonical_reference_hash: Some(reference.clone()),
            canonical_hash_type: Some(1),
            type_id_deployments: vec![(reference.clone(), 10)],
            ..Default::default()
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_script_version(&historical_binary, &history);
        batch.put_cell(
            &tx,
            0,
            &LiveCellInfo {
                data_hash: Some(replacement_data),
                data_size: 31_560,
                capacity: 3_200_000_000_000,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some(ckbadger_store::keys::TYPE_ID_CODE_HASH),
                type_hash_type: Some(1),
                type_args: Some(vec![0x13; 32]),
                udt_amount: None,
            },
            20,
        );
        batch.put_cell_by_type(&reference, 20, &tx, 0);
        batch.commit().unwrap();

        // Executable retirement applies to every observed native TypeID,
        // including copies with no registered family anchor.
        assert!(history.registered_references.is_empty());
        assert!(has_observed_type_id_lineage(&store, &reference).unwrap());
        assert!(!has_observed_type_id_lineage(&store, &[0x99; 32]).unwrap());
        for hash_type in [None, Some(1)] {
            let result = match hash_type {
                None => resolve_script_by_hash(&store, &store, &reference),
                Some(kind) => resolve_script_form_by_hash(&store, &store, kind, &reference),
            }
            .unwrap();
            assert_eq!(result, CurrentScriptVersionResolution::NotFound);
        }
        let historical = resolve_script_by_hash(&store, &store, &historical_binary).unwrap();
        assert!(
            matches!(historical, CurrentScriptVersionResolution::Resolved(version)
            if version.version_hash == historical_binary && version.version_info == Some(history.clone()))
        );
        assert_eq!(
            store.get_script_version(&historical_binary).unwrap(),
            Some(history)
        );
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            None
        );

        // A fresh chain observation reactivates the reference through its
        // canonical binding, never by inspecting the current typed payload.
        let reactivated = vec![0x71; 32];
        store
            .put_script_reference_to_version_direct(1, &reference, &reactivated)
            .unwrap();
        for result in [
            resolve_script_by_hash(&store, &store, &reference).unwrap(),
            resolve_script_form_by_hash(&store, &store, 1, &reference).unwrap(),
        ] {
            assert!(
                matches!(result, CurrentScriptVersionResolution::Resolved(version)
                if version.version_hash == reactivated)
            );
        }
    }

    #[test]
    fn test_resolve_script_by_hash_returns_ambiguity_for_live_type_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // Two live code cells of one type script (one type reference), with
        // different binaries.
        let reference_hash = crate::utils::address::compute_script_hash(&[0x13; 32], 1, &[]);
        let version_hash_a = [0x44; 32];
        let version_hash_b = [0x55; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x70; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                udt_amount: None,
                data_hash: Some(version_hash_a),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x70; 32], 0);
        // a live code cell, as the indexer files it (resync fix round S3)
        batch.put_live_code_cell(&reference_hash, &[0x70; 32], 0, &version_hash_a);
        batch.put_cell(
            &[0x71; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x22; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                udt_amount: None,
                data_hash: Some(version_hash_b),
            },
            11,
        );
        batch.put_cell_by_type(&reference_hash, 11, &[0x71; 32], 0);
        // a live code cell, as the indexer files it (resync fix round S3)
        batch.put_live_code_cell(&reference_hash, &[0x71; 32], 0, &version_hash_b);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Ambiguous(ambiguous) => {
                assert_eq!(
                    ambiguous.version_hashes,
                    vec![version_hash_a, version_hash_b]
                );
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_by_hash_reports_generic_live_type_ambiguity_despite_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // Two live code cells of one type script (one type reference), with
        // different binaries.
        let reference_hash = crate::utils::address::compute_script_hash(&[0x13; 32], 1, &[]);
        let mapped_version_hash = [0x74; 32];
        let conflicting_version_hash = [0x85; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &mapped_version_hash)
            .unwrap();
        store
            .put_script_version(
                &mapped_version_hash,
                &ScriptVersionInfo {
                    version_hash: mapped_version_hash.to_vec(),
                    name: Some("Mapped Version".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x90; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                udt_amount: None,
                data_hash: Some(mapped_version_hash),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x90; 32], 0);
        // a live code cell, as the indexer files it (resync fix round S3)
        batch.put_live_code_cell(&reference_hash, &[0x90; 32], 0, &mapped_version_hash);
        batch.put_cell(
            &[0x91; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x22; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                udt_amount: None,
                data_hash: Some(conflicting_version_hash),
            },
            11,
        );
        batch.put_cell_by_type(&reference_hash, 11, &[0x91; 32], 0);
        // a live code cell, as the indexer files it (resync fix round S3)
        batch.put_live_code_cell(&reference_hash, &[0x91; 32], 0, &conflicting_version_hash);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Ambiguous(ambiguous) => {
                assert_eq!(
                    ambiguous.version_hashes,
                    vec![mapped_version_hash, conflicting_version_hash]
                );
            }
            other => panic!("expected live generic ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn test_generic_persisted_mapping_without_live_code_is_not_current() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference = vec![0x66; 32];
        let historical = vec![0x77; 32];
        store
            .put_script_reference_to_version_direct(1, &reference, &historical)
            .unwrap();

        assert_eq!(
            resolve_script_form_by_hash(&store, &store, 1, &reference).unwrap(),
            CurrentScriptVersionResolution::NotFound
        );
        assert_eq!(
            resolve_script_by_hash(&store, &store, &reference).unwrap(),
            CurrentScriptVersionResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_script_by_hash_resolves_unique_live_type_match_without_persisted_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // The type reference is the derived hash of the code cell's type script.
        let reference_hash = generic_code_cell_reference();
        let version_hash = [0x58; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x92; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                udt_amount: None,
                data_hash: Some(version_hash),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x92; 32], 0);
        // a live code cell, as the indexer files it (resync fix round S3)
        batch.put_live_code_cell(&reference_hash, &[0x92; 32], 0, &version_hash);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
                assert!(resolved.version_info.is_none());
            }
            other => panic!("expected unique live type match resolution, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_by_hash_ignores_data_self_mapping_without_code_cell() {
        // Mainnet secp junk-lock scenario: the persisted type-form mapping
        // resolves the reference to the real bytecode version, while a garbage
        // data-form self-mapping exists for the same reference bytes even
        // though no on-chain code cell carries data whose hash equals the
        // reference. The junk self-mapping must not create ambiguity.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // The type reference is the derived hash of the code cell's type script.
        let reference_hash = generic_code_cell_reference();
        let version_hash = vec![0x70; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &version_hash)
            .unwrap();
        // Garbage self-mapping written from data-form usage of the same bytes.
        store
            .put_script_reference_to_version_direct(0, &reference_hash, &reference_hash)
            .unwrap();
        store
            .put_script_version(
                &version_hash,
                &ScriptVersionInfo {
                    version_hash: version_hash.clone(),
                    name: Some("SECP256K1_BLAKE160".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        seed_live_generic_code_cell(&store, &reference_hash, &version_hash);

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
                assert_eq!(
                    resolved
                        .version_info
                        .as_ref()
                        .and_then(|info| info.name.as_deref()),
                    Some("SECP256K1_BLAKE160")
                );
            }
            other => panic!("expected junk data self-mapping to be ignored, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_by_hash_keeps_data_self_mapping_backed_by_code_cell() {
        // A data-form self-mapping whose bytecode exists on chain (a code cell
        // with matching data hash) must stay resolvable.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = [0x70; 32];
        let code_cell_tx = vec![0x77; 32];

        store
            .put_script_reference_to_version_direct(0, &reference_hash, &reference_hash)
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &code_cell_tx,
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                data_size: 4,
                udt_amount: None,
                data_hash: Some(reference_hash),
            },
            5,
        );
        batch.put_cell_by_data_hash(&reference_hash, 5, &code_cell_tx, 0);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, reference_hash);
            }
            other => {
                panic!("expected code-cell-backed data self-mapping to resolve, got {other:?}")
            }
        }
    }

    #[test]
    fn test_resolve_script_form_separates_type_and_junk_data_forms() {
        // Mainnet secp junk-lock scenario, asked per form: the type form
        // resolves to the real bytecode version, while the data form of the
        // same reference bytes -- whose binary does not exist on chain --
        // resolves to nothing instead of borrowing the type form's version.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // The type reference is the derived hash of the code cell's type script.
        let reference_hash = generic_code_cell_reference();
        let version_hash = vec![0x70; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &version_hash)
            .unwrap();
        store
            .put_script_reference_to_version_direct(0, &reference_hash, &reference_hash)
            .unwrap();

        seed_live_generic_code_cell(&store, &reference_hash, &version_hash);

        match resolve_script_form_by_hash(&store, &store, 1, &reference_hash).unwrap() {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
            }
            other => panic!("expected the type form to resolve, got {other:?}"),
        }
        assert_eq!(
            resolve_script_form_by_hash(&store, &store, 0, &reference_hash).unwrap(),
            CurrentScriptVersionResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_script_form_resolves_data_form_backed_by_code_cell() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = [0x70; 32];
        let code_cell_tx = vec![0x77; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &code_cell_tx,
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                data_size: 4,
                udt_amount: None,
                data_hash: Some(reference_hash),
            },
            5,
        );
        batch.put_cell_by_data_hash(&reference_hash, 5, &code_cell_tx, 0);
        batch.commit().unwrap();

        match resolve_script_form_by_hash(&store, &store, 0, &reference_hash).unwrap() {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, reference_hash);
            }
            other => panic!("expected the code-cell-backed data form to resolve, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_form_uses_live_type_matches_without_persisted_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // The type reference is the derived hash of the code cell's type script.
        let reference_hash = generic_code_cell_reference();
        let version_hash = [0x58; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x92; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_code_hash: [0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                udt_amount: None,
                data_hash: Some(version_hash),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x92; 32], 0);
        // the live type match is a code cell (only code is a binary: lane
        // rd/api-lock-names, the lead's ruling), filed by its type script as
        // the indexer files it (resync fix round S3)
        batch.put_cell_by_data_hash(&version_hash, 10, &[0x92; 32], 0);
        batch.put_live_code_cell(&reference_hash, &[0x92; 32], 0, &version_hash);
        batch.commit().unwrap();

        match resolve_script_form_by_hash(&store, &store, 1, &reference_hash).unwrap() {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
            }
            other => panic!("expected the live type match to resolve, got {other:?}"),
        }
        // The same bytes used as a data form have no binary on chain.
        assert_eq!(
            resolve_script_form_by_hash(&store, &store, 0, &reference_hash).unwrap(),
            CurrentScriptVersionResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_script_form_rejects_unknown_hash_type() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let err = resolve_script_form_by_hash(&store, &store, 3, &[0x9b; 32]).unwrap_err();
        assert!(err.to_string().contains("unknown hash_type"), "{err}");
    }

    #[test]
    fn test_build_dep_cell_mappings_type_and_data() {
        let type_hash = vec![0x9b; 32];
        let data_hash = vec![0x70; 32];

        let dep_cells = vec![DepCellInfo {
            type_script_hash: Some(type_hash.clone()),
            data_hash: Some(data_hash.clone()),
            is_code: true,
        }];

        let mappings = build_dep_cell_mappings(&dep_cells);
        assert_eq!(mappings.get(&type_hash), Some(&data_hash));
        assert_eq!(mappings.get(&data_hash), Some(&data_hash));
    }

    /// Finding 8: a dep cell whose data is not code (`secp256k1_data`, a
    /// Type ID config cell) is not a binary version: it maps nothing, by
    /// data hash or by type hash.
    #[test]
    fn test_build_dep_cell_mappings_skips_non_code_dep_cells() {
        let code_type = vec![0x9b; 32];
        let code_data = vec![0x70; 32];
        let config_type = vec![0x9c; 32];
        let config_data = vec![0x71; 32];
        let table_data = vec![0x72; 32];
        let dep_cells = vec![
            DepCellInfo {
                type_script_hash: Some(code_type.clone()),
                data_hash: Some(code_data.clone()),
                is_code: true,
            },
            DepCellInfo {
                type_script_hash: Some(config_type.clone()),
                data_hash: Some(config_data.clone()),
                is_code: false,
            },
            DepCellInfo {
                type_script_hash: None,
                data_hash: Some(table_data.clone()),
                is_code: false,
            },
        ];

        let mappings = build_dep_cell_mappings(&dep_cells);
        assert_eq!(mappings.len(), 2, "{mappings:?}");
        assert_eq!(mappings.get(&code_type), Some(&code_data));
        assert_eq!(mappings.get(&code_data), Some(&code_data));
        assert!(!mappings.contains_key(&config_type));
        assert!(!mappings.contains_key(&config_data));
        assert!(!mappings.contains_key(&table_data));
    }

    fn dep_payload(data_hash: Option<[u8; 32]>, data_size: i32) -> LiveCellInfo {
        LiveCellInfo {
            capacity: 1_000,
            lock_code_hash: [0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size,
            udt_amount: None,
            data_hash,
        }
    }

    /// Finding 8: code-ness comes from the code-cell index; a payload the
    /// store has not indexed yet is a skip, and a read error propagates
    /// instead of reading as "no dep cell".
    #[test]
    fn test_read_dep_cell_info_reads_code_ness_and_propagates_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let code_tx = [0x31u8; 32];
        let table_tx = [0x32u8; 32];
        let corrupt_tx = [0x33u8; 32];
        let code_hash = [0x41u8; 32];
        let table_hash = [0x42u8; 32];
        for (tx, cell) in [
            (code_tx, dep_payload(Some(code_hash), 4_096)),
            (table_tx, dep_payload(Some(table_hash), 1_024)),
        ] {
            store
                .put_cf(
                    store.cf_cells(),
                    &ckbadger_store::keys::encode_outpoint(&tx, 0),
                    &cell.encode().unwrap(),
                )
                .unwrap();
        }
        store
            .put_cf(
                store.cf_cell_by_data_hash(),
                &ckbadger_store::keys::encode_cell_index_key(&code_hash, 7, &code_tx, 0),
                &[],
            )
            .unwrap();
        store
            .put_cf(
                store.cf_cells(),
                &ckbadger_store::keys::encode_outpoint(&corrupt_tx, 0),
                &[0xff, 0x00, 0x01],
            )
            .unwrap();

        let code = read_dep_cell_info(&store, &store, &code_tx, 0)
            .unwrap()
            .unwrap();
        assert!(code.is_code);
        assert_eq!(code.data_hash.as_deref(), Some(&code_hash[..]));
        let table = read_dep_cell_info(&store, &store, &table_tx, 0)
            .unwrap()
            .unwrap();
        assert!(!table.is_code, "secp256k1_data-like data is not code");
        assert!(read_dep_cell_info(&store, &store, &[0x34; 32], 0)
            .unwrap()
            .is_none());
        assert!(
            read_dep_cell_info(&store, &store, &corrupt_tx, 0).is_err(),
            "a corrupt payload must not read as a missing dep cell"
        );
    }

    #[test]
    fn test_build_dep_cell_mappings_first_seen_wins() {
        let type_hash = vec![0x9b; 32];
        let data_hash_a = vec![0x70; 32];
        let data_hash_b = vec![0x71; 32];

        let dep_cells = vec![
            DepCellInfo {
                type_script_hash: Some(type_hash.clone()),
                data_hash: Some(data_hash_a.clone()),
                is_code: true,
            },
            DepCellInfo {
                type_script_hash: Some(type_hash.clone()),
                data_hash: Some(data_hash_b.clone()),
                is_code: true,
            },
        ];

        let mappings = build_dep_cell_mappings(&dep_cells);
        assert_eq!(mappings.get(&type_hash), Some(&data_hash_a));
    }

    #[test]
    fn test_build_dep_cell_mappings_skips_none_fields() {
        let dep_cells = vec![
            DepCellInfo {
                type_script_hash: None,
                data_hash: None,
                is_code: true,
            },
            DepCellInfo {
                type_script_hash: Some(vec![0xAA; 32]),
                data_hash: None,
                is_code: true,
            },
            DepCellInfo {
                type_script_hash: None,
                data_hash: Some(vec![0xBB; 32]),
                is_code: true,
            },
        ];

        let mappings = build_dep_cell_mappings(&dep_cells);
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings.get(&vec![0xBB; 32]), Some(&vec![0xBB; 32]));
        assert!(!mappings.contains_key(&vec![0xAA; 32]));
    }

    #[test]
    fn test_resolve_script_by_hash_resolves_direct_version_hash_without_persisted_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x52; 32];

        store
            .put_script_version(
                &reference_hash,
                &ScriptVersionInfo {
                    version_hash: reference_hash.clone(),
                    name: Some("Legacy Fallback".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, reference_hash);
                assert_eq!(
                    resolved
                        .version_info
                        .as_ref()
                        .and_then(|info| info.name.as_deref()),
                    Some("Legacy Fallback")
                );
            }
            other => panic!("expected direct version resolution, got {other:?}"),
        }
    }

    /// The cells the indexer leaves for a generic type reference `reference`:
    /// a live code cell per binary (marker, payload, `cell_by_type`,
    /// `cell_by_data_hash`, `live_code_cells_by_type`), and `traps` live cells of the same type that are
    /// not code, each with its append-only payload withheld, so any read of
    /// one fails.
    fn seed_generic_reference(
        store: &CkbadgerStore,
        reference: &[u8],
        binaries: &[[u8; 32]],
        traps: u8,
    ) {
        let mut batch = ckbadger_store::StoreBatch::new(store);
        for (index, binary) in binaries.iter().enumerate() {
            let tx = [0xd0 + index as u8; 32];
            batch.put_cell(
                &tx,
                0,
                &ckbadger_store::LiveCellInfo {
                    capacity: 100,
                    lock_code_hash: [0x12; 32],
                    lock_hash_type: 1,
                    lock_args: vec![],
                    type_code_hash: Some([0x13; 32]),
                    type_hash_type: Some(1),
                    type_args: Some(vec![0x5e; 4]),
                    data_size: 64,
                    udt_amount: None,
                    data_hash: Some(*binary),
                },
                11,
            );
            batch.put_cell_by_type(reference, 11, &tx, 0);
            batch.put_cell_by_data_hash(binary, 11, &tx, 0);
            batch.put_live_code_cell(reference, &tx, 0, binary);
        }
        for trap in 0..traps {
            let tx = [0xa0u8.wrapping_add(trap); 32];
            batch.put_live_cell_marker(&ckbadger_store::keys::encode_outpoint(&tx, 0), 11);
            batch.put_cell_by_type(reference, 11, &tx, 0);
        }
        batch.commit().unwrap();
    }

    /// resync fix round S3 (the lead's option B): the API settles a generic
    /// type reference from its live CODE cells only, the read the indexer
    /// makes. The type holds two live code cells and fifty live cells that are
    /// not code, each a trap (any read of one fails): the reference runs the
    /// two binaries, and no trap is touched, for the form's code and for the
    /// hash's resolution.
    #[test]
    fn a_generic_type_reference_is_read_from_its_code_cells_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference = crate::utils::address::compute_script_hash(&[0x13; 32], 1, &[0x5e; 4]);
        let binaries = [[0x61u8; 32], [0x62; 32]];
        seed_generic_reference(&store, &reference, &binaries, 50);

        let code = reference_form_member_version_with_lineage(
            &store,
            &store,
            1,
            &reference,
            &|_| true,
            Some(false),
        );
        assert_eq!(
            code.map_err(|e| e.to_string()),
            Ok(ReferenceCode::Several(
                binaries.iter().map(|b| b.to_vec()).collect()
            )),
            "the form's code, from its code cells only"
        );
        match resolve_script_by_hash(&store, &store, &reference) {
            Ok(CurrentScriptVersionResolution::Ambiguous(ambiguous)) => assert_eq!(
                ambiguous.version_hashes,
                binaries.iter().map(|b| b.to_vec()).collect::<Vec<_>>()
            ),
            other => panic!("the hash's resolution, from its code cells only: {other:?}"),
        }
    }
}
