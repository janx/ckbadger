use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;

/// block_proposals: block_number, proposal_index, proposal_id
const PROPOSALS_COLUMN_COUNT: i16 = 3;

pub struct CopyProposalsWriter {
    buffer: BinaryCopyBuffer,
}

impl Default for CopyProposalsWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyProposalsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(PROPOSALS_COLUMN_COUNT),
        }
    }

    pub fn add_proposal(&mut self, block_number: i64, proposal_index: i16, proposal_id: &[u8]) {
        self.buffer.start_row();
        self.buffer.write_i64(block_number);
        self.buffer.write_i16(proposal_index);
        self.buffer.write_bytea(proposal_id);
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }
}

/// Type alias for proposal data tuple: (block_number, proposal_index, proposal_id)
pub type ProposalData<'a> = (i64, i16, &'a [u8]);

pub async fn copy_proposals(client: &Client, proposals: &[ProposalData<'_>]) -> Result<u64> {
    if proposals.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyProposalsWriter::new();
    for (block_number, proposal_index, proposal_id) in proposals {
        writer.add_proposal(*block_number, *proposal_index, proposal_id);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY block_proposals (block_number, proposal_index, proposal_id) FROM STDIN WITH (FORMAT BINARY)")
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_proposals_writer() {
        let writer = CopyProposalsWriter::new();
        let data = writer.finish();
        // Header (19) + trailer (2) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_proposals_writer_with_data() {
        let mut writer = CopyProposalsWriter::new();
        let proposal_id = vec![0u8; 10]; // 10-byte short transaction ID

        writer.add_proposal(100, 0, &proposal_id);
        let data = writer.finish();

        // Header (19) + column_count (2) + row_data + trailer (2)
        // row_data: i64 + i16 + bytea(10)
        //         = (4+8) + (4+2) + (4+10)
        //         = 12 + 6 + 14 = 32
        assert_eq!(data.len(), 19 + 2 + 32 + 2);
    }

    #[test]
    fn test_copy_proposals_writer_multiple_rows() {
        let mut writer = CopyProposalsWriter::new();
        let proposal_id = vec![0u8; 10];

        for i in 0..3 {
            writer.add_proposal(100, i as i16, &proposal_id);
        }

        let data = writer.finish();
        // Header (19) + (column_count (2) + row_data (32)) * 3 + trailer (2)
        assert_eq!(data.len(), 19 + (2 + 32) * 3 + 2);
    }
}
