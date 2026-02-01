//! Formatting utilities for CKB values and durations.

const SHANNON_PER_CKB: u128 = 100_000_000;

/// Converts shannon (smallest CKB unit, 1 CKB = 10^8 shannon) to CKB string.
pub fn shannon_to_ckb(shannon: &str) -> String {
    let num: u128 = shannon.parse().unwrap_or(0);
    shannon_to_ckb_u128(num)
}

/// Converts shannon (u128) to CKB string with up to 8 decimal places.
pub fn shannon_to_ckb_u128(shannon: u128) -> String {
    let ckb = shannon / SHANNON_PER_CKB;
    let remainder = shannon % SHANNON_PER_CKB;
    if remainder == 0 {
        format!("{}", ckb)
    } else {
        format!("{}.{:08}", ckb, remainder)
            .trim_end_matches('0')
            .to_string()
    }
}

/// Formats duration in seconds to human-readable string (e.g., "2h 30m", "3d 12h").
pub fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shannon_to_ckb_whole_number() {
        assert_eq!(shannon_to_ckb("10000000000"), "100");
        assert_eq!(shannon_to_ckb("100000000"), "1");
        assert_eq!(shannon_to_ckb("0"), "0");
    }

    #[test]
    fn test_shannon_to_ckb_with_decimals() {
        assert_eq!(shannon_to_ckb("12345678901"), "123.45678901");
        assert_eq!(shannon_to_ckb("100000001"), "1.00000001");
        assert_eq!(shannon_to_ckb("150000000"), "1.5");
    }

    #[test]
    fn test_shannon_to_ckb_invalid_input() {
        assert_eq!(shannon_to_ckb("invalid"), "0");
        assert_eq!(shannon_to_ckb(""), "0");
    }

    #[test]
    fn test_shannon_to_ckb_u128() {
        assert_eq!(shannon_to_ckb_u128(10000000000), "100");
        assert_eq!(shannon_to_ckb_u128(12345678901), "123.45678901");
        assert_eq!(shannon_to_ckb_u128(0), "0");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(3599), "59m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(86399), "23h 59m");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
        assert_eq!(format_duration(172800), "2d");
        assert_eq!(format_duration(176400), "2d 1h");
    }
}
