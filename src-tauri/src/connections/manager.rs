use crate::config::ConnectionConfig;
use crate::connections::client::{CancelSource, SSEClient};
use crate::state::AppState;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// One tracked connection task: the owner side of its cooperative
/// cancellation token plus the spawned task handle (the abort backstop).
struct ConnectionTask {
    cancel: CancelSource,
    handle: JoinHandle<()>,
}

/// Id-keyed registry of running connection tasks (roadmap 011, decision C).
///
/// Stopping is synchronous and cooperative: the token is cancelled first (the
/// task then exits at its next await point and never emits another status
/// event), and `handle.abort()` remains the backstop for code that never
/// awaits the token. Kept free of `AppState` so the invariants are testable
/// without a Tauri app.
#[derive(Default)]
struct TaskRegistry {
    tasks: RwLock<HashMap<String, ConnectionTask>>,
}

impl TaskRegistry {
    /// Track a freshly spawned task for `id`. A replaced entry (same id) is
    /// stopped like any other: cancel, then abort.
    fn insert(&self, id: &str, cancel: CancelSource, handle: JoinHandle<()>) {
        let previous = self
            .tasks
            .write()
            .insert(id.to_string(), ConnectionTask { cancel, handle });
        if let Some(previous) = previous {
            previous.cancel.cancel();
            previous.handle.abort();
        }
    }

    /// Stop the task for `id`: cancel its token, then abort as the backstop.
    /// No-op if none is running. Returns `true` if a task was running.
    fn stop(&self, id: &str) -> bool {
        match self.tasks.write().remove(id) {
            Some(task) => {
                task.cancel.cancel();
                task.handle.abort();
                true
            }
            None => false,
        }
    }

    /// Stop every tracked task (application shutdown).
    fn stop_all(&self) {
        let stopped: Vec<ConnectionTask> =
            self.tasks.write().drain().map(|(_, task)| task).collect();
        for task in stopped {
            task.cancel.cancel();
            task.handle.abort();
        }
    }
}

/// Owns the spawned SSE receive tasks so they can be stopped cooperatively.
///
/// One entry per active connection id; replaced if the same id is connected
/// again (the previous task is cancelled and aborted first). Stopping cancels
/// the task's token — it exits without emitting further status events — and
/// aborts the handle as the backstop.
#[derive(Clone)]
pub struct ConnectionManager {
    state: AppState,
    tasks: Arc<TaskRegistry>,
}

impl ConnectionManager {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tasks: Arc::new(TaskRegistry::default()),
        }
    }

    /// Start (or restart) the SSE receive loop for one connection.
    ///
    /// Returns `Ok(())` if the connection was started or skipped (disabled),
    /// or an error if the config lookup failed. The spawned task is tracked
    /// together with its cancellation source so `stop_connection` / `stop_all`
    /// can stop it cooperatively.
    pub async fn start_connection(&self, config: ConnectionConfig) -> anyhow::Result<()> {
        if !config.enabled {
            return Ok(());
        }

        // Cancel + abort a previous task for the same id before spawning a
        // new one, so reconnect doesn't leak the old receiver.
        self.tasks.stop(&config.id);

        let cancel = CancelSource::new();
        let token = cancel.token();
        let client = SSEClient::new(
            config.id.clone(),
            config.url.clone(),
            config.access_token.clone().into_inner(),
            Arc::new(self.state.clone()),
        );
        let handle = client.connect(token);
        self.tasks.insert(&config.id, cancel, handle);

        Ok(())
    }

    /// Stop the receive loop for `id`: cancel its token (the task stops
    /// without emitting any further status event) and abort it as the
    /// backstop. No-op if none is running. Does not emit a status event —
    /// the caller decides what status the UI should show. Returns `true` if
    /// a task was running.
    pub fn stop_connection(&self, id: &str) -> bool {
        self.tasks.stop(id)
    }

    /// Stop every running connection (application shutdown): cancel all
    /// tokens, then abort all handles.
    pub fn stop_all(&self) {
        self.tasks.stop_all();
    }

    pub async fn start_all(&self) -> anyhow::Result<()> {
        // Clone connections to avoid holding the lock across await points.
        let connections = self
            .state
            .settings_manager
            .read()
            .load()
            .connections
            .clone();

        for config in connections {
            if config.enabled {
                let id = config.id.clone();
                if let Err(e) = self.start_connection(config).await {
                    // The correlation id of the failed connection belongs in
                    // the log line (roadmap 011, task 007); the error is the
                    // settings-lookup failure, not request data.
                    tracing::warn!("Failed to start connection {id}: {e}");
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TaskRegistry;
    use crate::connections::client::{retry_loop, CancelSource, ConnectResult};
    use crate::events::ConnectionStatus;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Track the production retry loop under `id` with a never-resolving
    /// attempt — the worst case a stop has to win against.
    fn track_hanging_connection(
        registry: &TaskRegistry,
        id: &'static str,
        events: &Arc<Mutex<Vec<ConnectionStatus>>>,
    ) {
        let source = CancelSource::new();
        let token = source.token();
        let emit = {
            let events = Arc::clone(events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };
        let handle = tokio::spawn(retry_loop(
            id,
            token,
            std::future::pending::<ConnectResult>,
            emit,
        ));
        registry.insert(id, source, handle);
    }

    /// Disable/delete/disconnect-style stop (the exact call
    /// `update_connection { enabled = false }` makes): the task finishes and
    /// stays silent forever, whatever the retry clock does afterwards.
    #[tokio::test]
    async fn stop_connection_finishes_task_and_emits_nothing_more() {
        tokio::time::pause();

        let registry = TaskRegistry::default();
        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        track_hanging_connection(&registry, "conn-1", &events);

        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        let events_before = events.lock().unwrap().len();
        assert_eq!(
            events_before, 1,
            "Connecting was emitted for the started task"
        );

        assert!(registry.stop("conn-1"), "the task was running");
        assert!(!registry.stop("conn-1"), "a second stop is a no-op");

        tokio::time::advance(Duration::from_secs(120)).await;
        tokio::task::yield_now().await;

        assert_eq!(
            events.lock().unwrap().len(),
            events_before,
            "no status event after the stop"
        );
    }

    /// Shutdown-style stop: `stop_all` silences every connection at once.
    #[tokio::test]
    async fn stop_all_silences_every_connection() {
        tokio::time::pause();

        let registry = TaskRegistry::default();
        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        track_hanging_connection(&registry, "conn-1", &events);
        track_hanging_connection(&registry, "conn-2", &events);

        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            events.lock().unwrap().len(),
            2,
            "each connection emitted its Connecting"
        );

        registry.stop_all();

        tokio::time::advance(Duration::from_secs(120)).await;
        tokio::task::yield_now().await;

        assert_eq!(
            events.lock().unwrap().len(),
            2,
            "no status event from any connection after stop_all"
        );
    }
}
