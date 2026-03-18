use anyhow::Result;

use super::facts::ResolvedTxFacts;
use super::interner::IdentityInterner;
use super::materialize::Materializer;
use crate::sync::types::InternId;

pub(crate) mod address;
pub(crate) mod dao;
pub(crate) mod fiber;
pub(crate) mod object;
pub(crate) mod script;
pub(crate) mod token;

pub(crate) trait BulkReducer {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()>;

    fn flush_sealed(&mut self, _materializer: &mut Materializer<'_>) -> Result<()> {
        Ok(())
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReducerContext<'a> {
    identities: &'a IdentityInterner,
}

impl<'a> ReducerContext<'a> {
    pub(crate) fn new(identities: &'a IdentityInterner) -> Self {
        Self { identities }
    }

    pub(crate) fn resolve_identity(self, id: InternId) -> &'a [u8] {
        self.identities.resolve_bytes(id)
    }
}
