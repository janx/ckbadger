//! Crawler service entrypoint: `run_crawler` drives the perpetual crawl loop and
//! `select_geoip` resolves the configured geo backend (fail-fast on partial config).

use std::path::Path;
use std::time::Duration;

use ckbadger_config::{CkbadgerConfig, CrawlerConfig};
use ckbadger_store::{CkbadgerStore, StoreRuntimeConfig};

use crate::ckb_prober::CkbProber;
use crate::engine::{run_crawl_slice, CrawlSliceReport, RoundConfig, SystemCrawlClock};
use crate::geoip::{GeoIp, MaxmindGeoIp, NoGeo};

#[derive(Debug, PartialEq, Eq)]
struct CompletedRoundLogFields {
    candidate_peers: u64,
    reachable_peers: u64,
    verified_retained_peers: u64,
    verified_unavailable_peers: u64,
    address_attempts: u64,
}

fn completed_round_log_fields(
    status: &ckbadger_store::LatestStatus,
) -> anyhow::Result<CompletedRoundLogFields> {
    Ok(CompletedRoundLogFields {
        candidate_peers: status.peer_outcomes.candidate_peers(status.round_id)?,
        reachable_peers: status.peer_outcomes.reachable_peers(),
        verified_retained_peers: status
            .peer_outcomes
            .verified_retained_peers(status.round_id)?,
        verified_unavailable_peers: status
            .peer_outcomes
            .verified_unavailable_peers(status.round_id)?,
        address_attempts: status
            .address_observations
            .address_attempts(status.round_id)?,
    })
}

/// Resolve the geo backend from config: `NoGeo` when unconfigured, a fail-fast
/// [`MaxmindGeoIp`] when both MMDB paths are set, and an error when exactly one
/// path is set (a half-configured geo backend is a config bug, not a silent NoGeo).
pub fn select_geoip(cfg: &CrawlerConfig) -> anyhow::Result<Box<dyn GeoIp>> {
    match (&cfg.geoip_city_path, &cfg.geoip_asn_path) {
        (Some(c), Some(a)) => Ok(Box::new(MaxmindGeoIp::open(Path::new(c), Path::new(a))?)),
        (None, None) => Ok(Box::new(NoGeo)),
        _ => anyhow::bail!("geoip_city_path and geoip_asn_path must both be set or both unset"),
    }
}

/// Run the crawler service loop.
///
/// Startup configuration and every internal crawler/store error propagate.
/// Expected remote failures are typed observations inside the engine. A logical
/// round resumes immediately across checkpointed slices and is the unit of
/// `crawl --once` success.
///
/// `store_runtime_config` is this network's RAM share, computed by the CLI (the
/// crawler crate has no access to the orchestrator config). The network store is
/// this process's ONLY store open, so without it each of N co-resident crawlers
/// would size its cache/WriteBufferManager from undivided host RAM.
pub async fn run_crawler(
    work_dir: &Path,
    run_once: bool,
    store_runtime_config: StoreRuntimeConfig,
) -> anyhow::Result<()> {
    let cfg: CkbadgerConfig = ckbadger_config::load_config(work_dir)?;
    if !cfg.crawler.enabled && !run_once {
        tracing::info!("crawler disabled ([crawler].enabled=false); exiting");
        return Ok(());
    }
    cfg.crawler.validate()?;

    let store = CkbadgerStore::open_network_with_runtime(
        work_dir.join(&cfg.store.network_data_path),
        store_runtime_config,
    )?;
    let geoip = select_geoip(&cfg.crawler)?;
    let prober = CkbProber::new(&cfg.ckb.network, &cfg.crawler)?; // fail-fast on no bootnodes

    let round_cfg = RoundConfig {
        node_ttl_secs: 2_592_000,
        hourly_retention_days: cfg.crawler.history_hourly_retention_days,
        top_n: 20,
        max_dial_concurrency: cfg.crawler.max_dial_concurrency,
        max_addrs: None,
        max_frontier: Some(cfg.crawler.max_frontier),
        slice_budget: Some(std::time::Duration::from_secs(
            cfg.crawler.slice_budget_secs,
        )),
    };
    let clock = SystemCrawlClock;

    loop {
        loop {
            match run_crawl_slice(&store, &prober, geoip.as_ref(), &clock, &round_cfg).await? {
                CrawlSliceReport::Partial(progress) => {
                    tracing::info!(
                        round_id = progress.round_id,
                        completed_peers = progress.completed_peers,
                        candidate_peers = progress.candidate_peers,
                        address_attempts = progress.address_attempts,
                        "crawl slice checkpointed; resuming logical round"
                    );
                    tokio::task::yield_now().await;
                }
                CrawlSliceReport::Completed(status) => {
                    let fields = completed_round_log_fields(&status)?;
                    tracing::info!(
                        round_id = status.round_id,
                        candidate_peers = fields.candidate_peers,
                        reachable_peers = fields.reachable_peers,
                        exhausted_with_retained_verification =
                            status.peer_outcomes.exhausted_with_retained_verification,
                        exhausted_without_retained_verification =
                            status.peer_outcomes.exhausted_without_retained_verification,
                        foreign_with_retained_verification =
                            status.peer_outcomes.foreign_with_retained_verification,
                        foreign_without_retained_verification =
                            status.peer_outcomes.foreign_without_retained_verification,
                        verified_retained_peers = fields.verified_retained_peers,
                        verified_unavailable_peers = fields.verified_unavailable_peers,
                        address_attempts = fields.address_attempts,
                        dial_request_failed = status.address_observations.dial_request_failed,
                        no_authenticated_session_before_deadline = status
                            .address_observations
                            .no_authenticated_session_before_deadline,
                        authenticated_session_without_identify_before_deadline = status
                            .address_observations
                            .authenticated_session_without_identify_before_deadline,
                        malformed_identify = status.address_observations.malformed_identify,
                        foreign_network = status.address_observations.foreign_network,
                        same_network_identified =
                            status.address_observations.same_network_identified,
                        discovery_valid_nodes_messages = status.discovery.valid_nodes_messages,
                        discovery_malformed_messages = status.discovery.malformed_messages,
                        discovery_unexpected_messages = status.discovery.unexpected_messages,
                        discovery_normalized_advertised_addresses =
                            status.discovery.normalized_advertised_addresses,
                        discovery_rejected_advertised_addresses =
                            status.discovery.rejected_advertised_addresses,
                        "crawl round completed"
                    );
                    break;
                }
            }
        }
        if run_once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(cfg.crawler.round_interval_secs)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_config::CrawlerConfig;

    #[test]
    fn geoip_disabled_when_unconfigured() {
        let cfg = CrawlerConfig::default();
        // No paths ⇒ NoGeo (Ok), never an error.
        assert!(select_geoip(&cfg).is_ok());
    }

    #[test]
    fn geoip_fails_fast_when_path_set_but_missing() {
        // Both paths configured but unreadable ⇒ fail-fast (never a silent NoGeo).
        let cfg = CrawlerConfig {
            geoip_city_path: Some("/no/such/city.mmdb".into()),
            geoip_asn_path: Some("/no/such/asn.mmdb".into()),
            ..CrawlerConfig::default()
        };
        assert!(select_geoip(&cfg).is_err());
    }

    #[test]
    fn completed_round_log_fields_use_checked_matrix_projections() {
        let status = ckbadger_store::LatestStatus {
            round_id: 53,
            peer_outcomes: ckbadger_store::CompletedPeerOutcomes {
                same_network_identified: 57,
                exhausted_with_retained_verification: 1,
                exhausted_without_retained_verification: 86,
                ..Default::default()
            },
            address_observations: ckbadger_store::AddressObservationHistogram {
                same_network_identified: 57,
                dial_request_failed: 359,
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(
            completed_round_log_fields(&status).unwrap(),
            CompletedRoundLogFields {
                candidate_peers: 144,
                reachable_peers: 57,
                verified_retained_peers: 58,
                verified_unavailable_peers: 1,
                address_attempts: 416,
            }
        );
    }
}
