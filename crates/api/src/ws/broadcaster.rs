use std::sync::Arc;

use crate::cache::CacheBackend;
use crate::db::DbPool;
use crate::ws::WsManager;

pub async fn start_block_broadcaster(
    _pool: DbPool,
    _ws_manager: Arc<WsManager>,
    _ckb_rpc_url: String,
    _cache: CacheBackend,
) {
}

pub async fn start_reorg_broadcaster(_pool: DbPool, _ws_manager: Arc<WsManager>) {}
