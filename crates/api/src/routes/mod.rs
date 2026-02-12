mod activities;
pub mod assets;
mod blocks;
mod cells;
mod dao;
mod forks;
mod graph;
mod mempool;
mod scripts;
mod search;
mod spore;
mod statistics;
mod tasks;
mod tokens;
mod transactions;

use axum::Router;
use std::sync::Arc;

use crate::AppState;

pub fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .merge(activities::routes())
        .merge(assets::routes())
        .merge(blocks::routes())
        .merge(transactions::routes())
        .merge(cells::routes())
        .merge(statistics::routes())
        .merge(graph::routes())
        .merge(search::routes())
        .merge(tokens::routes())
        .merge(dao::routes())
        .merge(spore::routes())
        .merge(mempool::routes())
        .merge(scripts::routes())
        .merge(forks::routes())
        .merge(tasks::routes())
}
