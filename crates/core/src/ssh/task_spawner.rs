//! Issue #881: a small hand-over so transports below `app-logic` can start
//! background tasks (e.g. the SFTP tap pumps) into a scope owned by the
//! application, without depending on the application's task supervisor.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A boxed background task.
pub type BoxedTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

type SpawnFn = dyn Fn(&'static str, BoxedTask) -> bool + Send + Sync;

/// Starts named background tasks. The owner decides where they run and when
/// they are cancelled; the holder only hands the future over.
#[derive(Clone)]
pub struct TaskSpawner(Arc<SpawnFn>);

impl TaskSpawner {
    /// `spawn` returns `false` when the task was refused (scope closed); the
    /// future is then dropped without running.
    pub fn new(spawn: impl Fn(&'static str, BoxedTask) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(spawn))
    }

    /// Hands `fut` over. Returns `false` when it was refused.
    pub fn spawn<F>(&self, name: &'static str, fut: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        (self.0)(name, Box::pin(fut))
    }
}

impl std::fmt::Debug for TaskSpawner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TaskSpawner")
    }
}
