//! Crawler service entrypoint: `run_crawler` drives the perpetual crawl loop and
//! `select_geoip` resolves the configured geo backend (fail-fast on partial config).

use std::path::Path;
use std::time::Duration;

use ckbadger_config::{CkbadgerConfig, CrawlerConfig};
use ckbadger_store::CkbadgerStore;

use crate::ckb_prober::CkbProber;
use crate::engine::{run_round, RoundConfig};
use crate::geoip::{GeoIp, MaxmindGeoIp, NoGeo};

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
/// Startup fail-fast: a half-configured geo backend or a network with no
/// bootnodes both propagate `Err` and prevent the loop from starting. Once
/// running, a failed round is a *recorded observation* (logged via
/// `tracing::error!`), never fatal — the loop keeps going. When `run_once` is
/// set the loop returns after a single round (used by `crawl --once`).
pub async fn run_crawler(work_dir: &Path, run_once: bool) -> anyhow::Result<()> {
    let cfg: CkbadgerConfig = ckbadger_config::load_config(work_dir)?;
    if !cfg.crawler.enabled && !run_once {
        tracing::info!("crawler disabled ([crawler].enabled=false); exiting");
        return Ok(());
    }

    let store = CkbadgerStore::open_network(work_dir.join(&cfg.store.network_data_path))?;
    let geoip = select_geoip(&cfg.crawler)?;
    let prober = CkbProber::new(&cfg.ckb.network, &cfg.crawler)?; // fail-fast on no bootnodes

    let round_cfg = RoundConfig {
        node_ttl_secs: 2_592_000,
        hourly_retention_days: cfg.crawler.history_hourly_retention_days,
        top_n: 20,
        max_addrs: None,
        max_frontier: Some(cfg.crawler.max_frontier),
        round_budget: Some(std::time::Duration::from_secs(
            cfg.crawler.round_budget_secs,
        )),
    };

    // Resume the round counter from the persisted status so round ids are
    // monotonic across restarts.
    let mut round_id = store.get_network_status()?.map(|s| s.round_id).unwrap_or(0);

    loop {
        round_id += 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        match run_round(&store, &prober, geoip.as_ref(), &round_cfg, now, round_id).await {
            Ok(r) => tracing::info!(
                round_id,
                reachable = r.status.reachable,
                total = r.status.total_known,
                drained = r.status.frontier_drained,
                "crawl round done"
            ),
            // A failed round is recorded and skipped, not fatal — keep looping.
            Err(e) => tracing::error!(round_id, "crawl round failed: {e:#}"),
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
}
