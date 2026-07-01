//! Network-store key encoding. Manual byte layout (see keys.rs convention).

/// Reserved single-byte key for the latest-round status singleton in CF_NET_STATS.
/// 0x00 can never collide with a history key because metric ids start at 1.
pub const STATS_STATUS_KEY: [u8; 1] = [0x00];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Metric {
    TotalNodes = 1,
    ReachableNodes = 2,
    VersionShare = 3,
    CountryShare = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Granularity {
    Hour = 1,
    Day = 2,
}

impl Granularity {
    pub fn seconds(self) -> u64 {
        match self {
            Granularity::Hour => 3600,
            Granularity::Day => 86_400,
        }
    }
}

/// `[metric][gran][ts_bucket big-endian u64]` — BE keeps key order chronological.
pub fn history_key(metric: Metric, gran: Granularity, ts_bucket: u64) -> [u8; 10] {
    let mut k = [0u8; 10];
    k[0] = metric as u8;
    k[1] = gran as u8;
    k[2..10].copy_from_slice(&ts_bucket.to_be_bytes());
    k
}

pub fn history_prefix(metric: Metric, gran: Granularity) -> [u8; 2] {
    [metric as u8, gran as u8]
}

/// Floor a unix-seconds timestamp into a bucket index for the granularity.
pub fn bucket_of(unix_secs: u64, gran: Granularity) -> u64 {
    unix_secs / gran.seconds()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_key_layout_and_ordering() {
        let k = history_key(Metric::TotalNodes, Granularity::Hour, 0x0102030405060708);
        assert_eq!(k[0], Metric::TotalNodes as u8);
        assert_eq!(k[1], Granularity::Hour as u8);
        assert_eq!(&k[2..10], &0x0102030405060708u64.to_be_bytes());
        // Big-endian bucket ⇒ lexicographic key order == chronological order.
        let a = history_key(Metric::TotalNodes, Granularity::Hour, 10);
        let b = history_key(Metric::TotalNodes, Granularity::Hour, 11);
        assert!(a < b);
        // Prefix isolates a (metric, gran) series.
        assert_eq!(
            &history_prefix(Metric::TotalNodes, Granularity::Hour),
            &k[0..2]
        );
        // Status singleton can never collide with a history key (metric ids start at 1).
        assert_ne!(STATS_STATUS_KEY[0], Metric::TotalNodes as u8);
    }
    #[test]
    fn bucket_math() {
        assert_eq!(bucket_of(7200, Granularity::Hour), 2);
        assert_eq!(bucket_of(172_800, Granularity::Day), 2);
    }
}
