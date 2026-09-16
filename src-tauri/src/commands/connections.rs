use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::commands::{emit_settings_changed, persist_blocking};
use crate::config::settings::{AppSettings, ConnectionConfig, Theme};
use crate::connections::client::{probe_connection, sanitize_url_for_log, ProbeOutcome};
use crate::connections::{ConnectionError, ConnectionErrorKind, ConnectionManager};
use crate::events::{AppEvent, ConnectionStatus};
use crate::state::{AppState, ConnectionState};
use tauri::State;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionRuntimeSnapshotDto {
    pub id: String,
    pub status: String,
    pub last_message: Option<String>,
    /// Terminal failure detail from the last `Error` status (roadmap 011).
    pub error_kind: Option<ConnectionErrorKind>,
    pub error_message: Option<String>,
    /// Retry progress from the last `Retrying` status: the upcoming attempt
    /// number (1-based), the cycle's attempt budget, and the wait before the
    /// attempt, in seconds.
    pub attempt: Option<u32>,
    pub max_attempts: Option<u32>,
    pub next_retry_in_secs: Option<u64>,
    pub is_typing: bool,
    pub preview_text: Option<String>,
}

/// Map a runtime status to the DTO `status` string.
///
/// `Error` collapses to the bare status name: the structured failure detail
/// travels in the dedicated DTO fields, not in this string.
fn status_fields(status: Option<&ConnectionStatus>) -> String {
    match status {
        Some(ConnectionStatus::Error { .. }) => "Error".to_string(),
        Some(value) => value.to_string(),
        None => "Disconnected".to_string(),
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
    ConnectionRuntimeSnapshotDto {
        id: config_id.to_string(),
        status: status_fields(runtime.map(|value| &value.status)),
        last_message: runtime.and_then(|value| value.last_message.clone()),
        error_kind: runtime.and_then(|value| value.error_kind),
        error_message: runtime.and_then(|value| value.error_message.clone()),
        attempt: runtime.and_then(|value| value.attempt),
        max_attempts: runtime.and_then(|value| value.max_attempts),
        next_retry_in_secs: runtime.and_then(|value| value.next_retry_in_secs),
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
///
/// The old task is stopped (token cancelled, so it can emit no further
/// status) after the config is persisted, then: `enabled = false` leaves the
/// connection stopped — `Disconnected` is emitted only if a task was actually
/// running, preserving the last status of an already-stopped connection;
/// `enabled = true` restarts the connection with the new configuration, so
/// URL/token changes apply without a manual reconnect (the spawned retry loop
/// emits `Connecting` itself, after the cancellation).
#[tauri::command]
pub async fn update_connection(
    id: String,
    config: ConnectionConfig,
    app_state: State<'_, AppState>,
    manager: State<'_, ConnectionManager>,
) -> Result<(), String> {
    let enabled = config.enabled;
    // The persisted value IS this config (`update_connection` replaces the
    // entry wholesale), so it is also the restart configuration.
    let restart_config = config.clone();
    let lookup_id = id.clone();
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.update_connection(&lookup_id, config)
    })
    .await?;

    // Cancellation first, then the target status (roadmap 011 task 004):
    // the cancelled task can no longer race the event emitted below.
    let was_running = manager.stop_connection(&id);

    if enabled {
        manager
            .start_connection(restart_config)
            .await
            .map_err(|e| e.to_string())?;
    } else if was_running {
        app_state.emit_event(AppEvent::ConnectionStatusChanged(
            id,
            ConnectionStatus::Disconnected,
        ));
    }

    app_state.emit_event(AppEvent::ConnectionsChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Connect a connection by ID.
///
/// A disabled connection is a no-op: no `Connecting` event and no spawned
/// task (the old flow emitted `Connecting` before `start_connection` skipped
/// disabled configs, leaving a stale "connecting" state forever). For an
/// enabled connection, the connection is looked up in settings and its SSE
/// receive loop spawned via `ConnectionManager`; the spawned retry loop emits
/// `Connecting` as its first status, and a previously-running task for the
/// same id is cancelled and aborted first, so this is also the reconnect path.
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

    if !config.enabled {
        return Ok(());
    }

    manager
        .start_connection(config)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Disconnect a connection by ID.
///
/// Cancels the tracked SSE receive task's token (the task stops without
/// emitting any further status event, aborted as a backstop) and then emits
/// `Disconnected` so the UI reflects the stopped state immediately.
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

/* ==========================================================================
Pre-save endpoint check (roadmap 011, task 005)
========================================================================== */

/// The response of the pre-save endpoint check (`test_connection`).
///
/// Optional fields are omitted on the wire: a success carries only `ok` +
/// `latency_ms`, a failure only `ok` + the snake_case category and the fixed
/// English message. No field can carry the token: messages are fixed text,
/// and the category is a bare kind name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConnectionTestResultDto {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ConnectionErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

impl ConnectionTestResultDto {
    fn connected(latency_ms: u64) -> Self {
        Self {
            ok: true,
            error_kind: None,
            error_message: None,
            latency_ms: Some(latency_ms),
        }
    }

    fn failed(error: ConnectionError) -> Self {
        Self {
            ok: false,
            error_kind: Some(error.kind),
            error_message: Some(error.message),
            latency_ms: None,
        }
    }
}

/// Map a probe outcome to the wire DTO.
fn connection_test_result(outcome: ProbeOutcome) -> ConnectionTestResultDto {
    match outcome {
        ProbeOutcome::Connected { latency_ms } => ConnectionTestResultDto::connected(latency_ms),
        ProbeOutcome::Failed(error) => ConnectionTestResultDto::failed(error),
    }
}

/// Probe an SSE endpoint ONCE with the CURRENT dialog values — before
/// anything is saved (roadmap 011, task 005).
///
/// The arguments are the whole input and the return value the whole effect:
/// the command takes no settings state, reads and writes no settings, emits
/// no events, and spawns no retry loop. The endpoint is resolved by the same
/// function as the live path (the token rides the Cookie channel only), the
/// attempt is cut off by a short timeout, and every failure is classified
/// with the shared taxonomy (snake_case `error_kind` on the wire).
#[tauri::command]
pub async fn test_connection(
    url: String,
    access_token: Option<String>,
) -> Result<ConnectionTestResultDto, String> {
    let outcome = probe_connection(&url, access_token.as_deref()).await;
    Ok(connection_test_result(outcome))
}

/* ==========================================================================
Diagnostics export (roadmap 011, task 008, decision F)
========================================================================== */

/// The allowlisted diagnostics export written to the diagnostics file.
///
/// The structure IS the allowlist (decision F): only the fields declared
/// below can ever reach the file. Deliberately absent, unconditionally:
/// the access token / cookie, the connection name and any other user text
/// (`last_message`, `preview_text`), the raw URL with its query, and the
/// contents of the log file.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExportDto {
    /// Application version from the Tauri package configuration.
    pub app_version: String,
    /// Operating system (`std::env::consts::OS`).
    pub os: &'static str,
    /// CPU architecture (`std::env::consts::ARCH`).
    pub arch: &'static str,
    /// Export moment, Unix timestamp in milliseconds.
    pub exported_at_unix_ms: u64,
    pub connections: Vec<DiagnosticsConnectionDto>,
    pub settings: DiagnosticsSettingsSummaryDto,
}

/// One connection in the diagnostics export.
///
/// The `url` is the sanitized endpoint (`scheme://host:port/path`) — the
/// query string and fragment are dropped always, the same redaction the log
/// file gets (roadmap 011, task 007). The runtime detail mirrors the
/// snapshot DTO fields: machine category, fixed message, retry progress.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsConnectionDto {
    pub id: String,
    pub url: String,
    pub enabled: bool,
    pub status: String,
    pub error_kind: Option<ConnectionErrorKind>,
    pub error_message: Option<String>,
    pub attempt: Option<u32>,
    pub max_attempts: Option<u32>,
    pub next_retry_in_secs: Option<u64>,
    pub is_typing: bool,
}

/// The non-secret settings summary in the diagnostics export.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSettingsSummaryDto {
    pub theme: String,
    pub logging: DiagnosticsLoggingSummaryDto,
    pub message_clear_interval_seconds: u32,
}

/// The logging part of the settings summary.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsLoggingSummaryDto {
    pub enabled: bool,
    pub level: String,
}

/// The wire spelling of [`Theme`] (the serde `lowercase` form of the config
/// enum), spelled out here so the export does not depend on config serde.
fn theme_name(theme: &Theme) -> String {
    match theme {
        Theme::Dark => "dark".to_string(),
        Theme::Light => "light".to_string(),
    }
}

/// Build the diagnostics entry of one connection from its config and runtime
/// state.
///
/// Reads exactly three config fields (`id`, `url`, `enabled`); the URL passes
/// through [`sanitize_url_for_log`], and the name and token fields are never
/// touched, so no user text can enter the entry.
fn diagnostics_connection(
    config: &ConnectionConfig,
    runtime: Option<&ConnectionState>,
) -> DiagnosticsConnectionDto {
    DiagnosticsConnectionDto {
        id: config.id.clone(),
        url: sanitize_url_for_log(&config.url),
        enabled: config.enabled,
        status: status_fields(runtime.map(|value| &value.status)),
        error_kind: runtime.and_then(|value| value.error_kind),
        error_message: runtime.and_then(|value| value.error_message.clone()),
        attempt: runtime.and_then(|value| value.attempt),
        max_attempts: runtime.and_then(|value| value.max_attempts),
        next_retry_in_secs: runtime.and_then(|value| value.next_retry_in_secs),
        is_typing: runtime.is_some_and(|value| value.is_typing),
    }
}

/// Build the allowlisted diagnostics export (pure: data in, structure out —
/// no clock, no state, no filesystem), so the allowlist itself is testable
/// without a Tauri `State`.
pub(crate) fn build_diagnostics_export(
    app_version: &str,
    exported_at_unix_ms: u64,
    settings: &AppSettings,
    runtime: &HashMap<String, ConnectionState>,
) -> DiagnosticsExportDto {
    DiagnosticsExportDto {
        app_version: app_version.to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        exported_at_unix_ms,
        connections: settings
            .connections
            .iter()
            .map(|config| diagnostics_connection(config, runtime.get(&config.id)))
            .collect(),
        settings: DiagnosticsSettingsSummaryDto {
            theme: theme_name(&settings.theme),
            logging: DiagnosticsLoggingSummaryDto {
                enabled: settings.logging.enabled,
                level: settings.logging.level.clone(),
            },
            message_clear_interval_seconds: settings.general.message_clear_interval_seconds,
        },
    }
}

/// File name of a diagnostics export: `diagnostics-<unix-millis>.json`.
fn diagnostics_file_name(unix_ms: u64) -> String {
    format!("diagnostics-{unix_ms}.json")
}

/// Serialize the export and write it atomically into `dir` via
/// `config::atomic::write_atomic` (the only allowed touch of `config/**` —
/// a call, no edits), returning the written path.
fn write_diagnostics_file(
    dir: &Path,
    export: &DiagnosticsExportDto,
    unix_ms: u64,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create the diagnostics directory: {e}"))?;
    let json = serde_json::to_string_pretty(export)
        .map_err(|e| format!("Failed to serialize the diagnostics export: {e}"))?;
    let path = dir.join(diagnostics_file_name(unix_ms));
    crate::config::atomic::write_atomic(&path, &json).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Current Unix timestamp in milliseconds.
fn unix_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Export the allowlisted diagnostics file and return its path.
///
/// Runs only on the user's explicit action (the panel button) — the command
/// is never invoked by the backend itself. The file lands in
/// `%APPDATA%/ttsbard-echo/diagnostics/diagnostics-<unix-millis>.json`,
/// written atomically through [`crate::config::atomic::write_atomic`]; its
/// content is exactly [`build_diagnostics_export`]'s allowlist.
#[tauri::command]
pub fn export_diagnostics(app_state: State<'_, AppState>) -> Result<String, String> {
    let exported_at = unix_timestamp_millis();
    let settings = app_state.settings_manager.read().load();
    // Clone the small runtime map instead of holding the lock across the
    // filesystem write.
    let runtime: HashMap<String, ConnectionState> = app_state.connections.read().clone();
    let app_version = app_state
        .app_handle
        .config()
        .version
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let export = build_diagnostics_export(&app_version, exported_at, &settings, &runtime);
    let dir = dirs::config_dir()
        .ok_or_else(|| "Failed to get config dir".to_string())?
        .join("ttsbard-echo")
        .join("diagnostics");
    let path = write_diagnostics_file(&dir, &export, exported_at)?;
    Ok(path.to_string_lossy().into_owned())
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
        // The `Error` status collapses to the bare name; the kind and message
        // travel in dedicated fields, and no credential value is synthesized
        // into any status string.
        assert_eq!(
            status_fields(Some(&ConnectionStatus::Error {
                kind: ConnectionErrorKind::Network,
                message: "boom".to_string(),
            })),
            "Error"
        );

        for variant in [
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
            ConnectionStatus::Disconnected,
        ] {
            assert_eq!(status_fields(Some(&variant)), variant.to_string());
        }

        assert_eq!(
            status_fields(Some(&ConnectionStatus::Retrying {
                attempt: 3,
                max_attempts: 10,
                next_retry_in_secs: 5,
            })),
            "Retrying"
        );

        assert_eq!(status_fields(None), "Disconnected");
    }

    #[test]
    fn runtime_snapshot_carries_retry_and_error_detail_fields() {
        // A retrying runtime state exposes the retry progress.
        let retrying = ConnectionState::new(
            "conn-snap",
            ConnectionStatus::Retrying {
                attempt: 4,
                max_attempts: 10,
                next_retry_in_secs: 5,
            },
        );
        let dto = snapshot_for("conn-snap", Some(&retrying));
        assert_eq!(dto.status, "Retrying");
        assert_eq!(dto.attempt, Some(4));
        assert_eq!(dto.max_attempts, Some(10));
        assert_eq!(dto.next_retry_in_secs, Some(5));
        assert_eq!(dto.error_kind, None);
        assert!(dto.error_message.is_none());

        // A failed runtime state exposes the classified failure detail.
        let failed = ConnectionState::new(
            "conn-snap",
            ConnectionStatus::Error {
                kind: ConnectionErrorKind::Authentication,
                message: "The server rejected the credentials".to_string(),
            },
        );
        let dto = snapshot_for("conn-snap", Some(&failed));
        assert_eq!(dto.status, "Error");
        assert_eq!(dto.error_kind, Some(ConnectionErrorKind::Authentication));
        assert_eq!(
            dto.error_message.as_deref(),
            Some("The server rejected the credentials")
        );
        assert_eq!(dto.attempt, None);
        assert_eq!(dto.max_attempts, None);
        assert_eq!(dto.next_retry_in_secs, None);

        // The wire form serializes the kind as the snake_case category.
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"error_kind\":\"authentication\""));
        assert!(json.contains("\"attempt\":null"));
    }

    #[test]
    fn runtime_snapshot_without_runtime_state_has_null_detail_fields() {
        let dto = snapshot_for("conn-snap", None);
        assert_eq!(dto.status, "Disconnected");
        assert_eq!(dto.error_kind, None);
        assert!(dto.error_message.is_none());
        assert_eq!(dto.attempt, None);
        assert_eq!(dto.max_attempts, None);
        assert_eq!(dto.next_retry_in_secs, None);
    }

    /* ------------------------------------------------------------------
    Pre-save endpoint check (roadmap 011, task 005)
    ------------------------------------------------------------------ */

    /// Sentinel token value: its reproduction in the `test_connection`
    /// result (JSON, Debug) is a leak.
    const PROBE_TOKEN_SENTINEL: &str = "secret-sentinel-005-probe";

    #[test]
    fn test_result_success_contract_omits_error_fields() {
        let dto = connection_test_result(ProbeOutcome::Connected { latency_ms: 123 });
        assert_eq!(
            serde_json::to_string(&dto).unwrap(),
            r#"{"ok":true,"latency_ms":123}"#
        );
    }

    #[test]
    fn test_result_failure_contract_carries_snake_case_kind_and_fixed_message() {
        let dto = connection_test_result(ProbeOutcome::Failed(ConnectionError::for_kind(
            ConnectionErrorKind::Authentication,
        )));
        let json = serde_json::to_string(&dto).unwrap();
        assert_eq!(
            json,
            r#"{"ok":false,"error_kind":"authentication","error_message":"The server rejected the credentials"}"#
        );
        // The failure form carries no latency field at all.
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("latency_ms").is_none());
    }

    #[test]
    fn test_result_failure_dto_never_carries_request_details() {
        // The mapping only ever copies the category and the fixed message;
        // simulate a raw failure text that embeds the sentinel to prove the
        // raw text is dropped before the DTO.
        let error = ConnectionError::from_error_text(&format!(
            "http error: peer refused {PROBE_TOKEN_SENTINEL}"
        ));
        let dto = connection_test_result(ProbeOutcome::Failed(error));
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            !json.contains(PROBE_TOKEN_SENTINEL),
            "test result leaks the raw error text: {json}"
        );
        assert!(!format!("{dto:?}").contains(PROBE_TOKEN_SENTINEL));
    }

    /// The full command against an unparseable URL: no settings state is
    /// involved (the signature has none), no network is contacted, and the
    /// answer is the configuration category.
    #[tokio::test]
    async fn test_connection_rejects_invalid_url_offline_as_configuration() {
        let result = test_connection(
            "not a url".to_string(),
            Some(PROBE_TOKEN_SENTINEL.to_string()),
        )
        .await
        .unwrap();

        assert!(!result.ok);
        assert_eq!(result.error_kind, Some(ConnectionErrorKind::Configuration));
        assert_eq!(
            result.error_message.as_deref(),
            Some("Connection configuration is invalid")
        );
        assert_eq!(result.latency_ms, None);
    }

    /// The command result surface for a real (refused) loopback attempt with
    /// the sentinel in both the URL query and the token field: neither the
    /// JSON nor the Debug of the result ever reproduces the token, and no
    /// credential-bearing field exists on the contract at all.
    #[tokio::test]
    async fn test_connection_result_is_sentinel_free_for_token_carrying_probe() {
        let url = format!("http://127.0.0.1:9/?token={PROBE_TOKEN_SENTINEL}");
        let result = test_connection(url, Some(PROBE_TOKEN_SENTINEL.to_string()))
            .await
            .unwrap();

        assert!(!result.ok, "a refused loopback port cannot connect");
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains(PROBE_TOKEN_SENTINEL),
            "test_connection result leaks the token: {json}"
        );
        assert!(!format!("{result:?}").contains(PROBE_TOKEN_SENTINEL));

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("access_token").is_none());
        assert!(value.get("token").is_none());
        assert!(value.get("url").is_none());
    }

    /* ------------------------------------------------------------------
    Diagnostics export (roadmap 011, task 008, decision F)
    ------------------------------------------------------------------ */

    /// Sentinel secret: its reproduction in the export JSON or Debug is a
    /// leak.
    const EXPORT_TOKEN_SENTINEL: &str = "secret-sentinel-008-token";
    /// Sentinel user text: planted into name / last_message / preview_text —
    /// none of these may reach the export.
    const EXPORT_TEXT_SENTINEL: &str = "user-text-sentinel-008";

    fn export_settings() -> AppSettings {
        let mut settings = AppSettings::with_defaults();
        settings.theme = Theme::Light;
        settings.logging.enabled = false;
        settings.logging.level = "warn".to_string();
        settings.general.message_clear_interval_seconds = 42;
        settings.connections.push(ConnectionConfig {
            id: "conn-error".to_string(),
            name: format!("My {EXPORT_TEXT_SENTINEL}"),
            url: format!("http://127.0.0.1:10100/sse?token={EXPORT_TOKEN_SENTINEL}"),
            enabled: true,
            access_token: SecretAccessToken::new(Some(EXPORT_TOKEN_SENTINEL.to_string())),
        });
        settings.connections.push(ConnectionConfig {
            id: "conn-retry".to_string(),
            name: format!("Retry {EXPORT_TEXT_SENTINEL}"),
            url: format!("http://127.0.0.1:10101/sse?channel={EXPORT_TEXT_SENTINEL}"),
            enabled: true,
            access_token: SecretAccessToken::new(Some(EXPORT_TOKEN_SENTINEL.to_string())),
        });
        settings.connections.push(ConnectionConfig {
            id: "conn-offline".to_string(),
            name: format!("Offline {EXPORT_TEXT_SENTINEL}"),
            url: "http://127.0.0.1:10102/sse".to_string(),
            enabled: true,
            access_token: SecretAccessToken::none(),
        });
        settings.connections.push(ConnectionConfig {
            id: "conn-disabled".to_string(),
            name: format!("Disabled {EXPORT_TEXT_SENTINEL}"),
            url: "http://127.0.0.1:10103/sse".to_string(),
            enabled: false,
            access_token: SecretAccessToken::none(),
        });
        settings
    }

    fn export_runtime() -> HashMap<String, ConnectionState> {
        let mut runtime = HashMap::new();

        let mut failed = ConnectionState::new(
            "conn-error",
            ConnectionStatus::Error {
                kind: ConnectionErrorKind::Authentication,
                message: "The server rejected the credentials".to_string(),
            },
        );
        failed.last_message = Some(format!("last {EXPORT_TEXT_SENTINEL}"));
        failed.preview_text = Some(format!("preview {EXPORT_TEXT_SENTINEL}"));
        runtime.insert("conn-error".to_string(), failed);

        let mut retrying = ConnectionState::new(
            "conn-retry",
            ConnectionStatus::Retrying {
                attempt: 4,
                max_attempts: 10,
                next_retry_in_secs: 5,
            },
        );
        retrying.is_typing = true;
        retrying.preview_text = Some(format!("typing {EXPORT_TEXT_SENTINEL}"));
        runtime.insert("conn-retry".to_string(), retrying);

        runtime
    }

    fn export_json() -> (DiagnosticsExportDto, serde_json::Value) {
        let export = build_diagnostics_export(
            "1.2.3",
            1_726_300_000_000,
            &export_settings(),
            &export_runtime(),
        );
        let value = serde_json::to_value(&export).unwrap();
        (export, value)
    }

    #[test]
    fn diagnostics_export_carries_the_allowlisted_header_fields() {
        let (_, value) = export_json();

        assert_eq!(value["appVersion"], "1.2.3");
        assert_eq!(value["os"], std::env::consts::OS);
        assert_eq!(value["arch"], std::env::consts::ARCH);
        assert_eq!(value["exportedAtUnixMs"], 1_726_300_000_000u64);
        assert_eq!(value["connections"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn diagnostics_export_urls_are_sanitized_of_query_and_fragment() {
        let (_, value) = export_json();

        let connections = value["connections"].as_array().unwrap();
        assert_eq!(
            connections[0]["url"],
            serde_json::Value::String("http://127.0.0.1:10100/sse".to_string())
        );
        // The whole JSON must not carry the query payloads either...
        let json = serde_json::to_string(&value).unwrap();
        assert!(!json.contains("channel="), "query pair survived: {json}");

        // ...including for a URL that does not parse at all: the fixed
        // placeholder replaces it (no echo of arbitrary user input).
        let mut settings = export_settings();
        settings.connections[0].url = format!("totally not a url?token={EXPORT_TOKEN_SENTINEL}");
        let export = build_diagnostics_export("1.2.3", 1, &settings, &export_runtime());
        let json = serde_json::to_string(&export).unwrap();
        assert!(json.contains("\"url\":\"<invalid url>\""));
        assert!(!json.contains(EXPORT_TOKEN_SENTINEL), "leak: {json}");
    }

    #[test]
    fn diagnostics_export_carries_status_kind_and_retry_detail() {
        let (_, value) = export_json();
        let connections = value["connections"].as_array().unwrap();

        // The failed connection: bare status + machine category + fixed text.
        assert_eq!(connections[0]["id"], "conn-error");
        assert_eq!(connections[0]["enabled"], true);
        assert_eq!(connections[0]["status"], "Error");
        assert_eq!(connections[0]["errorKind"], "authentication");
        assert_eq!(
            connections[0]["errorMessage"],
            "The server rejected the credentials"
        );

        // The retrying connection: the retry progress and typing flag.
        assert_eq!(connections[1]["status"], "Retrying");
        assert_eq!(connections[1]["attempt"], 4);
        assert_eq!(connections[1]["maxAttempts"], 10);
        assert_eq!(connections[1]["nextRetryInSecs"], 5);
        assert_eq!(connections[1]["isTyping"], true);

        // No runtime entry reads as Disconnected with no detail.
        assert_eq!(connections[2]["status"], "Disconnected");
        assert_eq!(connections[2]["errorKind"], serde_json::Value::Null);
        assert_eq!(connections[2]["isTyping"], false);

        // The disabled flag travels, so support sees why nothing runs.
        assert_eq!(connections[3]["enabled"], false);
    }

    #[test]
    fn diagnostics_export_carries_the_non_secret_settings_summary() {
        let (_, value) = export_json();
        let settings = &value["settings"];

        assert_eq!(settings["theme"], "light");
        assert_eq!(settings["logging"]["enabled"], false);
        assert_eq!(settings["logging"]["level"], "warn");
        assert_eq!(settings["messageClearIntervalSeconds"], 42);
    }

    /// The decisive sentinel test: token, connection name, last message and
    /// typing preview must not appear anywhere in the JSON — and the
    /// forbidden fields must not exist as keys at all.
    #[test]
    fn diagnostics_export_is_strictly_sentinel_free() {
        let (export, value) = export_json();
        let json = serde_json::to_string(&value).unwrap();

        // The token never appears — neither its configured value nor any
        // token-carrying key (`access_token` / `accessToken` / `token`).
        assert!(
            !json.contains(EXPORT_TOKEN_SENTINEL),
            "diagnostics export leaks the token: {json}"
        );
        assert!(!json.contains("token"), "token-like key/value: {json}");

        // User text (name, last message, typing preview) never appears.
        assert!(
            !json.contains(EXPORT_TEXT_SENTINEL),
            "diagnostics export leaks user text: {json}"
        );
        for forbidden_key in [
            "\"name\"",
            "\"lastMessage\"",
            "\"last_message\"",
            "\"previewText\"",
            "\"preview_text\"",
            "\"preview\"",
        ] {
            assert!(
                !json.contains(forbidden_key),
                "forbidden key {forbidden_key} present: {json}"
            );
        }

        // The `Debug` of the export is clean too (a `{:?}` must not become a
        // leak path either).
        let debug = format!("{export:?}");
        assert!(!debug.contains(EXPORT_TOKEN_SENTINEL));
        assert!(!debug.contains(EXPORT_TEXT_SENTINEL));
    }

    #[test]
    fn diagnostics_writer_writes_timestamped_json_atomically() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ttsbard-echo-diagnostics-{}-{unique}",
            std::process::id()
        ));

        let (export, _) = export_json();
        let path = write_diagnostics_file(&dir, &export, 1_726_300_000_000).unwrap();

        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            "diagnostics-1726300000000.json"
        );
        let content = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["appVersion"], "1.2.3");
        assert!(!content.contains(EXPORT_TOKEN_SENTINEL));
        assert!(!content.contains(EXPORT_TEXT_SENTINEL));
        // No temporary artifacts next to the target.
        let leftover_temp = std::fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        });
        assert!(!leftover_temp);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The command's clock source: a monotonic-ish sanity check that the
    /// millisecond timestamp is a real Unix time (after 2020-01-01).
    #[test]
    fn unix_timestamp_millis_is_a_plausible_unix_time() {
        assert!(unix_timestamp_millis() > 1_577_836_800_000);
    }
}
