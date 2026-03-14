pub(crate) mod activities;
pub mod assets;
mod blocks;
mod cells;
mod dao;
mod fiber;
mod forks;
mod graph;
pub(crate) mod hardforks;
mod identities;
mod mempool;
mod scripts;
mod search;
mod spore;
pub(crate) mod statistics;
mod tokens;
mod transactions;
mod tx_lookup;

use axum::Router;
use std::sync::Arc;

use crate::AppState;

pub fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .merge(activities::routes())
        .merge(assets::routes())
        .merge(identities::routes())
        .merge(blocks::routes())
        .merge(transactions::routes())
        .merge(cells::routes())
        .merge(statistics::routes())
        .merge(graph::routes())
        .merge(hardforks::routes())
        .merge(search::routes())
        .merge(tokens::routes())
        .merge(dao::routes())
        .merge(fiber::routes())
        .merge(spore::routes())
        .merge(mempool::routes())
        .merge(scripts::routes())
        .merge(forks::routes())
}
