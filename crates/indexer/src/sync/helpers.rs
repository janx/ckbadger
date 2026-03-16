//! Pure utility functions with no CKB domain knowledge.
//!
//! Hex parsing, type conversions, panic/cgroup helpers, etc.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use ckb_hash::new_blake2b;

use crate::runtime_diag::CgroupMemorySnapshot;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub(crate) const STARTUP_PHASE_NONE: u8 = 0;
pub(crate) const STARTUP_PHASE_ROLLBACK_CLEANUP: u8 = 1;

pub(crate) const PIPELINE_RESET_REASON_UNKNOWN: u8 = 0;
pub(crate) const PIPELINE_RESET_REASON_BATCH_MISMATCH: u8 = 1;
pub(crate) const PIPELINE_RESET_REASON_REORG_HANDLED: u8 = 2;
pub(crate) const PIPELINE_RESET_REASON_DEEP_FORK_PAUSED: u8 = 3;
pub(crate) const PIPELINE_RESET_REASON_BATCH_WRITE_FAILED: u8 = 4;

pub(crate) const ADAPTIVE_REASON_UNKNOWN: u8 = 0;
pub(crate) const ADAPTIVE_REASON_PRESSURE_BACKOFF: u8 = 1;
pub(crate) const ADAPTIVE_REASON_PRESSURE_BACKOFF_FLOOR_DOWN: u8 = 2;
pub(crate) const ADAPTIVE_REASON_HEALTHY_STEP_UP: u8 = 3;
pub(crate) const ADAPTIVE_REASON_HEALTHY_STEP_UP_FLOOR_RECOVER: u8 = 4;
pub(crate) const ADAPTIVE_REASON_MODERATE_BACKOFF: u8 = 5;
pub(crate) const ADAPTIVE_REASON_MODERATE_BACKOFF_INFLIGHT_RELIEF: u8 = 6;
pub(crate) const ADAPTIVE_REASON_MODERATE_BACKOFF_FLOOR_DOWN: u8 = 7;
pub(crate) const ADAPTIVE_REASON_THROUGHPUT_BACKOFF: u8 = 8;
pub(crate) const ADAPTIVE_REASON_ADJUSTED: u8 = 9;
pub(crate) const ADAPTIVE_REASON_EARLY_HEIGHT_BOOST: u8 = 10;
pub(crate) const ADAPTIVE_REASON_SEVERE_PRESSURE_BACKOFF: u8 = 11;

// ---------------------------------------------------------------------------
// Startup phase codec
// ---------------------------------------------------------------------------

pub(crate) fn decode_startup_phase(phase: u8) -> Option<&'static str> {
    match phase {
        STARTUP_PHASE_ROLLBACK_CLEANUP => Some("rollback_cleanup"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Pipeline reset reason codec
// ---------------------------------------------------------------------------

pub(crate) fn encode_pipeline_reset_reason(reason: &'static str) -> u8 {
    match reason {
        "pipeline batch mismatch" => PIPELINE_RESET_REASON_BATCH_MISMATCH,
        "reorg handled" => PIPELINE_RESET_REASON_REORG_HANDLED,
        "deep fork paused" => PIPELINE_RESET_REASON_DEEP_FORK_PAUSED,
        "batch write failed" => PIPELINE_RESET_REASON_BATCH_WRITE_FAILED,
        _ => PIPELINE_RESET_REASON_UNKNOWN,
    }
}

pub(crate) fn decode_pipeline_reset_reason(reason_code: u8) -> &'static str {
    match reason_code {
        PIPELINE_RESET_REASON_BATCH_MISMATCH => "pipeline batch mismatch",
        PIPELINE_RESET_REASON_REORG_HANDLED => "reorg handled",
        PIPELINE_RESET_REASON_DEEP_FORK_PAUSED => "deep fork paused",
        PIPELINE_RESET_REASON_BATCH_WRITE_FAILED => "batch write failed",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Adaptive batch reason codec
// ---------------------------------------------------------------------------

pub(crate) fn encode_adaptive_batch_reason(reason: &'static str) -> u8 {
    match reason {
        "pressure_backoff" => ADAPTIVE_REASON_PRESSURE_BACKOFF,
        "pressure_backoff_floor_down" => ADAPTIVE_REASON_PRESSURE_BACKOFF_FLOOR_DOWN,
        "healthy_step_up" => ADAPTIVE_REASON_HEALTHY_STEP_UP,
        "healthy_step_up_floor_recover" => ADAPTIVE_REASON_HEALTHY_STEP_UP_FLOOR_RECOVER,
        "moderate_backoff" => ADAPTIVE_REASON_MODERATE_BACKOFF,
        "moderate_backoff_inflight_relief" => ADAPTIVE_REASON_MODERATE_BACKOFF_INFLIGHT_RELIEF,
        "moderate_backoff_floor_down" => ADAPTIVE_REASON_MODERATE_BACKOFF_FLOOR_DOWN,
        "throughput_backoff" => ADAPTIVE_REASON_THROUGHPUT_BACKOFF,
        "adjusted" => ADAPTIVE_REASON_ADJUSTED,
        "early_height_boost" => ADAPTIVE_REASON_EARLY_HEIGHT_BOOST,
        "severe_pressure_backoff" => ADAPTIVE_REASON_SEVERE_PRESSURE_BACKOFF,
        _ => ADAPTIVE_REASON_UNKNOWN,
    }
}

pub(crate) fn decode_adaptive_batch_reason(reason_code: u8) -> Option<&'static str> {
    match reason_code {
        ADAPTIVE_REASON_PRESSURE_BACKOFF => Some("pressure_backoff"),
        ADAPTIVE_REASON_PRESSURE_BACKOFF_FLOOR_DOWN => Some("pressure_backoff_floor_down"),
        ADAPTIVE_REASON_HEALTHY_STEP_UP => Some("healthy_step_up"),
        ADAPTIVE_REASON_HEALTHY_STEP_UP_FLOOR_RECOVER => Some("healthy_step_up_floor_recover"),
        ADAPTIVE_REASON_MODERATE_BACKOFF => Some("moderate_backoff"),
        ADAPTIVE_REASON_MODERATE_BACKOFF_INFLIGHT_RELIEF => {
            Some("moderate_backoff_inflight_relief")
        }
        ADAPTIVE_REASON_MODERATE_BACKOFF_FLOOR_DOWN => Some("moderate_backoff_floor_down"),
        ADAPTIVE_REASON_THROUGHPUT_BACKOFF => Some("throughput_backoff"),
        ADAPTIVE_REASON_ADJUSTED => Some("adjusted"),
        ADAPTIVE_REASON_EARLY_HEIGHT_BOOST => Some("early_height_boost"),
        ADAPTIVE_REASON_SEVERE_PRESSURE_BACKOFF => Some("severe_pressure_backoff"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Outpoint / index helpers
// ---------------------------------------------------------------------------

pub(crate) fn parsed_input_outpoint_index_i16(previous_output_index: i32, context: &str) -> i16 {
    if previous_output_index < 0 {
        panic!(
            "negative input previous_output_index while indexing outpoint: context={}, previous_output_index={}",
            context, previous_output_index
        );
    }
    i16::try_from(previous_output_index).unwrap_or_else(|_| {
        panic!(
            "input previous_output_index exceeds i16 range while indexing outpoint: context={}, previous_output_index={}",
            context, previous_output_index
        )
    })
}

pub(crate) fn format_outpoint_sample(outpoints: &[(Vec<u8>, i16)], max_items: usize) -> String {
    if outpoints.is_empty() {
        return "none".to_string();
    }

    outpoints
        .iter()
        .take(max_items)
        .map(|(tx_hash, output_index)| format!("0x{}:{}", hex::encode(tx_hash), output_index))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Checked numeric conversions
// ---------------------------------------------------------------------------

pub(crate) fn checked_usize_to_i16(value: usize, label: &str) -> Result<i16> {
    i16::try_from(value).map_err(|_| anyhow!("{} exceeds i16 range: {}", label, value))
}

pub(crate) fn checked_i32_to_i16(value: i32, label: &str) -> Result<i16> {
    i16::try_from(value).map_err(|_| anyhow!("{} exceeds i16 range: {}", label, value))
}

pub(crate) fn checked_usize_to_i32(value: usize, label: &str) -> i32 {
    i32::try_from(value).unwrap_or_else(|_| panic!("{} exceeds i32 range: {}", label, value))
}

// ---------------------------------------------------------------------------
// Tx hash helpers
// ---------------------------------------------------------------------------

pub(crate) fn tx_hash_key32(tx_hash: &[u8], context: &str) -> Result<[u8; 32]> {
    tx_hash.try_into().map_err(|_| {
        anyhow!(
            "{} tx hash must be 32 bytes, got {}",
            context,
            tx_hash.len()
        )
    })
}

pub(crate) fn short_tx_hash(tx_hash: &[u8]) -> String {
    let encoded = hex::encode(tx_hash);
    if encoded.len() <= 16 {
        return encoded;
    }
    format!("{}..{}", &encoded[..10], &encoded[encoded.len() - 6..])
}

// ---------------------------------------------------------------------------
// Duration / timing
// ---------------------------------------------------------------------------

pub(crate) fn duration_from_millis(ms: f64) -> Duration {
    assert!(
        ms >= 0.0,
        "duration_from_millis called with negative ms: {}",
        ms
    );
    let micros = (ms * 1000.0).round();
    Duration::from_micros(micros as u64)
}

pub(crate) fn next_fetch_start_after_batch(end_block: u64) -> u64 {
    end_block
        .checked_add(1)
        .expect("fetch batch end_block overflow while computing next start")
}

// ---------------------------------------------------------------------------
// Cgroup / memory
// ---------------------------------------------------------------------------

pub(crate) fn cgroup_memory_ratio_pct(snapshot: &CgroupMemorySnapshot) -> Option<f64> {
    match (snapshot.memory_current_bytes, snapshot.memory_max_bytes) {
        (Some(current), Some(max)) if max > 0 => Some((current as f64 / max as f64) * 100.0),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Panic helpers
// ---------------------------------------------------------------------------

pub(crate) fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        return (*msg).to_string();
    }
    if let Some(msg) = payload.downcast_ref::<String>() {
        return msg.clone();
    }
    "non-string panic payload".to_string()
}

// ---------------------------------------------------------------------------
// Atomics
// ---------------------------------------------------------------------------

pub(crate) fn atomic_checked_sub_u64(counter: &AtomicU64, value: u64) {
    if value == 0 {
        return;
    }
    loop {
        let current = counter.load(Ordering::Relaxed);
        let next = match current.checked_sub(value) {
            Some(n) => n,
            None => {
                panic!(
                    "pipeline counter underflow: current={}, sub_value={}",
                    current, value,
                );
            }
        };
        if counter
            .compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Hex parsing
// ---------------------------------------------------------------------------

pub(crate) fn parse_prefixed_hex_u32(field: &str, label: &str) -> Result<u32> {
    let Some(hex) = field.strip_prefix("0x") else {
        bail!("{} missing 0x prefix: {}", label, field);
    };
    u32::from_str_radix(hex, 16)
        .map_err(|e| anyhow::anyhow!("invalid {} hex '{}': {}", label, field, e))
}

pub(crate) fn parse_prefixed_hex_u64(field: &str, label: &str) -> Result<u64> {
    let Some(hex) = field.strip_prefix("0x") else {
        bail!("{} missing 0x prefix: {}", label, field);
    };
    u64::from_str_radix(hex, 16)
        .map_err(|e| anyhow::anyhow!("invalid {} hex '{}': {}", label, field, e))
}

pub(crate) fn parse_outpoint_index_i16(field: &str, label: &str) -> Result<i16> {
    let value = parse_prefixed_hex_u32(field, label)?;
    i16::try_from(value).map_err(|_| anyhow::anyhow!("{} exceeds i16 range: {}", label, value))
}

// ---------------------------------------------------------------------------
// Blake2b
// ---------------------------------------------------------------------------

pub(crate) fn blake160(data: &[u8]) -> [u8; 20] {
    let mut hasher = new_blake2b();
    hasher.update(data);

    let mut out = [0u8; 32];
    hasher.finalize(&mut out);

    let mut out160 = [0u8; 20];
    out160.copy_from_slice(&out[..20]);
    out160
}

// ---------------------------------------------------------------------------
// Tx cycles parsing
// ---------------------------------------------------------------------------

pub(crate) fn parse_tx_cycles(
    raw_cycles_hex: Option<&String>,
    tx_hash: &str,
    block_number: i64,
) -> Result<Option<i64>> {
    let Some(raw_cycles_hex) = raw_cycles_hex else {
        return Ok(None);
    };

    let cycles_u64 = u64::from_str_radix(
        raw_cycles_hex.strip_prefix("0x").unwrap_or(raw_cycles_hex),
        16,
    )
    .map_err(|e| {
        anyhow!(
            "invalid tx cycles hex '{}' for tx {} in block {}: {}",
            raw_cycles_hex,
            tx_hash,
            block_number,
            e
        )
    })?;

    // Historical CKB blocks may expose unavailable cycles as 0x0 for non-cellbase txs.
    // Treat this as missing data so cycles_worker can lazily recompute and persist real values.
    if cycles_u64 == 0 {
        return Ok(None);
    }

    i64::try_from(cycles_u64).map(Some).map_err(|_| {
        anyhow!(
            "tx cycles over i64 range '{}' for tx {} in block {}: {} (max={})",
            raw_cycles_hex,
            tx_hash,
            block_number,
            cycles_u64,
            i64::MAX
        )
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsed_input_outpoint_index_i16_accepts_non_negative_i16_range() {
        assert_eq!(parsed_input_outpoint_index_i16(0, "unit-test"), 0);
        assert_eq!(
            parsed_input_outpoint_index_i16(i16::MAX as i32, "unit-test"),
            i16::MAX
        );
    }

    #[test]
    #[should_panic(expected = "negative input previous_output_index")]
    fn test_parsed_input_outpoint_index_i16_rejects_negative() {
        let _ = parsed_input_outpoint_index_i16(-1, "unit-test");
    }

    #[test]
    #[should_panic(expected = "input previous_output_index exceeds i16 range")]
    fn test_parsed_input_outpoint_index_i16_rejects_overflow() {
        let _ = parsed_input_outpoint_index_i16(i16::MAX as i32 + 1, "unit-test");
    }

    #[test]
    fn test_format_outpoint_sample_limits_items() {
        let outpoints = vec![
            (vec![0x11; 32], 0),
            (vec![0x22; 32], 1),
            (vec![0x33; 32], 2),
        ];

        let sample = format_outpoint_sample(&outpoints, 2);
        assert!(sample.contains(&format!("0x{}:0", "11".repeat(32))));
        assert!(sample.contains(&format!("0x{}:1", "22".repeat(32))));
        assert!(!sample.contains(&format!("0x{}:2", "33".repeat(32))));
    }

    #[test]
    fn test_panic_payload_to_string_handles_common_payload_types() {
        assert_eq!(panic_payload_to_string(&"panic-str"), "panic-str");
        assert_eq!(
            panic_payload_to_string(&"panic-owned".to_string()),
            "panic-owned"
        );
        assert_eq!(panic_payload_to_string(&123u32), "non-string panic payload");
    }

    #[test]
    fn test_parse_prefixed_hex_u32_errors_on_invalid_input() {
        let err = parse_prefixed_hex_u32("1234", "test field").unwrap_err();
        assert!(err.to_string().contains("missing 0x prefix"));

        let err = parse_prefixed_hex_u32("0xzz", "test field").unwrap_err();
        assert!(err.to_string().contains("invalid test field hex"));
    }

    #[test]
    fn test_parse_prefixed_hex_u64_errors_on_invalid_input() {
        let err = parse_prefixed_hex_u64("1234", "test field").unwrap_err();
        assert!(err.to_string().contains("missing 0x prefix"));

        let err = parse_prefixed_hex_u64("0xzz", "test field").unwrap_err();
        assert!(err.to_string().contains("invalid test field hex"));
    }

    #[test]
    fn test_checked_usize_to_i16_errors_on_overflow() {
        let err = checked_usize_to_i16((i16::MAX as usize) + 1, "output_index").unwrap_err();
        assert!(err.to_string().contains("output_index exceeds i16 range"));
    }

    #[test]
    fn test_tx_hash_key32_errors_on_invalid_length() {
        let err = tx_hash_key32(&[0x11; 31], "cache lookup").unwrap_err();
        assert!(err
            .to_string()
            .contains("cache lookup tx hash must be 32 bytes"));
    }

    #[test]
    fn test_parse_tx_cycles_treats_zero_as_missing() {
        let raw = "0x0".to_string();
        let cycles = parse_tx_cycles(Some(&raw), "0xabc", 200).unwrap();
        assert_eq!(cycles, None);
    }

    #[test]
    fn test_parse_tx_cycles_parses_positive_value() {
        let raw = "0x1a".to_string();
        let cycles = parse_tx_cycles(Some(&raw), "0xabc", 200).unwrap();
        assert_eq!(cycles, Some(26));
    }

    #[test]
    fn test_parse_tx_cycles_errors_on_invalid_hex() {
        let raw = "0xzz".to_string();
        let err = parse_tx_cycles(Some(&raw), "0xabc", 200).unwrap_err();
        assert!(err.to_string().contains("invalid tx cycles hex"));
    }

    #[test]
    fn test_parse_outpoint_index_i16_errors_on_overflow() {
        let err = parse_outpoint_index_i16("0x10000", "index").unwrap_err();
        assert!(err.to_string().contains("exceeds i16 range"));
    }

    #[test]
    fn test_pipeline_reset_reason_roundtrip_known_values() {
        let reasons = [
            "pipeline batch mismatch",
            "reorg handled",
            "deep fork paused",
            "batch write failed",
        ];
        for reason in reasons {
            let code = encode_pipeline_reset_reason(reason);
            assert_ne!(code, PIPELINE_RESET_REASON_UNKNOWN);
            assert_eq!(decode_pipeline_reset_reason(code), reason);
        }
    }

    #[test]
    fn test_pipeline_reset_reason_unknown_fallback() {
        let code = encode_pipeline_reset_reason("unexpected reason");
        assert_eq!(code, PIPELINE_RESET_REASON_UNKNOWN);
        assert_eq!(decode_pipeline_reset_reason(code), "unknown");
        assert_eq!(decode_pipeline_reset_reason(255), "unknown");
    }

    #[test]
    fn test_adaptive_reason_roundtrip_known_values() {
        let reasons = [
            "pressure_backoff",
            "pressure_backoff_floor_down",
            "severe_pressure_backoff",
            "healthy_step_up",
            "healthy_step_up_floor_recover",
            "moderate_backoff",
            "moderate_backoff_inflight_relief",
            "moderate_backoff_floor_down",
            "throughput_backoff",
            "adjusted",
            "early_height_boost",
        ];
        for reason in reasons {
            let code = encode_adaptive_batch_reason(reason);
            assert_ne!(code, ADAPTIVE_REASON_UNKNOWN);
            assert_eq!(decode_adaptive_batch_reason(code), Some(reason));
        }
    }

    #[test]
    fn test_adaptive_reason_unknown_fallback() {
        let code = encode_adaptive_batch_reason("unexpected reason");
        assert_eq!(code, ADAPTIVE_REASON_UNKNOWN);
        assert_eq!(decode_adaptive_batch_reason(code), None);
        assert_eq!(decode_adaptive_batch_reason(255), None);
    }

    #[test]
    fn test_decode_startup_phase() {
        assert_eq!(decode_startup_phase(STARTUP_PHASE_NONE), None);
        assert_eq!(
            decode_startup_phase(STARTUP_PHASE_ROLLBACK_CLEANUP),
            Some("rollback_cleanup")
        );
        assert_eq!(decode_startup_phase(99), None);
    }

    #[test]
    fn test_cgroup_memory_ratio_pct() {
        let snapshot = CgroupMemorySnapshot {
            memory_current_bytes: Some(4),
            memory_max_bytes: Some(8),
            ..Default::default()
        };
        assert_eq!(cgroup_memory_ratio_pct(&snapshot), Some(50.0));

        let unlimited = CgroupMemorySnapshot {
            memory_current_bytes: Some(4),
            memory_max_bytes: None,
            ..Default::default()
        };
        assert_eq!(cgroup_memory_ratio_pct(&unlimited), None);

        let zero_max = CgroupMemorySnapshot {
            memory_current_bytes: Some(4),
            memory_max_bytes: Some(0),
            ..Default::default()
        };
        assert_eq!(cgroup_memory_ratio_pct(&zero_max), None);
    }

    #[test]
    fn test_next_fetch_start_after_batch_stays_contiguous_across_boundaries() {
        assert_eq!(next_fetch_start_after_batch(999), 1000);
        assert_eq!(next_fetch_start_after_batch(1000), 1001);
    }

    #[test]
    #[should_panic(expected = "fetch batch end_block overflow while computing next start")]
    fn test_next_fetch_start_after_batch_panics_on_u64_max() {
        let _ = next_fetch_start_after_batch(u64::MAX);
    }
}
