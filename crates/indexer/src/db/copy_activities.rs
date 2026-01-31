use anyhow::Result;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::parser::activity::ParsedActivity;

/// 13 columns: activity_id, activity_type, activity_category, block_number, tx_hash,
///             tx_index, activity_index, from_lock_hash, to_lock_hash, amount,
///             asset_id, metadata, timestamp
const ACTIVITIES_COLUMN_COUNT: i16 = 13;

pub struct CopyActivitiesWriter {
    buffer: BinaryCopyBuffer,
    row_count: usize,
}

impl CopyActivitiesWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(ACTIVITIES_COLUMN_COUNT),
            row_count: 0,
        }
    }

    /// Add an activity record to the buffer
    ///
    /// # Arguments
    /// * `activity` - Parsed activity from ActivityParser
    /// * `block_number` - Block number (for partition routing)
    /// * `timestamp` - Block timestamp
    pub fn add_activity(
        &mut self,
        activity: &ParsedActivity,
        block_number: i64,
        timestamp: DateTime<Utc>,
    ) {
        self.buffer.start_row();

        // activity_id (BYTEA)
        self.buffer.write_bytea(&activity.activity_id);
        // activity_type (VARCHAR)
        self.buffer.write_text(activity.activity_type.as_str());
        // activity_category (VARCHAR)
        self.buffer.write_text(activity.activity_category.as_str());
        // block_number (BIGINT)
        self.buffer.write_i64(block_number);
        // tx_hash (BYTEA)
        self.buffer.write_bytea(&activity.tx_hash);
        // tx_index (INTEGER)
        self.buffer.write_i32(activity.tx_index);
        // activity_index (SMALLINT)
        self.buffer.write_i16(activity.activity_index);
        // from_lock_hash (BYTEA, nullable)
        self.buffer
            .write_bytea_opt(activity.from_lock_hash.as_deref());
        // to_lock_hash (BYTEA, nullable)
        self.buffer
            .write_bytea_opt(activity.to_lock_hash.as_deref());
        // amount (NUMERIC as TEXT - PostgreSQL accepts text for numeric)
        self.buffer.write_text(&activity.amount);
        // asset_id (BYTEA, nullable)
        self.buffer.write_bytea_opt(activity.asset_id.as_deref());
        // metadata (JSONB as TEXT - PostgreSQL parses JSON from text)
        self.buffer.write_text(&activity.metadata.to_string());
        // timestamp (TIMESTAMPTZ)
        self.buffer.write_timestamptz(timestamp);

        self.row_count += 1;
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }

    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }
}

impl Default for CopyActivitiesWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Copy activities to database using binary COPY protocol
pub async fn copy_activities(
    client: &Client,
    activities: &[ParsedActivity],
    block_number: i64,
    timestamp: DateTime<Utc>,
) -> Result<u64> {
    if activities.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyActivitiesWriter::new();
    for activity in activities {
        writer.add_activity(activity, block_number, timestamp);
    }

    let data = writer.finish();

    let sink = client
        .copy_in(
            "COPY activities (activity_id, activity_type, activity_category, block_number, \
             tx_hash, tx_index, activity_index, from_lock_hash, to_lock_hash, amount, \
             asset_id, metadata, timestamp) FROM STDIN WITH (FORMAT BINARY)",
        )
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

/// Batch copy activities from multiple blocks
/// Each item is (block_number, timestamp, activities)
pub async fn copy_activities_batch(
    client: &Client,
    batch: &[(i64, DateTime<Utc>, Vec<ParsedActivity>)],
) -> Result<u64> {
    let total_count: usize = batch.iter().map(|(_, _, acts)| acts.len()).sum();
    if total_count == 0 {
        return Ok(0);
    }

    let mut writer = CopyActivitiesWriter::new();
    for (block_number, timestamp, activities) in batch {
        for activity in activities {
            writer.add_activity(activity, *block_number, *timestamp);
        }
    }

    let data = writer.finish();

    let sink = client
        .copy_in(
            "COPY activities (activity_id, activity_type, activity_category, block_number, \
             tx_hash, tx_index, activity_index, from_lock_hash, to_lock_hash, amount, \
             asset_id, metadata, timestamp) FROM STDIN WITH (FORMAT BINARY)",
        )
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

/// Delete activities for reorg rollback
/// Uses block_number range which is efficient due to RANGE partitioning
pub async fn delete_activities_range(
    client: &Client,
    from_block: i64,
    to_block: i64,
) -> Result<u64> {
    let result = client
        .execute(
            "DELETE FROM activities WHERE block_number >= $1 AND block_number <= $2",
            &[&from_block, &to_block],
        )
        .await?;
    Ok(result)
}

/// Delete all activities at or after a block number (for reorg)
pub async fn delete_activities_from(client: &Client, from_block: i64) -> Result<u64> {
    let result = client
        .execute(
            "DELETE FROM activities WHERE block_number >= $1",
            &[&from_block],
        )
        .await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use ckbadger_common::{ActivityCategory, ActivityMetadata, ActivityType};

    fn make_test_activity(
        activity_type: ActivityType,
        tx_index: i32,
        activity_index: i16,
    ) -> ParsedActivity {
        let tx_hash = vec![0xaa; 32];
        let activity_id =
            ParsedActivity::compute_activity_id(&tx_hash, &activity_type, activity_index);

        ParsedActivity {
            activity_id,
            activity_type,
            activity_category: activity_type.category(),
            tx_hash,
            tx_index,
            activity_index,
            from_lock_hash: Some(vec![0xbb; 32]),
            to_lock_hash: Some(vec![0xcc; 32]),
            amount: "100000000".to_string(),
            asset_id: None,
            metadata: ActivityMetadata::CkbTransfer {}.to_json(),
        }
    }

    #[test]
    fn test_copy_activities_writer_creates_buffer() {
        let writer = CopyActivitiesWriter::new();
        let data = writer.finish();
        // At minimum, has header (19 bytes) + trailer (2 bytes)
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_activities_writer_is_empty() {
        let writer = CopyActivitiesWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_activities_writer_add_activity() {
        let mut writer = CopyActivitiesWriter::new();
        let activity = make_test_activity(ActivityType::CkbTransfer, 0, 0);
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        writer.add_activity(&activity, 12345, timestamp);

        assert!(!writer.is_empty());
        assert_eq!(writer.row_count(), 1);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_activities_writer_multiple_activities() {
        let mut writer = CopyActivitiesWriter::new();
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let activity1 = make_test_activity(ActivityType::CkbTransfer, 0, 0);
        let activity2 = make_test_activity(ActivityType::TokenMint, 1, 0);
        let activity3 = make_test_activity(ActivityType::DaoDeposit, 2, 0);

        writer.add_activity(&activity1, 12345, timestamp);
        writer.add_activity(&activity2, 12345, timestamp);
        writer.add_activity(&activity3, 12346, timestamp);

        assert_eq!(writer.row_count(), 3);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_activities_writer_nullable_fields() {
        let mut writer = CopyActivitiesWriter::new();
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Create activity with NULL from_lock_hash (e.g., cellbase reward)
        let activity = ParsedActivity {
            activity_id: vec![0x11; 32],
            activity_type: ActivityType::CellbaseReward,
            activity_category: ActivityCategory::Cellbase,
            tx_hash: vec![0xaa; 32],
            tx_index: 0,
            activity_index: 0,
            from_lock_hash: None,
            to_lock_hash: Some(vec![0xcc; 32]),
            amount: "500000000000".to_string(),
            asset_id: None,
            metadata: ActivityMetadata::CellbaseReward {
                total_reward: "500000000000".to_string(),
                block_reward: "400000000000".to_string(),
                proposal_reward: "100000000000".to_string(),
            }
            .to_json(),
        };

        writer.add_activity(&activity, 12345, timestamp);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_activities_writer_with_asset_id() {
        let mut writer = CopyActivitiesWriter::new();
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Create token transfer with asset_id
        let activity = ParsedActivity {
            activity_id: vec![0x22; 32],
            activity_type: ActivityType::TokenTransfer,
            activity_category: ActivityCategory::Token,
            tx_hash: vec![0xaa; 32],
            tx_index: 1,
            activity_index: 0,
            from_lock_hash: Some(vec![0xbb; 32]),
            to_lock_hash: Some(vec![0xcc; 32]),
            amount: "1000000000".to_string(),
            asset_id: Some(vec![0xdd; 32]),
            metadata: ActivityMetadata::Token {
                symbol: Some("SEAL".to_string()),
                decimals: 8,
                token_type_hash: "0xdddd".to_string(),
            }
            .to_json(),
        };

        writer.add_activity(&activity, 12345, timestamp);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_activities_writer_large_amount() {
        let mut writer = CopyActivitiesWriter::new();
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Very large amount (token with 18 decimals and large supply)
        let mut activity = make_test_activity(ActivityType::TokenTransfer, 0, 0);
        activity.amount = "1000000000000000000000000000".to_string(); // 10^27

        writer.add_activity(&activity, 12345, timestamp);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_activities_writer_all_activity_types() {
        let mut writer = CopyActivitiesWriter::new();
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let activity_types = [
            ActivityType::CkbTransfer,
            ActivityType::CellbaseReward,
            ActivityType::TokenMint,
            ActivityType::TokenTransfer,
            ActivityType::TokenBurn,
            ActivityType::DobMint,
            ActivityType::DobTransfer,
            ActivityType::DobBurn,
            ActivityType::NftMint,
            ActivityType::NftTransfer,
            ActivityType::DaoDeposit,
            ActivityType::DaoWithdrawRequest,
            ActivityType::DaoWithdrawComplete,
            ActivityType::ScriptDeploy,
            ActivityType::RgbppTransfer,
            ActivityType::RgbppLeapIn,
            ActivityType::RgbppLeapOut,
            ActivityType::RgbppIssuance,
        ];

        for (i, activity_type) in activity_types.iter().enumerate() {
            let activity = make_test_activity(*activity_type, i as i32, 0);
            writer.add_activity(&activity, 12345, timestamp);
        }

        assert_eq!(writer.row_count(), 18);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_activities_writer_default() {
        let writer = CopyActivitiesWriter::default();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_activities_writer_metadata_json() {
        let mut writer = CopyActivitiesWriter::new();
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Complex metadata with nested structure
        let activity = ParsedActivity {
            activity_id: vec![0x33; 32],
            activity_type: ActivityType::RgbppLeapIn,
            activity_category: ActivityCategory::Rgbpp,
            tx_hash: vec![0xaa; 32],
            tx_index: 0,
            activity_index: 0,
            from_lock_hash: Some(vec![0xbb; 32]),
            to_lock_hash: Some(vec![0xcc; 32]),
            amount: "100000000".to_string(),
            asset_id: Some(vec![0xee; 32]),
            metadata: ActivityMetadata::Rgbpp {
                btc_txid: Some(
                    "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                        .to_string(),
                ),
                commitment: Some("0xabcdef".to_string()),
                asset_id: Some("0xdeadbeef".to_string()),
            }
            .to_json(),
        };

        writer.add_activity(&activity, 12345, timestamp);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }
}
