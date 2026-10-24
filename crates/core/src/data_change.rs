//! Coalesced "data changed" notification (issue #672).
//!
//! The persistence layer calls [`DataChangeNotifier::notify`] *after* a write
//! to the user data set has been committed. Changes within one
//! [`COALESCE_WINDOW`] are merged into a single notification that carries the
//! union of the affected [`DataKind`]s. The notification never carries entity
//! content, IDs or names: subscribers re-read what they care about.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

/// Changes closer together than this are merged into one notification.
pub const COALESCE_WINDOW: Duration = Duration::from_millis(500);

/// The kinds of user data a notification can refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataKind {
    Servers,
    Groups,
    Tags,
    Rules,
    Notes,
    AiProviders,
    Secrets,
    HostKeys,
}

/// A subscriber callback; receives the union of kinds of one window.
pub type DataChangeSubscriber = Arc<dyn Fn(&BTreeSet<DataKind>) + Send + Sync>;

#[derive(Default)]
struct Inner {
    pending: BTreeSet<DataKind>,
    /// A flush is scheduled for the open window.
    window_open: bool,
    subscribers: Vec<DataChangeSubscriber>,
}

/// Cheap to clone; all clones share state.
#[derive(Clone)]
pub struct DataChangeNotifier {
    inner: Arc<Mutex<Inner>>,
    window: Duration,
}

impl Default for DataChangeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl DataChangeNotifier {
    pub fn new() -> Self {
        Self::with_window(COALESCE_WINDOW)
    }

    pub fn with_window(window: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            window,
        }
    }

    /// Registers a subscriber. It is called on a background thread, once per
    /// coalesced window.
    pub fn subscribe(&self, subscriber: DataChangeSubscriber) {
        self.lock().subscribers.push(subscriber);
    }

    /// Records a committed change. Call only after the commit succeeded.
    pub fn notify(&self, kind: DataKind) {
        self.notify_many(&[kind]);
    }

    pub fn notify_many(&self, kinds: &[DataKind]) {
        let start_window = {
            let mut inner = self.lock();
            inner.pending.extend(kinds.iter().copied());
            if inner.subscribers.is_empty() {
                // Nobody listens: do not accumulate or spawn anything.
                inner.pending.clear();
                return;
            }
            !std::mem::replace(&mut inner.window_open, true)
        };
        if start_window {
            let this = self.clone();
            let spawned = std::thread::Builder::new()
                .name("data-change-flush".into())
                .spawn(move || {
                    std::thread::sleep(this.window);
                    this.flush();
                });
            if let Err(err) = spawned {
                // Cannot wait for the window: flush right away rather than
                // dropping the notification.
                tracing::warn!(error = %err, "data-change flush thread failed to start");
                self.flush();
            }
        }
    }

    fn flush(&self) {
        let (kinds, subscribers) = {
            let mut inner = self.lock();
            inner.window_open = false;
            (
                std::mem::take(&mut inner.pending),
                inner.subscribers.clone(),
            )
        };
        if kinds.is_empty() {
            return;
        }
        for subscriber in subscribers {
            subscriber(&kinds);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn collecting(window_ms: u64) -> (DataChangeNotifier, mpsc::Receiver<BTreeSet<DataKind>>) {
        let n = DataChangeNotifier::with_window(Duration::from_millis(window_ms));
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        n.subscribe(Arc::new(move |k| {
            tx.lock().unwrap().send(k.clone()).unwrap();
        }));
        (n, rx)
    }

    #[test]
    fn many_notifications_in_one_window_yield_one_union() {
        let (n, rx) = collecting(100);
        for i in 0..10 {
            n.notify(if i % 2 == 0 {
                DataKind::Servers
            } else {
                DataKind::Rules
            });
        }
        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, BTreeSet::from([DataKind::Servers, DataKind::Rules]));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn a_later_change_opens_a_new_window() {
        let (n, rx) = collecting(50);
        n.notify(DataKind::Groups);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        n.notify(DataKind::Notes);
        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, BTreeSet::from([DataKind::Notes]));
    }

    #[test]
    fn payload_serializes_to_kind_names_only() {
        let kinds = BTreeSet::from([DataKind::AiProviders, DataKind::HostKeys]);
        assert_eq!(
            serde_json::to_value(&kinds).unwrap(),
            serde_json::json!(["ai_providers", "host_keys"])
        );
    }

    #[test]
    fn without_subscribers_nothing_is_kept() {
        let n = DataChangeNotifier::with_window(Duration::from_millis(10));
        n.notify(DataKind::Servers);
        assert!(n.lock().pending.is_empty());
    }
}
