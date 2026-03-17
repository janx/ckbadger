#![allow(dead_code)]

pub(crate) mod facts;
pub(crate) mod interner;
pub(crate) mod live_cells;
pub(crate) mod materialize;
pub(crate) mod owners;
pub(crate) mod sequencer;

#[derive(Default)]
pub(crate) struct BulkBuildEngine;
