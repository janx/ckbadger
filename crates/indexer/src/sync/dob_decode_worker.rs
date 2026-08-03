//! Background worker that batch-decodes DOB spores using CKB-VM.
//!
//! After sync catches up, this worker iterates over all spore entries with
//! `dob/*` content types that lack a cached decode result, fetches the
//! decoder binary from chain, executes it in CKB-VM, and writes the
//! decoded traits + media sources into `CF_DOB_DECODED`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::stream::{self, StreamExt};
use serde_json::Value;
use tracing::{debug, info, warn};

use ckbadger_dob_decoder::cache::DecoderBinaryCache;
use ckbadger_dob_decoder::types::{DecoderRef, DobTrait};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    ClusterAggregate, CompositionTier, DobDecodedEntry, DobDecodedStep, DobDecodedTrait,
    ObjectEntry, ObjectExtra, SporeMediaProfile, SporeMediaSource, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;

use crate::media_store::{sniff_media_type, MediaBlobStore};

use crate::parser::media_source::{extract_uri_sources, parse_dna_hex_from_content, resolve_tier};
use crate::parser::spore::SporeParser;
use crate::rpc::{parse_hex_to_bytes, CkbRpcClient};
use crate::sync::dob_decode_error::DobDecodeError;

const BATCH_SIZE: usize = 500;
const MAX_MEDIA_SOURCES: usize = 24;
const DECODE_CONCURRENCY: usize = 8;

/// Background worker that decodes DOB spores via CKB-VM after sync catches up.
pub struct DobDecodeWorker {
    /// Domain store — reads spore/cluster entries and writes decode results.
    store: Arc<CkbadgerStore>,
    /// Append-only store — provides decoder code cell payload metadata.
    append_only_store: Arc<CkbadgerStore>,
    /// Disk cache for decoder RISC-V binaries.
    decoder_cache: Arc<DecoderBinaryCache>,
    /// Content-addressed blob store for decoded media files.
    media_store: Arc<MediaBlobStore>,
    /// Reusable RPC client for CKB node calls (connection-pooled).
    rpc_client: CkbRpcClient,
    /// Cooperative shutdown flag.
    shutdown: Arc<AtomicBool>,
}

impl DobDecodeWorker {
    pub fn new(
        store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
        decoder_cache: Arc<DecoderBinaryCache>,
        dob_decode_dir: PathBuf,
        rpc_url: String,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let rpc_client = CkbRpcClient::new(&rpc_url);
        let media_store = Arc::new(MediaBlobStore::new(dob_decode_dir));
        Self {
            store,
            append_only_store,
            decoder_cache,
            media_store,
            rpc_client,
            shutdown,
        }
    }

    /// Main loop: paginate through undecoded DOB spores and decode each one.
    pub async fn run(&self) -> Result<()> {
        info!("DOB decode worker started");

        let start = std::time::Instant::now();

        self.store.update_background_task("dob_decode", |entry| {
            entry.state = ckbadger_common::BackgroundTaskState::Running;
            entry.started_at = Some(chrono::Utc::now().timestamp());
            entry.progress_current = Some(0);
            entry.progress_total = None;
            entry.message = Some("Starting DOB decode".to_string());
        })?;

        let mut cursor: Option<Vec<u8>> = None;
        let mut total_decoded: u64 = 0;
        let mut total_skipped: u64 = 0;
        // Deterministic failures actually persisted as `Failed` (a subset of
        // `total_skipped`; the remainder are transient skips kept for retry).
        let mut total_recorded: u64 = 0;

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

            // Advance cursor before consuming the batch
            if let Some((last_key, _, _)) = batch_entries.last() {
                cursor = Some(last_key.clone());
            }

            // Decode spores concurrently within the batch
            let ctx = Arc::new(DecodeContext {
                store: Arc::clone(&self.store),
                append_only_store: Arc::clone(&self.append_only_store),
                decoder_cache: Arc::clone(&self.decoder_cache),
                media_store: Arc::clone(&self.media_store),
                rpc_client: self.rpc_client.clone(),
            });

            let decode_futures =
                batch_entries
                    .into_iter()
                    .map(|(spore_id, content_type, collection_id)| {
                        let ctx = Arc::clone(&ctx);

                        async move {
                            let result = decode_single_spore(
                                &spore_id,
                                &content_type,
                                collection_id.as_deref(),
                                &ctx,
                            )
                            .await;
                            (spore_id, result)
                        }
                    });

            let results: Vec<_> = stream::iter(decode_futures)
                .buffer_unordered(DECODE_CONCURRENCY)
                .collect()
                .await;

            // Separate successes from failures
            let mut decoded_results: Vec<(Vec<u8>, DobDecodedEntry)> = Vec::new();
            let mut failed_records: Vec<(Vec<u8>, ckbadger_store::types::DobDecodeFailure)> =
                Vec::new();
            let mut batch_skipped: u64 = 0;

            for (spore_id, result) in results {
                match result {
                    Ok(entry) => {
                        debug!(
                            spore_id = hex::encode(&spore_id),
                            steps = entry.steps.len(),
                            media_sources = entry.media_sources.len(),
                            "decoded DOB spore"
                        );
                        decoded_results.push((spore_id, entry));
                    }
                    Err(e) => {
                        batch_skipped += 1;
                        if e.is_transient() {
                            // Not a data problem — keep it retryable (no record).
                            warn!(
                                spore_id = hex::encode(&spore_id),
                                error = %e,
                                "skipping DOB spore decode (transient — will retry)"
                            );
                        } else {
                            // Deterministic — record once so it is not re-attempted.
                            debug!(
                                spore_id = hex::encode(&spore_id),
                                error = %e,
                                "recording un-decodable DOB spore"
                            );
                            failed_records.push((
                                spore_id,
                                ckbadger_store::types::DobDecodeFailure {
                                    category: e.category(),
                                    message: e.to_string(),
                                    failed_at: chrono::Utc::now().timestamp(),
                                },
                            ));
                        }
                    }
                }
            }

            // Write decoded results per-spore: update media profile first, then
            // mark as decoded in the same commit. This ensures a failed profile
            // update does not leave the spore permanently marked decoded with
            // stale media_profile (list_undecoded_dob_spores would never retry).
            let mut batch_committed: u64 = 0;
            for (spore_id, entry) in &decoded_results {
                if !entry.media_sources.is_empty() {
                    if let Err(e) = self.update_spore_media_profile(spore_id, &entry.media_sources)
                    {
                        warn!(
                            spore_id = hex::encode(spore_id),
                            error = %e,
                            "failed to update spore media profile after DOB decode — \
                             skipping put_dob_decoded so it will be retried"
                        );
                        batch_skipped += 1;
                        continue;
                    }
                }
                let mut store_batch = StoreBatch::new(&self.store);
                store_batch.put_dob_decoded(spore_id, entry);
                store_batch.commit()?;
                batch_committed += 1;
            }

            // Persist deterministic failures per-spore so they are not
            // re-listed for decode. Transient failures were dropped above and
            // remain in list_undecoded_dob_spores to retry next run.
            for (spore_id, failure) in &failed_records {
                let mut store_batch = StoreBatch::new(&self.store);
                store_batch.put_dob_decode_failure(spore_id, failure);
                store_batch.commit()?;
            }

            total_decoded += batch_committed;
            total_skipped += batch_skipped;
            total_recorded += failed_records.len() as u64;

            // Update progress at batch boundary
            let elapsed = start.elapsed();
            let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
            let processed = total_decoded + total_skipped;
            let rate = if elapsed.as_secs_f64() > 0.0 {
                total_decoded as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let _ = self.store.update_background_task("dob_decode", |entry| {
                entry.progress_current = Some(processed);
                entry.elapsed_ms = Some(elapsed_ms);
                entry.rate = Some(rate);
                entry.eta_seconds = None;
                entry.message = Some(format!(
                    "Decoded {}, recorded {} un-decodable, skipped {}",
                    total_decoded, total_recorded, total_skipped
                ));
            });

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
                "Done: {} decoded, {} recorded un-decodable, {} skipped",
                total_decoded, total_recorded, total_skipped
            ));
        });

        Ok(())
    }

    /// After decoding, update the spore's media profile with newly discovered
    /// media sources from the decoded traits.
    fn update_spore_media_profile(
        &self,
        spore_id: &[u8],
        new_sources: &[SporeMediaSource],
    ) -> Result<()> {
        let entry = self.store.get_spore(spore_id)?.with_context(|| {
            format!(
                "spore entry not found when updating media profile: spore_id=0x{}",
                hex::encode(spore_id)
            )
        })?;

        // Compute the new media_profile in isolation without mutating the entry.
        // We'll apply the result to a freshly-read entry below to avoid clobbering
        // concurrent canonical state changes (is_live, owner_lock_hash) made by
        // the live sync writer between our read and write.
        let (old_tier, new_media_profile) = if let ObjectExtra::Spore {
            ref media_profile, ..
        } = entry.extra
        {
            let old_tier = media_profile.tier;
            let mut mp = media_profile.clone();
            merge_media_sources(&mut mp, new_sources);
            (old_tier, mp)
        } else {
            warn!(
                spore_id = hex::encode(spore_id),
                "expected Spore extra when updating media profile"
            );
            return Ok(());
        };
        let new_tier = new_media_profile.tier;

        // Re-read the entry to get the latest canonical state (is_live,
        // owner_lock_hash, etc.) and apply only the media_profile change.
        // This prevents overwriting concurrent consume/transfer updates.
        let mut fresh_entry = self.store.get_spore(spore_id)?.with_context(|| {
            format!(
                "spore entry vanished during media profile update: spore_id=0x{}",
                hex::encode(spore_id)
            )
        })?;

        if let ObjectExtra::Spore {
            ref mut media_profile,
            ..
        } = fresh_entry.extra
        {
            *media_profile = new_media_profile;
        }

        let mut batch = StoreBatch::new(&self.store);
        if old_tier != new_tier {
            // Use fresh_entry.is_live for the cluster aggregate decision so we
            // don't incorrectly update aggregates for already-consumed spores.
            self.sync_cluster_aggregate_for_media_tier_change(
                spore_id,
                &fresh_entry,
                old_tier,
                new_tier,
                &mut batch,
            )?;
        }
        batch.put_spore(spore_id, &fresh_entry);
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

/// Shared resources for concurrent decode tasks.
struct DecodeContext {
    store: Arc<CkbadgerStore>,
    append_only_store: Arc<CkbadgerStore>,
    decoder_cache: Arc<DecoderBinaryCache>,
    media_store: Arc<MediaBlobStore>,
    rpc_client: CkbRpcClient,
}

#[derive(Debug, Clone)]
struct DecoderStep {
    decoder_ref: DecoderRef,
    pattern_json: String,
}

#[derive(Debug, Clone)]
struct ResolvedDecoderStep {
    binary: Vec<u8>,
    pattern_json: String,
}

/// Decode a single DOB spore. Standalone function for concurrent use.
///
/// Loads cluster metadata, fetches the decoder binary, and executes it
/// in CKB-VM on a blocking thread.
async fn decode_single_spore(
    spore_id: &[u8],
    content_type: &str,
    collection_id: Option<&[u8]>,
    ctx: &DecodeContext,
) -> Result<DobDecodedEntry, DobDecodeError> {
    let cluster_id = collection_id.ok_or_else(|| DobDecodeError::ClusterMetadataInvalid {
        detail: "DOB spore has no collection_id — cannot resolve cluster metadata".to_string(),
    })?;

    // Clusterless "Sole Spores": the indexer assigns this sentinel to spores with
    // no real cluster. There is no DOB cluster/decoder to decode — short-circuit
    // with a precise reason instead of a doomed get_spore(sentinel) lookup.
    if cluster_id == SOLE_SPORES_SENTINEL_COLLECTION {
        return Err(DobDecodeError::Clusterless);
    }

    let cluster_entry = ctx
        .store
        .get_spore(cluster_id)
        .map_err(DobDecodeError::Internal)?
        .ok_or_else(|| DobDecodeError::ClusterNotFound {
            cluster_id: cluster_id.to_vec(),
        })?;

    let cluster_description = cluster_entry.description.as_deref().ok_or_else(|| {
        DobDecodeError::ClusterMetadataInvalid {
            detail: "cluster entry has no description".to_string(),
        }
    })?;

    let metadata: Value = serde_json::from_str(cluster_description).map_err(|e| {
        DobDecodeError::ClusterMetadataInvalid {
            detail: format!("cluster description is not valid JSON: {e}"),
        }
    })?;

    let dob_obj = metadata
        .get("dob")
        .ok_or_else(|| DobDecodeError::ClusterMetadataInvalid {
            detail: "cluster metadata missing 'dob' field".to_string(),
        })?;

    // Cluster metadata `dob.ver` is the dispatch authority for the protocol
    // version (official server: types.rs unbox_dob). The spore's content_type
    // serves only as a consistency cross-check, never as the dispatch source.
    let dob_version = parse_dob_version_from_cluster(dob_obj).map_err(|e| {
        DobDecodeError::ClusterMetadataInvalid {
            detail: e.to_string(),
        }
    })?;
    if parse_dob_version_from_content_type(content_type) != Some(dob_version.number()) {
        warn!(
            spore_id = hex::encode(spore_id),
            content_type,
            cluster_ver = dob_version.number(),
            "spore content_type version differs from cluster dob.ver — \
             cluster metadata is authoritative"
        );
    }

    let dna_hex = extract_dna_from_spore(spore_id, &ctx.store, &ctx.rpc_client).await?;

    let decoder_steps =
        parse_decoder_steps(dob_obj).map_err(|e| DobDecodeError::ClusterMetadataInvalid {
            detail: e.to_string(),
        })?;
    let mut resolved_steps = Vec::with_capacity(decoder_steps.len());
    for step in decoder_steps {
        let binary = load_decoder_binary(&step.decoder_ref, ctx).await?;
        resolved_steps.push(ResolvedDecoderStep {
            binary,
            pattern_json: step.pattern_json,
        });
    }

    let decoded = tokio::task::spawn_blocking(move || match dob_version {
        DobVersion::V0 => {
            let Some(first_step) = resolved_steps.first() else {
                return Err(anyhow::anyhow!("decoder chain is empty"));
            };
            ckbadger_dob_decoder::decode_dob0(
                &first_step.binary,
                &dna_hex,
                &first_step.pattern_json,
            )
        }
        DobVersion::V1 => {
            let decoders: Vec<(&[u8], &str)> = resolved_steps
                .iter()
                .map(|step| (step.binary.as_slice(), step.pattern_json.as_str()))
                .collect();
            ckbadger_dob_decoder::decode_dob1_chain(&decoders, &dna_hex)
        }
    })
    .await
    .map_err(|e| DobDecodeError::Internal(anyhow::anyhow!("CKB-VM spawn_blocking panicked: {e}")))?
    .map_err(|e| DobDecodeError::DecoderExecution {
        detail: e.to_string(),
    })?;

    let coll_id = collection_id.expect("collection_id guaranteed by earlier check");

    let mut steps = Vec::with_capacity(decoded.step_outputs.len());
    let mut all_traits = Vec::new();

    for step_output in &decoded.step_outputs {
        let raw_bytes = step_output.raw_output.as_bytes();
        let media_type = sniff_media_type(raw_bytes);
        let hash = ctx
            .media_store
            .write(coll_id, raw_bytes)
            .map_err(DobDecodeError::Internal)?;

        let traits: Vec<DobDecodedTrait> = step_output
            .traits
            .iter()
            .map(|t| DobDecodedTrait {
                name: t.name.clone(),
                value: format_trait_value(&t.value),
            })
            .collect();

        all_traits.extend(step_output.traits.iter().cloned());

        steps.push(DobDecodedStep {
            step: step_output.step,
            media_type: media_type.to_string(),
            size: raw_bytes.len() as u64,
            hash,
            traits,
        });
    }

    let media_sources = extract_media_sources_from_traits(&all_traits);

    Ok(DobDecodedEntry {
        steps,
        media_sources,
        decoded_at: chrono::Utc::now().timestamp(),
    })
}

/// Load a decoder binary from cache or chain. Standalone for concurrent use.
async fn load_decoder_binary(
    decoder_ref: &DecoderRef,
    ctx: &DecodeContext,
) -> Result<Vec<u8>, DobDecodeError> {
    match decoder_ref {
        DecoderRef::CodeHash(code_hash) => {
            let cache_key = DecoderBinaryCache::code_hash_key(code_hash);
            if let Some(binary) = ctx.decoder_cache.get(&cache_key) {
                return Ok(Arc::try_unwrap(binary).unwrap_or_else(|arc| (*arc).clone()));
            }

            let (tx_hash, output_index, _) = ctx
                .store
                .find_any_cell_by_data_hash(code_hash, ctx.append_only_store.as_ref())
                .map_err(DobDecodeError::Internal)?
                .ok_or_else(|| DobDecodeError::DecoderNotFound {
                    detail: format!(
                        "decoder code cell missing from local data-hash index: code_hash=0x{}",
                        hex::encode(code_hash)
                    ),
                })?;

            let binary = fetch_output_data_by_outpoint(&tx_hash, output_index, &ctx.rpc_client)
                .await
                .map_err(DobDecodeError::DecoderBinaryFetch)?;

            verify_blake2b_hash(&binary, code_hash).map_err(|e| {
                DobDecodeError::Internal(anyhow::anyhow!(
                    "resolved decoder binary hash mismatch: code_hash=0x{}: {e}",
                    hex::encode(code_hash)
                ))
            })?;

            ctx.decoder_cache
                .put(&cache_key, &binary)
                .map_err(DobDecodeError::Internal)?;

            Ok(binary)
        }
        DecoderRef::TypeId(type_id_hash) => {
            let cache_key = DecoderBinaryCache::type_id_key(type_id_hash);
            if let Some(binary) = ctx.decoder_cache.get(&cache_key) {
                return Ok(Arc::try_unwrap(binary).unwrap_or_else(|arc| (*arc).clone()));
            }

            let type_id_code_hash =
                hex::decode(crate::parser::script::TYPE_ID_CODE_HASH).expect("valid hex constant");
            let type_script_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
                &type_id_code_hash,
                1,
                type_id_hash,
            );

            let cells = ctx
                .store
                .list_cells_by_type(&type_script_hash, 1, None, ctx.append_only_store.as_ref())
                .map_err(DobDecodeError::Internal)?;

            let (tx_hash, output_index, _) =
                cells
                    .into_iter()
                    .next()
                    .ok_or_else(|| DobDecodeError::DecoderNotFound {
                        detail: format!(
                            "no live cell found in local index for TypeID decoder: type_id=0x{}",
                            hex::encode(type_id_hash)
                        ),
                    })?;

            let binary = fetch_output_data_by_outpoint(&tx_hash, output_index, &ctx.rpc_client)
                .await
                .map_err(DobDecodeError::DecoderBinaryFetch)?;

            ctx.decoder_cache
                .put(&cache_key, &binary)
                .map_err(DobDecodeError::Internal)?;

            Ok(binary)
        }
    }
}

/// Extract DNA hex from a spore's on-chain cell content.
///
/// 1. Look up the spore's outpoint (tx_hash, output_index) from the domain store.
/// 2. Fetch the creation transaction via CKB RPC to obtain raw output data.
/// 3. Parse the Spore molecule to extract the content field.
/// 4. Extract the DNA hex from the content bytes (raw-binary or text form).
async fn extract_dna_from_spore(
    spore_id: &[u8],
    store: &CkbadgerStore,
    rpc_client: &CkbRpcClient,
) -> Result<String, DobDecodeError> {
    let outpoints = store
        .list_spore_outpoints_by_spore_id(spore_id)
        .map_err(DobDecodeError::Internal)?;

    let (tx_hash, output_index) = outpoints.first().ok_or_else(|| {
        DobDecodeError::Internal(anyhow::anyhow!(
            "no outpoint found for spore_id=0x{}",
            hex::encode(spore_id)
        ))
    })?;

    // Transient: RPC fetch of the spore's own creation tx.
    let output_data = fetch_output_data_by_outpoint(tx_hash, *output_index, rpc_client)
        .await
        .map_err(DobDecodeError::SporeCellFetch)?;

    // Deterministic: the on-chain content is immutable. Both parsers return
    // Option (None = malformed molecule / missing DNA), mapped to DnaInvalid.
    let content_bytes =
        SporeParser::parse_spore_content_from_data(&output_data).ok_or_else(|| {
            DobDecodeError::DnaInvalid {
                detail: "failed to parse Spore molecule content".to_string(),
            }
        })?;

    parse_dna_hex_from_content(&content_bytes).ok_or_else(|| DobDecodeError::DnaInvalid {
        detail: "failed to extract DNA hex from spore content".to_string(),
    })
}

async fn fetch_output_data_by_outpoint(
    tx_hash: &[u8],
    output_index: i16,
    rpc_client: &CkbRpcClient,
) -> Result<Vec<u8>> {
    let tx_hash_hex = format!("0x{}", hex::encode(tx_hash));
    let tx_with_status = rpc_client
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

/// DOB protocol version. Dispatch is decided by cluster metadata `dob.ver`
/// (the authority per the official server's `unbox_dob`), never by the
/// spore's content_type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DobVersion {
    V0,
    V1,
}

impl DobVersion {
    fn number(self) -> u64 {
        match self {
            DobVersion::V0 => 0,
            DobVersion::V1 => 1,
        }
    }
}

/// Parse the protocol version from the cluster metadata `dob` object.
///
/// Mirrors the official `ClusterDescriptionField::unbox_dob`: an absent (or
/// null) `ver` means version 0; `0`/`1` select their version; anything else —
/// an undefined version number or a non-integer value — is a loud error,
/// never a silent default.
fn parse_dob_version_from_cluster(dob: &Value) -> Result<DobVersion> {
    match dob.get("ver") {
        None | Some(Value::Null) => Ok(DobVersion::V0),
        Some(ver) => match ver.as_u64() {
            Some(0) => Ok(DobVersion::V0),
            Some(1) => Ok(DobVersion::V1),
            Some(other) => bail!(
                "unsupported DOB version in cluster metadata: dob.ver={other} (only 0 and 1 are defined)"
            ),
            None => bail!("cluster metadata dob.ver is not an unsigned integer: {ver}"),
        },
    }
}

/// Version implied by a spore's content type ("dob/0" → 0, "dob/1" → 1,
/// parameters after ';' ignored). Used only as a consistency cross-check
/// against the cluster's authoritative `dob.ver`; unparseable types yield
/// `None` — there is no silent default.
fn parse_dob_version_from_content_type(content_type: &str) -> Option<u64> {
    let normalized = content_type.trim().to_ascii_lowercase();
    let rest = normalized.strip_prefix("dob/")?;
    rest.split(';').next()?.trim().parse().ok()
}

/// Parse a DecoderRef from the DOB metadata's decoder entry.
///
/// Supports two formats:
/// - `{"dob": {"decoders": [{"type": "code_hash", "hash": "0x..."}]}}`
/// - `{"dob": {"decoder": {"type": "type_id", "hash": "0x..."}}}`
#[cfg(test)]
fn parse_decoder_ref(dob: &Value) -> Result<DecoderRef> {
    if let Some(decoders) = dob.get("decoders").and_then(|v| v.as_array()) {
        if let Some(first) = decoders.first() {
            return parse_single_decoder_ref(first.get("decoder").unwrap_or(first));
        }
    }

    if let Some(decoder) = dob.get("decoder") {
        return parse_single_decoder_ref(decoder);
    }

    bail!("no decoder reference found in DOB metadata")
}

fn parse_decoder_steps(dob: &Value) -> Result<Vec<DecoderStep>> {
    if let Some(decoders) = dob.get("decoders").and_then(|v| v.as_array()) {
        let mut steps = Vec::with_capacity(decoders.len());
        for (index, entry) in decoders.iter().enumerate() {
            let decoder_value = entry.get("decoder").unwrap_or(entry);
            let decoder_ref = parse_single_decoder_ref(decoder_value)
                .with_context(|| format!("invalid decoder entry at index {index}"))?;
            let pattern = entry.get("pattern").with_context(|| {
                format!("decoder entry missing 'pattern' field at index {index}")
            })?;
            let pattern_json = serialize_pattern(pattern);
            steps.push(DecoderStep {
                decoder_ref,
                pattern_json,
            });
        }
        if !steps.is_empty() {
            return Ok(steps);
        }
    }

    if let Some(decoder) = dob.get("decoder") {
        let decoder_ref = parse_single_decoder_ref(decoder)?;
        let pattern = dob
            .get("pattern")
            .context("no pattern found in DOB metadata")?;
        let pattern_json = serialize_pattern(pattern);
        return Ok(vec![DecoderStep {
            decoder_ref,
            pattern_json,
        }]);
    }

    bail!("no decoder reference found in DOB metadata")
}

/// Serialize a pattern value for passing to a DOB decoder binary.
///
/// If the pattern is a JSON string, return its inner string value (unwrapped).
/// Otherwise serialize to JSON. This matches the DOB protocol convention where
/// string-typed patterns contain pre-serialized data (e.g. molecule hex) that
/// decoders parse directly, without an outer JSON string wrapper.
fn serialize_pattern(pattern: &Value) -> String {
    match pattern {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
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
#[cfg(test)]
fn extract_pattern_json(dob: &Value) -> Result<String> {
    if let Some(decoders) = dob.get("decoders").and_then(|v| v.as_array()) {
        if let Some(first) = decoders.first() {
            if let Some(pattern) = first.get("pattern") {
                return serde_json::to_string(pattern).context("failed to serialize pattern");
            }
        }
    }

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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Real mainnet cluster description of `0x0d3cdf07dd357d55795f09e04d8394
    /// 5a6cf674e4dd5cc1e344c873d1c816740b` ("dob1-basic-shape"): declares
    /// `dob.ver = 1` with a two-decoder chain, yet its spores (e.g.
    /// `0x157f6a5782000f0c6a79e1709b7a425ef4347c4615e679c38f3f376ce9a781d6`)
    /// carry content_type `dob/0`. The reference server decodes them as
    /// dob/1 (three outputs incl. the rendered SVG) because the cluster is
    /// the version authority.
    const DOB1_BASIC_SHAPE_CLUSTER_DESCRIPTION: &str = r#"{"description":"This is a basic-shape example for dob1.","dob":{"ver":1,"decoders":[{"decoder":{"type":"code_hash","hash":"0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c"},"pattern":[["Shape","String",1,1,"options",["circle","square","triangle","star","text"]],["BackgroundColor","String",0,1,"options",["red","blue","green","yellow","pink"]]]},{"decoder":{"type":"code_hash","hash":"0xda3525549b72970b4c95f5b5749357f20d1293d335710b674f09c32f7d54b6dc"},"pattern":[["IMAGE.0","attributes","","raw","xmlns='http://www.w3.org/2000/svg' viewBox='0 0 500 500'"],["IMAGE.0","elements","BackgroundColor","options",[["red","<rect width='500' height='500' x='0' y='0' fill='red' />"],[["*"],"<rect width='500' height='500' x='0' y='0' fill='pink' />"]]]]}]}}"#;

    #[test]
    fn test_cluster_ver_is_dispatch_authority_over_content_type() {
        let metadata: Value = serde_json::from_str(DOB1_BASIC_SHAPE_CLUSTER_DESCRIPTION).unwrap();
        let dob = metadata.get("dob").unwrap();
        // Cluster says dob/1 — dispatch must follow it ...
        assert_eq!(
            parse_dob_version_from_cluster(dob).unwrap(),
            DobVersion::V1
        );
        // ... even though the live spores in this cluster are typed dob/0.
        assert_eq!(parse_dob_version_from_content_type("dob/0"), Some(0));
    }

    #[test]
    fn test_parse_dob_version_from_cluster_absent_ver_is_v0() {
        // Official unbox_dob: `Some(0) | None => V0`.
        let absent: Value = serde_json::json!({ "decoder": {}, "pattern": [] });
        assert_eq!(
            parse_dob_version_from_cluster(&absent).unwrap(),
            DobVersion::V0
        );
        let null: Value = serde_json::json!({ "ver": null });
        assert_eq!(
            parse_dob_version_from_cluster(&null).unwrap(),
            DobVersion::V0
        );
        let zero: Value = serde_json::json!({ "ver": 0 });
        assert_eq!(
            parse_dob_version_from_cluster(&zero).unwrap(),
            DobVersion::V0
        );
    }

    #[test]
    fn test_parse_dob_version_from_cluster_rejects_undefined_versions_loudly() {
        let v2: Value = serde_json::json!({ "ver": 2 });
        let err = parse_dob_version_from_cluster(&v2).unwrap_err();
        assert!(err.to_string().contains("unsupported DOB version"));

        let garbage: Value = serde_json::json!({ "ver": "abc" });
        let err = parse_dob_version_from_cluster(&garbage).unwrap_err();
        assert!(err.to_string().contains("not an unsigned integer"));

        let negative: Value = serde_json::json!({ "ver": -1 });
        let err = parse_dob_version_from_cluster(&negative).unwrap_err();
        assert!(err.to_string().contains("not an unsigned integer"));

        let fractional: Value = serde_json::json!({ "ver": 1.5 });
        let err = parse_dob_version_from_cluster(&fractional).unwrap_err();
        assert!(err.to_string().contains("not an unsigned integer"));
    }

    #[test]
    fn test_parse_dob_version_from_content_type_has_no_silent_default() {
        assert_eq!(parse_dob_version_from_content_type("dob/0"), Some(0));
        assert_eq!(parse_dob_version_from_content_type("dob/1"), Some(1));
        assert_eq!(parse_dob_version_from_content_type("DOB/0"), Some(0));
        assert_eq!(
            parse_dob_version_from_content_type("dob/0;charset=utf-8"),
            Some(0)
        );
        assert_eq!(
            parse_dob_version_from_content_type("dob/1;charset=utf-8"),
            Some(1)
        );
        // Non-DOB and unparseable types must be None, never a silent 0.
        assert_eq!(parse_dob_version_from_content_type("text/plain"), None);
        assert_eq!(parse_dob_version_from_content_type("dob/abc"), None);
        assert_eq!(parse_dob_version_from_content_type("dob/"), None);
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
    fn test_parse_decoder_ref_nested_decoder_entry() {
        let dob = json!({
            "decoders": [{
                "decoder": {
                    "type": "code_hash",
                    "hash": "0xabcd"
                },
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
    fn test_parse_decoder_steps_preserve_nested_chain_order() {
        let dob = json!({
            "decoders": [
                {
                    "decoder": {
                        "type": "code_hash",
                        "hash": "0xabcd"
                    },
                    "pattern": ["first"]
                },
                {
                    "decoder": {
                        "type": "type_id",
                        "hash": "0x1234"
                    },
                    "pattern": ["second"]
                }
            ]
        });
        let steps = parse_decoder_steps(&dob).unwrap();
        assert_eq!(steps.len(), 2);
        match &steps[0].decoder_ref {
            DecoderRef::CodeHash(hash) => assert_eq!(hash, &vec![0xAB, 0xCD]),
            _ => panic!("expected first step to use code_hash"),
        }
        assert_eq!(steps[0].pattern_json, "[\"first\"]");
        match &steps[1].decoder_ref {
            DecoderRef::TypeId(hash) => assert_eq!(hash, &vec![0x12, 0x34]),
            _ => panic!("expected second step to use type_id"),
        }
        assert_eq!(steps[1].pattern_json, "[\"second\"]");
    }

    #[test]
    fn test_parse_decoder_steps_from_singular_decoder() {
        let dob = json!({
            "decoder": {
                "type": "code_hash",
                "hash": "0xff"
            },
            "pattern": ["legacy"]
        });
        let steps = parse_decoder_steps(&dob).unwrap();
        assert_eq!(steps.len(), 1);
        match &steps[0].decoder_ref {
            DecoderRef::CodeHash(hash) => assert_eq!(hash, &vec![0xFF]),
            _ => panic!("expected CodeHash"),
        }
        assert_eq!(steps[0].pattern_json, "[\"legacy\"]");
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
    }

    #[test]
    fn test_update_spore_media_profile_updates_cluster_aggregate_tier_counts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let cache_dir = dir.path().join("decoder-cache");
        let decoder_cache = Arc::new(DecoderBinaryCache::new(&cache_dir).unwrap());
        let dob_decode_dir = dir.path().join("media");
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker = DobDecodeWorker::new(
            store.clone(),
            store.clone(),
            decoder_cache,
            dob_decode_dir,
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
    async fn test_extract_dna_from_spore_raw_binary_content_form() {
        // Real testnet spore 0x9ca1e7fc9a89254d5438fb32d99aadce1c24cd1d4a49b7
        // 35be9c13d8ceae9c9c ("Forgily Characters"), creation tx 0x05e84bc76e
        // 29309216e10702258a6af92ae8fdfce8d53ca39b03ca3bf621c050 output 0.
        // Its Spore content starts with 0x00: the official raw-binary DNA
        // form (dob-decoder-standalone-server decode_spore_content), where
        // the DNA is the hex of the bytes after the marker.
        let spore_cell_data_hex = "0xc90000001000000019000000a500000005000000646f622f30880000000001034764640000f2761adf8466504b02a91a95abaecebf6cc43599c5fcbf2db8973d7b91fcc30c68747470733a2f2f6172746966616374732e666f7267696c792e636f6d2f696d6167655f6172746966616374732f65353832343962342d626136302d346231622d626231312d6662316133333935346666632e706e670000000000000000000020000000288433dccb8a5f13602f3c63d7f7e6b3f4d401bc8e7fd4c0055ce1ee2e5d86d1";
        let expected_dna = "01034764640000f2761adf8466504b02a91a95abaecebf6cc43599c5fcbf2db8973d7b91fcc30c68747470733a2f2f6172746966616374732e666f7267696c792e636f6d2f696d6167655f6172746966616374732f65353832343962342d626136302d346231622d626231312d6662316133333935346666632e706e6700000000000000000000";

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let spore_id =
            hex::decode("9ca1e7fc9a89254d5438fb32d99aadce1c24cd1d4a49b735be9c13d8ceae9c9c")
                .unwrap();
        let tx_hash =
            hex::decode("05e84bc76e29309216e10702258a6af92ae8fdfce8d53ca39b03ca3bf621c050")
                .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_outpoint(&tx_hash, 0, &spore_id);
        batch.commit().unwrap();

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
                                "capacity": "0x34e62ce00",
                                "lock": dummy_lock,
                                "type": null
                            }
                        ],
                        "outputs_data": [spore_cell_data_hex],
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

        let rpc_client = CkbRpcClient::new(server.uri());
        let dna = extract_dna_from_spore(&spore_id, &store, &rpc_client)
            .await
            .expect("raw-binary DNA form must be extractable");
        assert_eq!(dna, expected_dna);
    }

    #[tokio::test]
    async fn test_extract_dna_from_spore_no_outpoints() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let spore_id = [0x11u8; 32];
        let rpc_client = CkbRpcClient::new("http://localhost:9999");
        let result = extract_dna_from_spore(&spore_id, &store, &rpc_client).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().is_transient(),
            "no-outpoint maps to transient Internal"
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

        let dob_decode_dir = tempfile::tempdir().unwrap();
        let media_store = Arc::new(MediaBlobStore::new(dob_decode_dir.path().join("media")));

        let ctx = DecodeContext {
            store,
            append_only_store,
            decoder_cache: decoder_cache.clone(),
            media_store,
            rpc_client: CkbRpcClient::new(server.uri()),
        };

        let loaded = load_decoder_binary(&DecoderRef::CodeHash(code_hash.clone()), &ctx)
            .await
            .unwrap();
        assert_eq!(loaded, decoder_binary);

        let cache_key = DecoderBinaryCache::code_hash_key(&code_hash);
        assert_eq!(*decoder_cache.get(&cache_key).unwrap(), decoder_binary);
    }

    #[tokio::test]
    async fn test_decode_single_spore_cluster_not_found_is_deterministic() {
        use super::super::dob_decode_error::DobDecodeError;
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let cache_dir = dir.path().join("decoder-cache");
        let decoder_cache = Arc::new(DecoderBinaryCache::new(&cache_dir).unwrap());
        let media_store = Arc::new(MediaBlobStore::new(dir.path().join("media")));
        let ctx = DecodeContext {
            store: store.clone(),
            append_only_store: store.clone(),
            decoder_cache,
            media_store,
            rpc_client: CkbRpcClient::new("http://localhost:9999"),
        };

        // collection_id points to a cluster that does not exist -> ClusterNotFound.
        let spore_id = [0x22u8; 32];
        let missing_cluster = vec![0x99u8; 32];
        let err = decode_single_spore(&spore_id, "dob/0", Some(&missing_cluster), &ctx)
            .await
            .unwrap_err();
        assert!(
            !err.is_transient(),
            "cluster-not-found must be deterministic"
        );
        assert!(matches!(err, DobDecodeError::ClusterNotFound { .. }));
    }

    #[tokio::test]
    async fn test_decode_single_spore_rejects_unknown_cluster_version_loudly() {
        use super::super::dob_decode_error::DobDecodeError;
        use ckbadger_store::types::{ObjectEntry, ObjectExtra, ObjectStandard};

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let decoder_cache = Arc::new(DecoderBinaryCache::new(&dir.path().join("cache")).unwrap());
        let media_store = Arc::new(MediaBlobStore::new(dir.path().join("media")));
        let ctx = DecodeContext {
            store: store.clone(),
            append_only_store: store.clone(),
            decoder_cache,
            media_store,
            rpc_client: CkbRpcClient::new("http://localhost:9999"),
        };

        // A cluster declaring dob.ver = 2 — undefined by the protocol (the
        // official unbox_dob returns DOBVersionNumberUndefined). Must be a
        // deterministic loud metadata failure, never silently decoded as the
        // content_type's version.
        let cluster_id = vec![0x11u8; 32];
        let cluster = ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x22; 32]),
            name: None,
            description: Some(
                r#"{"description":"x","dob":{"ver":2,"decoders":[{"decoder":{"type":"code_hash","hash":"0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c"},"pattern":[["A","String",0,1,"options",["a"]]]}]}}"#
                    .to_string(),
            ),
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0x33; 32],
            extra: ObjectExtra::SporeCluster,
        };
        store.put_spore_direct(&cluster_id, &cluster).unwrap();

        let spore_id = [0x44u8; 32];
        let err = decode_single_spore(&spore_id, "dob/0", Some(&cluster_id), &ctx)
            .await
            .unwrap_err();
        assert!(
            matches!(err, DobDecodeError::ClusterMetadataInvalid { .. }),
            "undefined cluster dob.ver must be a deterministic metadata failure, got: {err}"
        );
        assert!(
            err.to_string().contains("unsupported DOB version"),
            "error must name the version problem, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_worker_records_deterministic_failure_and_stops_relisting() {
        use ckbadger_store::types::{
            CompositionTier, DecodeOutcome, ObjectEntry, ObjectExtra, ObjectStandard,
            SporeMediaProfile,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let decoder_cache = Arc::new(DecoderBinaryCache::new(&dir.path().join("cache")).unwrap());
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker = DobDecodeWorker::new(
            store.clone(),
            store.clone(),
            decoder_cache,
            dir.path().join("media"),
            "http://localhost:9999".to_string(),
            shutdown,
        );

        // A dob/0 spore whose collection points at a non-existent cluster.
        let spore_id = [0x22u8; 32];
        let spore = ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(vec![0x99u8; 32]),
            token_id: None,
            owner_lock_hash: Some(vec![0x33; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0x44; 32],
            extra: ObjectExtra::Spore {
                content_type: "dob/0".to_string(),
                content_length: 3,
                media_profile: SporeMediaProfile {
                    tier: CompositionTier::PureCkb,
                    sources: vec![],
                    issues: vec![],
                },
            },
        };
        store.put_spore_direct(&spore_id, &spore).unwrap();

        assert_eq!(store.list_undecoded_dob_spores(100, None).unwrap().len(), 1);

        worker.run().await.unwrap();

        // A Failed outcome is recorded ...
        match store.get_dob_decode_outcome(&spore_id).unwrap().unwrap() {
            DecodeOutcome::Failed(f) => assert_eq!(
                f.category,
                ckbadger_store::types::DobDecodeFailureCategory::ClusterNotFound
            ),
            DecodeOutcome::Decoded(_) => panic!("expected Failed"),
        }
        // ... and the spore is no longer re-listed for decode.
        assert_eq!(store.list_undecoded_dob_spores(100, None).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_worker_transient_failure_writes_no_record_and_stays_relisted() {
        use ckbadger_store::types::{
            CompositionTier, ObjectEntry, ObjectExtra, ObjectStandard, SporeMediaProfile,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let decoder_cache = Arc::new(DecoderBinaryCache::new(&dir.path().join("cache")).unwrap());
        let shutdown = Arc::new(AtomicBool::new(false));
        // Unreachable RPC endpoint; not even reached in this test — the spore
        // fails transiently at outpoint lookup, before any RPC call.
        let worker = DobDecodeWorker::new(
            store.clone(),
            store.clone(),
            decoder_cache,
            dir.path().join("media"),
            "http://127.0.0.1:9".to_string(),
            shutdown,
        );

        // A real cluster with a dob-valid description, so decode gets PAST
        // ClusterNotFound / ClusterMetadataInvalid (incl. the version check,
        // which now runs before any RPC) and actually reaches
        // extract_dna_from_spore (where the transient fault occurs).
        let cluster_id = vec![0x11u8; 32];
        let cluster = ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x22; 32]),
            name: None,
            description: Some(
                r#"{"description":"x","dob":{"ver":0,"decoder":{"type":"code_hash","hash":"0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c"},"pattern":[["A","String",0,1,"options",["a"]]]}}"#
                    .to_string(),
            ),
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0x33; 32],
            extra: ObjectExtra::SporeCluster,
        };
        store.put_spore_direct(&cluster_id, &cluster).unwrap();

        // A dob/0 spore in that cluster with NO outpoint recorded:
        // extract_dna_from_spore finds no outpoint and returns
        // DobDecodeError::Internal — a TRANSIENT variant — so the worker must
        // write NO outcome and leave the spore listed for retry.
        let spore_id = [0x44u8; 32];
        let spore = ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(cluster_id.clone()),
            token_id: None,
            owner_lock_hash: Some(vec![0x55; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 2,
            created_at_tx: vec![0x66; 32],
            extra: ObjectExtra::Spore {
                content_type: "dob/0".to_string(),
                content_length: 3,
                media_profile: SporeMediaProfile {
                    tier: CompositionTier::PureCkb,
                    sources: vec![],
                    issues: vec![],
                },
            },
        };
        store.put_spore_direct(&spore_id, &spore).unwrap();

        // Only the dob spore is undecoded (the SporeCluster is not a dob item).
        assert_eq!(store.list_undecoded_dob_spores(100, None).unwrap().len(), 1);

        worker.run().await.unwrap();

        // A transient failure must NOT persist any outcome ...
        assert!(
            store.get_dob_decode_outcome(&spore_id).unwrap().is_none(),
            "transient failure must not write a DecodeOutcome"
        );
        // ... and the spore must remain listed so it retries on the next run.
        assert_eq!(store.list_undecoded_dob_spores(100, None).unwrap().len(), 1);
    }
}
