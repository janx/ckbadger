use anyhow::Result;

use crate::parser::transaction::{ParsedCellDep, ParsedInput};

use super::BatchWriter;

impl BatchWriter {
    pub async fn insert_transaction_inputs_batch(
        &self,
        inputs: &[(&[u8], i64, i16, &ParsedInput)],
    ) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _, _, _)| *h).collect();
        let tx_block_numbers: Vec<i64> = inputs.iter().map(|(_, b, _, _)| *b).collect();
        let input_indices: Vec<i16> = inputs.iter().map(|(_, _, i, _)| *i).collect();
        let prev_tx_hashes: Vec<&[u8]> = inputs
            .iter()
            .map(|(_, _, _, inp)| inp.previous_tx_hash.as_slice())
            .collect();
        let prev_output_indices: Vec<i16> = inputs
            .iter()
            .map(|(_, _, _, inp)| inp.previous_output_index as i16)
            .collect();
        let sinces: Vec<i64> = inputs.iter().map(|(_, _, _, inp)| inp.since).collect();

        sqlx::query(
            r#"
            INSERT INTO transaction_inputs (
                tx_hash, tx_block_number, input_index, previous_tx_hash, previous_output_index, since
            )
            SELECT * FROM UNNEST($1::bytea[], $2::bigint[], $3::smallint[], $4::bytea[], $5::smallint[], $6::numeric[])
            ON CONFLICT (tx_block_number, tx_hash, input_index) DO NOTHING
            "#,
        )
        .bind(&tx_hashes)
        .bind(&tx_block_numbers)
        .bind(&input_indices)
        .bind(&prev_tx_hashes)
        .bind(&prev_output_indices)
        .bind(&sinces)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_transaction_cell_deps_batch(
        &self,
        cell_deps: &[(&[u8], i64, i16, &ParsedCellDep)],
    ) -> Result<()> {
        if cell_deps.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = cell_deps.iter().map(|(h, _, _, _)| *h).collect();
        let tx_block_numbers: Vec<i64> = cell_deps.iter().map(|(_, b, _, _)| *b).collect();
        let dep_indices: Vec<i16> = cell_deps.iter().map(|(_, _, i, _)| *i).collect();
        let out_point_tx_hashes: Vec<&[u8]> = cell_deps
            .iter()
            .map(|(_, _, _, dep)| dep.out_point_tx_hash.as_slice())
            .collect();
        let out_point_indices: Vec<i16> = cell_deps
            .iter()
            .map(|(_, _, _, dep)| dep.out_point_index)
            .collect();
        let dep_types: Vec<i16> = cell_deps
            .iter()
            .map(|(_, _, _, dep)| dep.dep_type)
            .collect();

        sqlx::query(
            r#"
            INSERT INTO transaction_cell_deps (
                tx_hash, tx_block_number, dep_index, out_point_tx_hash, out_point_index, dep_type
            )
            SELECT * FROM UNNEST($1::bytea[], $2::bigint[], $3::smallint[], $4::bytea[], $5::smallint[], $6::smallint[])
            ON CONFLICT (tx_block_number, tx_hash, dep_index) DO NOTHING
            "#,
        )
        .bind(&tx_hashes)
        .bind(&tx_block_numbers)
        .bind(&dep_indices)
        .bind(&out_point_tx_hashes)
        .bind(&out_point_indices)
        .bind(&dep_types)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_block_proposals_batch(
        &self,
        block_number: i64,
        proposals: &[Vec<u8>],
    ) -> Result<()> {
        if proposals.is_empty() {
            return Ok(());
        }

        let block_numbers: Vec<i64> = vec![block_number; proposals.len()];
        let proposal_indices: Vec<i16> = (0..proposals.len() as i16).collect();
        let proposal_ids: Vec<&[u8]> = proposals.iter().map(|p| p.as_slice()).collect();

        sqlx::query(
            r#"
            INSERT INTO block_proposals (block_number, proposal_index, proposal_id)
            SELECT * FROM UNNEST($1::bigint[], $2::smallint[], $3::bytea[])
            ON CONFLICT (block_number, proposal_index) DO NOTHING
            "#,
        )
        .bind(&block_numbers)
        .bind(&proposal_indices)
        .bind(&proposal_ids)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert cell_flows batch using UNNEST
    /// Tuple: (block_number, tx_hash, output_index, flow_type, lock_script_hash, capacity, data_size)
    pub async fn insert_cell_flows_batch(
        &self,
        flows: &[(i64, &[u8], i16, i16, &[u8], i64, i32)],
    ) -> Result<()> {
        if flows.is_empty() {
            return Ok(());
        }

        let block_numbers: Vec<i64> = flows.iter().map(|(b, _, _, _, _, _, _)| *b).collect();
        let tx_hashes: Vec<&[u8]> = flows.iter().map(|(_, h, _, _, _, _, _)| *h).collect();
        let output_indices: Vec<i16> = flows.iter().map(|(_, _, o, _, _, _, _)| *o).collect();
        let flow_types: Vec<i16> = flows.iter().map(|(_, _, _, f, _, _, _)| *f).collect();
        let lock_script_hashes: Vec<&[u8]> = flows.iter().map(|(_, _, _, _, l, _, _)| *l).collect();
        let capacities: Vec<i64> = flows.iter().map(|(_, _, _, _, _, c, _)| *c).collect();
        let data_sizes: Vec<i32> = flows.iter().map(|(_, _, _, _, _, _, d)| *d).collect();

        sqlx::query(
            r#"
            INSERT INTO cell_flows (
                block_number, tx_hash, output_index, flow_type, lock_script_hash, capacity, data_size
            )
            SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::smallint[], $4::smallint[], $5::bytea[], $6::bigint[], $7::integer[])
            "#,
        )
        .bind(&block_numbers)
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&flow_types)
        .bind(&lock_script_hashes)
        .bind(&capacities)
        .bind(&data_sizes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_proposals_batch(&self, proposals: &[(i64, i16, &[u8])]) -> Result<()> {
        if proposals.is_empty() {
            return Ok(());
        }

        let block_numbers: Vec<i64> = proposals.iter().map(|(b, _, _)| *b).collect();
        let proposal_indices: Vec<i16> = proposals.iter().map(|(_, i, _)| *i).collect();
        let proposal_ids: Vec<&[u8]> = proposals.iter().map(|(_, _, p)| *p).collect();

        sqlx::query(
            r#"
            INSERT INTO block_proposals (block_number, proposal_index, proposal_id)
            SELECT * FROM UNNEST($1::bigint[], $2::smallint[], $3::bytea[])
            ON CONFLICT (block_number, proposal_index) DO NOTHING
            "#,
        )
        .bind(&block_numbers)
        .bind(&proposal_indices)
        .bind(&proposal_ids)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
