//! Formatting utilities for CKB values and durations.

use chrono::NaiveDate;

const SHANNON_PER_CKB: u128 = 100_000_000;

/// Converts shannon (smallest CKB unit, 1 CKB = 10^8 shannon) to CKB string.
pub fn shannon_to_ckb(shannon: &str) -> String {
    let num: u128 = match shannon.parse() {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(
                "shannon_to_ckb: failed to parse input {:?} as u128: {}",
                shannon,
                e
            );
            0
        }
    };
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

/// Parse chart date into YYYYMMDD (u32). Accepts `YYYY-MM-DD` and `YYYYMMDD`.
pub fn parse_chart_date_yyyymmdd(input: &str) -> Option<u32> {
    let s = input.trim();
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y%m%d"))
        .ok()?;
    date.format("%Y%m%d").to_string().parse::<u32>().ok()
}

/// Parse `from`/`to` date range for chart endpoints.
pub fn parse_chart_date_range(
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(Option<u32>, Option<u32>), String> {
    let from_date = match from {
        Some(value) => Some(
            parse_chart_date_yyyymmdd(value)
                .ok_or_else(|| "Invalid from date, expected YYYY-MM-DD or YYYYMMDD".to_string())?,
        ),
        None => None,
    };

    let to_date = match to {
        Some(value) => Some(
            parse_chart_date_yyyymmdd(value)
                .ok_or_else(|| "Invalid to date, expected YYYY-MM-DD or YYYYMMDD".to_string())?,
        ),
        None => None,
    };

    if let (Some(from_date), Some(to_date)) = (from_date, to_date) {
        if from_date > to_date {
            return Err("Invalid range: from date is after to date".to_string());
        }
    }

    Ok((from_date, to_date))
}

fn parse_yyyymmdd_key(date_yyyymmdd: u32) -> Result<NaiveDate, String> {
    let date_str = format!("{date_yyyymmdd:08}");
    NaiveDate::parse_from_str(&date_str, "%Y%m%d")
        .map_err(|_| format!("Invalid date key: {}", date_yyyymmdd))
}

/// Build inclusive daily keys in YYYYMMDD format.
pub fn date_keys_inclusive(
    from_date_yyyymmdd: u32,
    to_date_yyyymmdd: u32,
) -> Result<Vec<u32>, String> {
    let mut current = parse_yyyymmdd_key(from_date_yyyymmdd)?;
    let end = parse_yyyymmdd_key(to_date_yyyymmdd)?;
    if current > end {
        return Err(format!(
            "Invalid date key range: {} > {}",
            from_date_yyyymmdd, to_date_yyyymmdd
        ));
    }

    let mut out = Vec::new();
    loop {
        let key = current
            .format("%Y%m%d")
            .to_string()
            .parse::<u32>()
            .map_err(|e| format!("Failed to encode date key for {}: {}", current, e))?;
        out.push(key);
        if current == end {
            break;
        }
        current = current
            .succ_opt()
            .ok_or_else(|| format!("Date overflow while iterating from {}", current))?;
    }
    Ok(out)
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

    #[test]
    fn test_parse_chart_date_yyyymmdd() {
        assert_eq!(parse_chart_date_yyyymmdd("2024-01-15"), Some(20240115));
        assert_eq!(parse_chart_date_yyyymmdd("20240116"), Some(20240116));
        assert_eq!(parse_chart_date_yyyymmdd("2024-13-01"), None);
    }

    #[test]
    fn test_parse_chart_date_range() {
        assert_eq!(
            parse_chart_date_range(Some("2024-01-01"), Some("2024-01-31")).unwrap(),
            (Some(20240101), Some(20240131))
        );
        assert!(parse_chart_date_range(Some("invalid"), None).is_err());
        assert!(parse_chart_date_range(Some("2024-02-01"), Some("2024-01-01")).is_err());
    }

    #[test]
    fn test_date_keys_inclusive() {
        assert_eq!(
            date_keys_inclusive(20240115, 20240117).unwrap(),
            vec![20240115, 20240116, 20240117]
        );
    }

    #[test]
    fn test_date_keys_inclusive_rejects_invalid_range() {
        assert!(date_keys_inclusive(20240117, 20240115).is_err());
        assert!(date_keys_inclusive(20240230, 20240301).is_err());
    }
}
