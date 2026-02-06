#![allow(dead_code)]

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct DeferrableConstraint {
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
}

pub async fn rebuild_partitioned_constraint(_constraint: &DeferrableConstraint) -> Result<()> {
    Ok(())
}
