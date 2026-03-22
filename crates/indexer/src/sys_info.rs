use std::ffi::CStr;
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

/// High-level: detect the `/proc/diskstats` device name for a data path.
///
/// Reads `/proc/self/mountinfo` and `/proc/diskstats`, using major:minor
/// matching with fallback to source device name matching. For device-mapper
/// paths (`/dev/mapper/*`), follows the symlink to resolve the `dm-N` name.
/// Never fails (returns empty string on any resolution failure).
pub fn detect_disk_device(data_path: &str) -> String {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let diskstats = fs::read_to_string("/proc/diskstats").unwrap_or_default();

    // Try pure resolution (major:minor or source device basename)
    if let Some(device) = resolve_diskstats_device(&mountinfo, &diskstats, data_path) {
        return device;
    }

    // Fallback: source device may be a symlink (/dev/mapper/luks-... -> ../dm-0).
    // Follow it and check if the target appears in diskstats.
    if let Some(entry) = parse_mountinfo_entry(&mountinfo, data_path) {
        if let Ok(target) = fs::read_link(&entry.source_device) {
            if let Some(name) = target.file_name().and_then(|n| n.to_str()) {
                if find_diskstats_device_by_name(&diskstats, name).is_some() {
                    return name.to_string();
                }
            }
        }
    }

    String::new()
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
/// CPU/RAM/kernel, with Linux-specific procfs/sysfs for disk info.
/// Never fails (returns defaults on any resolution failure).
pub fn capture_environment(data_path: &str) -> EnvironmentSnapshot {
    // Disk info is Linux-specific (procfs/sysfs); returns empty on macOS.
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let (disk_device, filesystem) = parse_mount_info(&mounts, data_path).unwrap_or_default();
    let disk_scheduler = read_disk_scheduler(&disk_device);

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

/// Read per-batch environment. Uses cross-platform POSIX APIs for load/memory,
/// with Linux-specific procfs for disk I/O deltas.
/// Never fails (returns defaults on error).
pub fn read_batch_environment(disk_tracker: &mut DiskStatsTracker) -> BatchEnvironment {
    let (disk_read_mb, disk_write_mb) = disk_tracker.read_delta();

    BatchEnvironment {
        load_avg_1m: posix_load_avg_1m(),
        mem_available_mb: read_mem_available_mb(),
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
}
