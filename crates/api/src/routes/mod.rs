pub(crate) mod activities;
mod address_holdings;
pub(crate) mod address_pool;
pub(crate) mod address_presence;
mod asset_calendar;
mod asset_feed;
mod asset_genetics;
mod asset_history;
mod asset_protocol;
mod asset_read_feed;
mod asset_summary;
pub mod assets;
mod blocks;
pub(crate) mod cells;
mod collection_context;
mod dao;
mod fiber;
mod forks;
mod graph;
pub(crate) mod hardforks;
mod identities;
mod mempool;
mod network;
mod pipeline;
mod proposal_window;
mod scripts;
mod search;
mod spore;
pub(crate) mod statistics;
mod token_history;
mod tokens;
mod transactions;
pub(crate) mod tx_lookup;
/// Public so the verifier's own wire types can be kept in step with the
/// endpoint's, and so the export's shape is part of the crate's API surface.
pub mod verify;
mod vessels;

use axum::Router;
use std::sync::Arc;

use crate::AppState;

/// Public so tests can hold `/pipeline` to its own bound.
pub use forks::ForkView;
pub use pipeline::{PipelinePaysCache, PIPELINE_TIP_WAIT};

pub fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .merge(activities::routes())
        .merge(assets::routes())
        .merge(asset_history::routes())
        .merge(asset_read_feed::routes())
        .merge(asset_genetics::routes())
        .merge(identities::routes())
        .merge(blocks::routes())
        .merge(transactions::routes())
        .merge(cells::routes())
        .merge(address_holdings::routes())
        .merge(statistics::routes())
        .merge(graph::routes())
        .merge(hardforks::routes())
        .merge(search::routes())
        .merge(tokens::routes())
        .merge(dao::routes())
        .merge(fiber::routes())
        .merge(spore::routes())
        .merge(mempool::routes())
        .merge(network::routes())
        .merge(pipeline::routes())
        .merge(scripts::routes())
        .merge(forks::routes())
        .merge(verify::routes())
        .merge(vessels::routes())
}

mod pool_transactions;

mod transaction_structure;

mod transaction_cells;
