use std::ffi::CStr;
use std::fs;
use std::time::{Duration, Instant};

/// Static system environment captured once at sync start.
#[derive(Debug, Clone, Default)]
pub struct EnvironmentSnapshot {
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub ram_total_mb: u64,
    pub kernel: String,
    pub disk_device: String,
    pub disk_scheduler: String,
    pub filesystem: String,
}

/// Per-batch environment readings.
#[derive(Debug, Clone, Default)]
pub struct BatchEnvironment {
    pub load_avg_1m: f64,
    pub mem_available_mb: u64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
    pub disk_read_mb_s: Option<f64>,
    pub disk_write_mb_s: Option<f64>,
    pub disk_read_iops: Option<f64>,
    pub disk_write_iops: Option<f64>,
    pub disk_util_pct: Option<f64>,
    pub disk_await_ms: Option<f64>,
    pub disk_avg_queue_depth: Option<f64>,
    pub disk_in_flight: Option<u64>,
    pub disk_state: Option<String>,
}

/// Windowed disk state classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskTelemetryState {
    Idle,
    Active,
    Saturated,
    Unavailable,
}

/// Windowed disk telemetry values derived from `/proc/diskstats`.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskWindowMetrics {
    pub read_mb: f64,
    pub write_mb: f64,
    pub read_mb_s: f64,
    pub write_mb_s: f64,
    pub read_iops: f64,
    pub write_iops: f64,
    pub util_pct: Option<f64>,
    pub await_ms: Option<f64>,
    pub avg_queue_depth: Option<f64>,
    pub in_flight: Option<u64>,
    pub state: DiskTelemetryState,
}

/// Result of sampling a disk telemetry window.
#[derive(Debug, Clone, PartialEq)]
pub enum DiskWindowSample {
    Warmup,
    Sample(DiskWindowMetrics),
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
struct DiskStatsSnapshot {
    read_ios: u64,
    read_sectors: u64,
    read_time_ms: u64,
    write_ios: u64,
    write_sectors: u64,
    write_time_ms: u64,
    in_flight: u64,
    time_io_ms: u64,
    weighted_time_io_ms: u64,
}

#[derive(Debug)]
enum DiskStatsSnapshotError {
    MissingDevice,
    MalformedRow(String),
}

const DISK_IDLE_UTIL_PCT: f64 = 10.0;
const DISK_IDLE_MB_S: f64 = 1.0;
const DISK_IDLE_QUEUE_DEPTH: f64 = 0.5;
const DISK_SATURATED_UTIL_PCT: f64 = 90.0;
const DISK_SATURATED_UTIL_PCT_WITH_QUEUE: f64 = 85.0;
const DISK_SATURATED_QUEUE_DEPTH: f64 = 1.0;
const DISK_SATURATED_AWAIT_MS: f64 = 15.0;
const DISK_SATURATED_WRITE_MB_S: f64 = 1.0;

/// Tracks windowed disk counters between batches.
#[derive(Debug)]
pub struct DiskStatsTracker {
    device: String,
    prev_snapshot: Option<DiskStatsSnapshot>,
    prev_timestamp: Option<Instant>,
    warmup_next: bool,
}

impl DiskStatsTracker {
    pub fn new(device: String) -> Self {
        Self {
            device,
            prev_snapshot: None,
            prev_timestamp: None,
            warmup_next: false,
        }
    }

    /// Reads /proc/diskstats and returns the legacy MB delta view.
    /// First call returns (0.0, 0.0) since no previous reading exists.
    pub fn read_delta(&mut self) -> (f64, f64) {
        match self.read_window() {
            DiskWindowSample::Sample(metrics) => (metrics.read_mb, metrics.write_mb),
            DiskWindowSample::Warmup | DiskWindowSample::Unavailable { .. } => (0.0, 0.0),
        }
    }

    /// Testable variant that takes content string.
    #[cfg(test)]
    pub fn read_delta_from_content(&mut self, content: &str) -> (f64, f64) {
        match self.read_window_from_content(content, Instant::now()) {
            DiskWindowSample::Sample(metrics) => (metrics.read_mb, metrics.write_mb),
            DiskWindowSample::Warmup | DiskWindowSample::Unavailable { .. } => (0.0, 0.0),
        }
    }

    /// Reads /proc/diskstats and returns an explicit window sample.
    pub fn read_window(&mut self) -> DiskWindowSample {
        let content = match fs::read_to_string("/proc/diskstats") {
            Ok(content) => content,
            Err(err) => {
                return DiskWindowSample::Unavailable {
                    reason: format!("failed to read /proc/diskstats: {err}"),
                }
            }
        };
        self.read_window_from_content(&content, Instant::now())
    }

    fn read_window_from_content(&mut self, content: &str, now: Instant) -> DiskWindowSample {
        let snapshot = match parse_diskstats_snapshot(content, &self.device) {
            Ok(snapshot) => snapshot,
            Err(DiskStatsSnapshotError::MissingDevice) => {
                self.reset_baseline_after_parse_unavailable();
                return DiskWindowSample::Unavailable {
                    reason: format!("diskstats device '{}' not found", self.device),
                };
            }
            Err(DiskStatsSnapshotError::MalformedRow(reason)) => {
                self.reset_baseline_after_parse_unavailable();
                return DiskWindowSample::Unavailable { reason };
            }
        };

        if self.warmup_next {
            self.prev_snapshot = Some(snapshot);
            self.prev_timestamp = Some(now);
            self.warmup_next = false;
            return DiskWindowSample::Warmup;
        }

        let Some(prev_snapshot) = self.prev_snapshot.as_ref() else {
            self.prev_snapshot = Some(snapshot);
            self.prev_timestamp = Some(now);
            return DiskWindowSample::Warmup;
        };
        let Some(prev_timestamp) = self.prev_timestamp else {
            return DiskWindowSample::Unavailable {
                reason: format!(
                    "diskstats tracker for '{}' is in an inconsistent state",
                    self.device
                ),
            };
        };

        let Some(window) = now.checked_duration_since(prev_timestamp) else {
            return DiskWindowSample::Unavailable {
                reason: format!("diskstats window for '{}' moved backwards", self.device),
            };
        };

        let metrics =
            match compute_disk_window_metrics(prev_snapshot, &snapshot, window, &self.device) {
                Ok(metrics) => metrics,
                Err(reason) => {
                    self.prev_snapshot = Some(snapshot);
                    self.prev_timestamp = Some(now);
                    self.warmup_next = true;
                    return DiskWindowSample::Unavailable { reason };
                }
            };

        self.prev_snapshot = Some(snapshot);
        self.prev_timestamp = Some(now);
        self.warmup_next = false;
        DiskWindowSample::Sample(metrics)
    }

    fn reset_baseline_after_parse_unavailable(&mut self) {
        self.prev_snapshot = None;
        self.prev_timestamp = None;
        self.warmup_next = false;
    }
}

fn compute_disk_window_metrics(
    prev: &DiskStatsSnapshot,
    curr: &DiskStatsSnapshot,
    window: Duration,
    device: &str,
) -> Result<DiskWindowMetrics, String> {
    let window_secs = window.as_secs_f64();
    let window_ms = window_secs * 1000.0;
    if window_ms <= 0.0 {
        return Err(format!(
            "diskstats window for '{}' had zero duration",
            device
        ));
    }

    let read_ios_delta = checked_delta(curr.read_ios, prev.read_ios, "read_ios", device)?;
    let read_sectors_delta =
        checked_delta(curr.read_sectors, prev.read_sectors, "read_sectors", device)?;
    let read_time_ms_delta =
        checked_delta(curr.read_time_ms, prev.read_time_ms, "read_time_ms", device)?;
    let write_ios_delta = checked_delta(curr.write_ios, prev.write_ios, "write_ios", device)?;
    let write_sectors_delta = checked_delta(
        curr.write_sectors,
        prev.write_sectors,
        "write_sectors",
        device,
    )?;
    let write_time_ms_delta = checked_delta(
        curr.write_time_ms,
        prev.write_time_ms,
        "write_time_ms",
        device,
    )?;
    let time_io_ms_delta = checked_delta(curr.time_io_ms, prev.time_io_ms, "time_io_ms", device)?;
    let weighted_time_io_ms_delta = checked_delta(
        curr.weighted_time_io_ms,
        prev.weighted_time_io_ms,
        "weighted_time_io_ms",
        device,
    )?;

    let read_mb = read_sectors_delta as f64 * 512.0 / (1024.0 * 1024.0);
    let write_mb = write_sectors_delta as f64 * 512.0 / (1024.0 * 1024.0);
    let read_mb_s = read_mb / window_secs;
    let write_mb_s = write_mb / window_secs;
    let read_iops = read_ios_delta as f64 / window_secs;
    let write_iops = write_ios_delta as f64 / window_secs;
    let util_pct = Some(time_io_ms_delta as f64 / window_ms * 100.0);
    let await_ms = if read_ios_delta + write_ios_delta == 0 {
        None
    } else {
        Some(
            (read_time_ms_delta + write_time_ms_delta) as f64
                / (read_ios_delta + write_ios_delta) as f64,
        )
    };
    let avg_queue_depth = Some(weighted_time_io_ms_delta as f64 / window_ms);
    let state = classify_disk_window(read_mb_s, write_mb_s, util_pct, await_ms, avg_queue_depth);

    Ok(DiskWindowMetrics {
        read_mb,
        write_mb,
        read_mb_s,
        write_mb_s,
        read_iops,
        write_iops,
        util_pct,
        await_ms,
        avg_queue_depth,
        in_flight: Some(curr.in_flight),
        state,
    })
}

fn classify_disk_window(
    read_mb_s: f64,
    write_mb_s: f64,
    util_pct: Option<f64>,
    await_ms: Option<f64>,
    avg_queue_depth: Option<f64>,
) -> DiskTelemetryState {
    let util = util_pct.unwrap_or(0.0);
    let queue_depth = avg_queue_depth.unwrap_or(0.0);
    let await_ms = await_ms.unwrap_or(0.0);

    if util >= DISK_SATURATED_UTIL_PCT
        || (util >= DISK_SATURATED_UTIL_PCT_WITH_QUEUE && queue_depth >= DISK_SATURATED_QUEUE_DEPTH)
        || (await_ms >= DISK_SATURATED_AWAIT_MS && write_mb_s >= DISK_SATURATED_WRITE_MB_S)
    {
        DiskTelemetryState::Saturated
    } else if util <= DISK_IDLE_UTIL_PCT
        && read_mb_s <= DISK_IDLE_MB_S
        && write_mb_s <= DISK_IDLE_MB_S
        && queue_depth <= DISK_IDLE_QUEUE_DEPTH
    {
        DiskTelemetryState::Idle
    } else {
        DiskTelemetryState::Active
    }
}

fn checked_delta(curr: u64, prev: u64, field: &str, device: &str) -> Result<u64, String> {
    curr.checked_sub(prev).ok_or_else(|| {
        format!(
            "diskstats counter '{}' for device '{}' moved backwards: prev={}, curr={}",
            field, device, prev, curr
        )
    })
}

fn parse_diskstats_snapshot(
    content: &str,
    device: &str,
) -> Result<DiskStatsSnapshot, DiskStatsSnapshotError> {
    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.get(2) != Some(&device) {
            continue;
        }

        if fields.len() < 14 {
            return Err(DiskStatsSnapshotError::MalformedRow(format!(
                "diskstats row for device '{}' has {} fields, expected at least 14",
                device,
                fields.len()
            )));
        }

        let parse = |idx: usize, name: &str| -> Result<u64, DiskStatsSnapshotError> {
            fields
                .get(idx)
                .ok_or_else(|| {
                    DiskStatsSnapshotError::MalformedRow(format!(
                        "diskstats row for device '{}' missing field '{}'",
                        device, name
                    ))
                })?
                .parse::<u64>()
                .map_err(|err| {
                    DiskStatsSnapshotError::MalformedRow(format!(
                        "diskstats row for device '{}' has invalid '{}' value '{}': {}",
                        device, name, fields[idx], err
                    ))
                })
        };

        return Ok(DiskStatsSnapshot {
            read_ios: parse(3, "read_ios")?,
            read_sectors: parse(5, "read_sectors")?,
            read_time_ms: parse(6, "read_time_ms")?,
            write_ios: parse(7, "write_ios")?,
            write_sectors: parse(9, "write_sectors")?,
            write_time_ms: parse(10, "write_time_ms")?,
            in_flight: parse(11, "in_flight")?,
            time_io_ms: parse(12, "time_io_ms")?,
            weighted_time_io_ms: parse(13, "weighted_time_io_ms")?,
        });
    }

    Err(DiskStatsSnapshotError::MissingDevice)
}

// ---------------------------------------------------------------------------
// Parser functions — take string content for testability
// ---------------------------------------------------------------------------

/// Extract first "model name" from /proc/cpuinfo format.
pub fn parse_cpu_model(cpuinfo: &str) -> String {
    for line in cpuinfo.lines() {
        if let Some(rest) = line.strip_prefix("model name") {
            if let Some(value) = rest.split_once(':') {
                return value.1.trim().to_string();
            }
        }
    }
    String::new()
}

/// Count "processor" lines in /proc/cpuinfo.
pub fn parse_cpu_cores(cpuinfo: &str) -> u32 {
    cpuinfo
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count() as u32
}

/// Extract MemTotal in MB from /proc/meminfo.
pub fn parse_mem_total_mb(meminfo: &str) -> u64 {
    parse_meminfo_field(meminfo, "MemTotal")
}

/// Extract MemAvailable in MB from /proc/meminfo.
pub fn parse_mem_available_mb(meminfo: &str) -> u64 {
    parse_meminfo_field(meminfo, "MemAvailable")
}

fn parse_meminfo_field(meminfo: &str, field: &str) -> u64 {
    for line in meminfo.lines() {
        if line.starts_with(field) && line.as_bytes().get(field.len()) == Some(&b':') {
            // Format: "FieldName:    12345 kB"
            if let Some((_, rest)) = line.split_once(':') {
                let trimmed = rest.trim();
                // Strip " kB" suffix if present
                let numeric = trimmed.strip_suffix(" kB").unwrap_or(trimmed);
                if let Ok(kb) = numeric.trim().parse::<u64>() {
                    return kb / 1024;
                }
            }
        }
    }
    0
}

/// Extract first field from /proc/loadavg.
pub fn parse_load_avg_1m(loadavg: &str) -> f64 {
    loadavg
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// Extract (read_sectors, write_sectors) for device from /proc/diskstats.
///
/// /proc/diskstats format: fields [2]=name [5]=rd_sectors [9]=wr_sectors
/// (0-indexed after splitting whitespace)
pub fn parse_diskstats(content: &str, device: &str) -> Option<(u64, u64)> {
    parse_diskstats_snapshot(content, device)
        .ok()
        .map(|snapshot| (snapshot.read_sectors, snapshot.write_sectors))
}

/// Resolve (device_display_name, filesystem) for a path from /proc/mounts content.
///
/// Used for environment metadata display. Picks the longest matching mount
/// point and strips partition suffix for readability:
///   /dev/nvme0n1p2 -> nvme0n1, /dev/sda1 -> sda
///
/// For `/proc/diskstats` I/O tracking, use `resolve_diskstats_device()` instead.
pub fn parse_mount_info(mounts: &str, path: &str) -> Option<(String, String)> {
    let mut best_mount: Option<(&str, &str, usize)> = None; // (device, fs, mount_len)

    for line in mounts.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let mount_device = fields[0];
        let mount_point = fields[1];
        let mount_fs = fields[2];

        // Check if the path starts with this mount point
        if path.starts_with(mount_point)
            && (path.len() == mount_point.len()
                || mount_point == "/"
                || path.as_bytes().get(mount_point.len()) == Some(&b'/'))
        {
            let mount_len = mount_point.len();
            if best_mount.is_none() || mount_len > best_mount.unwrap().2 {
                best_mount = Some((mount_device, mount_fs, mount_len));
            }
        }
    }

    let (device, fs, _) = best_mount?;
    let short_name = strip_partition_suffix(device);
    Some((short_name, fs.to_string()))
}

/// Strip partition suffix from device path for display purposes:
/// /dev/nvme0n1p2 -> nvme0n1, /dev/sda1 -> sda
fn strip_partition_suffix(device: &str) -> String {
    let basename = device.rsplit('/').next().unwrap_or(device);

    // NVMe: nvme0n1p2 -> nvme0n1 (strip pN suffix)
    if basename.starts_with("nvme") {
        if let Some(pos) = basename.rfind('p') {
            let after_p = &basename[pos + 1..];
            if !after_p.is_empty() && after_p.chars().all(|c| c.is_ascii_digit()) {
                return basename[..pos].to_string();
            }
        }
        return basename.to_string();
    }

    // SCSI/SATA: sda1 -> sda, vdb2 -> vdb (strip trailing digits)
    let trimmed = basename.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        basename.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Parsed mount entry from `/proc/self/mountinfo`.
struct MountInfoEntry {
    major: u32,
    minor: u32,
    source_device: String,
}

/// Resolve the `/proc/diskstats` device name for a filesystem path.
///
/// Two strategies:
///   1. Match major:minor from mountinfo against diskstats (works for ext4/xfs
///      on raw partitions where the FS device ID is the block device ID).
///   2. Match the source device name from mountinfo against diskstats names
///      (handles btrfs/LUKS/LVM where the FS reports a virtual device ID).
///
/// All arguments are file contents passed as strings for testability.
pub fn resolve_diskstats_device(mountinfo: &str, diskstats: &str, path: &str) -> Option<String> {
    let entry = parse_mountinfo_entry(mountinfo, path)?;

    // Primary: major:minor match (ext4/xfs on raw partition)
    if let Some(name) = find_diskstats_device_by_id(diskstats, entry.major, entry.minor) {
        return Some(name);
    }

    // Fallback for virtual dev_ids (btrfs subvolumes, etc.):
    // match source device basename against diskstats names.
    let basename = entry.source_device.rsplit('/').next().unwrap_or("");
    if !basename.is_empty() && find_diskstats_device_by_name(diskstats, basename).is_some() {
        return Some(basename.to_string());
    }

    None
}

/// Parse `/proc/self/mountinfo` to find the mount entry for `path`.
///
/// Format: `mount_id parent_id major:minor root mount_point opts ... - fs_type source super_opts`
fn parse_mountinfo_entry(mountinfo: &str, path: &str) -> Option<MountInfoEntry> {
    let mut best: Option<(&str, &str, usize)> = None; // (dev_id, full_line, mount_len)

    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let mount_point = fields[4];

        if path.starts_with(mount_point)
            && (path.len() == mount_point.len()
                || mount_point == "/"
                || path.as_bytes().get(mount_point.len()) == Some(&b'/'))
        {
            let mount_len = mount_point.len();
            if best.is_none() || mount_len > best.unwrap().2 {
                best = Some((fields[2], line, mount_len));
            }
        }
    }

    let (dev_id, line, _) = best?;
    let (major_s, minor_s) = dev_id.split_once(':')?;
    let major = major_s.parse().ok()?;
    let minor = minor_s.parse().ok()?;

    // Extract source device: field after "- fs_type" separator
    let source_device = line
        .split_once(" - ")
        .and_then(|(_, rest)| rest.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string();

    Some(MountInfoEntry {
        major,
        minor,
        source_device,
    })
}

/// Scan `/proc/diskstats` for the device with matching major:minor numbers.
fn find_diskstats_device_by_id(diskstats: &str, major: u32, minor: u32) -> Option<String> {
    for line in diskstats.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 {
            let Ok(m) = fields[0].parse::<u32>() else {
                continue;
            };
            let Ok(n) = fields[1].parse::<u32>() else {
                continue;
            };
            if m == major && n == minor {
                return Some(fields[2].to_string());
            }
        }
    }
    None
}

/// Check if a device name exists in `/proc/diskstats`.
fn find_diskstats_device_by_name(diskstats: &str, name: &str) -> Option<()> {
    for line in diskstats.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 && fields[2] == name {
            return Some(());
        }
    }
    None
}

/// Detect the block device name for a data path.
///
/// On Linux, resolves to the `/proc/diskstats` device name for I/O tracking.
/// Uses major:minor matching with fallback to source device name matching.
/// For device-mapper paths (`/dev/mapper/*`), uses multiple strategies:
///
/// 1. Symlink: `readlink /dev/mapper/luks-... -> ../dm-0`
/// 2. Stat: `stat /dev/mapper/luks-...` to get real major:minor
/// 3. Sysfs: match `/sys/block/dm-*/dm/name` against the mapper name
///
/// On macOS, returns the device basename from `statfs()` (e.g., `"disk1s1"`).
/// `DiskStatsTracker` will still return 0.0 on macOS since `/proc/diskstats`
/// does not exist; per-disk I/O tracking would require IOKit.
///
/// Never fails (returns empty string on any resolution failure).
pub fn detect_disk_device(data_path: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        // Mount points in /proc/self/mountinfo are absolute, so canonicalize
        // the data path first.  Falls back to the original if the path doesn't
        // exist yet (e.g. first run before DB creation).
        let abs_path =
            fs::canonicalize(data_path).unwrap_or_else(|_| std::path::PathBuf::from(data_path));
        let data_path = abs_path.to_string_lossy();

        let mountinfo = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        let diskstats = fs::read_to_string("/proc/diskstats").unwrap_or_default();

        // Try pure resolution (major:minor or source device basename)
        if let Some(device) = resolve_diskstats_device(&mountinfo, &diskstats, &data_path) {
            return device;
        }

        // Source device needs deeper resolution (e.g., /dev/mapper/luks-...)
        if let Some(entry) = parse_mountinfo_entry(&mountinfo, &data_path) {
            // Fallback 1: symlink resolution (/dev/mapper/luks-... -> ../dm-0)
            if let Ok(target) = fs::read_link(&entry.source_device) {
                if let Some(name) = target.file_name().and_then(|n| n.to_str()) {
                    if find_diskstats_device_by_name(&diskstats, name).is_some() {
                        return name.to_string();
                    }
                }
            }

            // Fallback 2: stat() to get real major:minor, then find in diskstats
            if let Ok(meta) = fs::metadata(&entry.source_device) {
                use std::os::unix::fs::MetadataExt;
                let rdev = meta.rdev();
                let major = ((rdev >> 8) & 0xfff) as u32;
                let minor = (rdev & 0xff) as u32;
                if let Some(name) = find_diskstats_device_by_id(&diskstats, major, minor) {
                    return name;
                }
            }

            // Fallback 3: /sys/block/dm-*/dm/name (works when /dev/mapper/ is
            // inaccessible, e.g., in sandboxed/namespaced environments)
            if let Some(mapper_name) = entry.source_device.strip_prefix("/dev/mapper/") {
                if let Some(dm_device) = resolve_dm_by_sysfs_name(&diskstats, mapper_name) {
                    return dm_device;
                }
            }
        }

        String::new()
    }

    #[cfg(target_os = "macos")]
    {
        statfs_device_and_filesystem(data_path)
            .map(|(device, _)| device)
            .unwrap_or_default()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = data_path;
        String::new()
    }
}

/// Resolve a device-mapper name to its `dm-N` diskstats device via sysfs.
///
/// Scans `/sys/block/dm-*/dm/name` for each dm device present in diskstats
/// and returns the first whose name matches `mapper_name`.
fn resolve_dm_by_sysfs_name(diskstats: &str, mapper_name: &str) -> Option<String> {
    for line in diskstats.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 && fields[2].starts_with("dm-") {
            let dm_name_path = format!("/sys/block/{}/dm/name", fields[2]);
            if let Ok(name) = fs::read_to_string(&dm_name_path) {
                if name.trim() == mapper_name {
                    return Some(fields[2].to_string());
                }
            }
        }
    }
    None
}

/// Read /sys/block/{device}/queue/scheduler, extract bracketed active scheduler.
pub fn read_disk_scheduler(device: &str) -> String {
    let path = format!("/sys/block/{}/queue/scheduler", device);
    match fs::read_to_string(&path) {
        Ok(content) => parse_scheduler_content(&content),
        Err(_) => String::new(),
    }
}

fn parse_scheduler_content(content: &str) -> String {
    // Format: "none [mq-deadline] kyber bfq"
    // The active scheduler is in brackets.
    if let Some(start) = content.find('[') {
        if let Some(end) = content[start..].find(']') {
            return content[start + 1..start + end].to_string();
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Cross-platform system info readers (POSIX libc)
// ---------------------------------------------------------------------------

/// Read total physical memory in MB using POSIX sysconf.
fn sysconf_mem_total_mb() -> u64 {
    unsafe {
        let pages = libc::sysconf(libc::_SC_PHYS_PAGES);
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        if pages > 0 && page_size > 0 {
            (pages as u64 * page_size as u64) / (1024 * 1024)
        } else {
            0
        }
    }
}

/// Read CPU core count using POSIX sysconf.
fn sysconf_cpu_cores() -> u32 {
    unsafe {
        let n = libc::sysconf(libc::_SC_NPROCESSORS_ONLN);
        if n > 0 {
            n as u32
        } else {
            0
        }
    }
}

/// Read 1-minute load average using POSIX getloadavg.
fn posix_load_avg_1m() -> f64 {
    let mut loadavg = [0.0f64; 1];
    unsafe {
        if libc::getloadavg(loadavg.as_mut_ptr(), 1) == 1 {
            loadavg[0]
        } else {
            0.0
        }
    }
}

/// Read kernel/OS version using POSIX uname.
fn posix_kernel_version() -> String {
    unsafe {
        let mut info: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut info) == 0 {
            CStr::from_ptr(info.release.as_ptr())
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        }
    }
}

/// Read CPU model string (platform-specific).
#[cfg(target_os = "linux")]
fn read_cpu_model() -> String {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    parse_cpu_model(&cpuinfo)
}

#[cfg(target_os = "macos")]
fn read_cpu_model() -> String {
    sysctl_string("machdep.cpu.brand_string")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_cpu_model() -> String {
    String::new()
}

/// Read available memory in MB (platform-specific).
///
/// On Linux, reads MemAvailable from /proc/meminfo.
/// On other platforms, returns 0 (used for monitoring only, not critical decisions).
#[cfg(target_os = "linux")]
fn read_mem_available_mb() -> u64 {
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    parse_mem_available_mb(&meminfo)
}

#[cfg(not(target_os = "linux"))]
fn read_mem_available_mb() -> u64 {
    0
}

/// Read a sysctl string value (macOS/BSD).
#[cfg(target_os = "macos")]
fn sysctl_string(name: &str) -> String {
    use std::ffi::CString;
    let c_name = match CString::new(name) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    unsafe {
        let mut size: libc::size_t = 0;
        if libc::sysctlbyname(
            c_name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return String::new();
        }
        let mut buf = vec![0u8; size];
        if libc::sysctlbyname(
            c_name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return String::new();
        }
        if let Some(pos) = buf.iter().position(|&b| b == 0) {
            buf.truncate(pos);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

// ---------------------------------------------------------------------------
// High-level capture functions
// ---------------------------------------------------------------------------

/// Capture static environment snapshot. Uses cross-platform POSIX APIs for
/// CPU/RAM/kernel, with platform-specific disk info detection.
/// Never fails (returns defaults on any resolution failure).
pub fn capture_environment(data_path: &str) -> EnvironmentSnapshot {
    let (disk_device, filesystem) = detect_device_and_filesystem(data_path);

    // For dm devices, the scheduler lives on the parent physical device.
    // Walk /sys/block/dm-N/slaves/ to find it. No-op on macOS.
    let scheduler_device = resolve_scheduler_device(&disk_device);
    let disk_scheduler = read_disk_scheduler(&scheduler_device);

    EnvironmentSnapshot {
        cpu_model: read_cpu_model(),
        cpu_cores: sysconf_cpu_cores(),
        ram_total_mb: sysconf_mem_total_mb(),
        kernel: posix_kernel_version(),
        disk_device,
        disk_scheduler,
        filesystem,
    }
}

/// Detect disk device name and filesystem type for a path.
///
/// - Linux: procfs/sysfs resolution (handles LUKS/dm/btrfs).
/// - macOS: `statfs()` for device and filesystem type.
fn detect_device_and_filesystem(data_path: &str) -> (String, String) {
    #[cfg(target_os = "linux")]
    {
        let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
        let (_, filesystem) = parse_mount_info(&mounts, data_path).unwrap_or_default();
        let disk_device = detect_disk_device(data_path);
        (disk_device, filesystem)
    }

    #[cfg(target_os = "macos")]
    {
        statfs_device_and_filesystem(data_path).unwrap_or_default()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = data_path;
        (String::new(), String::new())
    }
}

/// macOS: use `statfs()` to get device name and filesystem type.
///
/// Returns e.g. `("disk1s1", "apfs")` or `("disk3s1", "hfs")`.
#[cfg(target_os = "macos")]
fn statfs_device_and_filesystem(path: &str) -> Option<(String, String)> {
    use std::ffi::{CStr, CString};
    let c_path = CString::new(path).ok()?;
    unsafe {
        let mut buf: libc::statfs = std::mem::zeroed();
        if libc::statfs(c_path.as_ptr(), &mut buf) != 0 {
            return None;
        }
        let device = CStr::from_ptr(buf.f_mntfromname.as_ptr())
            .to_string_lossy()
            .into_owned();
        let filesystem = CStr::from_ptr(buf.f_fstypename.as_ptr())
            .to_string_lossy()
            .into_owned();
        // Strip /dev/ prefix for display (e.g., "/dev/disk1s1" → "disk1s1")
        let short = device.strip_prefix("/dev/").unwrap_or(&device).to_string();
        Some((short, filesystem))
    }
}

/// For a dm device, walk `/sys/block/dm-N/slaves/` to find the parent
/// physical device (e.g., nvme0n1p2) whose scheduler is meaningful.
/// For non-dm devices, returns the device name unchanged.
fn resolve_scheduler_device(device: &str) -> String {
    if !device.starts_with("dm-") {
        return device.to_string();
    }
    let slaves_dir = format!("/sys/block/{}/slaves", device);
    if let Some(entry) = fs::read_dir(&slaves_dir)
        .ok()
        .and_then(|mut entries| entries.find_map(|e| e.ok()))
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("dm-") {
            // Recurse through stacked dm layers
            return resolve_scheduler_device(&name);
        }
        // Found a physical device (e.g., nvme0n1p2) — strip partition
        // suffix for the scheduler lookup (scheduler is on whole disk)
        return strip_partition_suffix(&format!("/dev/{}", name));
    }
    device.to_string()
}

/// Read per-batch environment. Uses cross-platform POSIX APIs for load/memory,
/// with Linux-specific procfs windowed disk telemetry.
/// Never fails (returns defaults on error).
pub fn read_batch_environment(disk_tracker: &mut DiskStatsTracker) -> BatchEnvironment {
    let load_avg_1m = posix_load_avg_1m();
    let mem_available_mb = read_mem_available_mb();
    batch_environment_from_disk_sample(load_avg_1m, mem_available_mb, disk_tracker.read_window())
}

fn batch_environment_from_disk_sample(
    load_avg_1m: f64,
    mem_available_mb: u64,
    sample: DiskWindowSample,
) -> BatchEnvironment {
    match sample {
        DiskWindowSample::Warmup => BatchEnvironment {
            load_avg_1m,
            mem_available_mb,
            disk_read_mb: 0.0,
            disk_write_mb: 0.0,
            disk_read_mb_s: None,
            disk_write_mb_s: None,
            disk_read_iops: None,
            disk_write_iops: None,
            disk_util_pct: None,
            disk_await_ms: None,
            disk_avg_queue_depth: None,
            disk_in_flight: None,
            disk_state: None,
        },
        DiskWindowSample::Sample(metrics) => BatchEnvironment {
            load_avg_1m,
            mem_available_mb,
            disk_read_mb: metrics.read_mb,
            disk_write_mb: metrics.write_mb,
            disk_read_mb_s: Some(metrics.read_mb_s),
            disk_write_mb_s: Some(metrics.write_mb_s),
            disk_read_iops: Some(metrics.read_iops),
            disk_write_iops: Some(metrics.write_iops),
            disk_util_pct: metrics.util_pct,
            disk_await_ms: metrics.await_ms,
            disk_avg_queue_depth: metrics.avg_queue_depth,
            disk_in_flight: metrics.in_flight,
            disk_state: Some(
                match metrics.state {
                    DiskTelemetryState::Idle => "idle",
                    DiskTelemetryState::Active => "active",
                    DiskTelemetryState::Saturated => "saturated",
                    DiskTelemetryState::Unavailable => "unavailable",
                }
                .to_string(),
            ),
        },
        DiskWindowSample::Unavailable { .. } => BatchEnvironment {
            load_avg_1m,
            mem_available_mb,
            disk_read_mb: 0.0,
            disk_write_mb: 0.0,
            disk_read_mb_s: None,
            disk_write_mb_s: None,
            disk_read_iops: None,
            disk_write_iops: None,
            disk_util_pct: None,
            disk_await_ms: None,
            disk_avg_queue_depth: None,
            disk_in_flight: None,
            disk_state: Some("unavailable".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu_model() {
        let cpuinfo = "\
processor\t: 0
vendor_id\t: AuthenticAMD
model name\t: AMD Ryzen 9 7950X 16-Core Processor
stepping\t: 2
processor\t: 1
model name\t: AMD Ryzen 9 7950X 16-Core Processor
";
        assert_eq!(
            parse_cpu_model(cpuinfo),
            "AMD Ryzen 9 7950X 16-Core Processor"
        );
    }

    #[test]
    fn test_parse_cpu_model_empty() {
        assert_eq!(parse_cpu_model(""), "");
    }

    #[test]
    fn test_parse_cpu_cores() {
        let cpuinfo = "\
processor\t: 0
model name\t: Test CPU
processor\t: 1
model name\t: Test CPU
processor\t: 2
model name\t: Test CPU
";
        assert_eq!(parse_cpu_cores(cpuinfo), 3);
    }

    #[test]
    fn test_parse_mem_total_mb() {
        let meminfo = "\
MemTotal:       97613824 kB
MemFree:         1234567 kB
MemAvailable:   45000000 kB
";
        assert_eq!(parse_mem_total_mb(meminfo), 95326);
    }

    #[test]
    fn test_parse_mem_available_mb() {
        let meminfo = "MemAvailable:   45000000 kB\n";
        assert_eq!(parse_mem_available_mb(meminfo), 43945);
    }

    #[test]
    fn test_parse_load_avg_1m() {
        assert_eq!(parse_load_avg_1m("8.23 6.01 4.50 3/1234 56789"), 8.23);
    }

    #[test]
    fn test_parse_load_avg_1m_empty() {
        assert_eq!(parse_load_avg_1m(""), 0.0);
    }

    #[test]
    fn test_parse_diskstats() {
        // /proc/diskstats fields (0-indexed after whitespace split):
        // [0]=major [1]=minor [2]=name [3]=rd_completed [4]=rd_merged
        // [5]=rd_sectors [6]=rd_ms [7]=wr_completed [8]=wr_merged
        // [9]=wr_sectors [10]=wr_ms ...
        let content = "\
 259       0 nvme0n1 123456 0 500000 0 654321 0 300000 0 0 0 0 0 0 0 0 0
 259       1 nvme0n1p1 100 0 200 0 300 0 400 0 0 0 0 0 0 0 0 0
   8       0 sda 111 0 222 0 333 0 444 0 0 0 0 0 0 0 0 0
";
        let result = parse_diskstats(content, "nvme0n1");
        assert_eq!(result, Some((500000, 300000)));
    }

    #[test]
    fn test_parse_diskstats_device_not_found() {
        let content = "\
 259       0 nvme0n1 123456 0 500000 0 654321 0 300000 0 0 0 0 0 0 0 0 0
";
        assert_eq!(parse_diskstats(content, "sda"), None);
    }

    #[test]
    fn test_parse_diskstats_snapshot_reads_required_fields() {
        let content = "\
 259       0 nvme0n1 123456 0 500000 17 654321 0 300000 29 4 31 41 0 0 0 0 0
";
        let snapshot = parse_diskstats_snapshot(content, "nvme0n1").unwrap();
        assert_eq!(snapshot.read_ios, 123456);
        assert_eq!(snapshot.read_sectors, 500000);
        assert_eq!(snapshot.read_time_ms, 17);
        assert_eq!(snapshot.write_ios, 654321);
        assert_eq!(snapshot.write_sectors, 300000);
        assert_eq!(snapshot.write_time_ms, 29);
        assert_eq!(snapshot.in_flight, 4);
        assert_eq!(snapshot.time_io_ms, 31);
        assert_eq!(snapshot.weighted_time_io_ms, 41);
    }

    #[test]
    fn test_parse_mount_info() {
        let mounts = "\
/dev/nvme0n1p2 /home btrfs rw,relatime 0 0
/dev/sda1 / ext4 rw,relatime 0 0
";
        let result = parse_mount_info(mounts, "/home/user/data");
        assert_eq!(result, Some(("nvme0n1".to_string(), "btrfs".to_string())));
    }

    #[test]
    fn test_parse_mount_info_longest_match() {
        let mounts = "\
/dev/sda1 / ext4 rw 0 0
/dev/nvme0n1p3 /data btrfs rw 0 0
";
        let result = parse_mount_info(mounts, "/data/ckbadger");
        assert_eq!(result, Some(("nvme0n1".to_string(), "btrfs".to_string())));
    }

    #[test]
    fn test_disk_stats_tracker_first_call_returns_zero() {
        let content = "\
 259       0 nvme0n1 100 0 1000 0 200 0 2000 0 0 0 0 0 0 0 0 0
";
        let mut tracker = DiskStatsTracker::new("nvme0n1".to_string());
        let (read_mb, write_mb) = tracker.read_delta_from_content(content);
        assert_eq!(read_mb, 0.0);
        assert_eq!(write_mb, 0.0);
    }

    #[test]
    fn test_disk_stats_tracker_second_call_returns_delta() {
        // [5]=rd_sectors [9]=wr_sectors
        let content1 = "\
 259       0 nvme0n1 100 0 1000 0 200 0 2000 0 0 0 0 0 0 0 0 0
";
        // 2048 more read sectors and 2048 more write sectors
        let content2 = "\
 259       0 nvme0n1 100 0 3048 0 200 0 4048 0 0 0 0 0 0 0 0 0
";
        let mut tracker = DiskStatsTracker::new("nvme0n1".to_string());
        tracker.read_delta_from_content(content1);
        let (read_mb, write_mb) = tracker.read_delta_from_content(content2);
        // 2048 sectors * 512 bytes = 1,048,576 bytes = 1.0 MB
        assert!((read_mb - 1.0).abs() < 0.001);
        assert!((write_mb - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_disk_stats_tracker_missing_device_returns_zero() {
        let content = "\
 259       0 nvme0n1 100 0 1000 0 200 0 2000 0 0 0 0 0 0 0 0 0
";
        let mut tracker = DiskStatsTracker::new("sda".to_string());
        let (read_mb, write_mb) = tracker.read_delta_from_content(content);
        assert_eq!(read_mb, 0.0);
        assert_eq!(write_mb, 0.0);
    }

    #[test]
    fn test_disk_tracker_reports_warmup_then_sample() {
        let content1 = "\
 259       0 nvme0n1 100 0 2048 10 200 0 4096 20 1 30 40 0 0 0 0 0
";
        let content2 = "\
 259       0 nvme0n1 110 0 4096 30 206 0 8192 80 4 90 180 0 0 0 0 0
";
        let mut tracker = DiskStatsTracker::new("nvme0n1".to_string());
        let start = Instant::now();
        assert!(matches!(
            tracker.read_window_from_content(content1, start),
            DiskWindowSample::Warmup
        ));

        let sample = tracker.read_window_from_content(content2, start + Duration::from_secs(2));
        let DiskWindowSample::Sample(metrics) = sample else {
            panic!("expected sample, got {:?}", sample);
        };

        assert!((metrics.read_mb - 1.0).abs() < f64::EPSILON);
        assert!((metrics.write_mb - 2.0).abs() < f64::EPSILON);
        assert!((metrics.read_mb_s - 0.5).abs() < f64::EPSILON);
        assert!((metrics.write_mb_s - 1.0).abs() < f64::EPSILON);
        assert!((metrics.read_iops - 5.0).abs() < f64::EPSILON);
        assert!((metrics.write_iops - 3.0).abs() < f64::EPSILON);
        assert_eq!(metrics.util_pct, Some(3.0));
        assert_eq!(metrics.await_ms, Some(5.0));
        assert_eq!(metrics.avg_queue_depth, Some(0.07));
        assert_eq!(metrics.in_flight, Some(4));
        assert_eq!(metrics.state, DiskTelemetryState::Idle);
    }

    #[test]
    fn test_disk_tracker_zero_io_window_marks_await_unavailable() {
        let content1 = "\
 259       0 nvme0n1 100 0 2048 10 200 0 4096 20 1 30 40 0 0 0 0 0
";
        let content2 = "\
 259       0 nvme0n1 100 0 2048 10 200 0 4096 20 0 30 40 0 0 0 0 0
";
        let mut tracker = DiskStatsTracker::new("nvme0n1".to_string());
        let start = Instant::now();
        tracker.read_window_from_content(content1, start);
        let sample = tracker.read_window_from_content(content2, start + Duration::from_millis(250));
        let DiskWindowSample::Sample(metrics) = sample else {
            panic!("expected sample, got {:?}", sample);
        };

        assert_eq!(metrics.read_iops, 0.0);
        assert_eq!(metrics.write_iops, 0.0);
        assert_eq!(metrics.await_ms, None);
        assert_eq!(metrics.util_pct, Some(0.0));
        assert_eq!(metrics.avg_queue_depth, Some(0.0));
        assert_eq!(metrics.in_flight, Some(0));
        assert_eq!(metrics.state, DiskTelemetryState::Idle);
    }

    #[test]
    fn test_disk_tracker_missing_device_reports_unavailable() {
        let content = "\
 259       0 nvme0n1 100 0 2048 10 200 0 4096 20 1 30 40 0 0 0 0 0
";
        let mut tracker = DiskStatsTracker::new("sda".to_string());
        let sample = tracker.read_window_from_content(content, Instant::now());
        let DiskWindowSample::Unavailable { reason } = sample else {
            panic!("expected unavailable, got {:?}", sample);
        };

        assert!(reason.contains("sda"));
    }

    #[test]
    fn test_disk_tracker_rearms_after_parse_unavailable() {
        let warmup = "\
 259       0 nvme0n1 100 0 2048 10 200 0 4096 20 1 30 40 0 0 0 0 0
";
        let malformed = "\
 259       0 nvme0n1 110 0 4096 30 206
";
        let missing_device = "\
 259       0 sda 110 0 4096 30 206 0 8192 80 4 90 180 0 0 0 0 0
";
        let valid_after_reset = "\
 259       0 nvme0n1 120 0 6144 50 210 0 8192 100 5 150 260 0 0 0 0 0
";
        let start = Instant::now();

        let mut tracker = DiskStatsTracker::new("nvme0n1".to_string());
        assert!(matches!(
            tracker.read_window_from_content(warmup, start),
            DiskWindowSample::Warmup
        ));

        let malformed_sample =
            tracker.read_window_from_content(malformed, start + Duration::from_secs(1));
        let DiskWindowSample::Unavailable { reason } = malformed_sample else {
            panic!("expected unavailable, got {:?}", malformed_sample);
        };
        assert!(reason.contains("expected at least 14"));

        assert!(matches!(
            tracker.read_window_from_content(valid_after_reset, start + Duration::from_secs(2)),
            DiskWindowSample::Warmup
        ));

        let sample = tracker.read_window_from_content(
            "\
 259       0 nvme0n1 130 0 8192 70 215 0 10240 120 6 210 360 0 0 0 0 0
",
            start + Duration::from_secs(3),
        );
        let DiskWindowSample::Sample(metrics) = sample else {
            panic!("expected sample after re-arm, got {:?}", sample);
        };
        assert!((metrics.read_mb - 1.0).abs() < f64::EPSILON);
        assert!((metrics.write_mb - 1.0).abs() < f64::EPSILON);

        let mut tracker = DiskStatsTracker::new("nvme0n1".to_string());
        assert!(matches!(
            tracker.read_window_from_content(warmup, start + Duration::from_secs(10)),
            DiskWindowSample::Warmup
        ));

        let missing_sample =
            tracker.read_window_from_content(missing_device, start + Duration::from_secs(11));
        let DiskWindowSample::Unavailable { reason } = missing_sample else {
            panic!("expected unavailable, got {:?}", missing_sample);
        };
        assert!(reason.contains("not found"));

        assert!(matches!(
            tracker.read_window_from_content(valid_after_reset, start + Duration::from_secs(12)),
            DiskWindowSample::Warmup
        ));
    }

    #[test]
    fn test_disk_tracker_recovers_after_backwards_sample() {
        let warmup = "\
 259       0 nvme0n1 100 0 2048 10 200 0 4096 20 1 30 40 0 0 0 0 0
";
        let backwards = "\
 259       0 nvme0n1 90 0 1024 5 190 0 3072 10 1 20 30 0 0 0 0 0
";
        let rearmed = "\
 259       0 nvme0n1 105 0 3072 12 205 0 5120 24 2 50 60 0 0 0 0 0
";
        let recovered = "\
 259       0 nvme0n1 115 0 5120 32 210 0 7168 44 3 150 260 0 0 0 0 0
";

        let mut tracker = DiskStatsTracker::new("nvme0n1".to_string());
        let start = Instant::now();

        assert!(matches!(
            tracker.read_window_from_content(warmup, start),
            DiskWindowSample::Warmup
        ));

        let sample = tracker.read_window_from_content(backwards, start + Duration::from_secs(1));
        let DiskWindowSample::Unavailable { reason } = sample else {
            panic!("expected unavailable, got {:?}", sample);
        };
        assert!(reason.contains("moved backwards"));

        assert!(matches!(
            tracker.read_window_from_content(rearmed, start + Duration::from_secs(2)),
            DiskWindowSample::Warmup
        ));

        let sample = tracker.read_window_from_content(recovered, start + Duration::from_secs(3));
        let DiskWindowSample::Sample(metrics) = sample else {
            panic!("expected sample, got {:?}", sample);
        };

        assert!((metrics.read_mb - 1.0).abs() < f64::EPSILON);
        assert!((metrics.write_mb - 1.0).abs() < f64::EPSILON);
        assert!((metrics.read_iops - 10.0).abs() < f64::EPSILON);
        assert!((metrics.write_iops - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_batch_environment_maps_disk_window_states_without_fake_zeros() {
        let warmup = batch_environment_from_disk_sample(12.5, 4096, DiskWindowSample::Warmup);
        assert_eq!(warmup.load_avg_1m, 12.5);
        assert_eq!(warmup.mem_available_mb, 4096);
        assert_eq!(warmup.disk_read_mb, 0.0);
        assert_eq!(warmup.disk_write_mb, 0.0);
        assert_eq!(warmup.disk_read_mb_s, None);
        assert_eq!(warmup.disk_write_mb_s, None);
        assert_eq!(warmup.disk_read_iops, None);
        assert_eq!(warmup.disk_write_iops, None);
        assert_eq!(warmup.disk_util_pct, None);
        assert_eq!(warmup.disk_await_ms, None);
        assert_eq!(warmup.disk_avg_queue_depth, None);
        assert_eq!(warmup.disk_in_flight, None);
        assert_eq!(warmup.disk_state, None);

        let sample = batch_environment_from_disk_sample(
            12.5,
            4096,
            DiskWindowSample::Sample(DiskWindowMetrics {
                read_mb: 1.25,
                write_mb: 2.5,
                read_mb_s: 1.25,
                write_mb_s: 2.5,
                read_iops: 10.0,
                write_iops: 20.0,
                util_pct: Some(87.5),
                await_ms: Some(3.5),
                avg_queue_depth: Some(1.25),
                in_flight: Some(7),
                state: DiskTelemetryState::Active,
            }),
        );
        assert_eq!(sample.disk_state, Some("active".to_string()));
        assert_eq!(sample.disk_read_mb_s, Some(1.25));
        assert_eq!(sample.disk_write_mb_s, Some(2.5));
        assert_eq!(sample.disk_read_iops, Some(10.0));
        assert_eq!(sample.disk_write_iops, Some(20.0));
        assert_eq!(sample.disk_util_pct, Some(87.5));
        assert_eq!(sample.disk_await_ms, Some(3.5));
        assert_eq!(sample.disk_avg_queue_depth, Some(1.25));
        assert_eq!(sample.disk_in_flight, Some(7));

        let unavailable = batch_environment_from_disk_sample(
            12.5,
            4096,
            DiskWindowSample::Unavailable {
                reason: "missing device".to_string(),
            },
        );
        assert_eq!(unavailable.disk_state, Some("unavailable".to_string()));
        assert_eq!(unavailable.disk_read_mb_s, None);
        assert_eq!(unavailable.disk_write_mb_s, None);
        assert_eq!(unavailable.disk_read_iops, None);
        assert_eq!(unavailable.disk_write_iops, None);
        assert_eq!(unavailable.disk_util_pct, None);
        assert_eq!(unavailable.disk_await_ms, None);
        assert_eq!(unavailable.disk_avg_queue_depth, None);
        assert_eq!(unavailable.disk_in_flight, None);
    }

    #[test]
    fn test_resolve_diskstats_device_nvme_direct() {
        // ext4 on raw NVMe partition: major:minor match
        let mountinfo = "36 1 259:2 / /home rw - ext4 /dev/nvme0n1p2 rw\n";
        let diskstats = "\
 259       0 nvme0n1 100 0 500000 0 200 0 300000 0 0 0 0 0 0 0 0 0
 259       2 nvme0n1p2 80 0 400000 0 180 0 280000 0 0 0 0 0 0 0 0 0
";
        assert_eq!(
            resolve_diskstats_device(mountinfo, diskstats, "/home/user/data"),
            Some("nvme0n1p2".to_string())
        );
    }

    #[test]
    fn test_resolve_diskstats_device_dm_direct() {
        // ext4 on dm device where major:minor matches directly
        let mountinfo = "42 1 253:0 / /data rw - ext4 /dev/dm-0 rw\n";
        let diskstats = "\
 253       0 dm-0 80 0 400000 0 180 0 280000 0 0 0 0 0 0 0 0 0
";
        assert_eq!(
            resolve_diskstats_device(mountinfo, diskstats, "/data/ckbadger"),
            Some("dm-0".to_string())
        );
    }

    #[test]
    fn test_resolve_diskstats_device_supports_dm_and_btrfs_cases() {
        let dm_mountinfo = "42 1 253:0 / /data rw - ext4 /dev/dm-0 rw\n";
        let dm_diskstats = "\
 253       0 dm-0 80 0 400000 0 180 0 280000 0 0 0 0 0 0 0 0 0
";
        assert_eq!(
            resolve_diskstats_device(dm_mountinfo, dm_diskstats, "/data/ckbadger"),
            Some("dm-0".to_string())
        );

        let btrfs_mountinfo = "42 1 0:27 /@home /home rw - btrfs /dev/sda1 rw,compress=zstd:3\n";
        let btrfs_diskstats = "\
   8       0 sda 100 0 200 0 300 0 400 0 0 0 0 0 0 0 0 0
   8       1 sda1 80 0 150 0 250 0 350 0 0 0 0 0 0 0 0 0
";
        assert_eq!(
            resolve_diskstats_device(btrfs_mountinfo, btrfs_diskstats, "/home/user/data"),
            Some("sda1".to_string())
        );
    }

    #[test]
    fn test_resolve_diskstats_device_btrfs_virtual_devid_falls_back_to_source() {
        // btrfs subvolume: dev_id 0:27 is virtual (not in diskstats).
        // Fallback matches source device basename "sda1" in diskstats.
        let mountinfo = "42 1 0:27 /@home /home rw - btrfs /dev/sda1 rw,compress=zstd:3\n";
        let diskstats = "\
   8       0 sda 100 0 200 0 300 0 400 0 0 0 0 0 0 0 0 0
   8       1 sda1 80 0 150 0 250 0 350 0 0 0 0 0 0 0 0 0
";
        assert_eq!(
            resolve_diskstats_device(mountinfo, diskstats, "/home/user/data"),
            Some("sda1".to_string())
        );
    }

    #[test]
    fn test_resolve_diskstats_device_btrfs_luks_source_not_in_diskstats() {
        // btrfs on LUKS: dev_id 0:27 virtual, source is /dev/mapper/luks-...
        // which is NOT in diskstats. Pure resolution returns None;
        // detect_disk_device() would follow the symlink as a final fallback.
        let mountinfo = "42 1 0:27 /@home /home rw - btrfs /dev/mapper/luks-abc123 rw\n";
        let diskstats = "\
 259       0 nvme0n1 100 0 500000 0 200 0 300000 0 0 0 0 0 0 0 0 0
 253       0 dm-0 80 0 400000 0 180 0 280000 0 0 0 0 0 0 0 0 0
";
        // Pure function returns None because neither dev_id 0:27 nor
        // "luks-abc123" exist in diskstats. detect_disk_device() handles
        // this by following the /dev/mapper/luks-... symlink to dm-0.
        assert_eq!(
            resolve_diskstats_device(mountinfo, diskstats, "/home/data"),
            None
        );
    }

    #[test]
    fn test_resolve_diskstats_device_longest_mount_match() {
        let mountinfo = "\
30 1 8:1 / / rw - ext4 /dev/sda1 rw
40 30 259:2 / /data rw - btrfs /dev/nvme0n1p2 rw
";
        let diskstats = "\
   8       1 sda1 100 0 200 0 300 0 400 0 0 0 0 0 0 0 0 0
 259       2 nvme0n1p2 50 0 100 0 60 0 120 0 0 0 0 0 0 0 0 0
";
        assert_eq!(
            resolve_diskstats_device(mountinfo, diskstats, "/data/ckbadger"),
            Some("nvme0n1p2".to_string())
        );
    }

    #[test]
    fn test_resolve_diskstats_device_no_mount_found() {
        let mountinfo = "36 1 259:2 / /other rw - ext4 /dev/sda1 rw\n";
        let diskstats = " 259 0 nvme0n1 100 0 200 0 300 0 400 0 0 0 0 0 0 0 0 0\n";
        assert_eq!(
            resolve_diskstats_device(mountinfo, diskstats, "/mnt/data"),
            None
        );
    }

    // -- Cross-platform reader tests --

    #[test]
    fn test_sysconf_mem_total_mb_returns_nonzero() {
        let mb = sysconf_mem_total_mb();
        assert!(mb > 0, "sysconf should detect system memory, got {mb}");
    }

    #[test]
    fn test_sysconf_cpu_cores_returns_nonzero() {
        let cores = sysconf_cpu_cores();
        assert!(cores > 0, "sysconf should detect CPU cores, got {cores}");
    }

    #[test]
    fn test_posix_kernel_version_returns_nonempty() {
        let version = posix_kernel_version();
        assert!(
            !version.is_empty(),
            "uname should return a kernel version string"
        );
    }

    #[test]
    fn test_read_cpu_model_returns_nonempty() {
        let model = read_cpu_model();
        assert!(
            !model.is_empty(),
            "CPU model should be detected on Linux and macOS"
        );
    }

    #[test]
    fn test_capture_environment_returns_populated_snapshot() {
        let env = capture_environment("/tmp");
        assert!(env.cpu_cores > 0);
        assert!(env.ram_total_mb > 0);
        assert!(!env.kernel.is_empty());
        assert!(!env.cpu_model.is_empty());
    }

    #[test]
    fn test_detect_disk_device_resolves_for_home() {
        // On Linux, /home is always mounted. detect_disk_device should find
        // the diskstats device, even on LUKS/dm/btrfs stacks where
        // /dev/mapper/ may be inaccessible.
        let device = detect_disk_device("/home");
        // On any standard Linux, /home is on a real block device
        if cfg!(target_os = "linux") {
            assert!(
                !device.is_empty(),
                "detect_disk_device(/home) returned empty — \
                 expected a diskstats device like dm-0, nvme0n1, sda, etc."
            );
        }
    }

    #[test]
    fn test_detect_disk_device_resolves_relative_path() {
        // Relative paths must be canonicalized to absolute before matching
        // against /proc/self/mountinfo entries (which are always absolute).
        if cfg!(target_os = "linux") {
            let device = detect_disk_device(".");
            assert!(
                !device.is_empty(),
                "detect_disk_device(\".\") returned empty — \
                 relative paths should be canonicalized before mount matching"
            );
        }
    }

    #[test]
    fn test_capture_environment_resolves_disk_for_home() {
        let env = capture_environment("/home");
        if cfg!(target_os = "linux") {
            assert!(
                !env.disk_device.is_empty(),
                "capture_environment(/home).disk_device was empty"
            );
            assert!(
                !env.filesystem.is_empty(),
                "capture_environment(/home).filesystem was empty"
            );
        }
    }

    #[test]
    fn test_resolve_dm_by_sysfs_name() {
        let diskstats = "\
 259       0 nvme0n1 100 0 500 0 200 0 300 0 0 0 0 0 0 0 0 0
 253       0 dm-0 80 0 400 0 180 0 280 0 0 0 0 0 0 0 0 0
";
        // When /sys/block/dm-0/dm/name exists with matching name, it resolves
        let sysfs_name_path = "/sys/block/dm-0/dm/name";
        if let Ok(name) = fs::read_to_string(sysfs_name_path) {
            let name = name.trim();
            eprintln!("dm-0 name from sysfs: {:?}", name);
            let result = resolve_dm_by_sysfs_name(diskstats, name);
            assert_eq!(result, Some("dm-0".to_string()));
        }

        // Non-existent mapper name should return None
        assert_eq!(
            resolve_dm_by_sysfs_name(diskstats, "nonexistent-mapper"),
            None
        );
    }

    #[test]
    fn test_resolve_scheduler_device_non_dm() {
        assert_eq!(resolve_scheduler_device("nvme0n1"), "nvme0n1");
        assert_eq!(resolve_scheduler_device("sda"), "sda");
        assert_eq!(resolve_scheduler_device(""), "");
    }

    #[test]
    fn test_resolve_scheduler_device_dm() {
        // If /sys/block/dm-0/slaves/ exists, resolve should find parent
        if std::path::Path::new("/sys/block/dm-0/slaves").exists() {
            let result = resolve_scheduler_device("dm-0");
            eprintln!("resolve_scheduler_device(dm-0) = {:?}", result);
            // Should NOT be dm-0 (should resolve to parent)
            assert!(
                !result.starts_with("dm-"),
                "expected physical device, got {}",
                result
            );
        }
    }
}
