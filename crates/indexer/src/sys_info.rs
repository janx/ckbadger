use std::fs;

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
}

/// Tracks cumulative disk sector counters between batches.
#[derive(Debug)]
pub struct DiskStatsTracker {
    device: String,
    prev_read_sectors: u64,
    prev_write_sectors: u64,
    initialized: bool,
}

impl DiskStatsTracker {
    pub fn new(device: String) -> Self {
        Self {
            device,
            prev_read_sectors: 0,
            prev_write_sectors: 0,
            initialized: false,
        }
    }

    /// Reads /proc/diskstats and returns (read_mb_delta, write_mb_delta).
    /// First call returns (0.0, 0.0) since no previous reading exists.
    pub fn read_delta(&mut self) -> (f64, f64) {
        let content = match fs::read_to_string("/proc/diskstats") {
            Ok(c) => c,
            Err(_) => return (0.0, 0.0),
        };
        self.compute_delta(&content)
    }

    /// Testable variant that takes content string.
    #[cfg(test)]
    pub fn read_delta_from_content(&mut self, content: &str) -> (f64, f64) {
        self.compute_delta(content)
    }

    fn compute_delta(&mut self, content: &str) -> (f64, f64) {
        let Some((read_sectors, write_sectors)) = parse_diskstats(content, &self.device) else {
            return (0.0, 0.0);
        };

        if !self.initialized {
            self.prev_read_sectors = read_sectors;
            self.prev_write_sectors = write_sectors;
            self.initialized = true;
            return (0.0, 0.0);
        }

        let read_delta = read_sectors - self.prev_read_sectors;
        let write_delta = write_sectors - self.prev_write_sectors;

        self.prev_read_sectors = read_sectors;
        self.prev_write_sectors = write_sectors;

        // Each sector is 512 bytes
        let read_mb = read_delta as f64 * 512.0 / (1024.0 * 1024.0);
        let write_mb = write_delta as f64 * 512.0 / (1024.0 * 1024.0);

        (read_mb, write_mb)
    }
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
    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 10 && fields[2] == device {
            let read_sectors = fields[5].parse::<u64>().ok()?;
            let write_sectors = fields[9].parse::<u64>().ok()?;
            return Some((read_sectors, write_sectors));
        }
    }
    None
}

/// Resolve (device_short_name, filesystem) for a path from /proc/mounts content.
///
/// Picks the longest matching mount point and strips partition suffix from
/// device name: /dev/nvme0n1p2 -> nvme0n1, /dev/sda1 -> sda.
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

/// Strip partition suffix from device path:
/// /dev/nvme0n1p2 -> nvme0n1, /dev/sda1 -> sda
fn strip_partition_suffix(device: &str) -> String {
    // Get the basename
    let basename = device.rsplit('/').next().unwrap_or(device);

    // NVMe: nvme0n1p2 -> nvme0n1 (strip pN suffix)
    if basename.starts_with("nvme") {
        if let Some(pos) = basename.rfind('p') {
            // Ensure what follows 'p' is a digit (partition number)
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
// High-level capture functions
// ---------------------------------------------------------------------------

/// Reads all procfs/sysfs to capture static environment. Never fails.
pub fn capture_environment(data_path: &str) -> EnvironmentSnapshot {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let kernel = fs::read_to_string("/proc/version")
        .ok()
        .map(|s| {
            // Extract just the kernel version, e.g. "Linux version 6.19.6-1-cachyos-eevdf ..."
            s.split_whitespace().nth(2).unwrap_or("").to_string()
        })
        .unwrap_or_default();

    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let (disk_device, filesystem) = parse_mount_info(&mounts, data_path).unwrap_or_default();
    let disk_scheduler = read_disk_scheduler(&disk_device);

    EnvironmentSnapshot {
        cpu_model: parse_cpu_model(&cpuinfo),
        cpu_cores: parse_cpu_cores(&cpuinfo),
        ram_total_mb: parse_mem_total_mb(&meminfo),
        kernel,
        disk_device,
        disk_scheduler,
        filesystem,
    }
}

/// Reads per-batch environment. Never fails (returns defaults on error).
pub fn read_batch_environment(disk_tracker: &mut DiskStatsTracker) -> BatchEnvironment {
    let loadavg = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let (disk_read_mb, disk_write_mb) = disk_tracker.read_delta();

    BatchEnvironment {
        load_avg_1m: parse_load_avg_1m(&loadavg),
        mem_available_mb: parse_mem_available_mb(&meminfo),
        disk_read_mb,
        disk_write_mb,
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
}
