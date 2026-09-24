//! The scripted node the mirror's tests run against. Test-only: compiled out
//! of every non-test build (`#[cfg(test)] mod fake` in `pool/mod.rs`).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::source::{NodeHeader, PoolSource, PoolTxLookup, RawTxPool, TxPoolInfo};

/// Scripted [`PoolSource`] for the mirror's state-machine tests. Records every
/// call so tests can assert what the mirror did *not* ask for.
#[derive(Default)]
pub struct FakePoolSource {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    info: Option<Result<TxPoolInfo, String>>,
    raw_pool: RawTxPool,
    transactions: HashMap<[u8; 32], Result<Option<PoolTxLookup>, String>>,
    headers: Vec<NodeHeader>,
    /// Every call records itself and then never returns — a node whose TCP
    /// peer stopped answering without a FIN or RST.
    hang: bool,
    calls: Vec<String>,
}

impl FakePoolSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_info(&self, info: TxPoolInfo) {
        self.lock().info = Some(Ok(info));
    }

    pub fn set_info_error(&self, error: impl Into<String>) {
        self.lock().info = Some(Err(error.into()));
    }

    pub fn set_raw_pool(&self, pool: RawTxPool) {
        self.lock().raw_pool = pool;
    }

    pub fn set_transaction(&self, tx_hash: [u8; 32], lookup: PoolTxLookup) {
        self.lock().transactions.insert(tx_hash, Ok(Some(lookup)));
    }

    /// A main-chain header, answerable by hash and by number.
    pub fn set_header(&self, header: NodeHeader) {
        self.lock().headers.push(header);
    }

    /// Make every later call hang forever (see `FakeState::hang`).
    pub fn set_hang(&self, hang: bool) {
        self.lock().hang = hang;
    }

    /// Record a call; `true` when the source is scripted to hang on it.
    fn record(&self, call: String) -> bool {
        let mut state = self.lock();
        state.calls.push(call);
        state.hang
    }

    /// Every RPC method name this source was asked for, in order.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.lock().calls.clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().expect("fake pool source lock poisoned")
    }
}

#[async_trait]
impl PoolSource for FakePoolSource {
    async fn tx_pool_info(&self) -> Result<TxPoolInfo, String> {
        if self.record("tx_pool_info".to_string()) {
            return std::future::pending().await;
        }
        match self.lock().info.clone() {
            Some(result) => result,
            None => Err("fake pool source has no tx_pool_info scripted".to_string()),
        }
    }

    async fn raw_tx_pool_verbose(&self) -> Result<RawTxPool, String> {
        if self.record("get_raw_tx_pool".to_string()) {
            return std::future::pending().await;
        }
        Ok(self.lock().raw_pool.clone())
    }

    async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<PoolTxLookup>, String> {
        if self.record(format!("get_transaction:0x{}", hex::encode(tx_hash))) {
            return std::future::pending().await;
        }
        match self.lock().transactions.get(tx_hash) {
            Some(result) => result.clone(),
            None => Ok(None),
        }
    }

    async fn get_header(&self, block_hash: &[u8; 32]) -> Result<Option<NodeHeader>, String> {
        if self.record(format!("get_header:0x{}", hex::encode(block_hash))) {
            return std::future::pending().await;
        }
        Ok(self
            .lock()
            .headers
            .iter()
            .find(|header| header.hash == *block_hash)
            .copied())
    }

    async fn get_header_by_number(&self, number: u64) -> Result<Option<NodeHeader>, String> {
        if self.record(format!("get_header_by_number:{number}")) {
            return std::future::pending().await;
        }
        Ok(self
            .lock()
            .headers
            .iter()
            .find(|header| header.number == number)
            .copied())
    }
}
