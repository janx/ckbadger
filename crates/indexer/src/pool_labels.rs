//! Mining-pool names for block miners (`docs/metadata/pools.toml`).
//!
//! The table is the operator's data: it maps a pool's display name to the
//! cellbase witness facts that identify its blocks — raw message prefixes and
//! exact miner locks. It is bundled at compile time and, like every label
//! file, replaced by `<work_dir>/metadata/pools.toml` when that file exists
//! (the per-file override label import applies to `tokens/` and `scripts/`).
//!
//! Read-only: nothing here writes a store. The API loads it once at router
//! build — a malformed file fails the build, naming the file and entry — and
//! matches each block's cellbase witness at read time with
//! [`PoolLabels::pool_of`]. A miner no entry names has no pool: there is no
//! match beyond the exact locks and byte prefixes an entry declares.

use anyhow::{bail, Context, Result};
use ckb_types::{bytes::Bytes, packed, prelude::*};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// The file name, bundled from `docs/metadata/` and overridden in
/// `<work_dir>/metadata/`.
pub const POOLS_FILE_NAME: &str = "pools.toml";

const BUNDLED_POOLS: &str = include_str!("../../../docs/metadata/pools.toml");
const BUNDLED_SOURCE: &str = "bundled docs/metadata/pools.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolsDocument {
    #[serde(default)]
    pools: Vec<PoolEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolEntry {
    name: String,
    #[serde(default)]
    mainnet: Option<PoolRules>,
    #[serde(default)]
    testnet: Option<PoolRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolRules {
    #[serde(default)]
    message_prefixes: Vec<String>,
    #[serde(default)]
    lock_hashes: Vec<String>,
    #[serde(default)]
    locks: Vec<LockRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockRule {
    code_hash: String,
    hash_type: String,
    args: String,
}

/// One network's validated pool table. No two rules of one kind can name the
/// same block: a lock is listed once, and no prefix is a prefix of another.
#[derive(Debug, Clone)]
pub struct PoolLabels {
    source: String,
    names: Vec<String>,
    by_lock_hash: HashMap<[u8; 32], usize>,
    message_prefixes: Vec<(Vec<u8>, usize)>,
}

impl PoolLabels {
    /// The table the deployment uses: `<metadata_path>/pools.toml` when that
    /// file exists, the bundled file otherwise — the label-import override.
    pub fn effective(metadata_path: Option<&Path>, network: &str) -> Result<Self> {
        if let Some(dir) = metadata_path {
            let path = dir.join(POOLS_FILE_NAME);
            if path.exists() {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                return Self::parse(&content, &path.display().to_string(), network);
            }
        }
        Self::bundled(network)
    }

    /// The table every binary carries.
    pub fn bundled(network: &str) -> Result<Self> {
        Self::parse(BUNDLED_POOLS, BUNDLED_SOURCE, network)
    }

    /// Parse and validate one `pools.toml` for `network`. `source` names the
    /// file in every error.
    pub fn parse(content: &str, source: &str, network: &str) -> Result<Self> {
        if !matches!(network, "mainnet" | "testnet") {
            bail!("{source}: no pool section exists for network `{network}`");
        }
        let document: PoolsDocument =
            toml::from_str(content).with_context(|| format!("malformed {source}"))?;
        let mut labels = Self {
            source: source.to_string(),
            names: Vec::with_capacity(document.pools.len()),
            by_lock_hash: HashMap::new(),
            message_prefixes: Vec::new(),
        };
        for entry in document.pools {
            labels.add(entry, network)?;
        }
        Ok(labels)
    }

    fn add(&mut self, entry: PoolEntry, network: &str) -> Result<()> {
        let source = self.source.clone();
        let name = entry.name;
        if name.is_empty() || name.trim() != name {
            bail!("{source}: pool name {name:?} is empty or has surrounding whitespace");
        }
        if self.names.contains(&name) {
            bail!("{source}: pool `{name}` is listed twice");
        }
        if entry.mainnet.is_none() && entry.testnet.is_none() {
            bail!("{source}: pool `{name}` has neither a [pools.mainnet] nor a [pools.testnet] section");
        }
        for (section, rules) in [("mainnet", &entry.mainnet), ("testnet", &entry.testnet)] {
            if let Some(rules) = rules {
                if rules.message_prefixes.is_empty()
                    && rules.lock_hashes.is_empty()
                    && rules.locks.is_empty()
                {
                    bail!("{source}: pool `{name}` [pools.{section}] lists no rule");
                }
            }
        }
        let rules = match network {
            "mainnet" => entry.mainnet,
            "testnet" => entry.testnet,
            other => bail!("{source}: no pool section exists for network `{other}`"),
        };
        let index = self.names.len();
        self.names.push(name);
        let Some(rules) = rules else {
            return Ok(());
        };
        let name = &self.names[index];
        let context = |rule: &str| format!("{source}: pool `{name}` [pools.{network}] {rule}");

        let mut lock_hashes = Vec::new();
        for raw in &rules.lock_hashes {
            lock_hashes.push((
                hash32(raw).with_context(|| context(&format!("lock_hashes entry {raw:?}")))?,
                format!("lock hash {raw}"),
            ));
        }
        for lock in &rules.locks {
            let label = format!(
                "lock {{ code_hash = {:?}, hash_type = {:?}, args = {:?} }}",
                lock.code_hash, lock.hash_type, lock.args
            );
            lock_hashes.push((
                lock_script_hash(lock).with_context(|| context(&label))?,
                label,
            ));
        }
        for (hash, label) in lock_hashes {
            if let Some(&other) = self.by_lock_hash.get(&hash) {
                bail!(
                    "{}: lock 0x{} ({label}) is already listed for pool `{}`",
                    context("locks"),
                    hex::encode(hash),
                    self.names[other]
                );
            }
            self.by_lock_hash.insert(hash, index);
        }

        for raw in &rules.message_prefixes {
            let prefix = hex_bytes(raw)
                .with_context(|| context(&format!("message_prefixes entry {raw:?}")))?;
            if prefix.is_empty() {
                bail!(
                    "{}: an empty prefix would match every block",
                    context("message_prefixes")
                );
            }
            if let Some((other, other_index)) = self
                .message_prefixes
                .iter()
                .find(|(other, _)| other.starts_with(&prefix) || prefix.starts_with(other))
            {
                bail!(
                    "{}: prefix {raw} overlaps prefix 0x{} of pool `{}` (one is a prefix of the other)",
                    context("message_prefixes"),
                    hex::encode(other),
                    self.names[*other_index]
                );
            }
            self.message_prefixes.push((prefix, index));
        }
        Ok(())
    }

    /// The pool a block's cellbase witness names, from its miner lock's script
    /// hash and its raw message bytes. `Ok(None)` when no entry names either;
    /// an error when the lock names one pool and the message another.
    pub fn pool_of(&self, lock_hash: &[u8; 32], message: &[u8]) -> Result<Option<&str>> {
        let by_lock = self.by_lock_hash.get(lock_hash).copied();
        let mut by_message = self
            .message_prefixes
            .iter()
            .filter(|(prefix, _)| message.starts_with(prefix));
        let first = by_message.next();
        if let (Some((a, _)), Some((b, _))) = (first, by_message.next()) {
            bail!(
                "{}: message 0x{} matches overlapping prefixes 0x{} and 0x{} (the load rejects overlaps)",
                self.source,
                hex::encode(message),
                hex::encode(a),
                hex::encode(b)
            );
        }
        let by_message = first.map(|(prefix, index)| (prefix, *index));
        let index = match (by_lock, by_message) {
            (None, None) => return Ok(None),
            (Some(index), None) | (None, Some((_, index))) => index,
            (Some(lock), Some((_, message_index))) if lock == message_index => lock,
            (Some(lock), Some((prefix, message_index))) => bail!(
                "{}: miner lock 0x{} is pool `{}` but cellbase message 0x{} starts with pool `{}`'s prefix 0x{}",
                self.source,
                hex::encode(lock_hash),
                self.names[lock],
                hex::encode(message),
                self.names[message_index],
                hex::encode(prefix)
            ),
        };
        Ok(Some(self.names[index].as_str()))
    }

    /// Whether this network's table names no lock and no prefix.
    pub fn is_empty(&self) -> bool {
        self.by_lock_hash.is_empty() && self.message_prefixes.is_empty()
    }
}

fn hex_bytes(raw: &str) -> Result<Vec<u8>> {
    let digits = raw
        .strip_prefix("0x")
        .ok_or_else(|| anyhow::anyhow!("hex must start with 0x"))?;
    hex::decode(digits).map_err(|e| anyhow::anyhow!("invalid hex: {e}"))
}

fn hash32(raw: &str) -> Result<[u8; 32]> {
    let bytes = hex_bytes(raw)?;
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| anyhow::anyhow!("expected 32 bytes, got {}", bytes.len()))
}

fn lock_script_hash(lock: &LockRule) -> Result<[u8; 32]> {
    let code_hash = hash32(&lock.code_hash).context("code_hash")?;
    let hash_type = ckbadger_common::hash_type_from_label(&lock.hash_type)
        .ok_or_else(|| anyhow::anyhow!("unknown hash_type {:?}", lock.hash_type))?;
    let args = hex_bytes(&lock.args).context("args")?;
    let script = packed::Script::new_builder()
        .code_hash(packed::Byte32::new(code_hash))
        .hash_type(packed::Byte::new(hash_type))
        .args(Bytes::from(args).pack())
        .build();
    Ok(script.calc_script_hash().unpack())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> Result<PoolLabels> {
        PoolLabels::parse(content, "test pools.toml", "mainnet")
    }

    fn err(content: &str) -> String {
        format!("{:#}", parse(content).unwrap_err())
    }

    const HASH_A: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn bundled_table_parses_on_both_networks() {
        // The bundled file ships empty: no cited public source exists yet.
        assert!(PoolLabels::bundled("mainnet").unwrap().is_empty());
        assert!(PoolLabels::bundled("testnet").unwrap().is_empty());
        assert!(PoolLabels::bundled("dev").is_err());
    }

    #[test]
    fn the_commented_example_is_a_valid_entry_once_uncommented() {
        let example: String = BUNDLED_POOLS
            .lines()
            .skip_while(|line| !line.starts_with("# [[pools]]"))
            .take_while(|line| line.starts_with('#') && !line.trim_end().eq("#"))
            .map(|line| line.trim_start_matches('#').trim_start())
            .collect::<Vec<_>>()
            .join("\n");
        let labels = parse(&example).unwrap();
        assert_eq!(labels.pool_of(&[0; 32], b"").unwrap(), Some("Example Pool"));
        assert_eq!(
            labels.pool_of(&[1; 32], b"example-pool/0.1").unwrap(),
            Some("Example Pool")
        );
    }

    #[test]
    fn lock_rules_match_exactly_and_nothing_else_matches() {
        let labels = parse(&format!(
            "[[pools]]\nname = \"A\"\n[pools.mainnet]\nlock_hashes = [\"{HASH_A}\"]\n"
        ))
        .unwrap();
        assert_eq!(labels.pool_of(&[0xaa; 32], b"any").unwrap(), Some("A"));
        let mut near = [0xaa; 32];
        near[31] = 0xab;
        assert_eq!(labels.pool_of(&near, b"any").unwrap(), None);
    }

    #[test]
    fn a_full_lock_script_matches_by_its_script_hash() {
        let code_hash = "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
        let labels = parse(&format!(
            "[[pools]]\nname = \"S\"\n[pools.mainnet]\nlocks = [{{ code_hash = \"0x{code_hash}\", \
             hash_type = \"type\", args = \"0x{}\" }}]\n",
            "11".repeat(20)
        ))
        .unwrap();
        let expected: [u8; 32] = packed::Script::new_builder()
            .code_hash(packed::Byte32::new(
                hash32(&format!("0x{code_hash}")).unwrap(),
            ))
            .hash_type(ckb_types::core::ScriptHashType::Type.into())
            .args(Bytes::from(vec![0x11; 20]).pack())
            .build()
            .calc_script_hash()
            .unpack();
        assert_eq!(labels.pool_of(&expected, b"").unwrap(), Some("S"));
        assert_eq!(labels.pool_of(&[0x11; 32], b"").unwrap(), None);
    }

    #[test]
    fn message_prefixes_compare_raw_bytes_from_byte_zero() {
        let labels = parse(
            "[[pools]]\nname = \"N\"\n[pools.mainnet]\nmessage_prefixes = [\"0x0000000020ff\"]\n",
        )
        .unwrap();
        assert_eq!(
            labels
                .pool_of(&[0; 32], &[0, 0, 0, 0, b' ', 0xff, 1])
                .unwrap(),
            Some("N")
        );
        // The same bytes later in the message, or a shorter message, do not match.
        assert_eq!(
            labels
                .pool_of(&[0; 32], &[1, 0, 0, 0, 0, b' ', 0xff])
                .unwrap(),
            None
        );
        assert_eq!(labels.pool_of(&[0; 32], &[0, 0, 0, 0, b' ']).unwrap(), None);
    }

    #[test]
    fn a_lock_and_a_message_naming_different_pools_is_an_error() {
        let labels = parse(&format!(
            "[[pools]]\nname = \"A\"\n[pools.mainnet]\nlock_hashes = [\"{HASH_A}\"]\n\
             [[pools]]\nname = \"B\"\n[pools.mainnet]\nmessage_prefixes = [\"0x62\"]\n"
        ))
        .unwrap();
        let error = format!("{:#}", labels.pool_of(&[0xaa; 32], b"b-pool").unwrap_err());
        assert!(error.contains("`A`") && error.contains("`B`"), "{error}");
        // Both rules of ONE pool agreeing is that pool.
        let one = parse(&format!(
            "[[pools]]\nname = \"A\"\n[pools.mainnet]\nlock_hashes = [\"{HASH_A}\"]\n\
             message_prefixes = [\"0x61\"]\n"
        ))
        .unwrap();
        assert_eq!(one.pool_of(&[0xaa; 32], b"a-pool").unwrap(), Some("A"));
    }

    #[test]
    fn sections_select_the_network() {
        let content =
            format!("[[pools]]\nname = \"T\"\n[pools.testnet]\nlock_hashes = [\"{HASH_A}\"]\n");
        let mainnet = PoolLabels::parse(&content, "t", "mainnet").unwrap();
        let testnet = PoolLabels::parse(&content, "t", "testnet").unwrap();
        assert_eq!(mainnet.pool_of(&[0xaa; 32], b"").unwrap(), None);
        assert_eq!(testnet.pool_of(&[0xaa; 32], b"").unwrap(), Some("T"));
    }

    #[test]
    fn ambiguous_or_malformed_tables_fail_the_load() {
        let lock = |name: &str| {
            format!("[[pools]]\nname = \"{name}\"\n[pools.mainnet]\nlock_hashes = [\"{HASH_A}\"]\n")
        };
        assert!(err(&format!("{}{}", lock("A"), lock("B"))).contains("already listed for pool `A`"));
        assert!(err(&format!("{}{}", lock("A"), lock("A"))).contains("listed twice"));
        let prefix = |name: &str, hex: &str| {
            format!(
                "[[pools]]\nname = \"{name}\"\n[pools.mainnet]\nmessage_prefixes = [\"{hex}\"]\n"
            )
        };
        assert!(err(&format!(
            "{}{}",
            prefix("A", "0x6162"),
            prefix("B", "0x616263")
        ))
        .contains("overlaps"));
        assert!(err(&format!(
            "{}{}",
            prefix("A", "0x616263"),
            prefix("B", "0x61")
        ))
        .contains("overlaps"));
        assert!(err(&prefix("A", "0x")).contains("empty prefix"));
        assert!(err(&prefix("A", "6162")).contains("start with 0x"));
        assert!(err(&prefix("A", "0xzz")).contains("invalid hex"));
        assert!(
            err("[[pools]]\nname = \"A\"\n[pools.mainnet]\nlock_hashes = [\"0xaa\"]\n")
                .contains("expected 32 bytes")
        );
        assert!(err("[[pools]]\nname = \"A\"\n").contains("neither"));
        assert!(err("[[pools]]\nname = \"A\"\n[pools.mainnet]\n").contains("lists no rule"));
        assert!(
            err("[[pools]]\nname = \" A\"\n[pools.mainnet]\nlock_hashes = []\n")
                .contains("surrounding whitespace")
        );
        assert!(err("[[pools]]\nname = \"A\"\nwebsite = \"x\"\n").contains("malformed"));
        assert!(err(&format!(
            "[[pools]]\nname = \"A\"\n[pools.mainnet]\nlocks = [{{ code_hash = \"{HASH_A}\", \
             hash_type = \"type2\", args = \"0x\" }}]\n"
        ))
        .contains("unknown hash_type"));
    }

    #[test]
    fn the_workdir_file_replaces_the_bundled_table() {
        let dir = tempfile::tempdir().unwrap();
        // No override file: the bundled table.
        assert!(PoolLabels::effective(Some(dir.path()), "mainnet")
            .unwrap()
            .is_empty());
        std::fs::write(
            dir.path().join(POOLS_FILE_NAME),
            format!("[[pools]]\nname = \"W\"\n[pools.mainnet]\nlock_hashes = [\"{HASH_A}\"]\n"),
        )
        .unwrap();
        let labels = PoolLabels::effective(Some(dir.path()), "mainnet").unwrap();
        assert_eq!(labels.pool_of(&[0xaa; 32], b"").unwrap(), Some("W"));
        // A malformed override fails, naming the file — never the bundled table instead.
        std::fs::write(dir.path().join(POOLS_FILE_NAME), "pools = 1").unwrap();
        let error = format!(
            "{:#}",
            PoolLabels::effective(Some(dir.path()), "mainnet").unwrap_err()
        );
        assert!(error.contains(POOLS_FILE_NAME), "{error}");
    }
}
