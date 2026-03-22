//! Background worker that batch-decodes DOB spores using CKB-VM.
//!
//! After sync catches up, this worker iterates over all spore entries with
//! `dob/*` content types that lack a cached decode result, fetches the
//! decoder binary from chain, executes it in CKB-VM, and writes the
//! decoded traits + media sources into `CF_DOB_DECODED`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use tracing::{debug, info, warn};

use ckbadger_dob_decoder::cache::DecoderBinaryCache;
use ckbadger_dob_decoder::fetch::fetch_decoder_binary;
use ckbadger_dob_decoder::types::{DecoderRef, DobTrait};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    ClusterAggregate, CompositionTier, DobDecodedEntry, DobDecodedTrait, ObjectEntry, ObjectExtra,
    SporeMediaProfile, SporeMediaSource,
};
use ckbadger_store::CkbadgerStore;

use crate::parser::media_source::{
    extract_uri_sources, parse_dna_hex_from_content_text, resolve_tier, uri_seems_image,
};
use crate::parser::spore::SporeParser;
use crate::rpc::{parse_hex_to_bytes, CkbRpcClient};

const BATCH_SIZE: usize = 500;
const MAX_MEDIA_SOURCES: usize = 24;

/// Background worker that decodes DOB spores via CKB-VM after sync catches up.
pub struct DobDecodeWorker {
    /// Domain store — reads spore/cluster entries and writes decode results.
    store: Arc<CkbadgerStore>,
    /// Append-only store — provides decoder code cell payload metadata.
    append_only_store: Arc<CkbadgerStore>,
    /// Disk cache for decoder RISC-V binaries.
    decoder_cache: Arc<DecoderBinaryCache>,
    /// CKB RPC endpoint URL for fetching decoder binaries.
    rpc_url: String,
    /// Cooperative shutdown flag.
    shutdown: Arc<AtomicBool>,
}

impl DobDecodeWorker {
    pub fn new(
        store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
        decoder_cache: Arc<DecoderBinaryCache>,
        rpc_url: String,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store,
            append_only_store,
            decoder_cache,
            rpc_url,
            shutdown,
        }
    }

    /// Main loop: paginate through undecoded DOB spores and decode each one.
    pub async fn run(&self) -> Result<()> {
        info!("DOB decode worker started");

        // Get total for progress tracking (one-time scan).
        let total = self.store.count_undecoded_dob_spores()?;
        let start = std::time::Instant::now();

        self.store.update_background_task("dob_decode", |entry| {
            entry.state = ckbadger_common::BackgroundTaskState::Running;
            entry.started_at = Some(chrono::Utc::now().timestamp());
            entry.progress_current = Some(0);
            entry.progress_total = Some(total);
            entry.message = Some(format!("{} undecoded spores", total));
        })?;

        let mut cursor: Option<Vec<u8>> = None;
        let mut total_decoded: u64 = 0;
        let mut total_skipped: u64 = 0;

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                info!(
                    total_decoded,
                    total_skipped, "DOB decode worker shutting down"
                );
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                let _ = self.store.update_background_task("dob_decode", |entry| {
                    entry.state = ckbadger_common::BackgroundTaskState::Completed;
                    entry.progress_current = Some(total_decoded + total_skipped);
                    entry.elapsed_ms = Some(elapsed_ms);
                    entry.message = Some("Shutdown requested".to_string());
                });
                return Ok(());
            }

            let batch_entries = self
                .store
                .list_undecoded_dob_spores(BATCH_SIZE, cursor.as_deref())?;

            if batch_entries.is_empty() {
                info!(
                    total_decoded,
                    total_skipped, "DOB decode worker completed — no more undecoded spores"
                );
                break;
            }

            let batch_len = batch_entries.len();
            debug!(batch_size = batch_len, "processing DOB decode batch");

            for (spore_id, content_type, collection_id) in &batch_entries {
                if self.shutdown.load(Ordering::Relaxed) {
                    info!(
                        total_decoded,
                        total_skipped, "DOB decode worker shutting down mid-batch"
                    );
                    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let _ = self.store.update_background_task("dob_decode", |entry| {
                        entry.state = ckbadger_common::BackgroundTaskState::Completed;
                        entry.progress_current = Some(total_decoded + total_skipped);
                        entry.elapsed_ms = Some(elapsed_ms);
                        entry.message = Some("Shutdown requested".to_string());
                    });
                    return Ok(());
                }

                match self
                    .decode_single_spore(spore_id, content_type, collection_id.as_deref())
                    .await
                {
                    Ok(entry) => {
                        // Write the decode result
                        let mut store_batch = StoreBatch::new(&self.store);
                        store_batch.put_dob_decoded(spore_id, &entry);
                        store_batch.commit()?;

                        // Update spore media profile if new media sources were found
                        if !entry.media_sources.is_empty() {
                            if let Err(e) =
                                self.update_spore_media_profile(spore_id, &entry.media_sources)
                            {
                                warn!(
                                    spore_id = hex::encode(spore_id),
                                    error = %e,
                                    "failed to update spore media profile after DOB decode"
                                );
                            }
                        }

                        total_decoded += 1;
                        debug!(
                            spore_id = hex::encode(spore_id),
                            traits = entry.traits.len(),
                            media_sources = entry.media_sources.len(),
                            "decoded DOB spore"
                        );
                    }
                    Err(e) => {
                        total_skipped += 1;
                        debug!(
                            spore_id = hex::encode(spore_id),
                            error = %e,
                            "skipping DOB spore decode"
                        );
                    }
                }
            }

            // Update progress at batch boundary
            let elapsed = start.elapsed();
            let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
            let processed = total_decoded + total_skipped;
            let rate = if elapsed.as_secs_f64() > 0.0 {
                total_decoded as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let eta = if rate > 0.0 && total > processed {
                Some((total - processed) as f64 / rate)
            } else {
                None
            };
            let _ = self.store.update_background_task("dob_decode", |entry| {
                entry.progress_current = Some(processed);
                entry.elapsed_ms = Some(elapsed_ms);
                entry.rate = Some(rate);
                entry.eta_seconds = eta;
                entry.message = Some(format!(
                    "Decoded {}, skipped {}",
                    total_decoded, total_skipped
                ));
            });

            // Advance cursor to last key in batch
            if let Some((last_key, _, _)) = batch_entries.last() {
                cursor = Some(last_key.clone());
            }

            // If we got fewer than BATCH_SIZE, we've reached the end
            if batch_len < BATCH_SIZE {
                info!(
                    total_decoded,
                    total_skipped, "DOB decode worker completed — reached end of spore scan"
                );
                break;
            }
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let _ = self.store.update_background_task("dob_decode", |entry| {
            entry.state = ckbadger_common::BackgroundTaskState::Completed;
            entry.progress_current = Some(total_decoded + total_skipped);
            entry.elapsed_ms = Some(elapsed_ms);
            entry.rate = None;
            entry.eta_seconds = None;
            entry.message = Some(format!(
                "Done: {} decoded, {} skipped",
                total_decoded, total_skipped
            ));
        });

        Ok(())
    }

    /// Decode a single DOB spore by loading its cluster metadata, fetching
    /// the decoder binary, and executing it in CKB-VM.
    async fn decode_single_spore(
        &self,
        spore_id: &[u8],
        content_type: &str,
        collection_id: Option<&[u8]>,
    ) -> Result<DobDecodedEntry> {
        // Load cluster entry for DOB metadata
        let cluster_id = collection_id
            .context("DOB spore has no collection_id — cannot resolve cluster metadata")?;

        let cluster_entry = self.store.get_spore(cluster_id)?.with_context(|| {
            format!(
                "cluster entry not found for cluster_id=0x{}",
                hex::encode(cluster_id)
            )
        })?;

        let cluster_description = cluster_entry
            .description
            .as_deref()
            .context("cluster entry has no description")?;

        let metadata: Value = serde_json::from_str(cluster_description)
            .context("cluster description is not valid JSON")?;

        // Extract DNA hex from the spore cell's on-chain content.
        let dna_hex = extract_dna_from_spore(spore_id, &self.store, &self.rpc_url).await?;

        // Parse decoder reference from cluster DOB metadata
        let dob_obj = metadata
            .get("dob")
            .context("cluster metadata missing 'dob' field")?;

        let decoder_ref = parse_decoder_ref(dob_obj)?;
        let pattern_json = extract_pattern_json(dob_obj)?;

        // Fetch decoder binary (from cache or chain)
        let decoder_binary = self.load_decoder_binary(&decoder_ref).await?;

        // Determine DOB version from content type
        let dob_version = parse_dob_version(content_type);

        // Execute decoder
        let decoded = match dob_version {
            0 => ckbadger_dob_decoder::decode_dob0(&decoder_binary, &dna_hex, &pattern_json)?,
            1 => {
                // DOB/1 uses a chain of decoders; for now we support single-decoder DOB/1
                let decoders: Vec<(&[u8], &str)> =
                    vec![(decoder_binary.as_slice(), pattern_json.as_str())];
                ckbadger_dob_decoder::decode_dob1_chain(&decoders, &dna_hex)?
            }
            v => bail!("unsupported DOB version: {v}"),
        };

        // Convert DobTrait -> DobDecodedTrait
        let traits: Vec<DobDecodedTrait> = decoded
            .traits
            .iter()
            .map(|t| DobDecodedTrait {
                name: t.name.clone(),
                value: format_trait_value(&t.value),
            })
            .collect();

        // Extract media sources from decoded trait values
        let media_sources = extract_media_sources_from_traits(&decoded.traits);

        // Check if any SVG markup was produced (DOB/1 rendering)
        let svg_markup = if decoded.raw_output.to_ascii_lowercase().contains("<svg") {
            Some(decoded.raw_output.clone())
        } else {
            None
        };

        Ok(DobDecodedEntry {
            traits,
            svg_markup,
            media_sources,
            decoded_at: chrono::Utc::now().timestamp(),
        })
    }

    async fn load_decoder_binary(&self, decoder_ref: &DecoderRef) -> Result<Vec<u8>> {
        match decoder_ref {
            DecoderRef::CodeHash(code_hash) => {
                let cache_key = DecoderBinaryCache::code_hash_key(code_hash);
                if let Some(binary) = self.decoder_cache.get(&cache_key) {
                    return Ok(binary);
                }

                let (tx_hash, output_index, _) = self
                    .store
                    .find_any_cell_by_data_hash(code_hash, self.append_only_store.as_ref())
                    .with_context(|| {
                        format!(
                            "failed to resolve decoder code cell from local data-hash index: code_hash=0x{}",
                            hex::encode(code_hash)
                        )
                    })?
                    .with_context(|| {
                        format!(
                            "decoder code cell missing from local data-hash index: code_hash=0x{}",
                            hex::encode(code_hash)
                        )
                    })?;

                let binary =
                    fetch_output_data_by_outpoint(&tx_hash, output_index, &self.rpc_url)
                        .await
                        .with_context(|| {
                            format!(
                                "failed to fetch decoder binary via resolved code cell: code_hash=0x{}, tx_hash=0x{}, output_index={}",
                                hex::encode(code_hash),
                                hex::encode(&tx_hash),
                                output_index
                            )
                        })?;

                verify_blake2b_hash(&binary, code_hash).with_context(|| {
                    format!(
                        "resolved decoder binary hash mismatch: code_hash=0x{}, tx_hash=0x{}, output_index={}",
                        hex::encode(code_hash),
                        hex::encode(&tx_hash),
                        output_index
                    )
                })?;

                self.decoder_cache
                    .put(&cache_key, &binary)
                    .context("failed to cache resolved decoder binary")?;

                Ok(binary)
            }
            DecoderRef::TypeId(_) => {
                fetch_decoder_binary(decoder_ref, &self.rpc_url, &self.decoder_cache).await
            }
        }
    }

    /// After decoding, update the spore's media profile with newly discovered
    /// media sources from the decoded traits.
    fn update_spore_media_profile(
        &self,
        spore_id: &[u8],
        new_sources: &[SporeMediaSource],
    ) -> Result<()> {
        let mut entry = self.store.get_spore(spore_id)?.with_context(|| {
            format!(
                "spore entry not found when updating media profile: spore_id=0x{}",
                hex::encode(spore_id)
            )
        })?;

        let (old_tier, new_tier) = if let ObjectExtra::Spore {
            ref mut media_profile,
            ..
        } = entry.extra
        {
            let old_tier = media_profile.tier;
            merge_media_sources(media_profile, new_sources);
            (old_tier, media_profile.tier)
        } else {
            // Not a spore entry — this shouldn't happen since we only decode DOB spores,
            // but log and return without error.
            warn!(
                spore_id = hex::encode(spore_id),
                "expected Spore extra when updating media profile"
            );
            return Ok(());
        };

        let mut batch = StoreBatch::new(&self.store);
        if old_tier != new_tier {
            self.sync_cluster_aggregate_for_media_tier_change(
                spore_id, &entry, old_tier, new_tier, &mut batch,
            )?;
        }
        batch.put_spore(spore_id, &entry);
        batch.commit()?;
        Ok(())
    }

    fn sync_cluster_aggregate_for_media_tier_change(
        &self,
        spore_id: &[u8],
        entry: &ObjectEntry,
        old_tier: CompositionTier,
        new_tier: CompositionTier,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if !entry.is_live {
            return Ok(());
        }

        let cluster_id = entry.collection_id.as_deref().with_context(|| {
            format!(
                "live spore missing collection_id during DOB media profile update: spore_id=0x{}",
                hex::encode(spore_id)
            )
        })?;
        let mut agg = self
            .store
            .get_cluster_aggregate(cluster_id)?
            .with_context(|| {
                format!(
                    "missing cluster aggregate during DOB media profile update: spore_id=0x{}, cluster_id=0x{}",
                    hex::encode(spore_id),
                    hex::encode(cluster_id)
                )
            })?;

        self.adjust_cluster_tier_count(
            cluster_id,
            &mut agg,
            old_tier,
            -1,
            "dob decode media profile update",
        )?;
        self.adjust_cluster_tier_count(
            cluster_id,
            &mut agg,
            new_tier,
            1,
            "dob decode media profile update",
        )?;
        batch.put_cluster_aggregate(cluster_id, &agg);
        Ok(())
    }

    fn adjust_cluster_tier_count(
        &self,
        cluster_id: &[u8],
        agg: &mut ClusterAggregate,
        tier: CompositionTier,
        delta: i64,
        context: &str,
    ) -> Result<()> {
        if delta == 0 {
            return Ok(());
        }

        let slot = match tier {
            CompositionTier::PureCkb => &mut agg.pure_ckb_count,
            CompositionTier::BtcCkb => &mut agg.btc_ckb_count,
            CompositionTier::DecentralizedMixture => &mut agg.decentralized_mixture_count,
            CompositionTier::CentralizedMixture => &mut agg.centralized_mixture_count,
            CompositionTier::Unknown => &mut agg.unknown_count,
        };
        let next = slot.checked_add(delta).ok_or_else(|| {
            anyhow::anyhow!(
                "cluster tier count overflow during DOB media profile update: cluster_id=0x{}, tier={}, current={}, delta={}, context={}",
                hex::encode(cluster_id),
                tier.as_str(),
                *slot,
                delta,
                context
            )
        })?;
        if next < 0 {
            bail!(
                "cluster tier count underflow during DOB media profile update: cluster_id=0x{}, tier={}, current={}, delta={}, context={}",
                hex::encode(cluster_id),
                tier.as_str(),
                *slot,
                delta,
                context
            );
        }
        *slot = next;
        Ok(())
    }
}

/// Extract DNA hex from a spore's on-chain cell content.
///
/// 1. Look up the spore's outpoint (tx_hash, output_index) from the domain store.
/// 2. Fetch the creation transaction via CKB RPC to obtain raw output data.
/// 3. Parse the Spore molecule to extract the content field.
/// 4. Decode the content as text and extract the DNA hex string.
async fn extract_dna_from_spore(
    spore_id: &[u8],
    store: &CkbadgerStore,
    rpc_url: &str,
) -> Result<String> {
    // Step 1: Find the spore's outpoint (most recent first).
    let outpoints = store
        .list_spore_outpoints_by_spore_id(spore_id)
        .with_context(|| {
            format!(
                "failed to list outpoints for spore_id=0x{}",
                hex::encode(spore_id)
            )
        })?;

    let (tx_hash, output_index) = outpoints
        .first()
        .with_context(|| format!("no outpoint found for spore_id=0x{}", hex::encode(spore_id)))?;

    // Step 2: Fetch the spore cell data from CKB node via RPC.
    let output_data = fetch_output_data_by_outpoint(tx_hash, *output_index, rpc_url)
        .await
        .with_context(|| {
            format!(
                "failed to fetch spore cell data via RPC: spore_id=0x{}, tx_hash=0x{}, output_index={}",
                hex::encode(spore_id),
                hex::encode(tx_hash),
                output_index
            )
        })?;

    // Step 3: Parse the Spore molecule to extract content bytes.
    let content_bytes =
        SporeParser::parse_spore_content_from_data(&output_data).with_context(|| {
            format!(
                "failed to parse Spore molecule content: spore_id=0x{}, tx_hash=0x{}",
                hex::encode(spore_id),
                hex::encode(tx_hash)
            )
        })?;

    // Step 4: Convert content to text and extract DNA hex.
    let content_text = String::from_utf8_lossy(&content_bytes);
    parse_dna_hex_from_content_text(&content_text).with_context(|| {
        format!(
            "failed to extract DNA hex from spore content: spore_id=0x{}",
            hex::encode(spore_id)
        )
    })
}

async fn fetch_output_data_by_outpoint(
    tx_hash: &[u8],
    output_index: i16,
    rpc_url: &str,
) -> Result<Vec<u8>> {
    let tx_hash_hex = format!("0x{}", hex::encode(tx_hash));
    let rpc = CkbRpcClient::new(rpc_url);
    let tx_with_status = rpc
        .get_transaction(&tx_hash_hex)
        .await
        .with_context(|| format!("RPC get_transaction failed for tx_hash={}", tx_hash_hex))?
        .with_context(|| format!("transaction not found via RPC: tx_hash={}", tx_hash_hex))?;

    let tx = tx_with_status.transaction.with_context(|| {
        format!(
            "transaction view missing in RPC response: tx_hash={}",
            tx_hash_hex
        )
    })?;

    let idx = usize::try_from(output_index).with_context(|| {
        format!(
            "negative output_index in outpoint lookup: tx_hash={}, output_index={}",
            tx_hash_hex, output_index
        )
    })?;
    let output_data_hex = tx.outputs_data.get(idx).with_context(|| {
        format!(
            "output_index {} out of range (outputs_data len={}): tx_hash={}",
            idx,
            tx.outputs_data.len(),
            tx_hash_hex
        )
    })?;

    Ok(parse_hex_to_bytes(output_data_hex))
}

fn verify_blake2b_hash(data: &[u8], expected_hash: &[u8]) -> Result<()> {
    let mut hasher = ckb_hash::new_blake2b();
    hasher.update(data);
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);

    if hash.as_slice() != expected_hash {
        bail!(
            "blake2b mismatch: expected {}, got {}",
            hex::encode(expected_hash),
            hex::encode(hash)
        );
    }

    Ok(())
}

/// Parse the DOB version from a content type string like "dob/0" or "dob/1".
fn parse_dob_version(content_type: &str) -> u32 {
    let normalized = content_type.trim().to_ascii_lowercase();
    if let Some(rest) = normalized.strip_prefix("dob/") {
        // Take just the version number, ignoring any parameters after ";"
        let version_str = rest.split(';').next().unwrap_or("0").trim();
        version_str.parse().unwrap_or(0)
    } else {
        0
    }
}

/// Parse a DecoderRef from the DOB metadata's decoder entry.
///
/// Supports two formats:
/// - `{"dob": {"decoders": [{"type": "code_hash", "hash": "0x..."}]}}`
/// - `{"dob": {"decoder": {"type": "type_id", "hash": "0x..."}}}`
fn parse_decoder_ref(dob: &Value) -> Result<DecoderRef> {
    // Try decoders array first (DOB/1 and newer DOB/0)
    if let Some(decoders) = dob.get("decoders").and_then(|v| v.as_array()) {
        if let Some(first) = decoders.first() {
            return parse_single_decoder_ref(first);
        }
    }

    // Try singular decoder field (older DOB/0)
    if let Some(decoder) = dob.get("decoder") {
        return parse_single_decoder_ref(decoder);
    }

    bail!("no decoder reference found in DOB metadata")
}

/// Parse a single decoder reference object.
fn parse_single_decoder_ref(decoder: &Value) -> Result<DecoderRef> {
    let decoder_type = decoder
        .get("type")
        .and_then(|v| v.as_str())
        .context("decoder entry missing 'type' field")?;

    let hash_hex = decoder
        .get("hash")
        .and_then(|v| v.as_str())
        .context("decoder entry missing 'hash' field")?;

    let hash_bytes = decode_hex_field(hash_hex).context("invalid decoder hash hex")?;

    match decoder_type {
        "code_hash" => Ok(DecoderRef::CodeHash(hash_bytes)),
        "type_id" => Ok(DecoderRef::TypeId(hash_bytes)),
        other => bail!("unknown decoder type: {other}"),
    }
}

/// Extract the pattern JSON string from DOB metadata.
///
/// Looks for the first decoder's `pattern` field and serializes it to JSON.
fn extract_pattern_json(dob: &Value) -> Result<String> {
    // Try decoders array
    if let Some(decoders) = dob.get("decoders").and_then(|v| v.as_array()) {
        if let Some(first) = decoders.first() {
            if let Some(pattern) = first.get("pattern") {
                return serde_json::to_string(pattern).context("failed to serialize pattern");
            }
        }
    }

    // Try top-level pattern (DOB/0 ver=0)
    if let Some(pattern) = dob.get("pattern") {
        return serde_json::to_string(pattern).context("failed to serialize pattern");
    }

    bail!("no pattern found in DOB metadata")
}

/// Decode a "0x"-prefixed or bare hex string into bytes.
fn decode_hex_field(hex_str: &str) -> Result<Vec<u8>> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(stripped).context("failed to decode hex")
}

/// Format a serde_json::Value as a display string for storage.
fn format_trait_value(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(s) => s.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

/// Scan decoded trait values for URI schemes and extract media sources.
fn extract_media_sources_from_traits(traits: &[DobTrait]) -> Vec<SporeMediaSource> {
    let mut sources = Vec::new();
    for t in traits {
        let text = format_trait_value(&t.value);
        extract_uri_sources(&text, "dob_decoded_trait", &mut sources);
        if sources.len() >= MAX_MEDIA_SOURCES {
            break;
        }
    }
    // Deduplicate by URI
    let mut seen = std::collections::HashSet::new();
    sources.retain(|s| seen.insert(s.uri.clone()));
    sources.truncate(MAX_MEDIA_SOURCES);
    sources
}

/// Merge newly discovered media sources into an existing media profile,
/// avoiding duplicates, and recalculate the storage dependency tier.
fn merge_media_sources(profile: &mut SporeMediaProfile, new_sources: &[SporeMediaSource]) {
    let existing_uris: std::collections::HashSet<String> =
        profile.sources.iter().map(|s| s.uri.clone()).collect();

    let mut added = false;
    for source in new_sources {
        if !existing_uris.contains(&source.uri) && profile.sources.len() < MAX_MEDIA_SOURCES {
            profile.sources.push(source.clone());
            added = true;
        }
    }

    if added {
        // Recalculate tier from merged sources
        profile.tier = resolve_tier(&profile.sources);

        // Update has_renderable_image if any new source looks like an image
        if !profile.has_renderable_image {
            profile.has_renderable_image = profile.sources.iter().any(|s| uri_seems_image(&s.uri));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_parse_dob_version() {
        assert_eq!(parse_dob_version("dob/0"), 0);
        assert_eq!(parse_dob_version("dob/1"), 1);
        assert_eq!(parse_dob_version("DOB/0"), 0);
        assert_eq!(parse_dob_version("dob/0;charset=utf-8"), 0);
        assert_eq!(parse_dob_version("dob/1;charset=utf-8"), 1);
        assert_eq!(parse_dob_version("text/plain"), 0);
    }

    #[test]
    fn test_parse_decoder_ref_code_hash() {
        let dob = json!({
            "decoders": [{
                "type": "code_hash",
                "hash": "0xabcd",
                "pattern": []
            }]
        });
        let decoder_ref = parse_decoder_ref(&dob).unwrap();
        match decoder_ref {
            DecoderRef::CodeHash(hash) => assert_eq!(hash, vec![0xAB, 0xCD]),
            _ => panic!("expected CodeHash"),
        }
    }

    #[test]
    fn test_parse_decoder_ref_type_id() {
        let dob = json!({
            "decoders": [{
                "type": "type_id",
                "hash": "0x1234",
                "pattern": []
            }]
        });
        let decoder_ref = parse_decoder_ref(&dob).unwrap();
        match decoder_ref {
            DecoderRef::TypeId(hash) => assert_eq!(hash, vec![0x12, 0x34]),
            _ => panic!("expected TypeId"),
        }
    }

    #[test]
    fn test_parse_decoder_ref_singular_decoder() {
        let dob = json!({
            "decoder": {
                "type": "code_hash",
                "hash": "0xff"
            }
        });
        let decoder_ref = parse_decoder_ref(&dob).unwrap();
        match decoder_ref {
            DecoderRef::CodeHash(hash) => assert_eq!(hash, vec![0xFF]),
            _ => panic!("expected CodeHash"),
        }
    }

    #[test]
    fn test_parse_decoder_ref_missing() {
        let dob = json!({"ver": 0});
        let err = parse_decoder_ref(&dob).unwrap_err();
        assert!(err.to_string().contains("no decoder reference found"));
    }

    #[test]
    fn test_parse_decoder_ref_unknown_type() {
        let dob = json!({
            "decoders": [{
                "type": "unknown",
                "hash": "0xab"
            }]
        });
        let err = parse_decoder_ref(&dob).unwrap_err();
        assert!(err.to_string().contains("unknown decoder type"));
    }

    #[test]
    fn test_extract_pattern_json_from_decoders() {
        let dob = json!({
            "decoders": [{
                "type": "code_hash",
                "hash": "0xab",
                "pattern": [["Background", "String", 0, 2, "options", ["Red", "Blue"]]]
            }]
        });
        let pattern = extract_pattern_json(&dob).unwrap();
        assert!(pattern.contains("Background"));
    }

    #[test]
    fn test_extract_pattern_json_from_top_level() {
        let dob = json!({
            "ver": 0,
            "pattern": [["Color", "String", 0, 1, "options", ["A", "B"]]]
        });
        let pattern = extract_pattern_json(&dob).unwrap();
        assert!(pattern.contains("Color"));
    }

    #[test]
    fn test_extract_pattern_json_missing() {
        let dob = json!({"ver": 0});
        let err = extract_pattern_json(&dob).unwrap_err();
        assert!(err.to_string().contains("no pattern found"));
    }

    #[test]
    fn test_format_trait_value() {
        assert_eq!(format_trait_value(&Value::Null), "-");
        assert_eq!(
            format_trait_value(&Value::String("hello".to_string())),
            "hello"
        );
        assert_eq!(format_trait_value(&Value::Bool(true)), "true");
        assert_eq!(format_trait_value(&json!(42)), "42");
        assert_eq!(format_trait_value(&json!({"a": 1})), r#"{"a":1}"#);
    }

    #[test]
    fn test_extract_media_sources_from_traits() {
        let traits = vec![
            DobTrait {
                name: "Background".to_string(),
                value: Value::String("btcfs://abc123/image.png".to_string()),
                type_tag: "String".to_string(),
            },
            DobTrait {
                name: "Color".to_string(),
                value: Value::String("Red".to_string()),
                type_tag: "String".to_string(),
            },
            DobTrait {
                name: "Avatar".to_string(),
                value: Value::String("ipfs://QmTest123".to_string()),
                type_tag: "String".to_string(),
            },
        ];
        let sources = extract_media_sources_from_traits(&traits);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].scheme, "btcfs");
        assert_eq!(sources[1].scheme, "ipfs");
    }

    #[test]
    fn test_extract_media_sources_deduplicates() {
        let traits = vec![
            DobTrait {
                name: "A".to_string(),
                value: Value::String("ipfs://QmSame".to_string()),
                type_tag: "String".to_string(),
            },
            DobTrait {
                name: "B".to_string(),
                value: Value::String("ipfs://QmSame".to_string()),
                type_tag: "String".to_string(),
            },
        ];
        let sources = extract_media_sources_from_traits(&traits);
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn test_merge_media_sources_no_duplicates() {
        let mut profile = SporeMediaProfile {
            tier: ckbadger_store::types::CompositionTier::PureCkb,
            sources: vec![SporeMediaSource {
                uri: "ipfs://existing".to_string(),
                scheme: "ipfs".to_string(),
                source_location: "payload_text".to_string(),
                dependency_tier: ckbadger_store::types::CompositionTier::DecentralizedMixture,
            }],
            has_renderable_image: false,
            issues: vec![],
        };
        let new_sources = vec![
            SporeMediaSource {
                uri: "ipfs://existing".to_string(), // duplicate
                scheme: "ipfs".to_string(),
                source_location: "dob_decoded_trait".to_string(),
                dependency_tier: ckbadger_store::types::CompositionTier::DecentralizedMixture,
            },
            SporeMediaSource {
                uri: "btcfs://new".to_string(),
                scheme: "btcfs".to_string(),
                source_location: "dob_decoded_trait".to_string(),
                dependency_tier: ckbadger_store::types::CompositionTier::BtcCkb,
            },
        ];
        merge_media_sources(&mut profile, &new_sources);
        assert_eq!(profile.sources.len(), 2); // existing + one new
        assert_eq!(profile.sources[1].uri, "btcfs://new");
    }

    #[test]
    fn test_merge_media_sources_recalculates_tier() {
        let mut profile = SporeMediaProfile {
            tier: ckbadger_store::types::CompositionTier::PureCkb,
            sources: vec![],
            has_renderable_image: false,
            issues: vec![],
        };
        let new_sources = vec![SporeMediaSource {
            uri: "https://example.com/image.png".to_string(),
            scheme: "https".to_string(),
            source_location: "dob_decoded_trait".to_string(),
            dependency_tier: ckbadger_store::types::CompositionTier::CentralizedMixture,
        }];
        merge_media_sources(&mut profile, &new_sources);
        assert_eq!(
            profile.tier,
            ckbadger_store::types::CompositionTier::CentralizedMixture
        );
        assert!(profile.has_renderable_image); // .png extension
    }

    #[test]
    fn test_update_spore_media_profile_updates_cluster_aggregate_tier_counts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let cache_dir = dir.path().join("decoder-cache");
        let decoder_cache = Arc::new(DecoderBinaryCache::new(&cache_dir).unwrap());
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker = DobDecodeWorker::new(
            store.clone(),
            store.clone(),
            decoder_cache,
            "http://localhost:9999".to_string(),
            shutdown,
        );

        let cluster_id = vec![0x11; 32];
        let spore_id = [0x22u8; 32];
        let spore_entry = ckbadger_store::types::ObjectEntry {
            standard: ckbadger_store::types::ObjectStandard::Spore,
            collection_id: Some(cluster_id.clone()),
            token_id: None,
            owner_lock_hash: Some(vec![0x33; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 42,
            created_at_tx: vec![0x44; 32],
            extra: ckbadger_store::types::ObjectExtra::Spore {
                content_type: "dob/0".to_string(),
                content_length: 3,
                media_profile: SporeMediaProfile {
                    tier: ckbadger_store::types::CompositionTier::PureCkb,
                    sources: vec![SporeMediaSource {
                        uri: "ckbfs://existing-cell".to_string(),
                        scheme: "ckbfs".to_string(),
                        source_location: "payload_text".to_string(),
                        dependency_tier: ckbadger_store::types::CompositionTier::PureCkb,
                    }],
                    has_renderable_image: false,
                    issues: vec![],
                },
            },
        };
        store.put_spore_direct(&spore_id, &spore_entry).unwrap();

        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_cluster_aggregate(
            &cluster_id,
            &ckbadger_store::types::ClusterAggregate {
                total_count: 1,
                live_count: 1,
                owner_count: 1,
                pure_ckb_count: 1,
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        let new_sources = vec![SporeMediaSource {
            uri: "btcfs://inscription123i0".to_string(),
            scheme: "btcfs".to_string(),
            source_location: "dob_decoded_trait".to_string(),
            dependency_tier: ckbadger_store::types::CompositionTier::BtcCkb,
        }];

        worker
            .update_spore_media_profile(&spore_id, &new_sources)
            .unwrap();

        let updated_spore = store.get_spore(&spore_id).unwrap().unwrap();
        match updated_spore.extra {
            ObjectExtra::Spore { media_profile, .. } => {
                assert_eq!(
                    media_profile.tier,
                    ckbadger_store::types::CompositionTier::BtcCkb
                );
            }
            other => panic!("expected spore extra, got {other:?}"),
        }

        let updated_agg = store.get_cluster_aggregate(&cluster_id).unwrap().unwrap();
        assert_eq!(updated_agg.pure_ckb_count, 0);
        assert_eq!(updated_agg.btc_ckb_count, 1);
        assert_eq!(updated_agg.live_count, 1);
        assert_eq!(updated_agg.total_count, 1);
    }

    #[test]
    fn test_decode_hex_field() {
        assert_eq!(decode_hex_field("0xabcd").unwrap(), vec![0xAB, 0xCD]);
        assert_eq!(decode_hex_field("abcd").unwrap(), vec![0xAB, 0xCD]);
        assert!(decode_hex_field("0xZZZZ").is_err());
    }

    #[tokio::test]
    async fn test_extract_dna_from_spore_no_outpoints() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let spore_id = [0x11u8; 32];
        let result = extract_dna_from_spore(&spore_id, &store, "http://localhost:9999").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no outpoint found"),
            "should fail when no outpoints exist for the spore"
        );
    }

    #[tokio::test]
    async fn test_load_decoder_binary_resolves_code_hash_via_local_data_hash_index() {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_only_dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();

        let store = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let append_only_store =
            Arc::new(CkbadgerStore::open_append_only(append_only_dir.path()).unwrap());
        let decoder_cache = Arc::new(DecoderBinaryCache::new(cache_dir.path()).unwrap());
        let shutdown = Arc::new(AtomicBool::new(false));

        let decoder_binary = b"test decoder binary".to_vec();
        let mut hasher = ckb_hash::new_blake2b();
        hasher.update(&decoder_binary);
        let mut expected_hash = [0u8; 32];
        hasher.finalize(&mut expected_hash);
        let code_hash = expected_hash.to_vec();

        let tx_hash = vec![0xAB; 32];
        let output_index: i16 = 1;
        let created_at_block = 42;
        let cell_info = ckbadger_store::types::LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: decoder_binary.len() as i32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(code_hash.clone()),
        };

        let mut append_batch = StoreBatch::new(append_only_store.as_ref());
        append_batch.put_cell_payload_by_outpoint(&tx_hash, output_index, &cell_info);
        append_batch.commit().unwrap();

        let mut domain_batch = StoreBatch::new(store.as_ref());
        domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, output_index, created_at_block);
        domain_batch.put_cell_by_data_hash(&code_hash, created_at_block, &tx_hash, output_index);
        domain_batch.commit().unwrap();

        let server = MockServer::start().await;
        let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));
        let dummy_lock = json!({
            "code_hash": format!("0x{}", "11".repeat(32)),
            "hash_type": "type",
            "args": "0x"
        });
        Mock::given(method("POST"))
            .and(body_partial_json(json!({
                "method": "get_transaction",
                "params": [tx_hash_hex.clone()]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "transaction": {
                        "hash": tx_hash_hex,
                        "version": "0x0",
                        "cell_deps": [],
                        "header_deps": [],
                        "inputs": [],
                        "outputs": [
                            {
                                "capacity": "0x0",
                                "lock": dummy_lock.clone(),
                                "type": null
                            },
                            {
                                "capacity": "0x0",
                                "lock": dummy_lock,
                                "type": null
                            }
                        ],
                        "outputs_data": [
                            "0x",
                            format!("0x{}", hex::encode(&decoder_binary))
                        ],
                        "witnesses": []
                    },
                    "tx_status": {
                        "status": "committed",
                        "block_hash": null,
                        "block_number": null
                    }
                }
            })))
            .mount(&server)
            .await;

        let worker = DobDecodeWorker::new(
            store,
            append_only_store,
            decoder_cache.clone(),
            server.uri(),
            shutdown,
        );

        let loaded = worker
            .load_decoder_binary(&DecoderRef::CodeHash(code_hash.clone()))
            .await
            .unwrap();
        assert_eq!(loaded, decoder_binary);

        let cache_key = DecoderBinaryCache::code_hash_key(&code_hash);
        assert_eq!(decoder_cache.get(&cache_key).unwrap(), decoder_binary);
    }
}
