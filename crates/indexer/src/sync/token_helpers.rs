use std::collections::{HashMap, HashSet};

use anyhow::Result;

use super::helpers::*;
use super::types::{TxData, XudtExtensionScript};

// ---------------------------------------------------------------------------
// Omnilock constants
// ---------------------------------------------------------------------------

/// Omnilock's mainnet reference (by `type`), for tests: the pairs Omnilock
/// runs are the registry's (`is_omnilock`).
#[cfg(test)]
pub(crate) const OMNILOCK_CODE_HASH_MAINNET_V2: &str =
    "0x9b819793a64463aed77c615d6cb226eea5487ccfc0783043a587254cda2b6f26";
const OMNILOCK_AUTH_LEN: usize = 21;
const OMNILOCK_SUPPLY_MODE_FLAG: u8 = 0b0000_1000;
const OMNILOCK_ADMIN_MODE_FLAG: u8 = 0b0000_0001;
const OMNILOCK_ACP_MODE_FLAG: u8 = 0b0000_0010;
const OMNILOCK_TIMELOCK_MODE_FLAG: u8 = 0b0000_0100;
const OMNILOCK_SUPPLY_INFO_CELL_MIN_DATA_LEN: usize = 65;
const OMNILOCK_SUPPLY_INFO_CELL_VERSION_V0: u8 = 0;

// ---------------------------------------------------------------------------
// XUDT / token-info constants
// ---------------------------------------------------------------------------

const XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN: usize = 32;
const XUDT_TYPE_ARGS_FLAGS_LEN: usize = 4;
const XUDT_TYPE_ARGS_MIN_LEN: usize = XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN + XUDT_TYPE_ARGS_FLAGS_LEN;
const XUDT_FLAGS_EXTENSION_MASK: u32 = 0x1FFF_FFFF;
const XUDT_FLAGS_EXTENSION_IN_ARGS: u32 = 0x1;
const XUDT_FLAGS_EXTENSION_IN_WITNESS: u32 = 0x2;
const XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN: usize = 20;
pub(crate) const UNIQUE_TYPE_ARGS_LEN: usize = 20;
const TOKEN_INFO_TAG_TOTAL_SUPPLY: u32 = 1;
const TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN: usize = 16;

/// The Unique Cell's mainnet binary, for tests (registered by data1 in
/// `docs/metadata/scripts/unique-cell.toml`; verified against the RGB++
/// Protocol issuance, tx 0xd088a12852664145773257eb1467cb0feca0d1d478968ce90b7f29bce24e2a4a,
/// block 12,654,045, out[0]). The pairs the Unique Cell runs are the
/// registry's ([`is_unique_cell`]).
#[cfg(test)]
pub(crate) const UNIQUE_CELL_CODE_HASH_MAINNET: &str =
    "0x2c8c11c985da60b0a330c61a85507416d6382c130ba67f0c47ab071e00aec628";

/// Whether a type script's pair runs the Unique Cell: the registry's Unique
/// Cell pairs (`ProtocolRegistry::is_unique_cell`: its registered reference
/// and its binary by every data hash type), the one recognition on both sync
/// paths (resync fix round S1). The Unique Cell's code hash by another hash
/// type runs no Unique Cell.
pub(crate) fn is_unique_cell(code_hash: &[u8], hash_type: i16) -> bool {
    crate::parser::registry::PROTOCOL_REGISTRY.is_unique_cell(code_hash, hash_type)
}

/// A transaction output that can be the Unique Cell an xUDT extension names:
/// type args of the extension's length (20 bytes) and data that parses as
/// token info. Whether its type script IS the Unique Cell is its pair's to
/// say ([`bound_unique_cell`] asks). The one definition for both sync paths.
pub(crate) fn unique_cell_candidate(type_args: &[u8], data: &[u8]) -> Option<UniqueTokenInfo> {
    if type_args.len() != UNIQUE_TYPE_ARGS_LEN {
        return None;
    }
    parse_unique_cell_token_info(data)
}

/// THE extension rule, both sync paths (resync fix round S1): of one
/// transaction's candidate outputs ([`unique_cell_candidate`], in output
/// order), the cell an xUDT extension naming `args` binds is the LAST whose
/// type args are `args` and whose type script is the Unique Cell by its
/// registered pair ([`is_unique_cell`]). Its info is the token's bound
/// metadata; its total supply, where it carries one, is the token's
/// `TokenExtension` max-supply claim. A cell under any other type script
/// with those args (anyone can create one) binds and claims nothing.
///
/// The last, not the first: both paths already bound the token's info from
/// the last output with those args, so the binding and the claim now come
/// from one cell (live took the claim from the first output with a total,
/// bulk from the last). Two Unique Cells with one args cannot both be valid
/// on chain (its args are a Type ID, unique by construction), so the choice
/// only has to be one.
///
/// `args_of` and `pair_of` read a candidate's type args and its type script
/// pair; the pair is read only for a candidate whose args match.
///
/// Of the pair, the code hash decides on chain today; the hash type cannot
/// (tests report NOTE-1): a cell typed by the Unique Cell's code hash under a
/// hash type the registry does not list would have to run that pair's code,
/// and by `type` no cell has the Unique Cell's code hash as its type hash, so
/// no such cell exists. The hash type is read because the registry's pair is
/// the one rule, not because a cell on chain needs it.
pub(crate) fn bound_unique_cell<'c, T>(
    candidates: impl DoubleEndedIterator<Item = T>,
    args: &[u8],
    args_of: impl Fn(&T) -> &'c [u8],
    pair_of: impl Fn(&T) -> (&'c [u8], i16),
) -> Option<T> {
    candidates.rev().find(|candidate| {
        args_of(candidate) == args && {
            let (code_hash, hash_type) = pair_of(candidate);
            is_unique_cell(code_hash, hash_type)
        }
    })
}

// ---------------------------------------------------------------------------
// Omnilock functions
// ---------------------------------------------------------------------------

/// Whether a lock script's pair runs Omnilock: the registry's Omnilock pairs
/// (`PROTOCOL_REGISTRY.script`), each registered reference and each version's
/// binary by the data hash types. Omnilock's code hash by another hash type
/// runs no Omnilock and claims nothing.
pub(crate) fn is_omnilock(code_hash: &[u8], hash_type: i16) -> bool {
    use crate::parser::registry::{RegisteredScript, StandardLock, PROTOCOL_REGISTRY};
    PROTOCOL_REGISTRY.script(code_hash, hash_type)
        == Some(RegisteredScript::StandardLock(StandardLock::Omnilock))
}

pub(crate) fn extract_omnilock_supply_info_type_hash(lock_args: &[u8]) -> Option<[u8; 32]> {
    if lock_args.len() <= OMNILOCK_AUTH_LEN {
        return None;
    }

    let omnilock_args = &lock_args[OMNILOCK_AUTH_LEN..];
    let flags = *omnilock_args.first()?;
    if flags & OMNILOCK_SUPPLY_MODE_FLAG == 0 {
        return None;
    }

    let mut offset = 1usize;
    if flags & OMNILOCK_ADMIN_MODE_FLAG != 0 {
        offset += 32;
    }
    if flags & OMNILOCK_ACP_MODE_FLAG != 0 {
        offset += 2;
    }
    if flags & OMNILOCK_TIMELOCK_MODE_FLAG != 0 {
        offset += 8;
    }

    if omnilock_args.len() < offset + 32 {
        return None;
    }

    let mut hash = [0u8; 32];
    hash.copy_from_slice(&omnilock_args[offset..offset + 32]);
    Some(hash)
}

pub(crate) fn parse_omnilock_supply_info_cell_data(data: &[u8]) -> Option<(u128, [u8; 32])> {
    if data.len() < OMNILOCK_SUPPLY_INFO_CELL_MIN_DATA_LEN {
        return None;
    }

    let version = data[0];
    if version != OMNILOCK_SUPPLY_INFO_CELL_VERSION_V0 {
        return None;
    }

    let current_supply = u128::from_le_bytes(data[1..17].try_into().ok()?);
    let max_supply = u128::from_le_bytes(data[17..33].try_into().ok()?);
    if current_supply > max_supply {
        return None;
    }

    let mut token_type_hash = [0u8; 32];
    token_type_hash.copy_from_slice(&data[33..65]);
    Some((max_supply, token_type_hash))
}

// ---------------------------------------------------------------------------
// Molecule parsing (one decoder for every parser: `crate::parser::molecule`)
// ---------------------------------------------------------------------------

use crate::parser::molecule::{parse_bytes, parse_dynvec_items, parse_table_fields_exact};

fn parse_molecule_script(data: &[u8]) -> Option<XudtExtensionScript> {
    let fields = parse_table_fields_exact(data, 3)?;
    if fields[0].len() != 32 || fields[1].len() != 1 {
        return None;
    }
    let args = parse_bytes(fields[2])?.to_vec();
    Some(XudtExtensionScript { args })
}

// ---------------------------------------------------------------------------
// XUDT extension script extraction
// ---------------------------------------------------------------------------

pub(crate) fn parse_xudt_extension_scripts_from_script_vec(
    script_vec: &[u8],
) -> Option<Vec<XudtExtensionScript>> {
    let mut scripts = Vec::new();
    for item in parse_dynvec_items(script_vec)? {
        scripts.push(parse_molecule_script(item)?);
    }
    Some(scripts)
}

pub(crate) fn extract_xudt_witness_extension_script_vec(xudt_witness: &[u8]) -> Option<&[u8]> {
    let fields = parse_table_fields_exact(xudt_witness, 4)?;
    if fields[2].is_empty() {
        None
    } else {
        Some(fields[2])
    }
}

/// The extension scripts in a transaction's witnesses for an xUDT in flags-0x2
/// mode: the first `WitnessArgs` whose `input_type` or `output_type` holds an
/// `XudtWitness` whose `raw_extension_data` hashes to the args' blake160.
fn extension_scripts_from_witnesses<W: AsRef<[u8]>>(
    witnesses: impl IntoIterator<Item = W>,
    expected_script_vec_hash: &[u8; 20],
) -> Option<Vec<XudtExtensionScript>> {
    for witness in witnesses {
        let witness_bytes = witness.as_ref();
        let witness_fields = match parse_table_fields_exact(witness_bytes, 3) {
            Some(fields) => fields,
            None => continue,
        };

        for bytes_opt_field in [&witness_fields[1], &witness_fields[2]] {
            if bytes_opt_field.is_empty() {
                continue;
            }
            let Some(xudt_witness_bytes) = parse_bytes(bytes_opt_field) else {
                continue;
            };
            let Some(script_vec_bytes) =
                extract_xudt_witness_extension_script_vec(xudt_witness_bytes)
            else {
                continue;
            };
            if blake160(script_vec_bytes) != *expected_script_vec_hash {
                continue;
            }
            if let Some(parsed) = parse_xudt_extension_scripts_from_script_vec(script_vec_bytes) {
                return Some(parsed);
            }
        }
    }
    None
}

/// An xUDT's extension scripts, by its RFC-0052 flags (`type_args[32..36]`):
/// in the args for flags 0x1, in the witness the args hash for flags 0x2.
///
/// The one rule for both sync paths. `witnesses` is called only for flags
/// 0x2, the one mode that reads a witness: live passes the transaction's hex
/// witnesses, bulk its raw witnesses at facts time.
pub(crate) fn extract_xudt_extension_scripts_with<W, I>(
    type_args: &[u8],
    witnesses: impl FnOnce() -> I,
) -> Option<Vec<XudtExtensionScript>>
where
    W: AsRef<[u8]>,
    I: IntoIterator<Item = W>,
{
    if type_args.len() < XUDT_TYPE_ARGS_MIN_LEN {
        return None;
    }
    let flags = u32::from_le_bytes(
        type_args[XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN..XUDT_TYPE_ARGS_MIN_LEN]
            .try_into()
            .ok()?,
    );
    match flags & XUDT_FLAGS_EXTENSION_MASK {
        XUDT_FLAGS_EXTENSION_IN_ARGS => {
            parse_xudt_extension_scripts_from_script_vec(&type_args[XUDT_TYPE_ARGS_MIN_LEN..])
        }
        XUDT_FLAGS_EXTENSION_IN_WITNESS => {
            let tail = &type_args[XUDT_TYPE_ARGS_MIN_LEN..];
            if tail.len() < XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN {
                return None;
            }
            let mut expected = [0u8; XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN];
            expected.copy_from_slice(&tail[..XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN]);
            extension_scripts_from_witnesses(witnesses(), &expected)
        }
        _ => None,
    }
}

/// [`extract_xudt_extension_scripts_with`] over a live transaction's hex
/// witnesses.
pub(crate) fn extract_xudt_extension_scripts(
    type_args: &[u8],
    witnesses: &[String],
) -> Option<Vec<XudtExtensionScript>> {
    extract_xudt_extension_scripts_with(type_args, || {
        witnesses
            .iter()
            .map(|witness| crate::rpc::parse_hex_to_bytes(witness))
    })
}

// ---------------------------------------------------------------------------
// Token info / supply helpers
// ---------------------------------------------------------------------------

/// Token metadata extracted from an xUDT Unique Cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UniqueTokenInfo {
    pub decimal: u8,
    pub name: String,
    pub symbol: String,
    pub total_supply: Option<u128>,
}

/// Parse all fields from a Unique Cell's data:
/// `[1B decimal][1B name_len][name UTF-8][1B symbol_len][symbol UTF-8][tag-value pairs...]`
///
/// Returns `None` if data is malformed or name/symbol are not valid UTF-8 (fail fast).
pub(crate) fn parse_unique_cell_token_info(data: &[u8]) -> Option<UniqueTokenInfo> {
    if data.len() < 3 {
        return None;
    }

    let mut index = 0usize;
    let decimal = *data.get(index)?;
    index += 1;

    let name_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + name_len + 1 {
        return None;
    }
    let name = std::str::from_utf8(data.get(index..index + name_len)?)
        .ok()?
        .to_string();
    index += name_len;

    let symbol_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + symbol_len {
        return None;
    }
    let symbol = std::str::from_utf8(data.get(index..index + symbol_len)?)
        .ok()?
        .to_string();
    index += symbol_len;

    let mut total_supply = None;
    while index + 8 <= data.len() {
        let tag = u32::from_le_bytes(data[index..index + 4].try_into().ok()?);
        index += 4;
        let data_len = u32::from_le_bytes(data[index..index + 4].try_into().ok()?) as usize;
        index += 4;
        if data.len() < index + data_len {
            return None;
        }
        let value = &data[index..index + data_len];
        if tag == TOKEN_INFO_TAG_TOTAL_SUPPLY && data_len == TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN {
            let raw = u128::from_le_bytes(value.try_into().ok()?);
            total_supply = Some(raw);
            break;
        }
        index += data_len;
    }

    Some(UniqueTokenInfo {
        decimal,
        name,
        symbol,
        total_supply,
    })
}

/// One live transaction's candidate outputs ([`unique_cell_candidate`]), in
/// output order: the info, the type script's code hash and hash type, the
/// type args. A typed `ParsedCell` carries its code hash, hash type and args
/// together (the parser fills the three from one RPC script).
struct LiveUniqueCandidate<'c> {
    info: UniqueTokenInfo,
    code_hash: &'c [u8],
    hash_type: i16,
    args: &'c [u8],
}

fn live_unique_cell_candidates(
    cells: &[crate::parser::cell::ParsedCell],
) -> Vec<LiveUniqueCandidate<'_>> {
    cells
        .iter()
        .filter_map(|cell| {
            let args = cell.type_args.as_deref()?;
            let info = unique_cell_candidate(args, &cell.data)?;
            let (code_hash, hash_type) = cell.type_code_hash.as_deref().zip(cell.type_hash_type)?;
            Some(LiveUniqueCandidate {
                info,
                code_hash,
                hash_type,
                args,
            })
        })
        .collect()
}

/// [`bound_unique_cell`] over a live transaction's candidates.
fn live_bound_unique_cell<'a>(
    candidates: &'a [LiveUniqueCandidate<'_>],
    args: &[u8],
) -> Option<&'a UniqueTokenInfo> {
    bound_unique_cell(
        candidates.iter(),
        args,
        |candidate| candidate.args,
        |candidate| (candidate.code_hash, candidate.hash_type),
    )
    .map(|candidate| &candidate.info)
}

/// How an on-chain token-info observation was bound to a token type (one
/// definition, in the store: it is persisted on `TokenInfo.chain_metadata`).
pub(crate) use ckbadger_store::types::OnchainInfoBinding;

/// A Unique Cell token-info observation together with how it was bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OnchainTokenInfo {
    pub info: UniqueTokenInfo,
    pub binding: OnchainInfoBinding,
}

impl OnchainTokenInfo {
    /// What this observation asserts about the token, as stored: an empty
    /// name or symbol asserts nothing.
    pub(crate) fn chain_metadata(&self) -> ckbadger_store::types::ChainTokenMetadata {
        ckbadger_store::types::ChainTokenMetadata {
            name: Some(self.info.name.clone()).filter(|name| !name.is_empty()),
            symbol: Some(self.info.symbol.clone()).filter(|symbol| !symbol.is_empty()),
            decimals: self.info.decimal,
            binding: self.binding,
        }
    }
}

/// A token's on-chain info binding across one live batch.
///
/// The batch settles on one binding per token (the last extension binding, else
/// the first co-occurrence binding, in transaction order). A shallow rollback
/// to a block inside the batch must restore the row a batch ending at that
/// block would have stored, so every time the batch's choice is (re)assigned the
/// assignment is kept with the block of the transaction that made it.
/// Transactions arrive in chain order, so the assignments are in block order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OnchainTokenInfoTimeline {
    /// `(block, binding)` per assignment, in transaction order; never empty.
    assignments: Vec<(i64, OnchainTokenInfo)>,
}

impl OnchainTokenInfoTimeline {
    fn new(block_number: i64, bound: OnchainTokenInfo) -> Self {
        Self {
            assignments: vec![(block_number, bound)],
        }
    }

    fn assign(&mut self, block_number: i64, bound: OnchainTokenInfo) {
        self.assignments.push((block_number, bound));
    }

    /// The binding the whole batch settled on (`winner_through` its last block).
    #[cfg(test)]
    pub(crate) fn winner(&self) -> &OnchainTokenInfo {
        &self
            .assignments
            .last()
            .expect("an on-chain info timeline is created with its first assignment")
            .1
    }

    /// The binding a batch ending at `block_number` would have settled on.
    pub(crate) fn winner_through(&self, block_number: i64) -> Option<&OnchainTokenInfo> {
        self.assignments
            .iter()
            .rev()
            .find(|(block, _)| *block <= block_number)
            .map(|(_, bound)| bound)
    }

    /// The blocks whose transactions assigned a binding.
    pub(crate) fn blocks(&self) -> impl Iterator<Item = i64> + '_ {
        self.assignments.iter().map(|(block, _)| *block)
    }
}

/// Who made a max-supply claim: what binds it to the token it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaxSupplyClaimant {
    /// An Omnilock supply-info cell held under its own supply-mode Omnilock
    /// (the lock args name the cell's type script): the hash of that lock
    /// script. Anyone can write such a cell naming any token; it is the
    /// token's only when the token's owner lock IS this lock (RFC 0042: the
    /// token is then minted only with this lock in the inputs).
    SupplyOmnilock([u8; 32]),
    /// A Unique Cell that the token's own xUDT output names in its extension
    /// scripts, created by the same transaction: bound by construction (the
    /// token's own output carries the reference, and the named cell is the
    /// Unique Cell by its registered pair, [`bound_unique_cell`]).
    TokenExtension,
}

/// One max-supply claim (an Omnilock supply-info cell, or a Unique Cell an
/// xUDT extension binds), who made it, and its place in chain order: the
/// block, the transaction and the output that carry it. A shallow rollback
/// below its block must undo it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaxSupplyObservation {
    pub max_supply: u128,
    pub block_number: i64,
    pub tx_index: i32,
    pub output_index: u32,
    pub claimant: MaxSupplyClaimant,
}

impl MaxSupplyObservation {
    pub(crate) fn place(&self) -> (i64, i32, u32) {
        (self.block_number, self.tx_index, self.output_index)
    }
}

/// Every claim on one token, in chain order (block, transaction, output).
pub(crate) type MaxSupplyClaims = Vec<MaxSupplyObservation>;

/// Add `claim` to a token's claims at its place in chain order.
pub(crate) fn insert_max_supply_claim(claims: &mut MaxSupplyClaims, claim: MaxSupplyObservation) {
    let at = claims.partition_point(|existing| existing.place() <= claim.place());
    claims.insert(at, claim);
}

/// THE max-supply rule, both sync paths: a token's max supply is the first
/// claim in chain order at or after the token's first BLOCK (the block of its
/// first cell). `type_args` and `first_block` are the token row's; a token
/// row exists only once the chain holds the token, so its first block is
/// always that block (label import writes no row for a token the chain does
/// not hold: resync fix round M1).
///
/// - The rule is by block, so that it cannot depend on batch cuts or on the
///   order of transactions inside a block: a claim made in a block before the
///   token's first block is not the token's (it names an id nothing carries
///   yet), and a claim earlier in the token's own first block, in a
///   transaction before the one that creates its first cell, counts (the
///   lead's ruling: a counted Omnilock claim comes only from the token's
///   owner lock, the one lock the token is minted under, so only the owner's
///   own claim can be that early).
/// - An Omnilock supply-info claim is the token's only when the token's owner
///   lock hash (the first 32 bytes of its type args, sUDT and xUDT alike) is
///   the hash of the claiming supply-mode Omnilock: one comparison. A claim
///   that fails it is ignored and the next one counts. A token whose type
///   args are shorter than 32 bytes names no owner lock: no Omnilock claim
///   is its.
/// - An xUDT extension claim is bound by construction: the token's own output
///   names the Unique Cell in its extension scripts, and only the Unique Cell
///   by its registered pair makes the claim ([`bound_unique_cell`]), so no
///   owner check applies.
///
/// `through_block` is the last block the row is being computed for
/// (`i64::MAX` for the row after all of them). `claims` is in chain order.
pub(crate) fn token_max_supply_claim(
    claims: &[MaxSupplyObservation],
    type_args: &[u8],
    first_block: i64,
    through_block: i64,
) -> Option<MaxSupplyObservation> {
    let owner_lock_hash = type_args.get(..32);
    claims
        .iter()
        .find(|claim| {
            claim.block_number >= first_block
                && claim.block_number <= through_block
                && match &claim.claimant {
                    MaxSupplyClaimant::SupplyOmnilock(lock_hash) => {
                        owner_lock_hash == Some(lock_hash.as_slice())
                    }
                    MaxSupplyClaimant::TokenExtension => true,
                }
        })
        .copied()
}

/// Type script hash by outpoint — a borrowed view over cells the caller has
/// already resolved, used to answer "does this input carry type X?" without
/// resolving anything again.
///
/// Two such views are needed to cover every input of a batch: outputs created
/// inside the batch, and inputs created earlier and prefetched from the store.
pub(crate) type OutpointTypeHashes<'a> = HashMap<(&'a [u8], i32), &'a [u8]>;

/// Resolve Unique Cell token info to xUDT token type_hashes.
///
/// Two binding paths, in priority order:
/// 1. Extension scripts: each xUDT output whose extension scripts point at a
///    Unique Cell in the same tx maps the xUDT's type_script_hash to that
///    Unique Cell's token info (cryptographic binding).
/// 2. Issuance co-occurrence: the ecosystem-standard issuance shape
///    (RGB++-style `[unique info cell, xudt cells…]`) where the xUDT args are
///    a plain owner lock hash without extension flags. Bound only when the tx
///    contains at least one output typed with the deployed Unique Cell type
///    script (all parseable ones agreeing on a single info) AND mints exactly
///    one xUDT type. An extension binding is never displaced by a
///    co-occurrence binding.
///
/// `persisted_input_types` carries the types of inputs created before this
/// batch; without it a transfer of an already-issued token would look like a
/// mint (see [`collect_issuance_cooccurrence_binding`]).
pub(crate) fn collect_token_onchain_info(
    all_tx_data: &[TxData],
    persisted_input_types: &OutpointTypeHashes<'_>,
) -> HashMap<Vec<u8>, OnchainTokenInfoTimeline> {
    let mut result: HashMap<Vec<u8>, OnchainTokenInfoTimeline> = HashMap::new();

    // Outpoint → type_script_hash for outputs created in this batch. Used
    // together with `persisted_input_types` to screen transfers out of the
    // issuance co-occurrence rule (an input carrying the candidate type means
    // the tx moves, not mints it).
    let mut batch_output_types: OutpointTypeHashes<'_> = HashMap::new();
    for tx_data in all_tx_data {
        for (output_index, cell) in tx_data.cells.iter().enumerate() {
            let Some(type_script_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };
            let Ok(output_index) = i32::try_from(output_index) else {
                continue;
            };
            batch_output_types.insert((tx_data.hash.as_slice(), output_index), type_script_hash);
        }
    }

    for tx_data in all_tx_data {
        // -- 1. extension-script binding (cryptographic) --------------------
        let unique_candidates = live_unique_cell_candidates(&tx_data.cells);
        if !unique_candidates.is_empty() {
            for cell in &tx_data.cells {
                let Some(type_code_hash) = cell.type_code_hash.as_ref() else {
                    continue;
                };
                let Some(type_hash_type) = cell.type_hash_type else {
                    continue;
                };
                if !crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, type_hash_type)
                    .is_some_and(|standard| standard.is_xudt())
                {
                    continue;
                }

                let Some(type_args) = cell.type_args.as_ref() else {
                    continue;
                };
                let Some(token_type_hash) = cell.type_script_hash.as_ref() else {
                    continue;
                };

                let Some(extension_scripts) =
                    extract_xudt_extension_scripts(type_args, &tx_data.witnesses)
                else {
                    continue;
                };

                for extension in extension_scripts {
                    if extension.args.len() != UNIQUE_TYPE_ARGS_LEN {
                        continue;
                    }
                    if let Some(info) = live_bound_unique_cell(&unique_candidates, &extension.args)
                    {
                        // Unconditional: an extension binding displaces any
                        // earlier co-occurrence binding for the same token.
                        let bound = OnchainTokenInfo {
                            info: info.clone(),
                            binding: OnchainInfoBinding::XudtExtension,
                        };
                        match result.entry(token_type_hash.clone()) {
                            std::collections::hash_map::Entry::Occupied(mut timeline) => {
                                timeline.get_mut().assign(tx_data.block_number, bound);
                            }
                            std::collections::hash_map::Entry::Vacant(slot) => {
                                slot.insert(OnchainTokenInfoTimeline::new(
                                    tx_data.block_number,
                                    bound,
                                ));
                            }
                        }
                    }
                }
            }
        }

        // -- 2. issuance co-occurrence binding (heuristic) ------------------
        if let Some((token_type_hash, info)) = collect_issuance_cooccurrence_binding(
            tx_data,
            &batch_output_types,
            persisted_input_types,
        ) {
            result.entry(token_type_hash).or_insert_with(|| {
                OnchainTokenInfoTimeline::new(
                    tx_data.block_number,
                    OnchainTokenInfo {
                        info,
                        binding: OnchainInfoBinding::IssuanceCooccurrence,
                    },
                )
            });
        }
    }

    result
}

/// Detect the single-type issuance co-occurrence shape in one transaction and
/// return the `(token type_hash, unique info)` pair to bind, if any.
///
/// Safety rules (see `collect_token_onchain_info`): at least one output typed
/// with the deployed Unique Cell type script, all parseable unique outputs
/// agreeing on exactly one info, and exactly one minted xUDT type — an
/// amount-carrying xUDT output whose type is present on no input of the
/// transaction.
///
/// The input veto set is the union of two disjoint provenances that together
/// cover every input: cells created by this batch (`batch_output_types`) and
/// cells created earlier and resolved from the store
/// (`persisted_input_types`). This mirrors the bulk reducer, which vetoes on
/// `ResolvedTxFacts::resolved_inputs`; both paths must classify mints
/// identically or the same chain yields different stored metadata.
fn collect_issuance_cooccurrence_binding(
    tx_data: &TxData,
    batch_output_types: &OutpointTypeHashes<'_>,
    persisted_input_types: &OutpointTypeHashes<'_>,
) -> Option<(Vec<u8>, UniqueTokenInfo)> {
    // Exactly one distinct parsed info among Unique-Cell-typed outputs.
    let mut unique_info: Option<UniqueTokenInfo> = None;
    for cell in &tx_data.cells {
        let (Some(type_code_hash), Some(type_hash_type)) =
            (cell.type_code_hash.as_ref(), cell.type_hash_type)
        else {
            continue;
        };
        if !is_unique_cell(type_code_hash, type_hash_type) {
            continue;
        }
        let Some(info) = parse_unique_cell_token_info(&cell.data) else {
            continue;
        };
        match &unique_info {
            None => unique_info = Some(info),
            Some(existing) if *existing == info => {}
            Some(_) => return None, // ambiguous: multiple distinct infos
        }
    }
    let unique_info = unique_info?;

    // Types carried by this transaction's inputs, whichever side they were
    // resolved from (same-batch outputs or persisted store state).
    let mut input_types: HashSet<&[u8]> = HashSet::new();
    for input in &tx_data.inputs {
        let key = (
            input.previous_tx_hash.as_slice(),
            input.previous_output_index,
        );
        if let Some(type_hash) = batch_output_types
            .get(&key)
            .or_else(|| persisted_input_types.get(&key))
        {
            input_types.insert(type_hash);
        }
    }

    // Exactly one minted xUDT type among amount-carrying outputs.
    let mut minted: Option<&Vec<u8>> = None;
    for cell in &tx_data.cells {
        let (Some(type_code_hash), Some(type_hash_type)) =
            (cell.type_code_hash.as_ref(), cell.type_hash_type)
        else {
            continue;
        };
        if !crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, type_hash_type)
            .is_some_and(|standard| standard.is_xudt())
        {
            continue;
        }
        if crate::parser::UdtParser::parse_amount(&cell.data).is_none() {
            continue;
        }
        let Some(token_type_hash) = cell.type_script_hash.as_ref() else {
            continue;
        };
        if input_types.contains(token_type_hash.as_slice()) {
            continue; // moved within the tx: a transfer, not a mint
        }
        match minted {
            None => minted = Some(token_type_hash),
            Some(existing) if existing == token_type_hash => {}
            Some(_) => return None, // more than one minted type: ambiguous
        }
    }

    minted.map(|token_type_hash| (token_type_hash.clone(), unique_info))
}

/// Record one claim by `claimant` of `tx`'s output `output_index` on
/// `token_type_hash`. Every claim is kept, in chain order: which one is the
/// token's is the rule's to say ([`token_max_supply_claim`]), once the
/// token's row is known.
pub(crate) fn observe_max_supply(
    observations: &mut HashMap<Vec<u8>, MaxSupplyClaims>,
    tx: &TxData,
    output_index: usize,
    token_type_hash: Vec<u8>,
    max_supply: u128,
    claimant: MaxSupplyClaimant,
) {
    let output_index = u32::try_from(output_index)
        .expect("a transaction's output index fits u32 (CKB caps outputs)");
    insert_max_supply_claim(
        observations.entry(token_type_hash).or_default(),
        MaxSupplyObservation {
            max_supply,
            block_number: tx.block_number,
            tx_index: tx.tx_index,
            output_index,
            claimant,
        },
    );
}

/// The claimant of a supply-info cell under its own supply-mode Omnilock:
/// that lock script's hash.
pub(crate) fn supply_omnilock_claimant(lock_script_hash: &[u8]) -> MaxSupplyClaimant {
    MaxSupplyClaimant::SupplyOmnilock(
        lock_script_hash
            .try_into()
            .expect("a lock script hash is 32 bytes"),
    )
}

/// Every max-supply claim of a batch, per token, in chain order.
pub(crate) fn collect_token_max_supply_observations(
    all_tx_data: &[TxData],
) -> HashMap<Vec<u8>, MaxSupplyClaims> {
    let mut observations = HashMap::new();

    for tx_data in all_tx_data {
        let unique_candidates = live_unique_cell_candidates(&tx_data.cells);

        for (output_index, cell) in tx_data.cells.iter().enumerate() {
            if !is_omnilock(&cell.lock_code_hash, cell.lock_hash_type) {
                continue;
            }

            let Some(supply_info_type_hash) =
                extract_omnilock_supply_info_type_hash(&cell.lock_args)
            else {
                continue;
            };
            let Some(cell_type_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };
            if cell_type_hash.as_slice() != supply_info_type_hash {
                continue;
            }

            let Some((max_supply, token_type_hash)) =
                parse_omnilock_supply_info_cell_data(&cell.data)
            else {
                continue;
            };
            observe_max_supply(
                &mut observations,
                tx_data,
                output_index,
                token_type_hash.to_vec(),
                max_supply,
                supply_omnilock_claimant(&cell.lock_script_hash),
            );
        }

        if unique_candidates.is_empty() {
            continue;
        }

        for (output_index, cell) in tx_data.cells.iter().enumerate() {
            let Some(type_code_hash) = cell.type_code_hash.as_ref() else {
                continue;
            };
            let Some(type_hash_type) = cell.type_hash_type else {
                continue;
            };
            if !crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, type_hash_type)
                .is_some_and(|standard| standard.is_xudt())
            {
                continue;
            }

            let Some(type_args) = cell.type_args.as_ref() else {
                continue;
            };
            let Some(token_type_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };

            let Some(extension_scripts) =
                extract_xudt_extension_scripts(type_args, &tx_data.witnesses)
            else {
                continue;
            };

            for extension in extension_scripts {
                if extension.args.len() != UNIQUE_TYPE_ARGS_LEN {
                    continue;
                }
                // The extension rule: the Unique Cell it binds, by its pair,
                // and that cell's total supply.
                let Some(max_supply) = live_bound_unique_cell(&unique_candidates, &extension.args)
                    .and_then(|info| info.total_supply)
                else {
                    continue;
                };
                observe_max_supply(
                    &mut observations,
                    tx_data,
                    output_index,
                    token_type_hash.clone(),
                    max_supply,
                    MaxSupplyClaimant::TokenExtension,
                );
            }
        }
    }

    observations
}

/// A cell's UDT amount, when its type pair is a UDT (THE pair rule,
/// `UdtParser::is_udt_code_hash_bytes`). Nothing else makes a cell a UDT: the
/// standard a token row or a label states is no hint (resync fix round 2, B:
/// a token row exists only for a UDT pair, so a hint could never change the
/// answer).
pub(crate) fn parse_parsed_cell_udt_amount(
    cell: &crate::parser::cell::ParsedCell,
    tx_hash: &[u8],
    output_index: i16,
) -> Result<Option<u128>> {
    let standard = if let (Some(type_code_hash), Some(hash_type)) =
        (cell.type_code_hash.as_deref(), cell.type_hash_type)
    {
        crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, hash_type)
    } else {
        None
    };
    let Some(standard) = standard else {
        return Ok(None);
    };
    let type_code_hash = cell.type_code_hash.as_deref().unwrap_or(&[]);

    let Some(amount) = crate::parser::UdtParser::parse_amount(&cell.data) else {
        // xUDT-compatible cells can carry non-amount payloads (for example owner-mode
        // cells). They should not be indexed as fungible UDT balances/transfers —
        // common and expected, tolerated silently.
        if standard.is_xudt() {
            return Ok(None);
        }
        // A cell using the sUDT type script but with data too short for a u128 amount is
        // a rare non-standard on-chain cell (seen on testnet). Record no amount rather
        // than fail-fasting the whole sync (consistent with the bulk-build binary_facts
        // path); logged since it is unexpected for sUDT.
        tracing::warn!(
            outpoint = %format!("0x{}:{}", hex::encode(tx_hash), output_index),
            type_code_hash = %format!("0x{}", hex::encode(type_code_hash)),
            data_len = cell.data.len(),
            "sUDT cell amount data too short (<16 bytes); recording no amount"
        );
        return Ok(None);
    };
    Ok(Some(amount))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // -- molecule encoding helpers (test-only) --------------------------------

    fn molecule_u32(value: usize) -> [u8; 4] {
        (value as u32).to_le_bytes()
    }

    fn molecule_table(fields: &[Vec<u8>]) -> Vec<u8> {
        let header_size = 4 + fields.len() * 4;
        let total_size = header_size + fields.iter().map(|field| field.len()).sum::<usize>();

        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&molecule_u32(total_size));

        let mut offset = header_size;
        for field in fields {
            out.extend_from_slice(&molecule_u32(offset));
            offset += field.len();
        }
        for field in fields {
            out.extend_from_slice(field);
        }
        out
    }

    /// Molecule `Bytes`: the header is the content length (as the node
    /// serializes it — this helper used to write the total size, agreeing
    /// with a reader that had the same mistake).
    fn molecule_bytes(value: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + value.len());
        out.extend_from_slice(&molecule_u32(value.len()));
        out.extend_from_slice(value);
        out
    }

    fn molecule_dynvec(items: &[Vec<u8>]) -> Vec<u8> {
        if items.is_empty() {
            return molecule_u32(4).to_vec();
        }

        let header_size = 4 + items.len() * 4;
        let total_size = header_size + items.iter().map(|item| item.len()).sum::<usize>();

        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&molecule_u32(total_size));

        let mut offset = header_size;
        for item in items {
            out.extend_from_slice(&molecule_u32(offset));
            offset += item.len();
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    // -- D-1: the real molecule layouts, serialized by ckb-types ---------------

    /// One extension `Script` (args = `unique_args`), serialized by ckb-types
    /// — the `Bytes` header of `args` is its content length — in a
    /// `ScriptVec` (dynvec; ckb-types has no `ScriptVec`, the xUDT schema does).
    fn real_script_vec(unique_args: &[u8]) -> Vec<u8> {
        use ckb_types::{bytes::Bytes, packed, prelude::*};
        let script = packed::Script::new_builder()
            .code_hash(packed::Byte32::new([0xCC; 32]))
            .hash_type(packed::Byte::new(1))
            .args(Bytes::copy_from_slice(unique_args).pack())
            .build();
        molecule_dynvec(&[script.as_slice().to_vec()])
    }

    #[test]
    fn test_extract_xudt_extension_scripts_reads_the_real_args_layout() {
        let unique_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let script_vec = real_script_vec(&unique_args);
        let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);

        let scripts = extract_xudt_extension_scripts(&type_args, &[])
            .expect("a ckb-types ScriptVec in the args must decode");

        assert_eq!(
            scripts.into_iter().map(|s| s.args).collect::<Vec<_>>(),
            vec![unique_args]
        );
    }

    #[test]
    fn test_extract_xudt_extension_scripts_reads_the_real_witness_layout() {
        use ckb_types::{bytes::Bytes, packed, prelude::*};
        let unique_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let script_vec = real_script_vec(&unique_args);
        // XudtWitness { owner_script: None, owner_signature: None,
        //   raw_extension_data: Some(script_vec), extension_data: [] }.
        let xudt_witness = molecule_table(&[
            Vec::new(),
            Vec::new(),
            script_vec.clone(),
            molecule_u32(4).to_vec(),
        ]);
        let witness = packed::WitnessArgs::new_builder()
            .input_type(Some(Bytes::from(xudt_witness)).pack())
            .build();
        let type_args =
            build_xudt_type_args_with_extension_in_witness([0x01; 32], blake160(&script_vec));

        let scripts = extract_xudt_extension_scripts(
            &type_args,
            &[format!("0x{}", hex::encode(witness.as_slice()))],
        )
        .expect("a ckb-types WitnessArgs carrying the ScriptVec must decode");

        assert_eq!(
            scripts.into_iter().map(|s| s.args).collect::<Vec<_>>(),
            vec![unique_args]
        );
    }

    fn encode_script(args: &[u8]) -> Vec<u8> {
        molecule_table(&[vec![0xCC; 32], vec![1], molecule_bytes(args)])
    }

    fn encode_script_vec_with_unique_args(unique_type_args: &[u8]) -> Vec<u8> {
        molecule_dynvec(&[encode_script(unique_type_args)])
    }

    fn encode_xudt_witness(script_vec: &[u8]) -> Vec<u8> {
        molecule_table(&[Vec::new(), Vec::new(), script_vec.to_vec(), Vec::new()])
    }

    fn encode_witness_args(input_type: Option<&[u8]>, output_type: Option<&[u8]>) -> Vec<u8> {
        let lock = Vec::new();
        let input_type = input_type.map(molecule_bytes).unwrap_or_default();
        let output_type = output_type.map(molecule_bytes).unwrap_or_default();
        molecule_table(&[lock, input_type, output_type])
    }

    // -- token data helpers (test-only) ----------------------------------------

    fn build_token_info_data(total_supply: u128) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(8); // decimal
        data.push(5); // name len
        data.extend_from_slice(b"Token");
        data.push(3); // symbol len
        data.extend_from_slice(b"TKN");
        data.extend_from_slice(&TOKEN_INFO_TAG_TOTAL_SUPPLY.to_le_bytes());
        data.extend_from_slice(&(TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN as u32).to_le_bytes());
        data.extend_from_slice(&total_supply.to_le_bytes());
        data
    }

    /// A Unique Cell by its registered pair (the mainnet binary by data1:
    /// resync fix round S1, re-pinned from code hash 0x40.. by type, which is
    /// no Unique Cell) carrying token info with `total_supply`.
    fn dummy_unique_token_info_cell(
        unique_type_args: Vec<u8>,
        total_supply: u128,
    ) -> crate::parser::cell::ParsedCell {
        let data = build_token_info_data(total_supply);
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x10; 32],
            lock_hash_type: 1,
            lock_args: vec![0x20; 20],
            lock_script_hash: vec![0x30; 32],
            type_code_hash: Some(crate::rpc::parse_hex_to_bytes(
                UNIQUE_CELL_CODE_HASH_MAINNET,
            )),
            type_hash_type: Some(2),
            type_args: Some(unique_type_args),
            type_script_hash: Some(vec![0x50; 32]),
            data_hash: [0x60; 32],
            data_size: data.len() as i32,
            data,
        }
    }

    fn dummy_xudt_cell(
        token_type_hash: [u8; 32],
        type_args: Vec<u8>,
    ) -> crate::parser::cell::ParsedCell {
        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(type_args),
            type_script_hash: Some(token_type_hash.to_vec()),
            data_hash: [0x44; 32],
            data_size: 16,
            data: vec![0u8; 16],
        }
    }

    fn build_xudt_type_args_with_extension_in_args(
        owner_lock_hash: [u8; 32],
        script_vec: &[u8],
    ) -> Vec<u8> {
        let mut type_args = Vec::with_capacity(XUDT_TYPE_ARGS_MIN_LEN + script_vec.len());
        type_args.extend_from_slice(&owner_lock_hash);
        type_args.extend_from_slice(&XUDT_FLAGS_EXTENSION_IN_ARGS.to_le_bytes());
        type_args.extend_from_slice(script_vec);
        type_args
    }

    fn build_xudt_type_args_with_extension_in_witness(
        owner_lock_hash: [u8; 32],
        script_vec_hash: [u8; 20],
    ) -> Vec<u8> {
        let mut type_args = Vec::with_capacity(XUDT_TYPE_ARGS_MIN_LEN + script_vec_hash.len());
        type_args.extend_from_slice(&owner_lock_hash);
        type_args.extend_from_slice(&XUDT_FLAGS_EXTENSION_IN_WITNESS.to_le_bytes());
        type_args.extend_from_slice(&script_vec_hash);
        type_args
    }

    fn dummy_tx_data(
        hash: [u8; 32],
        is_cellbase: bool,
        inputs: Vec<crate::parser::transaction::ParsedInput>,
        cells: Vec<crate::parser::cell::ParsedCell>,
        witnesses: Vec<String>,
    ) -> TxData {
        let inputs_count =
            i16::try_from(inputs.len()).expect("test helper inputs_count exceeds i16 range");
        let outputs_count =
            i16::try_from(cells.len()).expect("test helper outputs_count exceeds i16 range");
        TxData {
            hash,
            block_number: 0,
            tx_index: 0,
            inputs_count,
            outputs_count,
            is_cellbase,
            inputs,
            cells,
            witnesses,
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: Utc::now(),
            semantic_tags: 0,
        }
    }

    // -- parse_parsed_cell_udt_amount tests ------------------------------------

    #[test]
    fn test_parse_parsed_cell_udt_amount_allows_xudt_without_amount_payload() {
        let mut cell = dummy_xudt_cell([0xAB; 32], vec![0xCD; XUDT_TYPE_ARGS_MIN_LEN]);
        cell.data.clear();
        cell.data_size = 0;

        let tx_hash = [0x81; 32];
        let amount = parse_parsed_cell_udt_amount(&cell, &tx_hash, 3).unwrap();
        assert_eq!(amount, None);
    }

    #[test]
    fn test_parse_parsed_cell_udt_amount_tolerates_invalid_sudt_payload() {
        // A cell using the sUDT type script but with data too short for a u128 amount
        // (a non-standard on-chain cell, e.g. testnet) is tolerated — recorded as no
        // amount rather than fail-fasting the sync (consistent with the bulk-build path
        // and the xUDT branch), so one junk cell can't halt the whole index.
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(sudt_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            type_script_hash: Some(vec![0x55; 32]),
            data_hash: [0x66; 32],
            data_size: 0,
            data: vec![],
        };

        let tx_hash = [0x82; 32];
        let amount = parse_parsed_cell_udt_amount(&cell, &tx_hash, 7).unwrap();
        assert_eq!(amount, None);
    }

    /// Re-pinned in resync fix round 2, B: a cell whose type pair is no UDT
    /// carries no amount; the `xudt_compatible` standard hint a token row
    /// gave (the half that read it is gone with the hint) never made it one.
    #[test]
    fn test_parse_parsed_cell_udt_amount_of_a_non_udt_pair_is_none() {
        let amount = 15_778_600u128;
        let mut data = vec![0u8; 16];
        data.copy_from_slice(&amount.to_le_bytes());
        let cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(vec![0x42; 32]), // non-standard xUDT code hash
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            type_script_hash: Some(vec![0x55; 32]),
            data_hash: [0x66; 32],
            data_size: 16,
            data,
        };

        let tx_hash = [0x83; 32];
        let parsed = parse_parsed_cell_udt_amount(&cell, &tx_hash, 0).unwrap();
        assert_eq!(
            parsed, None,
            "an amount-shaped payload under a non-UDT pair"
        );
        let _ = amount;
    }

    // -- omnilock tests --------------------------------------------------------

    #[test]
    fn test_extract_omnilock_supply_info_type_hash_with_all_modes() {
        let mut lock_args = vec![0u8; OMNILOCK_AUTH_LEN];
        let flags = OMNILOCK_SUPPLY_MODE_FLAG
            | OMNILOCK_ADMIN_MODE_FLAG
            | OMNILOCK_ACP_MODE_FLAG
            | OMNILOCK_TIMELOCK_MODE_FLAG;
        lock_args.push(flags);
        lock_args.extend_from_slice(&[0xAA; 32]); // admin list type id
        lock_args.extend_from_slice(&[0x01, 0x02]); // ACP min
        lock_args.extend_from_slice(&[0xBB; 8]); // since
        lock_args.extend_from_slice(&[0xCC; 32]); // supply info type script hash

        let parsed = extract_omnilock_supply_info_type_hash(&lock_args).unwrap();
        assert_eq!(parsed, [0xCC; 32]);
    }

    #[test]
    fn test_parse_omnilock_supply_info_cell_data_validates_bounds() {
        let mut data = Vec::with_capacity(65);
        data.push(0u8); // version
        data.extend_from_slice(&5u128.to_le_bytes()); // current
        data.extend_from_slice(&10u128.to_le_bytes()); // max
        data.extend_from_slice(&[0x11; 32]); // sUDT/xUDT type script hash

        let parsed = parse_omnilock_supply_info_cell_data(&data).unwrap();
        assert_eq!(parsed.0, 10);
        assert_eq!(parsed.1, [0x11; 32]);

        let mut invalid = data.clone();
        invalid[1..17].copy_from_slice(&11u128.to_le_bytes()); // current > max
        assert!(parse_omnilock_supply_info_cell_data(&invalid).is_none());
    }

    // -- collect_token_max_supply_observations tests ---------------------------

    #[test]
    fn test_collect_token_max_supply_observations_from_omnilock_info_cells() {
        let supply_info_type_hash = [0x22; 32];
        let token_type_hash = [0x33; 32];

        let mut lock_args = vec![0u8; OMNILOCK_AUTH_LEN];
        lock_args.push(OMNILOCK_SUPPLY_MODE_FLAG);
        lock_args.extend_from_slice(&supply_info_type_hash);

        let mut info_cell_data = Vec::with_capacity(65);
        info_cell_data.push(0u8);
        info_cell_data.extend_from_slice(&100u128.to_le_bytes());
        info_cell_data.extend_from_slice(&1_000u128.to_le_bytes());
        info_cell_data.extend_from_slice(&token_type_hash);

        let info_cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: crate::rpc::parse_hex_to_bytes(OMNILOCK_CODE_HASH_MAINNET_V2),
            lock_hash_type: 1,
            lock_args,
            lock_script_hash: vec![0x44; 32],
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x66; 32]),
            type_script_hash: Some(supply_info_type_hash.to_vec()),
            data_hash: [0x77; 32],
            data_size: info_cell_data.len() as i32,
            data: info_cell_data,
        };

        let tx = dummy_tx_data([0x88; 32], false, vec![], vec![info_cell.clone()], vec![]);
        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(
            observations
                .get(token_type_hash.as_slice())
                .map(|claims| (claims[0].max_supply, claims[0].claimant)),
            // the claimant is the info cell's own lock, by its script hash
            Some((1_000, MaxSupplyClaimant::SupplyOmnilock([0x44; 32])))
        );

        // resync/protocol-pairs: the claim is Omnilock's by its registered
        // pair only. The same info cell under Omnilock's code hash by `data`
        // (a pair anyone can lock a cell with; no such binary runs) claims no
        // max supply.
        let by_data = crate::parser::cell::ParsedCell {
            lock_hash_type: 0,
            ..info_cell
        };
        let tx = dummy_tx_data([0x89; 32], false, vec![], vec![by_data], vec![]);
        assert!(
            collect_token_max_supply_observations(&[tx]).is_empty(),
            "Omnilock's code hash by data claims nothing"
        );
    }

    /// The rule, alone: an Omnilock claim is the token's only when the
    /// token's owner lock (its first 32 type-args bytes) is the claiming
    /// lock; an extension claim is bound by construction; a token with no
    /// 32-byte owner takes no Omnilock claim.
    #[test]
    fn test_token_max_supply_claim_binds_an_omnilock_claim_to_the_tokens_owner() {
        let claim = |block_number, claimant| MaxSupplyObservation {
            max_supply: u128::try_from(block_number).unwrap(),
            block_number,
            tx_index: 0,
            output_index: 0,
            claimant,
        };
        let owner = [0x44; 32];
        let mut xudt_args = owner.to_vec();
        xudt_args.extend_from_slice(&[0, 0, 0, 0]);
        let forged = claim(10, MaxSupplyClaimant::SupplyOmnilock([0x45; 32]));
        let genuine = claim(11, MaxSupplyClaimant::SupplyOmnilock(owner));
        let extension = claim(12, MaxSupplyClaimant::TokenExtension);

        for args in [owner.as_slice(), xudt_args.as_slice()] {
            assert_eq!(
                token_max_supply_claim(&[forged, genuine], args, 0, i64::MAX),
                Some(genuine),
                "the forged claim is skipped, the owner's counts"
            );
            assert_eq!(token_max_supply_claim(&[forged], args, 0, i64::MAX), None);
        }
        assert_eq!(
            token_max_supply_claim(&[forged, extension], &[0x99; 32], 0, i64::MAX),
            Some(extension),
            "an extension claim needs no owner check"
        );
        assert_eq!(
            token_max_supply_claim(&[genuine], &owner[..20], 0, i64::MAX),
            None,
            "type args shorter than 32 bytes name no owner lock"
        );
    }

    #[test]
    fn test_collect_token_max_supply_observations_from_xudt_extension_flags_0x1() {
        let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 42_000u128;
        let token_type_hash = [0x91; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);

        let unique_cell = dummy_unique_token_info_cell(unique_type_args.clone(), total_supply);
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data(
            [0xEE; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![],
        );

        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(
            observations
                .get(token_type_hash.as_slice())
                .map(|claims| claims[0].max_supply),
            Some(total_supply)
        );
    }

    /// A cell with token-info data under 20-byte type args whose type script is
    /// NOT the Unique Cell (code hash 0x40.., by type): anyone can create one,
    /// naming any args (resync fix round S1).
    fn forged_token_info_cell(
        args: Vec<u8>,
        total_supply: u128,
    ) -> crate::parser::cell::ParsedCell {
        let data = build_token_info_data(total_supply);
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x10; 32],
            lock_hash_type: 1,
            lock_args: vec![0x20; 20],
            lock_script_hash: vec![0x30; 32],
            type_code_hash: Some(vec![0x40; 32]),
            type_hash_type: Some(1),
            type_args: Some(args),
            type_script_hash: Some(vec![0x51; 32]),
            data_hash: [0x61; 32],
            data_size: data.len() as i32,
            data,
        }
    }

    /// resync fix round S1 (logic S1): the xUDT extension claimant is bound by
    /// its script, not by 20 bytes of args. A transaction that outputs the
    /// token (its extension names args A) and a cell typed by another code
    /// with args A and token-info data records no claim and binds no
    /// metadata.
    #[test]
    fn a_forged_unique_cell_beside_the_tokens_output_claims_and_binds_nothing() {
        let args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let token_type_hash = [0x93; 32];
        let script_vec = encode_script_vec_with_unique_args(&args);
        let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);
        let tx = dummy_tx_data(
            [0xEF; 32],
            false,
            vec![],
            vec![
                forged_token_info_cell(args, 1111),
                dummy_xudt_cell(token_type_hash, type_args),
            ],
            vec![],
        );

        assert_eq!(
            collect_token_max_supply_observations(std::slice::from_ref(&tx)),
            HashMap::new(),
            "a forged Unique Cell makes no claim"
        );
        assert_eq!(
            collect_token_onchain_info(std::slice::from_ref(&tx), &HashMap::new()),
            HashMap::new(),
            "a forged Unique Cell binds no metadata"
        );
    }

    #[test]
    fn test_collect_token_max_supply_observations_from_xudt_extension_flags_0x2_witness() {
        let unique_type_args = vec![0xBC; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 100_001u128;
        let token_type_hash = [0x92; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let script_vec_hash = blake160(&script_vec);
        let type_args = build_xudt_type_args_with_extension_in_witness([0x02; 32], script_vec_hash);

        let xudt_witness = encode_xudt_witness(&script_vec);
        let witness_args = encode_witness_args(Some(&xudt_witness), None);
        let witness_hex = format!("0x{}", hex::encode(witness_args));

        let unique_cell = dummy_unique_token_info_cell(unique_type_args.clone(), total_supply);
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data(
            [0xEF; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![witness_hex],
        );

        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(
            observations
                .get(token_type_hash.as_slice())
                .map(|claims| claims[0].max_supply),
            Some(total_supply)
        );
    }

    #[test]
    fn test_collect_token_max_supply_observations_skips_xudt_extension_flags_0x2_when_witness_invalid(
    ) {
        let unique_type_args = vec![0xCD; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 77_700u128;
        let token_type_hash = [0x93; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args =
            build_xudt_type_args_with_extension_in_witness([0x03; 32], blake160(&script_vec));

        let mismatched_script_vec =
            encode_script_vec_with_unique_args(&[0xDD; UNIQUE_TYPE_ARGS_LEN]);
        let mismatched_witness =
            encode_witness_args(Some(&encode_xudt_witness(&mismatched_script_vec)), None);
        let tx_with_hash_mismatch = dummy_tx_data(
            [0xA1; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args.clone(), total_supply),
                dummy_xudt_cell(token_type_hash, type_args.clone()),
            ],
            vec![format!("0x{}", hex::encode(mismatched_witness))],
        );

        let tx_without_witness = dummy_tx_data(
            [0xA2; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args, total_supply),
                dummy_xudt_cell(token_type_hash, type_args),
            ],
            vec![],
        );

        let mismatch_observations = collect_token_max_supply_observations(&[tx_with_hash_mismatch]);
        assert!(!mismatch_observations.contains_key(token_type_hash.as_slice()));

        let missing_observations = collect_token_max_supply_observations(&[tx_without_witness]);
        assert!(!missing_observations.contains_key(token_type_hash.as_slice()));
    }

    // -- parse_unique_cell_token_info tests ------------------------------------

    #[test]
    fn test_parse_unique_cell_token_info_with_all_fields() {
        let data = build_token_info_data(42_000);
        let info = parse_unique_cell_token_info(&data).unwrap();
        assert_eq!(info.decimal, 8);
        assert_eq!(info.name, "Token");
        assert_eq!(info.symbol, "TKN");
        assert_eq!(info.total_supply, Some(42_000));
    }

    #[test]
    fn test_parse_unique_cell_token_info_without_tags() {
        let mut data = Vec::new();
        data.push(18); // decimal
        data.push(4); // name len
        data.extend_from_slice(b"Test");
        data.push(2); // symbol len
        data.extend_from_slice(b"TS");
        let info = parse_unique_cell_token_info(&data).unwrap();
        assert_eq!(info.decimal, 18);
        assert_eq!(info.name, "Test");
        assert_eq!(info.symbol, "TS");
        assert_eq!(info.total_supply, None);
    }

    #[test]
    fn test_parse_unique_cell_token_info_empty_name_symbol() {
        let data = vec![0u8, 0u8, 0u8]; // decimal=0, name_len=0, symbol_len=0
        let info = parse_unique_cell_token_info(&data).unwrap();
        assert_eq!(info.decimal, 0);
        assert_eq!(info.name, "");
        assert_eq!(info.symbol, "");
    }

    #[test]
    fn test_parse_unique_cell_token_info_truncated_data() {
        assert!(parse_unique_cell_token_info(&[]).is_none());
        assert!(parse_unique_cell_token_info(&[8]).is_none());
        assert!(parse_unique_cell_token_info(&[8, 5]).is_none()); // name_len=5 but no name bytes
    }

    #[test]
    fn test_parse_unique_cell_token_info_invalid_utf8() {
        let mut data = Vec::new();
        data.push(8);
        data.push(2);
        data.extend_from_slice(&[0xFF, 0xFE]); // invalid UTF-8
        data.push(2);
        data.extend_from_slice(b"OK");
        assert!(parse_unique_cell_token_info(&data).is_none());
    }

    // -- collect_token_onchain_info tests --------------------------------------

    #[test]
    fn test_collect_token_onchain_info_resolves_unique_cell_to_xudt() {
        let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let token_type_hash = [0x91; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);

        let unique_cell = dummy_unique_token_info_cell(unique_type_args, 42_000);
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data(
            [0xF0; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![],
        );

        let infos = collect_token_onchain_info(&[tx], &HashMap::new());
        let bound = infos.get(token_type_hash.as_slice()).unwrap().winner();
        assert_eq!(bound.binding, OnchainInfoBinding::XudtExtension);
        assert_eq!(bound.info.name, "Token");
        assert_eq!(bound.info.symbol, "TKN");
        assert_eq!(bound.info.decimal, 8);
        assert_eq!(bound.info.total_supply, Some(42_000));
    }

    #[test]
    fn test_collect_token_onchain_info_empty_when_no_unique_cell() {
        let token_type_hash = [0x92; 32];
        let type_args = vec![0x01; 36]; // just owner_lock_hash + flags, no extensions
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data([0xF1; 32], false, vec![], vec![xudt_cell], vec![]);

        let infos = collect_token_onchain_info(&[tx], &HashMap::new());
        assert!(infos.is_empty());
    }

    // -- issuance co-occurrence binding tests -----------------------------------

    /// Real mainnet vector: RGB++ Protocol issuance
    /// (tx 0xd088a12852664145773257eb1467cb0feca0d1d478968ce90b7f29bce24e2a4a,
    /// block 12,654,045), out[0] Unique Cell data.
    const RGBPP_UNIQUE_CELL_DATA_HEX: &str = "080e5247422b2b2050726f746f636f6c055247422b2b";

    fn rgbpp_unique_cell_data() -> Vec<u8> {
        hex::decode(RGBPP_UNIQUE_CELL_DATA_HEX).unwrap()
    }

    /// A Unique Cell output carrying token info, typed with an explicit
    /// (network-deployed) Unique Cell code hash.
    fn dummy_unique_info_cell_with_code_hash(
        type_code_hash: Vec<u8>,
        type_args: Vec<u8>,
        data: Vec<u8>,
    ) -> crate::parser::cell::ParsedCell {
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x10; 32],
            lock_hash_type: 1,
            lock_args: vec![0x20; 20],
            lock_script_hash: vec![0x30; 32],
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(2),
            type_args: Some(type_args),
            type_script_hash: Some(vec![0x50; 32]),
            data_hash: [0x60; 32],
            data_size: data.len() as i32,
            data,
        }
    }

    fn mainnet_unique_code_hash() -> Vec<u8> {
        crate::rpc::parse_hex_to_bytes(
            "0x2c8c11c985da60b0a330c61a85507416d6382c130ba67f0c47ab071e00aec628",
        )
    }

    #[test]
    fn test_parse_unique_cell_token_info_rgbpp_mainnet_vector() {
        let info = parse_unique_cell_token_info(&rgbpp_unique_cell_data()).unwrap();
        assert_eq!(info.decimal, 8);
        assert_eq!(info.name, "RGB++ Protocol");
        assert_eq!(info.symbol, "RGB++");
        assert_eq!(info.total_supply, None);
    }

    #[test]
    fn test_collect_token_onchain_info_binds_issuance_cooccurrence() {
        // RGB++ issuance shape: out[0] = Unique Cell (by code hash), out[1] = xUDT
        // whose args are a pure 32-byte owner lock hash (no flags) — the extension
        // path cannot bind this; the same-tx co-occurrence rule must.
        let token_type_hash = [0x91; 32];
        let unique_cell = dummy_unique_info_cell_with_code_hash(
            mainnet_unique_code_hash(),
            vec![0x98; UNIQUE_TYPE_ARGS_LEN],
            rgbpp_unique_cell_data(),
        );
        let xudt_cell = dummy_xudt_cell(token_type_hash, vec![0x08; 32]);
        let tx = dummy_tx_data(
            [0xD0; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![],
        );

        let infos = collect_token_onchain_info(&[tx], &HashMap::new());
        let bound = infos
            .get(token_type_hash.as_slice())
            .expect("single-mint issuance tx with a Unique Cell output must bind token info")
            .winner();
        assert_eq!(bound.binding, OnchainInfoBinding::IssuanceCooccurrence);
        assert_eq!(bound.info.name, "RGB++ Protocol");
        assert_eq!(bound.info.symbol, "RGB++");
        assert_eq!(bound.info.decimal, 8);
    }

    /// resync fix round S1: the co-occurrence rule recognises the Unique Cell
    /// by its pair too. The mainnet Unique Cell's code hash by `type` (no
    /// registered pair: no Unique Cell runs there) binds nothing.
    #[test]
    fn test_collect_token_onchain_info_cooccurrence_reads_the_unique_cell_by_its_pair() {
        let token_type_hash = [0x91; 32];
        let mut by_type = dummy_unique_info_cell_with_code_hash(
            mainnet_unique_code_hash(),
            vec![0x98; UNIQUE_TYPE_ARGS_LEN],
            rgbpp_unique_cell_data(),
        );
        by_type.type_hash_type = Some(1);
        let tx = dummy_tx_data(
            [0xD0; 32],
            false,
            vec![],
            vec![by_type, dummy_xudt_cell(token_type_hash, vec![0x08; 32])],
            vec![],
        );
        assert_eq!(
            collect_token_onchain_info(&[tx], &HashMap::new()),
            HashMap::new()
        );
    }

    #[test]
    fn test_collect_token_onchain_info_cooccurrence_skips_same_batch_transfer() {
        // tx1 mints T (no unique cell). tx2 spends tx1's T output — a transfer,
        // not an issuance — while also creating a unique-typed cell. The
        // co-occurrence rule must not bind: T is not minted by tx2.
        let token_type_hash = [0x93; 32];
        let tx1_hash = [0xD1; 32];
        let tx1 = dummy_tx_data(
            tx1_hash,
            false,
            vec![],
            vec![dummy_xudt_cell(token_type_hash, vec![0x09; 32])],
            vec![],
        );
        let tx2 = dummy_tx_data(
            [0xD2; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: tx1_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![
                dummy_unique_info_cell_with_code_hash(
                    mainnet_unique_code_hash(),
                    vec![0x97; UNIQUE_TYPE_ARGS_LEN],
                    rgbpp_unique_cell_data(),
                ),
                dummy_xudt_cell(token_type_hash, vec![0x09; 32]),
            ],
            vec![],
        );

        let infos = collect_token_onchain_info(&[tx1, tx2], &HashMap::new());
        assert!(
            !infos.contains_key(token_type_hash.as_slice()),
            "a same-batch transfer must not be treated as an issuance"
        );
    }

    #[test]
    fn test_collect_token_onchain_info_cooccurrence_skips_persisted_transfer_input() {
        // The token was issued in an earlier batch, so its input resolves from
        // the store rather than from this batch's outputs. It is still a
        // transfer, not an issuance, and must not adopt the Unique Cell info.
        let token_type_hash = [0x9C; 32];
        let previous_tx_hash = [0xE1; 32];
        let tx = dummy_tx_data(
            [0xE2; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![
                dummy_unique_info_cell_with_code_hash(
                    mainnet_unique_code_hash(),
                    vec![0x9D; UNIQUE_TYPE_ARGS_LEN],
                    rgbpp_unique_cell_data(),
                ),
                dummy_xudt_cell(token_type_hash, vec![0x0C; 32]),
            ],
            vec![],
        );

        // The consumed cell was created before this batch, so its type reaches
        // the rule through the persisted (prefetched) input view.
        let mut persisted_input_types: OutpointTypeHashes<'_> = HashMap::new();
        persisted_input_types.insert((previous_tx_hash.as_slice(), 0), token_type_hash.as_slice());

        let infos = collect_token_onchain_info(&[tx], &persisted_input_types);
        assert!(
            !infos.contains_key(token_type_hash.as_slice()),
            "an input resolved from persisted store state must veto the mint classification"
        );
    }

    #[test]
    fn test_collect_token_onchain_info_cooccurrence_requires_single_minted_type() {
        // Two distinct xUDT types minted alongside one unique cell: ambiguous,
        // must bind neither.
        let token_a = [0x94; 32];
        let token_b = [0x95; 32];
        let tx = dummy_tx_data(
            [0xD3; 32],
            false,
            vec![],
            vec![
                dummy_unique_info_cell_with_code_hash(
                    mainnet_unique_code_hash(),
                    vec![0x96; UNIQUE_TYPE_ARGS_LEN],
                    rgbpp_unique_cell_data(),
                ),
                dummy_xudt_cell(token_a, vec![0x0A; 32]),
                dummy_xudt_cell(token_b, vec![0x0B; 32]),
            ],
            vec![],
        );

        let infos = collect_token_onchain_info(&[tx], &HashMap::new());
        assert!(!infos.contains_key(token_a.as_slice()));
        assert!(!infos.contains_key(token_b.as_slice()));
    }

    #[test]
    fn test_collect_token_onchain_info_extension_binding_not_displaced_by_cooccurrence() {
        // tx1 binds T via the cryptographic extension-script path ("Token"/"TKN").
        // tx2 is a co-occurrence-shaped tx for the same T with different unique
        // info; the heuristic binding must never displace the extension binding.
        let token_type_hash = [0x9A; 32];
        let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let ext_type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);

        let tx1 = dummy_tx_data(
            [0xD4; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args, 42_000),
                dummy_xudt_cell(token_type_hash, ext_type_args.clone()),
            ],
            vec![],
        );

        let mut other_info = Vec::new();
        other_info.push(9u8); // decimal
        other_info.push(5u8);
        other_info.extend_from_slice(b"Other");
        other_info.push(3u8);
        other_info.extend_from_slice(b"OTH");
        let tx2 = dummy_tx_data(
            [0xD5; 32],
            false,
            vec![],
            vec![
                dummy_unique_info_cell_with_code_hash(
                    mainnet_unique_code_hash(),
                    vec![0x9B; UNIQUE_TYPE_ARGS_LEN],
                    other_info,
                ),
                dummy_xudt_cell(token_type_hash, ext_type_args),
            ],
            vec![],
        );

        let infos = collect_token_onchain_info(&[tx1, tx2], &HashMap::new());
        let bound = infos
            .get(token_type_hash.as_slice())
            .expect("extension binding must be present")
            .winner();
        assert_eq!(bound.binding, OnchainInfoBinding::XudtExtension);
        assert_eq!(bound.info.name, "Token");
        assert_eq!(bound.info.symbol, "TKN");
        assert_eq!(bound.info.decimal, 8);
    }

    /// J4: a live batch spans many blocks, so a shallow rollback to a block
    /// inside it needs the binding a batch ending there would have chosen.
    /// Block 5 binds by co-occurrence ("Other"), block 7 displaces it with
    /// the extension binding ("Token").
    #[test]
    fn test_collect_token_onchain_info_keeps_the_block_of_each_assignment() {
        let token_type_hash = [0x9C; 32];
        let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let ext_type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);
        let mut other_info = vec![9u8, 5u8];
        other_info.extend_from_slice(b"Other");
        other_info.push(3u8);
        other_info.extend_from_slice(b"OTH");

        let mut cooccurrence_tx = dummy_tx_data(
            [0xD6; 32],
            false,
            vec![],
            vec![
                dummy_unique_info_cell_with_code_hash(
                    mainnet_unique_code_hash(),
                    vec![0x9B; UNIQUE_TYPE_ARGS_LEN],
                    other_info,
                ),
                dummy_xudt_cell(token_type_hash, ext_type_args.clone()),
            ],
            vec![],
        );
        cooccurrence_tx.block_number = 5;
        let mut extension_tx = dummy_tx_data(
            [0xD7; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args, 42_000),
                dummy_xudt_cell(token_type_hash, ext_type_args),
            ],
            vec![],
        );
        extension_tx.block_number = 7;

        let infos = collect_token_onchain_info(&[cooccurrence_tx, extension_tx], &HashMap::new());
        let timeline = infos.get(token_type_hash.as_slice()).unwrap();
        assert_eq!(timeline.blocks().collect::<Vec<_>>(), vec![5, 7]);
        assert_eq!(timeline.winner_through(4), None);
        let at = |block| timeline.winner_through(block).map(|b| b.info.name.as_str());
        assert_eq!(at(5), Some("Other"));
        assert_eq!(at(6), Some("Other"));
        assert_eq!(at(7), Some("Token"));
        assert_eq!(timeline.winner().info.name, "Token");
        assert_eq!(
            timeline.winner_through(6).unwrap().binding,
            OnchainInfoBinding::IssuanceCooccurrence
        );
    }

    /// J4: the batch's max supply is its first observation, and it carries
    /// the block that observed it.
    #[test]
    fn test_collect_token_max_supply_observations_keeps_the_first_observing_block() {
        let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let token_type_hash = [0x93; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);
        let observing_tx = |hash: u8, block: i64, supply: u128| {
            let mut tx = dummy_tx_data(
                [hash; 32],
                false,
                vec![],
                vec![
                    dummy_unique_token_info_cell(unique_type_args.clone(), supply),
                    dummy_xudt_cell(token_type_hash, type_args.clone()),
                ],
                vec![],
            );
            tx.block_number = block;
            tx
        };

        let observations = collect_token_max_supply_observations(&[
            observing_tx(0xE1, 11, 42_000),
            observing_tx(0xE2, 12, 43_000),
        ]);
        // every claim, in chain order; the token's is the rule's to pick
        assert_eq!(
            observations
                .get(token_type_hash.as_slice())
                .map(Vec::as_slice),
            Some(
                [
                    MaxSupplyObservation {
                        max_supply: 42_000,
                        block_number: 11,
                        tx_index: 0,
                        output_index: 1,
                        claimant: MaxSupplyClaimant::TokenExtension,
                    },
                    MaxSupplyObservation {
                        max_supply: 43_000,
                        block_number: 12,
                        tx_index: 0,
                        output_index: 1,
                        claimant: MaxSupplyClaimant::TokenExtension,
                    },
                ]
                .as_slice()
            )
        );
    }
}
