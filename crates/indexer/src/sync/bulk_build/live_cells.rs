use std::collections::HashMap;

#[derive(Debug, Default)]
pub(crate) struct LiveCellOwner {
    live: HashMap<OutPointKey, LiveCellSlot>,
}

impl LiveCellOwner {
    pub(crate) fn live_count(&self) -> usize {
        self.live.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct OutPointKey {
    tx_hash: [u8; 32],
    index: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LiveCellSlot;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_cell_owner_starts_empty() {
        let owner = LiveCellOwner::default();
        assert_eq!(owner.live_count(), 0);
    }
}
