use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Process-local cooperative shutdown signal.
///
/// The atomic flag is shared with blocking workers. The watch channel retains
/// the requested state and wakes async stages without polling or lost-wakeup
/// races.
#[derive(Debug, Clone)]
pub(crate) struct ShutdownSignal {
    requested: Arc<AtomicBool>,
    changed: tokio::sync::watch::Sender<bool>,
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        let (changed, _receiver) = tokio::sync::watch::channel(false);
        Self {
            requested: Arc::new(AtomicBool::new(false)),
            changed,
        }
    }
}

impl ShutdownSignal {
    pub(crate) fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
        self.changed.send_replace(true);
    }

    pub(crate) fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub(crate) fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.requested)
    }

    pub(crate) async fn cancelled(&self) {
        if self.is_requested() {
            return;
        }

        let mut receiver = self.changed.subscribe();
        while !*receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}
