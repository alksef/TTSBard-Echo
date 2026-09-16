use crate::config::ConnectionConfig;
use crate::connections::client::SSEClient;
use crate::state::AppState;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Owns the spawned SSE receive tasks so they can be aborted on disconnect.
///
/// One entry per active connection id; replaced if the same id is connected
/// again (the previous task is aborted first).
#[derive(Clone)]
pub struct ConnectionManager {
    state: AppState,
    handles: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
}

impl ConnectionManager {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start (or restart) the SSE receive loop for one connection.
    ///
    /// Returns `Ok(())` if the connection was started or skipped (disabled),
    /// or an error if the config lookup failed. The spawned task handle is
    /// tracked so `stop_connection` can abort it.
    pub async fn start_connection(&self, config: ConnectionConfig) -> anyhow::Result<()> {
        if !config.enabled {
            return Ok(());
        }

        // Abort a previous task for the same id before spawning a new one,
        // so reconnect doesn't leak the old receiver.
        self.stop_connection(&config.id);

        let client = SSEClient::new(
            config.id.clone(),
            config.url.clone(),
            config.access_token.clone().into_inner(),
            Arc::new(self.state.clone()),
        );
        let handle = client.connect();
        self.handles.write().insert(config.id, handle);

        Ok(())
    }

    /// Abort the receive loop for `id` (no-op if none is running). Does not emit
    /// a status event — the caller decides what status the UI should show.
    pub fn stop_connection(&self, id: &str) {
        if let Some(handle) = self.handles.write().remove(id) {
            handle.abort();
        }
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
                if let Err(e) = self.start_connection(config).await {
                    tracing::warn!("Failed to start connection: {}", e);
                }
            }
        }

        Ok(())
    }
}
