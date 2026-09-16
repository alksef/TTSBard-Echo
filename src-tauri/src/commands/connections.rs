use crate::commands::{emit_settings_changed, persist_blocking};
use crate::config::ConnectionConfig;
use crate::connections::ConnectionManager;
use crate::events::{AppEvent, ConnectionStatus};
use crate::state::{AppState, ConnectionState};
use tauri::State;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionRuntimeSnapshotDto {
    pub id: String,
    pub status: String,
    pub last_message: Option<String>,
    pub error_message: Option<String>,
    pub is_typing: bool,
    pub preview_text: Option<String>,
}

/// Map a runtime status to the `(status, error_message)` DTO pair.
fn status_fields(status: Option<&ConnectionStatus>) -> (String, Option<String>) {
    match status {
        Some(ConnectionStatus::Error(message)) => ("Error".to_string(), Some(message.clone())),
        Some(value) => (value.to_string(), None),
        None => ("Disconnected".to_string(), None),
    }
}

/// Build the non-secret runtime snapshot for one connection.
///
/// Deliberately derived from the connection id and the runtime state only —
/// never from the other [`ConnectionConfig`] fields — so the settings-stored
/// token (whose single legitimate channel is the settings DTO, ADR-0024)
/// cannot reach this surface (roadmap 010, task 005).
fn snapshot_for(
    config_id: &str,
    runtime: Option<&ConnectionState>,
) -> ConnectionRuntimeSnapshotDto {
    let (status, error_message) = status_fields(runtime.map(|value| &value.status));

    ConnectionRuntimeSnapshotDto {
        id: config_id.to_string(),
        status,
        last_message: runtime.and_then(|value| value.last_message.clone()),
        error_message,
        is_typing: runtime.is_some_and(|value| value.is_typing),
        preview_text: runtime.and_then(|value| value.preview_text.clone()),
    }
}

/// Get all connections.
#[tauri::command]
pub fn get_connections(app_state: State<'_, AppState>) -> Result<Vec<ConnectionConfig>, String> {
    Ok(app_state.settings_manager.read().load().connections)
}

/// Read the non-secret runtime state for every persisted connection.
#[tauri::command]
pub fn get_connection_runtime_snapshot(
    app_state: State<'_, AppState>,
) -> Result<Vec<ConnectionRuntimeSnapshotDto>, String> {
    let configs = app_state.settings_manager.read().load().connections;
    let runtime = app_state.connections.read();

    Ok(configs
        .into_iter()
        .map(|config| snapshot_for(&config.id, runtime.get(&config.id)))
        .collect())
}

/// Add a new connection.
#[tauri::command]
pub async fn add_connection(
    config: ConnectionConfig,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let id = config.id.clone();
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.add_connection(config)
    })
    .await?;
    app_state.emit_event(AppEvent::ConnectionAdded(id));
    app_state.emit_event(AppEvent::ConnectionsChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Remove a connection by ID.
#[tauri::command]
pub async fn remove_connection(
    id: String,
    app_state: State<'_, AppState>,
    manager: State<'_, ConnectionManager>,
) -> Result<(), String> {
    manager.stop_connection(&id);
    app_state.emit_event(AppEvent::ConnectionStatusChanged(
        id.clone(),
        crate::events::ConnectionStatus::Disconnected,
    ));
    let removed_id = id.clone();
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.remove_connection(&id)
    })
    .await?;
    app_state.emit_event(AppEvent::ConnectionRemoved(removed_id));
    app_state.emit_event(AppEvent::ConnectionsChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Update a connection.
#[tauri::command]
pub async fn update_connection(
    id: String,
    config: ConnectionConfig,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.update_connection(&id, config)
    })
    .await?;
    app_state.emit_event(AppEvent::ConnectionsChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Connect a connection by ID.
///
/// Looks the connection up in settings, spawns its SSE receive loop via
/// `ConnectionManager` (tracking the task handle), and emits `Connecting`.
/// A previously-running task for the same id is aborted first, so this is
/// also the reconnect path.
#[tauri::command]
pub async fn connect_connection(
    id: String,
    app_state: State<'_, AppState>,
    manager: State<'_, ConnectionManager>,
) -> Result<(), String> {
    // Resolve the config (validated on add/update, but re-check here too).
    let config = app_state
        .settings_manager
        .read()
        .load()
        .connections
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("Connection not found: {}", id))?;

    app_state.emit_event(AppEvent::ConnectionStatusChanged(
        id.clone(),
        crate::events::ConnectionStatus::Connecting,
    ));

    manager
        .start_connection(config)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Disconnect a connection by ID.
///
/// Aborts the tracked SSE receive task and emits `Disconnected` so the UI
/// reflects the stopped state immediately.
#[tauri::command]
pub async fn disconnect_connection(
    id: String,
    app_state: State<'_, AppState>,
    manager: State<'_, ConnectionManager>,
) -> Result<(), String> {
    manager.stop_connection(&id);
    app_state.emit_event(AppEvent::ConnectionStatusChanged(
        id,
        crate::events::ConnectionStatus::Disconnected,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::SecretAccessToken;

    /// Sentinel token value of the connection under test: its reproduction
    /// anywhere in the runtime snapshot is a leak (roadmap 010, task 005).
    const TOKEN_SENTINEL: &str = "secret-sentinel-005";

    fn token_carrying_config() -> ConnectionConfig {
        ConnectionConfig {
            id: "conn-snap".to_string(),
            name: "Snapshot".to_string(),
            url: "http://127.0.0.1:10100".to_string(),
            enabled: true,
            access_token: SecretAccessToken::new(Some(TOKEN_SENTINEL.to_string())),
        }
    }

    #[test]
    fn runtime_snapshot_of_token_carrying_connection_is_sentinel_free() {
        let config = token_carrying_config();

        // No runtime state (the connection was never started)...
        let dto = snapshot_for(&config.id, None);
        let json = serde_json::to_string(&dto).unwrap();
        // Positive control: the snapshot is really derived from the config.
        assert!(json.contains("\"id\":\"conn-snap\""));
        assert!(
            !json.contains(TOKEN_SENTINEL),
            "runtime snapshot leaks the token: {json}"
        );

        // ...and the DTO shape has no token field at all.
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("access_token").is_none());
        assert!(value.get("token").is_none());

        // The serialized `Debug` of the DTO is clean too (a `{:?}` of the
        // snapshot must not become a leak path either).
        assert!(!format!("{dto:?}").contains(TOKEN_SENTINEL));
    }

    #[test]
    fn status_mapping_keeps_error_text_and_status_strings_only() {
        // Error messages are fixed backend texts; they pass through, and no
        // credential value is synthesized into any status string.
        let (status, error) = status_fields(Some(&ConnectionStatus::Error("boom".to_string())));
        assert_eq!(status, "Error");
        assert_eq!(error.as_deref(), Some("boom"));

        for variant in [
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
            ConnectionStatus::Disconnected,
        ] {
            let (status, error) = status_fields(Some(&variant));
            assert_eq!(error, None);
            assert_eq!(status, variant.to_string());
        }

        let (status, error) = status_fields(None);
        assert_eq!(status, "Disconnected");
        assert_eq!(error, None);
    }
}
