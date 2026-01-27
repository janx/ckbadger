pub mod clickhouse;
pub mod writer;

pub use clickhouse::ClickHouseClient;
pub use writer::{
    BatchWriter, ClickHouseWriter, DaoDepositRow, ReorgResult, SecondaryIssuanceBreakdown,
};

pub fn vec_to_hash32(v: &[u8]) -> [u8; 32] {
    v.try_into().expect("hash must be 32 bytes")
}
