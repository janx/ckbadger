use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const STATUS_RUNNING: &str = "running";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STALL_THRESHOLD_MULTIPLIER: f64 = 2.0;

#[derive(Debug, Clone)]
pub struct RocksDbConfig {
    pub rocksdb_budget_gb: u64,
    pub block_cache_bulk_mb: u64,
    pub wbm_bulk_mb: u64,
    pub write_buffer_mega_mb: u64,
    pub l0_slowdown_bulk: u32,
    pub l0_stop_bulk: u32,
    pub max_background_jobs: i32,
    pub max_subcompactions: u32,
    pub unordered_write: bool,
    pub direct_io_reads: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchSample {
    pub engine: String,
    pub blocks: u64,
    pub batch_seconds: f64,
    pub commit_ms: f64,
    pub txs: u64,
    pub cells: u64,
    pub inputs: u64,
    pub parse_ms: f64,
    pub precompute_ms: f64,
    pub write_ms: f64,
    pub prefetch_ms: f64,
    pub finalize_ms: f64,
    pub t1_ms: f64,
    pub t1b_ms: f64,
    pub t2_ms: f64,
    pub t4_ms: f64,
    pub t5_ms: f64,
    pub t6a_ms: f64,
    pub t6b_ms: f64,
    pub t7_ms: f64,
    pub t_act_ms: f64,
    pub t_track_ms: f64,
    // Bulk build sub-step timings (zero when engine=pipeline)
    pub fetch_ms: f64,
    pub facts_ms: f64,
    pub resolve_ms: f64,
    pub reduce_ms: f64,
    pub history_ms: f64,
    pub address_reduce_ms: f64,
    pub activity_stats_ms: f64,
    pub flush_ms: f64,
    pub flush_wait_ms: f64,
    pub prefetch_collect_ms: f64,
    pub facts_par_iter_ms: f64,
    pub facts_merge_ms: f64,
    pub facts_serial_equivalent_ms: f64,
    pub facts_intern_slow_path_count: u64,
    pub facts_intern_total_count: u64,
    pub facts_cell_count: u64,
    pub compaction_pending_mb: u64,
    pub l0_files: u64,
    pub imm_memtables: u64,
    pub timestamp_utc: String,
    pub load_avg_1m: f64,
    pub mem_available_mb: u64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
    pub owner_memory_bytes: HashMap<String, u64>,
    pub live_cell_count: u64,
    pub cumulative_history_rows: u64,
    pub cumulative_sealed_rows: u64,
    pub cumulative_snapshot_rows: u64,
}

impl BatchSample {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        blocks: u64,
        batch_seconds: f64,
        commit_ms: f64,
        compaction_pending_mb: u64,
        l0_files: u64,
        imm_memtables: u64,
        timestamp_utc: String,
        load_avg_1m: f64,
        mem_available_mb: u64,
        disk_read_mb: f64,
        disk_write_mb: f64,
    ) -> Self {
        Self {
            engine: "pipeline".to_string(),
            blocks,
            batch_seconds,
            commit_ms,
            txs: 0,
            cells: 0,
            inputs: 0,
            parse_ms: 0.0,
            precompute_ms: 0.0,
            write_ms: 0.0,
            prefetch_ms: 0.0,
            finalize_ms: 0.0,
            t1_ms: 0.0,
            t1b_ms: 0.0,
            t2_ms: 0.0,
            t4_ms: 0.0,
            t5_ms: 0.0,
            t6a_ms: 0.0,
            t6b_ms: 0.0,
            t7_ms: 0.0,
            t_act_ms: 0.0,
            t_track_ms: 0.0,
            fetch_ms: 0.0,
            facts_ms: 0.0,
            resolve_ms: 0.0,
            reduce_ms: 0.0,
            history_ms: 0.0,
            activity_stats_ms: 0.0,
            address_reduce_ms: 0.0,
            flush_ms: 0.0,
            flush_wait_ms: 0.0,
            prefetch_collect_ms: 0.0,
            facts_par_iter_ms: 0.0,
            facts_merge_ms: 0.0,
            facts_serial_equivalent_ms: 0.0,
            facts_intern_slow_path_count: 0,
            facts_intern_total_count: 0,
            facts_cell_count: 0,
            compaction_pending_mb,
            l0_files,
            imm_memtables,
            timestamp_utc,
            load_avg_1m,
            mem_available_mb,
            disk_read_mb,
            disk_write_mb,
            owner_memory_bytes: HashMap::new(),
            live_cell_count: 0,
            cumulative_history_rows: 0,
            cumulative_sealed_rows: 0,
            cumulative_snapshot_rows: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatSample {
    pub current_block: u64,
    pub target_block: u64,
    pub compaction_pending_mb: u64,
    pub l0_files: u64,
    pub imm_memtables: u64,
}

impl HeartbeatSample {
    pub fn new(
        current_block: u64,
        target_block: u64,
        compaction_pending_mb: u64,
        l0_files: u64,
        imm_memtables: u64,
    ) -> Self {
        Self {
            current_block,
            target_block,
            compaction_pending_mb,
            l0_files,
            imm_memtables,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BulkSyncPerfMetrics {
    pub run_id: String,
    pub status: String,
    pub started_at_utc: String,
    pub finished_at_utc: Option<String>,
    pub wall_clock_seconds: f64,
    pub batches: u64,
    pub blocks: u64,
    pub total_txs: u64,
    pub blocks_per_sec_wall: f64,
    pub txs_per_sec_wall: f64,
    pub blocks_per_batch: f64,
    pub avg_batch_seconds: f64,
    pub p95_batch_seconds: f64,
    pub p99_batch_seconds: f64,
    pub total_commit_seconds: f64,
    pub avg_commit_ms: f64,
    pub p95_commit_ms: f64,
    pub p99_commit_ms: f64,
    pub finalize_seconds: f64,
    pub max_compaction_pending_mb: u64,
    pub max_l0_files: u64,
    pub max_imm_memtables: u64,
    pub avg_load_avg_1m: f64,
    pub max_load_avg_1m: f64,
    pub min_mem_available_mb: u64,
    pub avg_disk_write_mb_per_batch: f64,
    pub peak_owner_memory_bytes: HashMap<String, u64>,
    pub peak_live_cell_count: u64,
    pub streamed_history_rows: u64,
    pub sealed_aggregate_rows: u64,
    pub final_snapshot_rows: u64,
    pub history_flushes: u64,
    pub sealed_aggregate_flushes: u64,
    pub final_snapshot_flushes: u64,
    pub total_batch_seconds: f64,
    pub stall_count: u64,
}

pub struct BulkSyncPerfRun {
    output_root: PathBuf,
    run_dir: PathBuf,
    run_id: String,
    build_version: String,
    started_at_utc: String,
    status: String,
    batch_samples: Vec<BatchSample>,
    heartbeat_samples: Vec<HeartbeatSample>,
    environment: Option<crate::sys_info::EnvironmentSnapshot>,
    rocksdb_config: Option<RocksDbConfig>,
    materialization_report: Option<crate::sync::MaterializationReport>,
    finalize_seconds: f64,
}

impl BulkSyncPerfRun {
    pub fn start(
        output_root: &Path,
        run_id: impl Into<String>,
        build_version: impl Into<String>,
    ) -> Result<Self> {
        let run_id = run_id.into();
        let build_version = build_version.into();
        if build_version.trim().is_empty() {
            bail!("bulk sync perf build_version must not be blank");
        }
        let run_dir = output_root.join(&run_id);
        fs::create_dir_all(&run_dir)?;

        let run = Self {
            output_root: output_root.to_path_buf(),
            run_dir,
            run_id,
            build_version,
            started_at_utc: utc_now_string(),
            status: STATUS_RUNNING.to_string(),
            batch_samples: Vec::new(),
            heartbeat_samples: Vec::new(),
            environment: None,
            rocksdb_config: None,
            materialization_report: None,
            finalize_seconds: 0.0,
        };
        run.write_metadata()?;
        run.write_status(None)?;
        run.write_metrics_file(&run.build_metrics(STATUS_RUNNING, None))?;
        Ok(run)
    }

    #[cfg(test)]
    pub fn start_for_test(output_root: &Path, run_id: &str, build_version: &str) -> Result<Self> {
        Self::start(output_root, run_id, build_version)
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn set_environment(
        &mut self,
        env: crate::sys_info::EnvironmentSnapshot,
        rocksdb_config: RocksDbConfig,
    ) -> Result<()> {
        self.write_environment_env(&env, &rocksdb_config)?;
        self.environment = Some(env);
        self.rocksdb_config = Some(rocksdb_config);
        Ok(())
    }

    pub fn record_batch_sample(&mut self, sample: BatchSample) -> Result<()> {
        self.append_sample("batch", &sample)?;
        self.batch_samples.push(sample);
        self.write_metrics_file(&self.build_metrics(STATUS_RUNNING, None))?;
        Ok(())
    }

    pub fn record_heartbeat_sample(&mut self, sample: HeartbeatSample) -> Result<()> {
        self.append_sample("heartbeat", &sample)?;
        self.heartbeat_samples.push(sample);
        self.write_metrics_file(&self.build_metrics(STATUS_RUNNING, None))?;
        Ok(())
    }

    pub fn set_materialization_report(
        &mut self,
        report: crate::sync::MaterializationReport,
    ) -> Result<()> {
        self.materialization_report = Some(report);
        self.write_metrics_file(&self.build_metrics(STATUS_RUNNING, None))?;
        Ok(())
    }

    pub fn set_finalize_seconds(&mut self, seconds: f64) {
        self.finalize_seconds = seconds;
    }

    #[cfg(test)]
    pub fn build_metrics_for_test(&self, status: &str) -> BulkSyncPerfMetrics {
        self.build_metrics(status, None)
    }

    pub fn finish_completed(&mut self) -> Result<()> {
        self.status = STATUS_COMPLETED.to_string();
        let metrics = self.finalize()?;
        self.update_latest()?;
        self.append_trend_line(&metrics)?;
        Ok(())
    }

    pub fn finish_failed(&mut self) -> Result<()> {
        self.status = STATUS_FAILED.to_string();
        self.finalize()?;
        Ok(())
    }

    fn finalize(&self) -> Result<BulkSyncPerfMetrics> {
        let finished_at_utc = utc_now_string();
        let metrics = self.build_metrics(&self.status, Some(finished_at_utc.clone()));
        self.write_status(Some(&finished_at_utc))?;
        self.write_metrics_file(&metrics)?;
        self.write_report(&metrics)?;
        Ok(metrics)
    }

    fn update_latest(&self) -> Result<()> {
        let latest_dir = self.output_root.join("latest");
        fs::create_dir_all(&latest_dir)?;
        fs::copy(
            self.run_dir.join("metadata.env"),
            latest_dir.join("metadata.env"),
        )?;
        fs::copy(
            self.run_dir.join("metrics.env"),
            latest_dir.join("metrics.env"),
        )?;
        fs::copy(self.run_dir.join("report.md"), latest_dir.join("report.md"))?;
        let env_src = self.run_dir.join("environment.env");
        if env_src.exists() {
            fs::copy(&env_src, latest_dir.join("environment.env"))?;
        }
        Ok(())
    }

    fn build_metrics(&self, status: &str, finished_at_utc: Option<String>) -> BulkSyncPerfMetrics {
        let batches = self.batch_samples.len() as u64;
        let blocks: u64 = self.batch_samples.iter().map(|sample| sample.blocks).sum();
        let total_txs: u64 = self.batch_samples.iter().map(|sample| sample.txs).sum();

        let batch_seconds = self
            .batch_samples
            .iter()
            .map(|sample| sample.batch_seconds)
            .collect::<Vec<_>>();
        let commit_ms = self
            .batch_samples
            .iter()
            .map(|sample| sample.commit_ms)
            .collect::<Vec<_>>();
        let total_commit_seconds = commit_ms.iter().sum::<f64>() / 1000.0;
        let wall_clock_seconds =
            elapsed_wall_clock_seconds(&self.started_at_utc, finished_at_utc.as_deref());
        let blocks_per_sec_wall = if wall_clock_seconds > 0.0 {
            blocks as f64 / wall_clock_seconds
        } else {
            0.0
        };
        let txs_per_sec_wall = if wall_clock_seconds > 0.0 {
            total_txs as f64 / wall_clock_seconds
        } else {
            0.0
        };
        let blocks_per_batch = if batches > 0 {
            blocks as f64 / batches as f64
        } else {
            0.0
        };

        let max_compaction_pending_mb = self
            .batch_samples
            .iter()
            .map(|sample| sample.compaction_pending_mb)
            .chain(
                self.heartbeat_samples
                    .iter()
                    .map(|sample| sample.compaction_pending_mb),
            )
            .max()
            .unwrap_or(0);
        let max_l0_files = self
            .batch_samples
            .iter()
            .map(|sample| sample.l0_files)
            .chain(self.heartbeat_samples.iter().map(|sample| sample.l0_files))
            .max()
            .unwrap_or(0);
        let max_imm_memtables = self
            .batch_samples
            .iter()
            .map(|sample| sample.imm_memtables)
            .chain(
                self.heartbeat_samples
                    .iter()
                    .map(|sample| sample.imm_memtables),
            )
            .max()
            .unwrap_or(0);

        // Environment pressure aggregates
        let load_avgs: Vec<f64> = self.batch_samples.iter().map(|s| s.load_avg_1m).collect();
        let avg_load_avg_1m = average(&load_avgs);
        let max_load_avg_1m = load_avgs
            .iter()
            .copied()
            .max_by(f64::total_cmp)
            .unwrap_or(0.0);
        let min_mem_available_mb = self
            .batch_samples
            .iter()
            .map(|s| s.mem_available_mb)
            .min()
            .unwrap_or(0);
        let disk_writes: Vec<f64> = self.batch_samples.iter().map(|s| s.disk_write_mb).collect();
        let avg_disk_write_mb_per_batch = average(&disk_writes);
        let mut peak_owner_memory_bytes = HashMap::new();
        for sample in &self.batch_samples {
            for (owner, bytes) in &sample.owner_memory_bytes {
                let current_peak = peak_owner_memory_bytes.entry(owner.clone()).or_insert(0);
                *current_peak = (*current_peak).max(*bytes);
            }
        }
        let peak_live_cell_count = self
            .batch_samples
            .iter()
            .map(|s| s.live_cell_count)
            .max()
            .unwrap_or(0);
        let materialization_report = self.materialization_report.clone().unwrap_or_default();
        let total_batch_seconds = batch_seconds.iter().sum::<f64>();
        let avg_for_stall = average(&batch_seconds);
        let stall_threshold = avg_for_stall * STALL_THRESHOLD_MULTIPLIER;
        let stall_count = if batches >= 3 {
            batch_seconds
                .iter()
                .filter(|&&s| s > stall_threshold)
                .count() as u64
        } else {
            0
        };

        BulkSyncPerfMetrics {
            run_id: self.run_id.clone(),
            status: status.to_string(),
            started_at_utc: self.started_at_utc.clone(),
            finished_at_utc,
            wall_clock_seconds,
            batches,
            blocks,
            total_txs,
            blocks_per_sec_wall,
            txs_per_sec_wall,
            blocks_per_batch,
            avg_batch_seconds: average(&batch_seconds),
            p95_batch_seconds: percentile(batch_seconds.clone(), 95),
            p99_batch_seconds: percentile(batch_seconds, 99),
            total_commit_seconds,
            avg_commit_ms: average(&commit_ms),
            p95_commit_ms: percentile(commit_ms.clone(), 95),
            p99_commit_ms: percentile(commit_ms, 99),
            finalize_seconds: self.finalize_seconds,
            max_compaction_pending_mb,
            max_l0_files,
            max_imm_memtables,
            avg_load_avg_1m,
            max_load_avg_1m,
            min_mem_available_mb,
            avg_disk_write_mb_per_batch,
            peak_owner_memory_bytes,
            peak_live_cell_count,
            streamed_history_rows: materialization_report.streamed_history_rows as u64,
            sealed_aggregate_rows: materialization_report.sealed_aggregate_rows as u64,
            final_snapshot_rows: materialization_report.final_snapshot_rows as u64,
            history_flushes: materialization_report.history_flushes as u64,
            sealed_aggregate_flushes: materialization_report.sealed_aggregate_flushes as u64,
            final_snapshot_flushes: materialization_report.final_snapshot_flushes as u64,
            total_batch_seconds,
            stall_count,
        }
    }

    fn write_metadata(&self) -> Result<()> {
        let content = format!(
            "run_id={}\nstarted_at_utc={}\nbuild_version={}\n",
            self.run_id, self.started_at_utc, self.build_version
        );
        fs::write(self.run_dir.join("metadata.env"), content)?;
        Ok(())
    }

    fn write_status(&self, finished_at_utc: Option<&str>) -> Result<()> {
        let mut content = format!("status={}\nrun_id={}\n", self.status, self.run_id);
        if let Some(finished_at_utc) = finished_at_utc {
            content.push_str(&format!("finished_at_utc={}\n", finished_at_utc));
        }
        fs::write(self.run_dir.join("status.env"), content)?;
        Ok(())
    }

    fn write_environment_env(
        &self,
        env: &crate::sys_info::EnvironmentSnapshot,
        config: &RocksDbConfig,
    ) -> Result<()> {
        let content = format!(
            "# Hardware\ncpu_model={}\ncpu_cores={}\nram_total_mb={}\ndisk_device={}\ndisk_scheduler={}\n\n# OS\nkernel={}\nfilesystem={}\n\n# RocksDB config\nrocksdb_budget_gb={}\nblock_cache_bulk_mb={}\nwbm_bulk_mb={}\nwrite_buffer_mega_mb={}\nl0_slowdown_bulk={}\nl0_stop_bulk={}\nmax_background_jobs={}\nmax_subcompactions={}\nunordered_write={}\ndirect_io_reads={}\n",
            env.cpu_model, env.cpu_cores, env.ram_total_mb,
            env.disk_device, env.disk_scheduler,
            env.kernel, env.filesystem,
            config.rocksdb_budget_gb, config.block_cache_bulk_mb,
            config.wbm_bulk_mb, config.write_buffer_mega_mb,
            config.l0_slowdown_bulk, config.l0_stop_bulk,
            config.max_background_jobs, config.max_subcompactions,
            config.unordered_write, config.direct_io_reads,
        );
        fs::write(self.run_dir.join("environment.env"), content)?;
        Ok(())
    }

    fn write_metrics_file(&self, metrics: &BulkSyncPerfMetrics) -> Result<()> {
        let mut content = format!(
            "run_id={}\nstatus={}\nstarted_at_utc={}\n",
            metrics.run_id, metrics.status, metrics.started_at_utc
        );
        if let Some(finished_at_utc) = metrics.finished_at_utc.as_deref() {
            content.push_str(&format!("finished_at_utc={}\n", finished_at_utc));
        }
        content.push_str(&format!(
            "wall_clock_seconds={}\nbatches={}\nblocks={}\ntotal_txs={}\nblocks_per_sec_wall={}\ntxs_per_sec_wall={}\nblocks_per_batch={}\navg_batch_seconds={}\np95_batch_seconds={}\np99_batch_seconds={}\ntotal_commit_seconds={}\navg_commit_ms={}\np95_commit_ms={}\np99_commit_ms={}\nfinalize_seconds={}\nmax_compaction_pending_mb={}\nmax_l0_files={}\nmax_imm_memtables={}\navg_load_avg_1m={}\nmax_load_avg_1m={}\nmin_mem_available_mb={}\navg_disk_write_mb_per_batch={}\npeak_live_cell_count={}\nstreamed_history_rows={}\nsealed_aggregate_rows={}\nfinal_snapshot_rows={}\nhistory_flushes={}\nsealed_aggregate_flushes={}\nfinal_snapshot_flushes={}\ntotal_batch_seconds={}\nstall_count={}\n",
            format_float(metrics.wall_clock_seconds),
            metrics.batches,
            metrics.blocks,
            metrics.total_txs,
            format_float(metrics.blocks_per_sec_wall),
            format_float(metrics.txs_per_sec_wall),
            format_float(metrics.blocks_per_batch),
            format_float(metrics.avg_batch_seconds),
            format_float(metrics.p95_batch_seconds),
            format_float(metrics.p99_batch_seconds),
            format_float(metrics.total_commit_seconds),
            format_float(metrics.avg_commit_ms),
            format_float(metrics.p95_commit_ms),
            format_float(metrics.p99_commit_ms),
            format_float(metrics.finalize_seconds),
            metrics.max_compaction_pending_mb,
            metrics.max_l0_files,
            metrics.max_imm_memtables,
            format_float(metrics.avg_load_avg_1m),
            format_float(metrics.max_load_avg_1m),
            metrics.min_mem_available_mb,
            format_float(metrics.avg_disk_write_mb_per_batch),
            metrics.peak_live_cell_count,
            metrics.streamed_history_rows,
            metrics.sealed_aggregate_rows,
            metrics.final_snapshot_rows,
            metrics.history_flushes,
            metrics.sealed_aggregate_flushes,
            metrics.final_snapshot_flushes,
            format_float(metrics.total_batch_seconds),
            metrics.stall_count,
        ));
        for (owner, bytes) in owner_memory_entries(&metrics.peak_owner_memory_bytes) {
            content.push_str(&format!("peak_owner_memory_bytes_{}={}\n", owner, bytes));
        }
        fs::write(self.run_dir.join("metrics.env"), content)?;
        Ok(())
    }

    fn write_report(&self, metrics: &BulkSyncPerfMetrics) -> Result<()> {
        let baseline = read_metrics_env(&self.output_root.join("latest/metrics.env"))?;

        let mut content = String::new();
        content.push_str("# Bulk Sync Perf Report\n\n");
        content.push_str(&format!("- Run ID: {}\n", metrics.run_id));
        content.push_str(&format!("- Build Version: {}\n", self.build_version));
        content.push_str(&format!("- Status: {}\n", metrics.status));
        content.push_str(&format!("- Started at (UTC): {}\n", metrics.started_at_utc));
        if let Some(finished_at_utc) = metrics.finished_at_utc.as_deref() {
            content.push_str(&format!("- Finished at (UTC): {}\n", finished_at_utc));
        }

        // Environment section
        self.write_report_environment_section(&mut content)?;

        content.push_str("\n## Throughput\n\n");
        content.push_str("| Metric | Value |\n");
        content.push_str("| --- | ---: |\n");
        content.push_str(&format!(
            "| wall_clock_seconds | {} |\n",
            format_float(metrics.wall_clock_seconds)
        ));
        content.push_str(&format!("| batches | {} |\n", metrics.batches));
        content.push_str(&format!("| blocks | {} |\n", metrics.blocks));
        content.push_str(&format!("| total_txs | {} |\n", metrics.total_txs));
        content.push_str(&format!(
            "| blocks_per_sec_wall | {} |\n",
            format_float(metrics.blocks_per_sec_wall)
        ));
        content.push_str(&format!(
            "| txs_per_sec_wall | {} |\n",
            format_float(metrics.txs_per_sec_wall)
        ));
        content.push_str(&format!(
            "| blocks_per_batch | {} |\n",
            format_float(metrics.blocks_per_batch)
        ));
        content.push('\n');

        content.push_str("## Batch Timing\n\n");
        content.push_str("| Metric | Value |\n");
        content.push_str("| --- | ---: |\n");
        content.push_str(&format!(
            "| avg_batch_seconds | {} |\n",
            format_float(metrics.avg_batch_seconds)
        ));
        content.push_str(&format!(
            "| p95_batch_seconds | {} |\n",
            format_float(metrics.p95_batch_seconds)
        ));
        content.push_str(&format!(
            "| p99_batch_seconds | {} |\n",
            format_float(metrics.p99_batch_seconds)
        ));
        content.push_str(&format!(
            "| total_commit_seconds | {} |\n",
            format_float(metrics.total_commit_seconds)
        ));
        content.push_str(&format!(
            "| avg_commit_ms | {} |\n",
            format_float(metrics.avg_commit_ms)
        ));
        content.push_str(&format!(
            "| p95_commit_ms | {} |\n",
            format_float(metrics.p95_commit_ms)
        ));
        content.push_str(&format!(
            "| p99_commit_ms | {} |\n",
            format_float(metrics.p99_commit_ms)
        ));
        content.push_str(&format!(
            "| finalize_seconds | {} |\n",
            format_float(metrics.finalize_seconds)
        ));
        content.push('\n');

        self.write_report_wall_clock_breakdown(&mut content, metrics);
        self.write_report_stall_events(&mut content, metrics);

        content.push_str("## System Pressure\n\n");
        content.push_str("| Metric | Value |\n");
        content.push_str("| --- | ---: |\n");
        content.push_str(&format!(
            "| max_compaction_pending_mb | {} |\n",
            metrics.max_compaction_pending_mb
        ));
        content.push_str(&format!("| max_l0_files | {} |\n", metrics.max_l0_files));
        content.push_str(&format!(
            "| max_imm_memtables | {} |\n",
            metrics.max_imm_memtables
        ));
        content.push_str(&format!(
            "| avg_load_avg_1m | {} |\n",
            format_float(metrics.avg_load_avg_1m)
        ));
        content.push_str(&format!(
            "| max_load_avg_1m | {} |\n",
            format_float(metrics.max_load_avg_1m)
        ));
        content.push_str(&format!(
            "| min_mem_available_mb | {} |\n",
            metrics.min_mem_available_mb
        ));
        content.push_str(&format!(
            "| avg_disk_write_mb_per_batch | {} |\n",
            format_float(metrics.avg_disk_write_mb_per_batch)
        ));
        content.push_str(&format!(
            "| peak_live_cell_count | {} |\n",
            metrics.peak_live_cell_count
        ));
        content.push('\n');

        content.push_str("## Materialization\n\n");
        content.push_str("| Metric | Value |\n");
        content.push_str("| --- | ---: |\n");
        content.push_str(&format!(
            "| streamed_history_rows | {} |\n",
            metrics.streamed_history_rows
        ));
        content.push_str(&format!(
            "| sealed_aggregate_rows | {} |\n",
            metrics.sealed_aggregate_rows
        ));
        content.push_str(&format!(
            "| final_snapshot_rows | {} |\n",
            metrics.final_snapshot_rows
        ));
        content.push_str(&format!(
            "| history_flushes | {} |\n",
            metrics.history_flushes
        ));
        content.push_str(&format!(
            "| sealed_aggregate_flushes | {} |\n",
            metrics.sealed_aggregate_flushes
        ));
        content.push_str(&format!(
            "| final_snapshot_flushes | {} |\n",
            metrics.final_snapshot_flushes
        ));
        content.push('\n');

        if !metrics.peak_owner_memory_bytes.is_empty() {
            content.push_str("## Peak Owner Memory\n\n");
            content.push_str("| Component | Bytes |\n");
            content.push_str("| --- | ---: |\n");
            for (owner, bytes) in owner_memory_entries(&metrics.peak_owner_memory_bytes) {
                content.push_str(&format!("| {} | {} |\n", owner, bytes));
            }
            content.push('\n');
        }

        if let Some(baseline) = baseline {
            content.push_str("## Baseline Comparison\n\n");
            content.push_str(&format!("- Baseline run: {}\n\n", baseline.run_id));
            content.push_str("| Metric | Current | Baseline | Delta |\n");
            content.push_str("| --- | ---: | ---: | ---: |\n");
            for (name, current, previous) in [
                (
                    "wall_clock_seconds",
                    metrics.wall_clock_seconds,
                    baseline.wall_clock_seconds,
                ),
                (
                    "blocks_per_sec_wall",
                    metrics.blocks_per_sec_wall,
                    baseline.blocks_per_sec_wall,
                ),
                (
                    "txs_per_sec_wall",
                    metrics.txs_per_sec_wall,
                    baseline.txs_per_sec_wall,
                ),
                (
                    "blocks_per_batch",
                    metrics.blocks_per_batch,
                    baseline.blocks_per_batch,
                ),
                (
                    "avg_batch_seconds",
                    metrics.avg_batch_seconds,
                    baseline.avg_batch_seconds,
                ),
                (
                    "p95_batch_seconds",
                    metrics.p95_batch_seconds,
                    baseline.p95_batch_seconds,
                ),
                (
                    "p99_batch_seconds",
                    metrics.p99_batch_seconds,
                    baseline.p99_batch_seconds,
                ),
                (
                    "total_commit_seconds",
                    metrics.total_commit_seconds,
                    baseline.total_commit_seconds,
                ),
                (
                    "avg_commit_ms",
                    metrics.avg_commit_ms,
                    baseline.avg_commit_ms,
                ),
                (
                    "p95_commit_ms",
                    metrics.p95_commit_ms,
                    baseline.p95_commit_ms,
                ),
                (
                    "p99_commit_ms",
                    metrics.p99_commit_ms,
                    baseline.p99_commit_ms,
                ),
                (
                    "finalize_seconds",
                    metrics.finalize_seconds,
                    baseline.finalize_seconds,
                ),
                (
                    "avg_load_avg_1m",
                    metrics.avg_load_avg_1m,
                    baseline.avg_load_avg_1m,
                ),
                (
                    "max_load_avg_1m",
                    metrics.max_load_avg_1m,
                    baseline.max_load_avg_1m,
                ),
                (
                    "avg_disk_write_mb_per_batch",
                    metrics.avg_disk_write_mb_per_batch,
                    baseline.avg_disk_write_mb_per_batch,
                ),
            ] {
                content.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    name,
                    format_float(current),
                    format_float(previous),
                    format_delta_pct(current, previous),
                ));
            }
            // min_mem_available_mb is u64 — handle separately
            content.push_str(&format!(
                "| min_mem_available_mb | {} | {} | {} |\n",
                metrics.min_mem_available_mb,
                baseline.min_mem_available_mb,
                format_delta_pct(
                    metrics.min_mem_available_mb as f64,
                    baseline.min_mem_available_mb as f64
                ),
            ));
        }

        fs::write(self.run_dir.join("report.md"), content)?;
        Ok(())
    }

    fn write_report_wall_clock_breakdown(
        &self,
        content: &mut String,
        metrics: &BulkSyncPerfMetrics,
    ) {
        if self.batch_samples.is_empty() {
            return;
        }

        let sum_ms = |f: fn(&BatchSample) -> f64| -> f64 {
            self.batch_samples.iter().map(&f).sum::<f64>() / 1000.0
        };

        let mut phases: Vec<(&str, f64)> = Vec::new();

        // Bulk build phases
        let fetch = sum_ms(|s| s.fetch_ms);
        let facts = sum_ms(|s| s.facts_ms);
        let resolve = sum_ms(|s| s.resolve_ms);
        let reduce = sum_ms(|s| s.reduce_ms);
        let addr_reduce = sum_ms(|s| s.address_reduce_ms);
        let activity_stats = sum_ms(|s| s.activity_stats_ms);
        let history = sum_ms(|s| s.history_ms);
        let flush = sum_ms(|s| s.flush_ms);

        // Pipeline phases
        let parse = sum_ms(|s| s.parse_ms);
        let precompute = sum_ms(|s| s.precompute_ms);
        let write = sum_ms(|s| s.write_ms);
        let prefetch = sum_ms(|s| s.prefetch_ms);
        let finalize_batch = sum_ms(|s| s.finalize_ms);

        // Common
        let commit = metrics.total_commit_seconds;

        if fetch > 0.01 {
            phases.push(("fetch", fetch));
        }
        if facts > 0.01 || resolve > 0.01 {
            phases.push(("facts+resolve", facts + resolve));
        }
        if reduce > 0.01 {
            phases.push(("reduce", reduce));
        }
        if addr_reduce > 0.01 {
            phases.push(("addr_reduce", addr_reduce));
        }
        if activity_stats > 0.01 {
            phases.push(("activity_stats", activity_stats));
        }
        if history > 0.01 {
            phases.push(("history", history));
        }
        if flush > 0.01 {
            phases.push(("flush", flush));
        }
        if parse > 0.01 || precompute > 0.01 {
            phases.push(("parse+precompute", parse + precompute));
        }
        if write > 0.01 {
            phases.push(("write", write));
        }
        if prefetch > 0.01 {
            phases.push(("prefetch", prefetch));
        }
        if finalize_batch > 0.01 {
            phases.push(("batch_finalize", finalize_batch));
        }
        if commit > 0.01 {
            phases.push(("commit", commit));
        }

        // Sort descending by time
        phases.sort_by(|a, b| b.1.total_cmp(&a.1));

        let accounted: f64 = phases.iter().map(|(_, s)| *s).sum();
        let total_batch = metrics.total_batch_seconds;
        let unaccounted = total_batch - accounted;

        content.push_str("## Wall Clock Breakdown\n\n");
        content.push_str("| Phase | Total (s) | % Batch |\n");
        content.push_str("| --- | ---: | ---: |\n");

        for (name, seconds) in &phases {
            let pct = if total_batch > 0.0 {
                seconds / total_batch * 100.0
            } else {
                0.0
            };
            content.push_str(&format!(
                "| {} | {} | {}% |\n",
                name,
                format_float(*seconds),
                format_float(pct)
            ));
        }

        if unaccounted > 0.01 {
            let pct = if total_batch > 0.0 {
                unaccounted / total_batch * 100.0
            } else {
                0.0
            };
            content.push_str(&format!(
                "| _unaccounted_ | {} | {}% |\n",
                format_float(unaccounted),
                format_float(pct)
            ));
        }

        content.push_str(&format!(
            "| **batch total** | {} | 100.0% |\n",
            format_float(total_batch)
        ));

        let overhead = metrics.wall_clock_seconds - total_batch - metrics.finalize_seconds;
        if metrics.finalize_seconds > 0.01 {
            content.push_str(&format!(
                "| finalize | {} | |\n",
                format_float(metrics.finalize_seconds)
            ));
        }
        if overhead > 0.01 {
            content.push_str(&format!("| _overhead_ | {} | |\n", format_float(overhead)));
        }
        content.push_str(&format!(
            "| **wall clock** | {} | |\n",
            format_float(metrics.wall_clock_seconds)
        ));
        content.push('\n');
    }

    fn write_report_stall_events(&self, content: &mut String, metrics: &BulkSyncPerfMetrics) {
        if self.batch_samples.len() < 3 {
            return;
        }

        let avg = metrics.avg_batch_seconds;
        let threshold = avg * STALL_THRESHOLD_MULTIPLIER;

        let stalls: Vec<(usize, &BatchSample)> = self
            .batch_samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.batch_seconds > threshold)
            .collect();

        content.push_str(&format!(
            "## Stall Events ({} detected, threshold: {:.1}x avg = {}s)\n\n",
            stalls.len(),
            STALL_THRESHOLD_MULTIPLIER,
            format_float(threshold)
        ));

        if stalls.is_empty() {
            content.push_str("No stalls detected.\n\n");
            return;
        }

        content
            .push_str("| Batch # | Duration (s) | Ratio | L0 Files | Compaction Pending (MB) |\n");
        content.push_str("| ---: | ---: | ---: | ---: | ---: |\n");
        for (idx, sample) in &stalls {
            let ratio = if avg > 0.0 {
                sample.batch_seconds / avg
            } else {
                0.0
            };
            content.push_str(&format!(
                "| {} | {} | {:.1}x | {} | {} |\n",
                idx + 1,
                format_float(sample.batch_seconds),
                ratio,
                sample.l0_files,
                sample.compaction_pending_mb
            ));
        }
        content.push('\n');
    }

    fn append_trend_line(&self, metrics: &BulkSyncPerfMetrics) -> Result<()> {
        let entry = TrendEntry {
            run_id: &metrics.run_id,
            build_version: &self.build_version,
            status: &metrics.status,
            started_at_utc: &metrics.started_at_utc,
            finished_at_utc: metrics.finished_at_utc.as_deref(),
            wall_clock_seconds: metrics.wall_clock_seconds,
            blocks_per_sec_wall: metrics.blocks_per_sec_wall,
            txs_per_sec_wall: metrics.txs_per_sec_wall,
            batches: metrics.batches,
            blocks: metrics.blocks,
            total_txs: metrics.total_txs,
            avg_batch_seconds: metrics.avg_batch_seconds,
            total_commit_seconds: metrics.total_commit_seconds,
            finalize_seconds: metrics.finalize_seconds,
            stall_count: metrics.stall_count,
        };

        let json = serde_json::to_string(&entry)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.output_root.join("trend.jsonl"))?;
        writeln!(file, "{json}")?;
        Ok(())
    }

    fn write_report_environment_section(&self, content: &mut String) -> Result<()> {
        let (env, config) = match (&self.environment, &self.rocksdb_config) {
            (Some(e), Some(c)) => (e, c),
            _ => return Ok(()),
        };

        content.push_str("\n## Environment\n\n");

        // Check baseline for diff
        let baseline_env_path = self.output_root.join("latest/environment.env");
        if baseline_env_path.exists() {
            let current_pairs = environment_key_value_pairs(env, config);
            let baseline_content = fs::read_to_string(&baseline_env_path)?;
            let baseline_map: HashMap<String, String> = baseline_content
                .lines()
                .filter(|line| !line.starts_with('#') && !line.is_empty())
                .filter_map(|line| line.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            let mut diffs: Vec<(String, String, String)> = Vec::new();
            for (key, current_val) in &current_pairs {
                let baseline_val = baseline_map.get(key).cloned().unwrap_or_default();
                if *current_val != baseline_val {
                    diffs.push((key.clone(), current_val.clone(), baseline_val));
                }
            }

            if diffs.is_empty() {
                content.push_str("Environment: identical to baseline\n");
            } else {
                content.push_str("| Parameter | Current | Baseline |\n");
                content.push_str("| --- | --- | --- |\n");
                for (key, current_val, baseline_val) in &diffs {
                    content.push_str(&format!(
                        "| {} | {} | {} |\n",
                        key, current_val, baseline_val
                    ));
                }
            }
        } else {
            content.push_str("| Parameter | Value |\n");
            content.push_str("| --- | --- |\n");
            for (key, value) in environment_key_value_pairs(env, config) {
                content.push_str(&format!("| {} | {} |\n", key, value));
            }
        }

        Ok(())
    }

    fn append_sample<T: Serialize>(&self, kind: &str, sample: &T) -> Result<()> {
        let json = serde_json::to_string(&SampleRecord { kind, sample })?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.run_dir.join("samples.jsonl"))?;
        writeln!(file, "{json}")?;
        Ok(())
    }
}

fn environment_key_value_pairs(
    env: &crate::sys_info::EnvironmentSnapshot,
    config: &RocksDbConfig,
) -> Vec<(String, String)> {
    vec![
        ("cpu_model".to_string(), env.cpu_model.clone()),
        ("cpu_cores".to_string(), env.cpu_cores.to_string()),
        ("ram_total_mb".to_string(), env.ram_total_mb.to_string()),
        ("disk_device".to_string(), env.disk_device.clone()),
        ("disk_scheduler".to_string(), env.disk_scheduler.clone()),
        ("kernel".to_string(), env.kernel.clone()),
        ("filesystem".to_string(), env.filesystem.clone()),
        (
            "rocksdb_budget_gb".to_string(),
            config.rocksdb_budget_gb.to_string(),
        ),
        (
            "block_cache_bulk_mb".to_string(),
            config.block_cache_bulk_mb.to_string(),
        ),
        ("wbm_bulk_mb".to_string(), config.wbm_bulk_mb.to_string()),
        (
            "write_buffer_mega_mb".to_string(),
            config.write_buffer_mega_mb.to_string(),
        ),
        (
            "l0_slowdown_bulk".to_string(),
            config.l0_slowdown_bulk.to_string(),
        ),
        ("l0_stop_bulk".to_string(), config.l0_stop_bulk.to_string()),
        (
            "max_background_jobs".to_string(),
            config.max_background_jobs.to_string(),
        ),
        (
            "max_subcompactions".to_string(),
            config.max_subcompactions.to_string(),
        ),
        (
            "unordered_write".to_string(),
            config.unordered_write.to_string(),
        ),
        (
            "direct_io_reads".to_string(),
            config.direct_io_reads.to_string(),
        ),
    ]
}

#[derive(Serialize)]
struct SampleRecord<'a, T> {
    kind: &'a str,
    sample: &'a T,
}

#[derive(Serialize)]
struct TrendEntry<'a> {
    run_id: &'a str,
    build_version: &'a str,
    status: &'a str,
    started_at_utc: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at_utc: Option<&'a str>,
    wall_clock_seconds: f64,
    blocks_per_sec_wall: f64,
    txs_per_sec_wall: f64,
    batches: u64,
    blocks: u64,
    total_txs: u64,
    avg_batch_seconds: f64,
    total_commit_seconds: f64,
    finalize_seconds: f64,
    stall_count: u64,
}

fn utc_now_string() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn elapsed_wall_clock_seconds(started_at_utc: &str, finished_at_utc: Option<&str>) -> f64 {
    let started_at = parse_rfc3339_utc(started_at_utc);
    let finished_at = finished_at_utc
        .map(parse_rfc3339_utc)
        .unwrap_or_else(Utc::now);
    let elapsed_ms = finished_at
        .signed_duration_since(started_at)
        .num_milliseconds();
    assert!(
        elapsed_ms >= 0,
        "bulk sync perf finished_at_utc must not be earlier than started_at_utc"
    );
    elapsed_ms as f64 / 1000.0
}

fn parse_rfc3339_utc(value: &str) -> chrono::DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap_or_else(|err| panic!("bulk sync perf timestamp must be RFC3339: {value} ({err})"))
        .with_timezone(&Utc)
}

fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn percentile(mut values: Vec<f64>, pct: usize) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = (pct * values.len()).div_ceil(100);
    values[index.saturating_sub(1).min(values.len() - 1)]
}

fn format_float(value: f64) -> String {
    format!("{value:.3}")
}

fn format_delta_pct(current: f64, baseline: f64) -> String {
    if baseline == 0.0 {
        return "n/a".to_string();
    }
    format!("{:.2}%", ((current - baseline) / baseline) * 100.0)
}

fn owner_memory_entries(entries: &HashMap<String, u64>) -> Vec<(String, u64)> {
    let mut rows = entries
        .iter()
        .map(|(owner, bytes)| (owner.clone(), *bytes))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn read_metrics_env(path: &Path) -> Result<Option<BulkSyncPerfMetrics>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let map = content
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<HashMap<_, _>>();
    let peak_owner_memory_bytes = map
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("peak_owner_memory_bytes_")
                .map(|owner| (owner.to_string(), value.parse::<u64>().unwrap_or_default()))
        })
        .collect::<HashMap<_, _>>();

    Ok(Some(BulkSyncPerfMetrics {
        run_id: read_string(&map, "run_id"),
        status: read_string(&map, "status"),
        started_at_utc: read_string(&map, "started_at_utc"),
        finished_at_utc: map.get("finished_at_utc").cloned(),
        wall_clock_seconds: read_f64(&map, "wall_clock_seconds"),
        batches: read_u64(&map, "batches"),
        blocks: read_u64(&map, "blocks"),
        total_txs: read_u64(&map, "total_txs"),
        blocks_per_sec_wall: read_f64(&map, "blocks_per_sec_wall"),
        txs_per_sec_wall: read_f64(&map, "txs_per_sec_wall"),
        blocks_per_batch: read_f64(&map, "blocks_per_batch"),
        avg_batch_seconds: read_f64(&map, "avg_batch_seconds"),
        p95_batch_seconds: read_f64(&map, "p95_batch_seconds"),
        p99_batch_seconds: read_f64(&map, "p99_batch_seconds"),
        total_commit_seconds: read_f64(&map, "total_commit_seconds"),
        avg_commit_ms: read_f64(&map, "avg_commit_ms"),
        p95_commit_ms: read_f64(&map, "p95_commit_ms"),
        p99_commit_ms: read_f64(&map, "p99_commit_ms"),
        finalize_seconds: read_f64(&map, "finalize_seconds"),
        max_compaction_pending_mb: read_u64(&map, "max_compaction_pending_mb"),
        max_l0_files: read_u64(&map, "max_l0_files"),
        max_imm_memtables: read_u64(&map, "max_imm_memtables"),
        avg_load_avg_1m: read_f64(&map, "avg_load_avg_1m"),
        max_load_avg_1m: read_f64(&map, "max_load_avg_1m"),
        min_mem_available_mb: read_u64(&map, "min_mem_available_mb"),
        avg_disk_write_mb_per_batch: read_f64(&map, "avg_disk_write_mb_per_batch"),
        peak_owner_memory_bytes,
        peak_live_cell_count: read_u64(&map, "peak_live_cell_count"),
        streamed_history_rows: read_u64(&map, "streamed_history_rows"),
        sealed_aggregate_rows: read_u64(&map, "sealed_aggregate_rows"),
        final_snapshot_rows: read_u64(&map, "final_snapshot_rows"),
        history_flushes: read_u64(&map, "history_flushes"),
        sealed_aggregate_flushes: read_u64(&map, "sealed_aggregate_flushes"),
        final_snapshot_flushes: read_u64(&map, "final_snapshot_flushes"),
        total_batch_seconds: read_f64(&map, "total_batch_seconds"),
        stall_count: read_u64(&map, "stall_count"),
    }))
}

fn read_string(map: &HashMap<String, String>, key: &str) -> String {
    map.get(key).cloned().unwrap_or_default()
}

fn read_u64(map: &HashMap<String, String>, key: &str) -> u64 {
    map.get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn read_f64(map: &HashMap<String, String>, key: &str) -> f64 {
    map.get(key)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{BatchSample, BulkSyncPerfRun, HeartbeatSample, RocksDbConfig};
    use crate::sync::MaterializationReport;
    use crate::sys_info::EnvironmentSnapshot;
    use tempfile::TempDir;

    const TEST_BUILD_VERSION: &str = "0.1.0+feature/foo@abcdef123456";

    fn test_batch_sample(
        blocks: u64,
        batch_seconds: f64,
        commit_ms: f64,
        compaction_pending_mb: u64,
        l0_files: u64,
        imm_memtables: u64,
    ) -> BatchSample {
        BatchSample::new(
            blocks,
            batch_seconds,
            commit_ms,
            compaction_pending_mb,
            l0_files,
            imm_memtables,
            String::new(),
            0.0,
            0,
            0.0,
            0.0,
        )
    }

    fn test_env_snapshot() -> EnvironmentSnapshot {
        EnvironmentSnapshot {
            cpu_model: "AMD Ryzen 9 7950X".to_string(),
            cpu_cores: 32,
            ram_total_mb: 95326,
            kernel: "6.19.6-1-cachyos-eevdf".to_string(),
            disk_device: "nvme0n1".to_string(),
            disk_scheduler: "none".to_string(),
            filesystem: "btrfs".to_string(),
        }
    }

    fn test_rocksdb_config() -> RocksDbConfig {
        RocksDbConfig {
            rocksdb_budget_gb: 22,
            block_cache_bulk_mb: 4096,
            wbm_bulk_mb: 2048,
            write_buffer_mega_mb: 256,
            l0_slowdown_bulk: 40,
            l0_stop_bulk: 60,
            max_background_jobs: 8,
            max_subcompactions: 4,
            unordered_write: true,
            direct_io_reads: false,
        }
    }

    #[test]
    fn test_bulk_sync_perf_run_start_writes_initial_artifacts() {
        let dir = TempDir::new().unwrap();
        let run = BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        assert!(dir.path().join("run-1/metadata.env").exists());
        assert!(dir.path().join("run-1/status.env").exists());
        assert!(dir.path().join("run-1/metrics.env").exists());
        assert_eq!(run.status(), "running");
    }

    #[test]
    fn test_bulk_sync_perf_run_start_writes_build_version_to_metadata() {
        let dir = TempDir::new().unwrap();
        BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        let metadata = std::fs::read_to_string(dir.path().join("run-1/metadata.env")).unwrap();
        assert!(metadata.contains("build_version=0.1.0+feature/foo@abcdef123456"));
    }

    #[test]
    fn test_bulk_sync_perf_completed_run_updates_latest() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        run.finish_completed().unwrap();

        assert!(dir.path().join("latest/metrics.env").exists());
    }

    #[test]
    fn test_bulk_sync_perf_completed_run_writes_build_version_to_report_and_latest() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        let latest_metadata =
            std::fs::read_to_string(dir.path().join("latest/metadata.env")).unwrap();

        assert!(report.contains("Build Version: 0.1.0+feature/foo@abcdef123456"));
        assert!(latest_metadata.contains("build_version=0.1.0+feature/foo@abcdef123456"));
    }

    #[test]
    fn test_bulk_sync_perf_failed_run_does_not_update_latest() {
        let dir = TempDir::new().unwrap();
        let mut completed =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        completed.finish_completed().unwrap();

        let mut failed =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-2", TEST_BUILD_VERSION).unwrap();
        failed.finish_failed().unwrap();

        let latest = std::fs::read_to_string(dir.path().join("latest/metrics.env")).unwrap();
        assert!(latest.contains("run_id=run-1"));
    }

    #[test]
    fn test_bulk_sync_metrics_use_committed_batch_samples_for_percentiles() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(20, 2.0, 80.0, 200, 7, 2))
            .unwrap();
        run.record_heartbeat_sample(HeartbeatSample::new(15, 100, 150, 6, 1))
            .unwrap();

        let metrics = run.build_metrics_for_test("running");

        assert_eq!(metrics.batches, 2);
        assert_eq!(metrics.blocks, 30);
        assert_eq!(metrics.max_l0_files, 7);
        assert_eq!(metrics.max_imm_memtables, 2);
    }

    #[test]
    fn test_report_includes_baseline_table_when_latest_exists() {
        let dir = TempDir::new().unwrap();
        let mut baseline =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        baseline
            .record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        baseline.finish_completed().unwrap();

        let mut current =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-2", TEST_BUILD_VERSION).unwrap();
        current
            .record_batch_sample(test_batch_sample(10, 2.0, 80.0, 120, 5, 1))
            .unwrap();
        current.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-2/report.md")).unwrap();
        assert!(report.contains("## Baseline Comparison"));
        assert!(report.contains("avg_batch_seconds"));
    }

    #[test]
    fn test_metrics_and_report_include_wall_clock_fields() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(20, 2.0, 80.0, 200, 7, 2))
            .unwrap();

        run.finish_completed().unwrap();

        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(metrics.contains("wall_clock_seconds="));
        assert!(metrics.contains("blocks_per_sec_wall="));
        assert!(metrics.contains("blocks_per_batch=15.000"));
        assert!(metrics.contains("total_commit_seconds=0.120"));

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("wall_clock_seconds"));
        assert!(report.contains("blocks_per_sec_wall"));
        assert!(report.contains("blocks_per_batch"));
        assert!(report.contains("total_commit_seconds"));
    }

    #[test]
    fn test_batch_samples_omit_retired_nft_precompute_fields_but_keep_live_workload_fields() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();

        let samples = std::fs::read_to_string(dir.path().join("run-1/samples.jsonl")).unwrap();
        assert!(samples.contains("\"txs\""));
        assert!(samples.contains("\"cells\""));
        assert!(samples.contains("\"inputs\""));
        assert!(samples.contains("\"parse_ms\""));
        assert!(samples.contains("\"precompute_ms\""));
        assert!(!samples.contains("\"nft_precompute_ms\""));
        assert!(!samples.contains("\"nft_fallback_db_ms\""));
        assert!(!samples.contains("\"nft_dotbit_witness_parse_ms\""));
        assert!(!samples.contains("\"nft_output_scan_ms\""));
        assert!(!samples.contains("\"nft_input_scan_ms\""));
        assert!(samples.contains("\"write_ms\""));
        assert!(samples.contains("\"t1_ms\""));
        assert!(samples.contains("\"t_act_ms\""));
    }

    #[test]
    fn test_metrics_and_report_do_not_aggregate_batch_breakdown_fields() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(BatchSample {
            txs: 123,
            cells: 456,
            inputs: 321,
            parse_ms: 11.0,
            precompute_ms: 22.0,
            write_ms: 44.0,
            t1_ms: 55.0,
            t_act_ms: 66.0,
            ..test_batch_sample(10, 1.0, 40.0, 100, 4, 1)
        })
        .unwrap();
        run.finish_completed().unwrap();

        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(!metrics.contains("\ntxs="));
        assert!(!metrics.contains("\nparse_ms="));
        assert!(!metrics.contains("\nwrite_ms="));

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(!report.contains("nft_precompute_ms"));
        assert!(!report.contains("t_act_ms"));
    }

    #[test]
    fn test_set_environment_writes_environment_env() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        run.set_environment(test_env_snapshot(), test_rocksdb_config())
            .unwrap();

        let env_file = std::fs::read_to_string(dir.path().join("run-1/environment.env")).unwrap();

        assert!(env_file.contains("cpu_model=AMD Ryzen 9 7950X"));
        assert!(env_file.contains("cpu_cores=32"));
        assert!(env_file.contains("ram_total_mb=95326"));
        assert!(env_file.contains("disk_device=nvme0n1"));
        assert!(env_file.contains("disk_scheduler=none"));
        assert!(env_file.contains("kernel=6.19.6-1-cachyos-eevdf"));
        assert!(env_file.contains("filesystem=btrfs"));
        assert!(env_file.contains("rocksdb_budget_gb=22"));
        assert!(env_file.contains("block_cache_bulk_mb=4096"));
        assert!(env_file.contains("wbm_bulk_mb=2048"));
        assert!(env_file.contains("write_buffer_mega_mb=256"));
        assert!(env_file.contains("l0_slowdown_bulk=40"));
        assert!(env_file.contains("l0_stop_bulk=60"));
        assert!(env_file.contains("max_background_jobs=8"));
        assert!(env_file.contains("max_subcompactions=4"));
        assert!(env_file.contains("unordered_write=true"));
        assert!(env_file.contains("direct_io_reads=false"));
    }

    #[test]
    fn test_metrics_include_environment_pressure_aggregates() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        run.record_batch_sample(BatchSample::new(
            10,
            1.0,
            40.0,
            100,
            4,
            1,
            String::new(),
            4.5,
            80000,
            10.0,
            200.0,
        ))
        .unwrap();
        run.record_batch_sample(BatchSample::new(
            20,
            2.0,
            80.0,
            200,
            7,
            2,
            String::new(),
            8.5,
            60000,
            20.0,
            400.0,
        ))
        .unwrap();

        run.finish_completed().unwrap();

        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(metrics.contains("avg_load_avg_1m=6.500"));
        assert!(metrics.contains("max_load_avg_1m=8.500"));
        assert!(metrics.contains("min_mem_available_mb=60000"));
        assert!(metrics.contains("avg_disk_write_mb_per_batch=300.000"));
    }

    #[test]
    fn test_perf_report_tracks_owner_memory_breakdown_and_materialization_totals() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        let mut sample = test_batch_sample(10, 1.0, 40.0, 100, 4, 1);
        sample
            .owner_memory_bytes
            .insert("live_cells".to_string(), 4096);
        sample
            .owner_memory_bytes
            .insert("owner.dao".to_string(), 2048);
        run.record_batch_sample(sample).unwrap();
        run.set_materialization_report(MaterializationReport {
            streamed_history_rows: 11,
            sealed_aggregate_rows: 7,
            final_snapshot_rows: 3,
            history_flushes: 2,
            sealed_aggregate_flushes: 4,
            final_snapshot_flushes: 1,
        })
        .unwrap();
        run.finish_completed().unwrap();

        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(metrics.contains("peak_owner_memory_bytes_live_cells=4096"));
        assert!(metrics.contains("peak_owner_memory_bytes_owner.dao=2048"));
        assert!(metrics.contains("streamed_history_rows=11"));
        assert!(metrics.contains("sealed_aggregate_rows=7"));
        assert!(metrics.contains("final_snapshot_rows=3"));
        assert!(metrics.contains("history_flushes=2"));
        assert!(metrics.contains("sealed_aggregate_flushes=4"));
        assert!(metrics.contains("final_snapshot_flushes=1"));

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("## Peak Owner Memory"));
        assert!(report.contains("live_cells"));
        assert!(report.contains("owner.dao"));
        assert!(report.contains("streamed_history_rows"));
        assert!(report.contains("final_snapshot_flushes"));
    }

    #[test]
    fn test_report_includes_environment_section() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.set_environment(test_env_snapshot(), test_rocksdb_config())
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("## Environment"));
        assert!(report.contains("cpu_model"));
        assert!(report.contains("AMD Ryzen 9 7950X"));
        assert!(report.contains("rocksdb_budget_gb"));
    }

    #[test]
    fn test_report_environment_diff_when_baseline_exists() {
        let dir = TempDir::new().unwrap();

        // Baseline run with environment
        let mut baseline =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        baseline
            .set_environment(test_env_snapshot(), test_rocksdb_config())
            .unwrap();
        baseline
            .record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        baseline.finish_completed().unwrap();

        // Current run with different config
        let mut current =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-2", TEST_BUILD_VERSION).unwrap();
        let mut changed_config = test_rocksdb_config();
        changed_config.block_cache_bulk_mb = 8192; // Changed from 4096
        current
            .set_environment(test_env_snapshot(), changed_config)
            .unwrap();
        current
            .record_batch_sample(test_batch_sample(10, 2.0, 80.0, 120, 5, 1))
            .unwrap();
        current.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-2/report.md")).unwrap();
        assert!(report.contains("## Environment"));
        assert!(report.contains("block_cache_bulk_mb"));
        assert!(report.contains("8192"));
        assert!(report.contains("4096"));
    }

    #[test]
    fn test_report_environment_identical_to_baseline() {
        let dir = TempDir::new().unwrap();

        // Baseline run
        let mut baseline =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        baseline
            .set_environment(test_env_snapshot(), test_rocksdb_config())
            .unwrap();
        baseline
            .record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        baseline.finish_completed().unwrap();

        // Current run with same env
        let mut current =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-2", TEST_BUILD_VERSION).unwrap();
        current
            .set_environment(test_env_snapshot(), test_rocksdb_config())
            .unwrap();
        current
            .record_batch_sample(test_batch_sample(10, 2.0, 80.0, 120, 5, 1))
            .unwrap();
        current.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-2/report.md")).unwrap();
        assert!(report.contains("Environment: identical to baseline"));
    }

    #[test]
    fn test_environment_env_copied_to_latest_on_completion() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.set_environment(test_env_snapshot(), test_rocksdb_config())
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.finish_completed().unwrap();

        let latest_env =
            std::fs::read_to_string(dir.path().join("latest/environment.env")).unwrap();
        assert!(latest_env.contains("cpu_model=AMD Ryzen 9 7950X"));
        assert!(latest_env.contains("rocksdb_budget_gb=22"));
    }

    #[test]
    fn test_batch_samples_include_environment_fields_in_jsonl() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(BatchSample::new(
            10,
            1.0,
            40.0,
            100,
            4,
            1,
            "2026-03-11T10:00:00.000Z".to_string(),
            4.5,
            80000,
            10.0,
            200.0,
        ))
        .unwrap();

        let samples = std::fs::read_to_string(dir.path().join("run-1/samples.jsonl")).unwrap();
        assert!(samples.contains("\"timestamp_utc\""));
        assert!(samples.contains("\"load_avg_1m\""));
        assert!(samples.contains("\"mem_available_mb\""));
        assert!(samples.contains("\"disk_read_mb\""));
        assert!(samples.contains("\"disk_write_mb\""));
    }

    #[test]
    fn test_batch_sample_includes_engine_and_bulk_build_sub_step_fields() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        let mut sample = test_batch_sample(10, 1.0, 40.0, 100, 4, 1);
        sample.engine = "bulk_build".to_string();
        sample.fetch_ms = 50.0;
        sample.facts_ms = 120.0;
        sample.resolve_ms = 30.0;
        sample.reduce_ms = 200.0;
        sample.flush_ms = 80.0;
        sample.live_cell_count = 5000;
        sample.cumulative_history_rows = 100;
        sample.cumulative_sealed_rows = 42;
        sample.cumulative_snapshot_rows = 0;
        run.record_batch_sample(sample).unwrap();

        let samples = std::fs::read_to_string(dir.path().join("run-1/samples.jsonl")).unwrap();
        assert!(samples.contains("\"engine\":\"bulk_build\""));
        assert!(samples.contains("\"fetch_ms\":50.0"));
        assert!(samples.contains("\"facts_ms\":120.0"));
        assert!(samples.contains("\"resolve_ms\":30.0"));
        assert!(samples.contains("\"reduce_ms\":200.0"));
        assert!(samples.contains("\"flush_ms\":80.0"));
        assert!(samples.contains("\"live_cell_count\":5000"));
        assert!(samples.contains("\"cumulative_history_rows\":100"));
        assert!(samples.contains("\"cumulative_sealed_rows\":42"));
        assert!(samples.contains("\"cumulative_snapshot_rows\":0"));
    }

    #[test]
    fn test_metrics_include_throughput_and_finalize_fields() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        let mut sample = test_batch_sample(10, 1.0, 40.0, 100, 4, 1);
        sample.txs = 500;
        sample.live_cell_count = 3000;
        run.record_batch_sample(sample).unwrap();
        run.set_finalize_seconds(12.5);
        run.finish_completed().unwrap();

        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(metrics.contains("total_txs=500"));
        assert!(metrics.contains("txs_per_sec_wall="));
        assert!(metrics.contains("finalize_seconds=12.500"));
        assert!(metrics.contains("peak_live_cell_count=3000"));

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("## Throughput"));
        assert!(report.contains("## Batch Timing"));
        assert!(report.contains("## System Pressure"));
        assert!(report.contains("## Materialization"));
        assert!(report.contains("txs_per_sec_wall"));
        assert!(report.contains("finalize_seconds"));
        assert!(report.contains("peak_live_cell_count"));
    }

    #[test]
    fn test_pipeline_batch_sample_defaults_to_pipeline_engine() {
        let sample = test_batch_sample(10, 1.0, 40.0, 100, 4, 1);
        assert_eq!(sample.engine, "pipeline");
        assert_eq!(sample.fetch_ms, 0.0);
        assert_eq!(sample.facts_ms, 0.0);
        assert_eq!(sample.resolve_ms, 0.0);
        assert_eq!(sample.reduce_ms, 0.0);
        assert_eq!(sample.flush_ms, 0.0);
        assert_eq!(sample.live_cell_count, 0);
        assert_eq!(sample.cumulative_history_rows, 0);
    }

    // ── Wall Clock Breakdown tests ──────────────────────────────────────

    #[test]
    fn test_report_includes_wall_clock_breakdown_with_bulk_build_phases() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        let mut sample = test_batch_sample(10, 2.0, 200.0, 100, 4, 1);
        sample.engine = "bulk_build".to_string();
        sample.fetch_ms = 300.0;
        sample.facts_ms = 150.0;
        sample.resolve_ms = 50.0;
        sample.reduce_ms = 400.0;
        sample.history_ms = 100.0;
        sample.flush_ms = 500.0;
        run.record_batch_sample(sample).unwrap();
        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("## Wall Clock Breakdown"));
        assert!(report.contains("| fetch |"));
        assert!(report.contains("| facts+resolve |"));
        assert!(report.contains("| reduce |"));
        assert!(report.contains("| history |"));
        assert!(report.contains("| flush |"));
        assert!(report.contains("| commit |"));
        assert!(report.contains("| **batch total** |"));
        assert!(report.contains("| **wall clock** |"));
        assert!(report.contains("% Batch"));
    }

    #[test]
    fn test_wall_clock_breakdown_omits_zero_phases() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        // Pipeline sample: only commit_ms is non-zero from common fields
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("## Wall Clock Breakdown"));
        assert!(report.contains("| commit |"));
        // Bulk build phases should not appear for pipeline-only batches
        assert!(!report.contains("| fetch |"));
        assert!(!report.contains("| reduce |"));
    }

    #[test]
    fn test_wall_clock_breakdown_phases_sorted_descending() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        let mut sample = test_batch_sample(10, 2.0, 100.0, 100, 4, 1);
        sample.engine = "bulk_build".to_string();
        sample.fetch_ms = 100.0; // 0.1s — smaller
        sample.reduce_ms = 800.0; // 0.8s — larger
        run.record_batch_sample(sample).unwrap();
        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        let reduce_pos = report.find("| reduce |").unwrap();
        let fetch_pos = report.find("| fetch |").unwrap();
        assert!(
            reduce_pos < fetch_pos,
            "reduce (0.8s) should appear before fetch (0.1s)"
        );
    }

    #[test]
    fn test_metrics_include_total_batch_seconds_and_stall_count() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(20, 2.0, 80.0, 200, 7, 2))
            .unwrap();
        run.record_batch_sample(test_batch_sample(15, 1.5, 60.0, 150, 5, 1))
            .unwrap();
        run.finish_completed().unwrap();

        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(metrics.contains("total_batch_seconds=4.500"));
        assert!(metrics.contains("stall_count=0"));
    }

    // ── Stall Detection tests ───────────────────────────────────────────

    #[test]
    fn test_stall_events_detected_when_batch_exceeds_threshold() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        // 3 normal batches at ~1s, then 1 stall at 5s (>2x avg of 1.0)
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 5.0, 40.0, 500, 20, 3))
            .unwrap();
        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("## Stall Events (1 detected"));
        assert!(report.contains("| 4 |")); // Batch #4

        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(metrics.contains("stall_count=1"));
    }

    #[test]
    fn test_stall_events_no_stalls_message() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        // 3 uniform batches — no stalls
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        assert!(report.contains("## Stall Events (0 detected"));
        assert!(report.contains("No stalls detected."));
    }

    #[test]
    fn test_stall_events_skipped_with_fewer_than_3_batches() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 5.0, 40.0, 500, 20, 3))
            .unwrap();
        run.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
        // With <3 batches, stall section should not appear
        assert!(!report.contains("## Stall Events"));
        // And stall_count should be 0 in metrics
        let metrics = std::fs::read_to_string(dir.path().join("run-1/metrics.env")).unwrap();
        assert!(metrics.contains("stall_count=0"));
    }

    // ── Trend File tests ────────────────────────────────────────────────

    #[test]
    fn test_trend_jsonl_created_on_completed_run() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.finish_completed().unwrap();

        let trend = std::fs::read_to_string(dir.path().join("trend.jsonl")).unwrap();
        let lines: Vec<&str> = trend.lines().collect();
        assert_eq!(lines.len(), 1);

        let entry: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(entry["run_id"], "run-1");
        assert_eq!(entry["build_version"], TEST_BUILD_VERSION);
        assert_eq!(entry["status"], "completed");
        assert_eq!(entry["blocks"], 10);
        assert!(entry["wall_clock_seconds"].as_f64().is_some());
        assert!(entry["blocks_per_sec_wall"].as_f64().is_some());
        assert!(entry["stall_count"].as_u64().is_some());
    }

    #[test]
    fn test_trend_jsonl_appends_across_multiple_runs() {
        let dir = TempDir::new().unwrap();

        let mut run1 =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run1.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run1.finish_completed().unwrap();

        let mut run2 =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-2", TEST_BUILD_VERSION).unwrap();
        run2.record_batch_sample(test_batch_sample(20, 2.0, 80.0, 200, 7, 2))
            .unwrap();
        run2.finish_completed().unwrap();

        let trend = std::fs::read_to_string(dir.path().join("trend.jsonl")).unwrap();
        let lines: Vec<&str> = trend.lines().collect();
        assert_eq!(lines.len(), 2);

        let entry1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let entry2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(entry1["run_id"], "run-1");
        assert_eq!(entry2["run_id"], "run-2");
        assert_eq!(entry1["blocks"], 10);
        assert_eq!(entry2["blocks"], 20);
    }

    #[test]
    fn test_trend_jsonl_not_created_on_failed_run() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.finish_failed().unwrap();

        assert!(!dir.path().join("trend.jsonl").exists());
    }

    #[test]
    fn test_trend_entry_includes_stall_count() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-1", TEST_BUILD_VERSION).unwrap();

        // 3 normal + 1 stall
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(test_batch_sample(10, 5.0, 40.0, 500, 20, 3))
            .unwrap();
        run.finish_completed().unwrap();

        let trend = std::fs::read_to_string(dir.path().join("trend.jsonl")).unwrap();
        let entry: serde_json::Value = serde_json::from_str(trend.lines().next().unwrap()).unwrap();
        assert_eq!(entry["stall_count"], 1);
    }
}
