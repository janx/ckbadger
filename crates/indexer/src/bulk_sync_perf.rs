use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const STATUS_RUNNING: &str = "running";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";

#[derive(Debug, Clone, Serialize)]
pub struct BatchSample {
    pub blocks: u64,
    pub batch_seconds: f64,
    pub commit_ms: f64,
    pub compaction_pending_mb: u64,
    pub l0_files: u64,
    pub imm_memtables: u64,
}

impl BatchSample {
    pub fn new(
        blocks: u64,
        batch_seconds: f64,
        commit_ms: f64,
        compaction_pending_mb: u64,
        l0_files: u64,
        imm_memtables: u64,
    ) -> Self {
        Self {
            blocks,
            batch_seconds,
            commit_ms,
            compaction_pending_mb,
            l0_files,
            imm_memtables,
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
    pub batches: u64,
    pub blocks: u64,
    pub avg_batch_seconds: f64,
    pub p95_batch_seconds: f64,
    pub p99_batch_seconds: f64,
    pub avg_commit_ms: f64,
    pub p95_commit_ms: f64,
    pub p99_commit_ms: f64,
    pub max_compaction_pending_mb: u64,
    pub max_l0_files: u64,
    pub max_imm_memtables: u64,
}

pub struct BulkSyncPerfRun {
    output_root: PathBuf,
    run_dir: PathBuf,
    run_id: String,
    started_at_utc: String,
    status: String,
    batch_samples: Vec<BatchSample>,
    heartbeat_samples: Vec<HeartbeatSample>,
}

impl BulkSyncPerfRun {
    pub fn start(output_root: &Path, run_id: impl Into<String>) -> Result<Self> {
        let run_id = run_id.into();
        let run_dir = output_root.join(&run_id);
        fs::create_dir_all(&run_dir)?;

        let run = Self {
            output_root: output_root.to_path_buf(),
            run_dir,
            run_id,
            started_at_utc: utc_now_string(),
            status: STATUS_RUNNING.to_string(),
            batch_samples: Vec::new(),
            heartbeat_samples: Vec::new(),
        };
        run.write_metadata()?;
        run.write_status(None)?;
        run.write_metrics_file(&run.build_metrics(STATUS_RUNNING, None))?;
        Ok(run)
    }

    #[cfg(test)]
    pub fn start_for_test(output_root: &Path, run_id: &str) -> Result<Self> {
        Self::start(output_root, run_id)
    }

    pub fn status(&self) -> &str {
        &self.status
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

    #[cfg(test)]
    pub fn build_metrics_for_test(&self, status: &str) -> BulkSyncPerfMetrics {
        self.build_metrics(status, None)
    }

    pub fn finish_completed(&mut self) -> Result<()> {
        self.status = STATUS_COMPLETED.to_string();
        self.finalize()?;
        self.update_latest()?;
        Ok(())
    }

    pub fn finish_failed(&mut self) -> Result<()> {
        self.status = STATUS_FAILED.to_string();
        self.finalize()
    }

    fn finalize(&self) -> Result<()> {
        let finished_at_utc = utc_now_string();
        let metrics = self.build_metrics(&self.status, Some(finished_at_utc.clone()));
        self.write_status(Some(&finished_at_utc))?;
        self.write_metrics_file(&metrics)?;
        self.write_report(&metrics)?;
        Ok(())
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
        Ok(())
    }

    fn build_metrics(&self, status: &str, finished_at_utc: Option<String>) -> BulkSyncPerfMetrics {
        let batches = self.batch_samples.len() as u64;
        let blocks = self.batch_samples.iter().map(|sample| sample.blocks).sum();

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

        BulkSyncPerfMetrics {
            run_id: self.run_id.clone(),
            status: status.to_string(),
            started_at_utc: self.started_at_utc.clone(),
            finished_at_utc,
            batches,
            blocks,
            avg_batch_seconds: average(&batch_seconds),
            p95_batch_seconds: percentile(batch_seconds.clone(), 95),
            p99_batch_seconds: percentile(batch_seconds, 99),
            avg_commit_ms: average(&commit_ms),
            p95_commit_ms: percentile(commit_ms.clone(), 95),
            p99_commit_ms: percentile(commit_ms, 99),
            max_compaction_pending_mb,
            max_l0_files,
            max_imm_memtables,
        }
    }

    fn write_metadata(&self) -> Result<()> {
        let content = format!(
            "run_id={}\nstarted_at_utc={}\n",
            self.run_id, self.started_at_utc
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

    fn write_metrics_file(&self, metrics: &BulkSyncPerfMetrics) -> Result<()> {
        let mut content = format!(
            "run_id={}\nstatus={}\nstarted_at_utc={}\n",
            metrics.run_id, metrics.status, metrics.started_at_utc
        );
        if let Some(finished_at_utc) = metrics.finished_at_utc.as_deref() {
            content.push_str(&format!("finished_at_utc={}\n", finished_at_utc));
        }
        content.push_str(&format!(
            "batches={}\nblocks={}\navg_batch_seconds={}\np95_batch_seconds={}\np99_batch_seconds={}\navg_commit_ms={}\np95_commit_ms={}\np99_commit_ms={}\nmax_compaction_pending_mb={}\nmax_l0_files={}\nmax_imm_memtables={}\n",
            metrics.batches,
            metrics.blocks,
            format_float(metrics.avg_batch_seconds),
            format_float(metrics.p95_batch_seconds),
            format_float(metrics.p99_batch_seconds),
            format_float(metrics.avg_commit_ms),
            format_float(metrics.p95_commit_ms),
            format_float(metrics.p99_commit_ms),
            metrics.max_compaction_pending_mb,
            metrics.max_l0_files,
            metrics.max_imm_memtables,
        ));
        fs::write(self.run_dir.join("metrics.env"), content)?;
        Ok(())
    }

    fn write_report(&self, metrics: &BulkSyncPerfMetrics) -> Result<()> {
        let baseline = read_metrics_env(&self.output_root.join("latest/metrics.env"))?;

        let mut content = String::new();
        content.push_str("# Bulk Sync Perf Report\n\n");
        content.push_str(&format!("- Run ID: {}\n", metrics.run_id));
        content.push_str(&format!("- Status: {}\n", metrics.status));
        content.push_str(&format!("- Started at (UTC): {}\n", metrics.started_at_utc));
        if let Some(finished_at_utc) = metrics.finished_at_utc.as_deref() {
            content.push_str(&format!("- Finished at (UTC): {}\n", finished_at_utc));
        }
        content.push_str("\n## Current Metrics\n\n");
        content.push_str("| Metric | Value |\n");
        content.push_str("| --- | ---: |\n");
        content.push_str(&format!("| batches | {} |\n", metrics.batches));
        content.push_str(&format!("| blocks | {} |\n", metrics.blocks));
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
            "| max_compaction_pending_mb | {} |\n",
            metrics.max_compaction_pending_mb
        ));
        content.push_str(&format!("| max_l0_files | {} |\n", metrics.max_l0_files));
        content.push_str(&format!(
            "| max_imm_memtables | {} |\n",
            metrics.max_imm_memtables
        ));
        content.push('\n');

        if let Some(baseline) = baseline {
            content.push_str("## Baseline Comparison\n\n");
            content.push_str(&format!("- Baseline run: {}\n\n", baseline.run_id));
            content.push_str("| Metric | Current | Baseline | Delta |\n");
            content.push_str("| --- | ---: | ---: | ---: |\n");
            for (name, current, previous) in [
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
            ] {
                content.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    name,
                    format_float(current),
                    format_float(previous),
                    format_delta_pct(current, previous),
                ));
            }
        }

        fs::write(self.run_dir.join("report.md"), content)?;
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

#[derive(Serialize)]
struct SampleRecord<'a, T> {
    kind: &'a str,
    sample: &'a T,
}

fn utc_now_string() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
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
    let index = ((pct * values.len()) + 99) / 100;
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

    Ok(Some(BulkSyncPerfMetrics {
        run_id: read_string(&map, "run_id"),
        status: read_string(&map, "status"),
        started_at_utc: read_string(&map, "started_at_utc"),
        finished_at_utc: map.get("finished_at_utc").cloned(),
        batches: read_u64(&map, "batches"),
        blocks: read_u64(&map, "blocks"),
        avg_batch_seconds: read_f64(&map, "avg_batch_seconds"),
        p95_batch_seconds: read_f64(&map, "p95_batch_seconds"),
        p99_batch_seconds: read_f64(&map, "p99_batch_seconds"),
        avg_commit_ms: read_f64(&map, "avg_commit_ms"),
        p95_commit_ms: read_f64(&map, "p95_commit_ms"),
        p99_commit_ms: read_f64(&map, "p99_commit_ms"),
        max_compaction_pending_mb: read_u64(&map, "max_compaction_pending_mb"),
        max_l0_files: read_u64(&map, "max_l0_files"),
        max_imm_memtables: read_u64(&map, "max_imm_memtables"),
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
    use super::{BatchSample, BulkSyncPerfRun, HeartbeatSample};
    use tempfile::TempDir;

    #[test]
    fn test_bulk_sync_perf_run_start_writes_initial_artifacts() {
        let dir = TempDir::new().unwrap();
        let run = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();

        assert!(dir.path().join("run-1/metadata.env").exists());
        assert!(dir.path().join("run-1/status.env").exists());
        assert!(dir.path().join("run-1/metrics.env").exists());
        assert_eq!(run.status(), "running");
    }

    #[test]
    fn test_bulk_sync_perf_completed_run_updates_latest() {
        let dir = TempDir::new().unwrap();
        let mut run = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();

        run.finish_completed().unwrap();

        assert!(dir.path().join("latest/metrics.env").exists());
    }

    #[test]
    fn test_bulk_sync_perf_failed_run_does_not_update_latest() {
        let dir = TempDir::new().unwrap();
        let mut completed = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();
        completed.finish_completed().unwrap();

        let mut failed = BulkSyncPerfRun::start_for_test(dir.path(), "run-2").unwrap();
        failed.finish_failed().unwrap();

        let latest = std::fs::read_to_string(dir.path().join("latest/metrics.env")).unwrap();
        assert!(latest.contains("run_id=run-1"));
    }

    #[test]
    fn test_bulk_sync_metrics_use_committed_batch_samples_for_percentiles() {
        let dir = TempDir::new().unwrap();
        let mut run = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();
        run.record_batch_sample(BatchSample::new(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        run.record_batch_sample(BatchSample::new(20, 2.0, 80.0, 200, 7, 2))
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
        let mut baseline = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();
        baseline
            .record_batch_sample(BatchSample::new(10, 1.0, 40.0, 100, 4, 1))
            .unwrap();
        baseline.finish_completed().unwrap();

        let mut current = BulkSyncPerfRun::start_for_test(dir.path(), "run-2").unwrap();
        current
            .record_batch_sample(BatchSample::new(10, 2.0, 80.0, 120, 5, 1))
            .unwrap();
        current.finish_completed().unwrap();

        let report = std::fs::read_to_string(dir.path().join("run-2/report.md")).unwrap();
        assert!(report.contains("## Baseline Comparison"));
        assert!(report.contains("avg_batch_seconds"));
    }
}
