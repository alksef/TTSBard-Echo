use crate::config::{SettingsManager, WindowsManager};
use crate::connections::ConnectionErrorKind;
use crate::events::{AppEvent, ConnectionStatus};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::mpsc::Sender;
use tracing::warn;

/// Per-connection typing timeout in seconds.
///
/// If no typing update, final message, or status change arrives within this
/// window after the last `isTyping: true` indicator, the backend clears the
/// typing state and emits `typing-changed false` to prevent a stale indicator.
const TYPING_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub id: String,
    pub status: ConnectionStatus,
    pub last_message: Option<String>,
    last_message_generation: u64,
    pub is_typing: bool,
    pub preview_text: Option<String>,
    typing_generation: u64,
    /// Terminal failure detail from the last `Error` status (roadmap 011):
    /// machine category plus the fixed message. Reset by every other status.
    pub error_kind: Option<ConnectionErrorKind>,
    pub error_message: Option<String>,
    /// Retry progress from the last `Retrying` status: the number of the
    /// upcoming attempt (1-based), the attempt budget of the cycle, and the
    /// wait before that attempt. Reset by every other status.
    pub attempt: Option<u32>,
    pub max_attempts: Option<u32>,
    pub next_retry_in_secs: Option<u64>,
}

impl ConnectionState {
    /// A fresh state with no runtime detail (no message, no typing) whose
    /// retry/error fields are derived from `status`.
    pub fn new(id: impl Into<String>, status: ConnectionStatus) -> Self {
        let mut state = Self {
            id: id.into(),
            status: ConnectionStatus::Disconnected,
            last_message: None,
            last_message_generation: 0,
            is_typing: false,
            preview_text: None,
            typing_generation: 0,
            error_kind: None,
            error_message: None,
            attempt: None,
            max_attempts: None,
            next_retry_in_secs: None,
        };
        state.apply_status(&status);
        state
    }

    /// Sets `status` and recomputes the retry/error detail fields the status
    /// carries: every status first resets the detail to `None`, then `Retrying`
    /// fills the attempt counters and `Error` fills the failure category and
    /// message (roadmap 011, task 002).
    fn apply_status(&mut self, status: &ConnectionStatus) {
        self.status = status.clone();
        self.error_kind = None;
        self.error_message = None;
        self.attempt = None;
        self.max_attempts = None;
        self.next_retry_in_secs = None;
        match status {
            ConnectionStatus::Retrying {
                attempt,
                max_attempts,
                next_retry_in_secs,
            } => {
                self.attempt = Some(*attempt);
                self.max_attempts = Some(*max_attempts);
                self.next_retry_in_secs = Some(*next_retry_in_secs);
            }
            ConnectionStatus::Error { kind, message } => {
                self.error_kind = Some(*kind);
                self.error_message = Some(message.clone());
            }
            ConnectionStatus::Disconnected
            | ConnectionStatus::Connecting
            | ConnectionStatus::Connected => {}
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub event_sender: Arc<Mutex<Option<Sender<AppEvent>>>>,
    pub connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
    pub settings_manager: Arc<RwLock<SettingsManager>>,
    pub windows_manager: Arc<RwLock<WindowsManager>>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub backend_ready: Arc<AtomicBool>,
    /// Cloned `AppHandle` so non-command code (event channel, managers) can emit
    /// Tauri events without receiving it as a parameter. `AppHandle` is cheap to clone.
    pub app_handle: AppHandle,
}

impl AppState {
    pub fn new(app_handle: AppHandle) -> anyhow::Result<Self> {
        Ok(Self {
            event_sender: Arc::new(Mutex::new(None)),
            connections: Arc::new(RwLock::new(HashMap::new())),
            settings_manager: Arc::new(RwLock::new(SettingsManager::new()?)),
            windows_manager: Arc::new(RwLock::new(WindowsManager::new()?)),
            runtime: Arc::new(tokio::runtime::Runtime::new()?),
            backend_ready: Arc::new(AtomicBool::new(false)),
            app_handle,
        })
    }

    pub fn emit_event(&self, event: AppEvent) {
        let emitted = {
            let mut connections = self.connections.write();
            process_connection_runtime(&mut connections, &event)
        };

        self.schedule_message_clear(&event);
        self.schedule_typing_timeout(&event);

        if let Some(sender) = self.event_sender.lock().as_ref() {
            for event in emitted {
                match sender.try_send(event) {
                    Ok(()) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
                        warn!(
                            event = event_log_name(&event),
                            "Event channel is full, dropping event"
                        );
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(event)) => {
                        warn!(
                            event = event_log_name(&event),
                            "Event channel is closed, dropping event"
                        );
                    }
                }
            }
        }
    }

    fn schedule_message_clear(&self, event: &AppEvent) {
        let AppEvent::MessageReceived(id, _) = event else {
            return;
        };
        let Some(generation) = self
            .connections
            .read()
            .get(id)
            .map(|connection| connection.last_message_generation)
        else {
            return;
        };
        let seconds = self
            .settings_manager
            .read()
            .load()
            .general
            .message_clear_interval_seconds;
        let state = self.clone();
        let id = id.clone();
        self.runtime.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(u64::from(seconds))).await;
            let should_clear = {
                let mut connections = state.connections.write();
                let Some(connection) = connections.get_mut(&id) else {
                    return;
                };
                if connection.last_message_generation != generation {
                    false
                } else {
                    connection.last_message = None;
                    true
                }
            };
            if should_clear {
                state.emit_event(AppEvent::MessageCleared(id));
            }
        });
    }

    fn schedule_typing_timeout(&self, event: &AppEvent) {
        let AppEvent::TypingChanged(id, is_typing, _) = event else {
            return;
        };
        if !*is_typing {
            return;
        }
        let Some(generation) = self
            .connections
            .read()
            .get(id)
            .map(|connection| connection.typing_generation)
        else {
            return;
        };
        let state = self.clone();
        let id = id.clone();
        self.runtime.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS)).await;
            let should_clear = {
                let mut connections = state.connections.write();
                let Some(connection) = connections.get_mut(&id) else {
                    return;
                };
                if connection.typing_generation != generation {
                    false
                } else {
                    connection.is_typing = false;
                    connection.preview_text = None;
                    true
                }
            };
            if should_clear {
                state.emit_event(AppEvent::TypingChanged(id, false, None));
            }
        });
    }

    pub fn set_event_sender(&self, sender: Sender<AppEvent>) {
        *self.event_sender.lock() = Some(sender);
    }

    pub fn set_backend_ready(&self) {
        self.backend_ready.store(true, Ordering::SeqCst);
    }

    pub fn is_backend_ready(&self) -> bool {
        self.backend_ready.load(Ordering::SeqCst)
    }
}

/// Content-free event identifier for diagnostics. `AppEvent` variants may
/// carry user messages or typing previews, so their `Debug` output is never a
/// safe logging surface.
fn event_log_name(event: &AppEvent) -> &'static str {
    event.to_tauri_event()
}

/* ==========================================================================
Pure runtime helpers.
These are shared between `AppState::emit_event` and the regression tests so
the tests exercise exactly the same event-derivation logic as production.
========================================================================== */

/// Returns the `TypingChanged(false)` event derived from `event`, if any.
fn typing_clear_for(event: &AppEvent) -> Option<AppEvent> {
    match event {
        AppEvent::MessageReceived(id, _) => Some(AppEvent::TypingChanged(id.clone(), false, None)),
        AppEvent::ConnectionStatusChanged(id, status) => {
            if matches!(
                status,
                ConnectionStatus::Disconnected
                    | ConnectionStatus::Retrying { .. }
                    | ConnectionStatus::Error { .. }
            ) {
                Some(AppEvent::TypingChanged(id.clone(), false, None))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Clears typing state for `id` without creating a runtime entry.
fn clear_typing_only(connections: &mut HashMap<String, ConnectionState>, id: &str) {
    if let Some(connection) = connections.get_mut(id) {
        connection.is_typing = false;
        connection.preview_text = None;
        connection.typing_generation = connection.typing_generation.wrapping_add(1);
    }
}

/// Applies a single `event` to the runtime connection map.
fn apply_connection_event(connections: &mut HashMap<String, ConnectionState>, event: &AppEvent) {
    match event {
        AppEvent::ConnectionAdded(id) => {
            connections.insert(
                id.clone(),
                ConnectionState::new(id.clone(), ConnectionStatus::Disconnected),
            );
        }
        AppEvent::ConnectionRemoved(id) => {
            connections.remove(id);
        }
        AppEvent::ConnectionStatusChanged(id, status) => {
            let entry = connections.entry(id.clone()).or_insert_with(|| {
                ConnectionState::new(id.clone(), ConnectionStatus::Disconnected)
            });
            entry.apply_status(status);

            if matches!(
                status,
                ConnectionStatus::Disconnected
                    | ConnectionStatus::Retrying { .. }
                    | ConnectionStatus::Error { .. }
            ) {
                entry.is_typing = false;
                entry.preview_text = None;
                entry.typing_generation = entry.typing_generation.wrapping_add(1);
            }
        }
        AppEvent::MessageReceived(id, message) => {
            if let Some(connection) = connections.get_mut(id) {
                connection.last_message = Some(message.clone());
                connection.last_message_generation =
                    connection.last_message_generation.wrapping_add(1);
                connection.is_typing = false;
                connection.preview_text = None;
                connection.typing_generation = connection.typing_generation.wrapping_add(1);
            }
        }
        AppEvent::MessageCleared(id) => {
            if let Some(connection) = connections.get_mut(id) {
                connection.last_message = None;
            }
        }
        AppEvent::TypingChanged(id, is_typing, preview) => {
            let entry = connections.entry(id.clone()).or_insert_with(|| {
                ConnectionState::new(id.clone(), ConnectionStatus::Disconnected)
            });
            entry.is_typing = *is_typing;
            entry.preview_text = if *is_typing { preview.clone() } else { None };
            entry.typing_generation = entry.typing_generation.wrapping_add(1);
        }
        _ => {}
    }
}

/// Applies `event` (and any derived typing-clear) to the runtime connection map
/// and returns the frontend events in emission order.
///
/// For `ConnectionRemoved` the final `typing-changed false` is emitted *before*
/// `connection-removed` and the runtime entry is only cleared, never recreated.
/// This guarantees removal leaves no runtime state behind, so a typing timeout
/// scheduled before removal finds no entry and emits nothing.
fn process_connection_runtime(
    connections: &mut HashMap<String, ConnectionState>,
    event: &AppEvent,
) -> Vec<AppEvent> {
    if let AppEvent::ConnectionRemoved(id) = event {
        clear_typing_only(connections, id);
        connections.remove(id);
        return vec![
            AppEvent::TypingChanged(id.clone(), false, None),
            event.clone(),
        ];
    }

    apply_connection_event(connections, event);
    let mut emitted = vec![event.clone()];
    if let Some(clear) = typing_clear_for(event) {
        apply_connection_event(connections, &clear);
        emitted.push(clear);
    }
    emitted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ConnectionStatus;
    use std::collections::HashMap;

    #[test]
    fn event_queue_diagnostic_never_contains_user_text() {
        let sentinel = "private-message-sentinel";
        for event in [
            AppEvent::MessageReceived("conn-1".into(), sentinel.into()),
            AppEvent::TypingChanged("conn-1".into(), true, Some(sentinel.into())),
        ] {
            let diagnostic = event_log_name(&event);
            assert!(!diagnostic.contains(sentinel));
            assert!(matches!(diagnostic, "message-received" | "typing-changed"));
        }
    }

    fn connections() -> Arc<RwLock<HashMap<String, ConnectionState>>> {
        Arc::new(RwLock::new(HashMap::new()))
    }

    fn insert_connected(conns: &Arc<RwLock<HashMap<String, ConnectionState>>>, id: &str) {
        conns.write().insert(
            id.to_string(),
            ConnectionState::new(id.to_string(), ConnectionStatus::Connected),
        );
    }

    // Delegates to the production helper so tests exercise the exact same
    // event-derivation logic as `AppState::emit_event`.
    fn update_runtime(conns: &Arc<RwLock<HashMap<String, ConnectionState>>>, event: &AppEvent) {
        process_connection_runtime(&mut conns.write(), event);
    }

    #[test]
    fn typing_changed_true_sets_state() {
        let conns = connections();
        insert_connected(&conns, "conn-1");

        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, Some("preview...".to_string())),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(conn.is_typing);
        assert_eq!(conn.preview_text.as_deref(), Some("preview..."));
    }

    #[test]
    fn typing_changed_false_clears_state() {
        let conns = connections();
        insert_connected(&conns, "conn-1");
        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, Some("preview...".to_string())),
        );

        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), false, None),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(!conn.is_typing);
        assert!(conn.preview_text.is_none());
    }

    #[test]
    fn message_received_clears_typing() {
        let conns = connections();
        insert_connected(&conns, "conn-1");
        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, Some("preview...".to_string())),
        );

        update_runtime(
            &conns,
            &AppEvent::MessageReceived("conn-1".to_string(), "final message".to_string()),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(!conn.is_typing);
        assert!(conn.preview_text.is_none());
    }

    #[test]
    fn status_changed_to_disconnected_clears_typing() {
        let conns = connections();
        insert_connected(&conns, "conn-1");
        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, Some("preview...".to_string())),
        );

        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged(
                "conn-1".to_string(),
                ConnectionStatus::Disconnected,
            ),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(!conn.is_typing);
        assert!(conn.preview_text.is_none());
    }

    #[test]
    fn status_changed_to_error_clears_typing() {
        let conns = connections();
        insert_connected(&conns, "conn-1");
        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, Some("preview...".to_string())),
        );

        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged(
                "conn-1".to_string(),
                ConnectionStatus::Error {
                    kind: ConnectionErrorKind::Network,
                    message: "fail".to_string(),
                },
            ),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(!conn.is_typing);
        assert!(conn.preview_text.is_none());
        assert_eq!(conn.error_kind, Some(ConnectionErrorKind::Network));
        assert_eq!(conn.error_message.as_deref(), Some("fail"));
    }

    #[test]
    fn retrying_status_stores_retry_fields_and_resets_error_fields() {
        let conns = connections();
        insert_connected(&conns, "conn-1");
        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged(
                "conn-1".to_string(),
                ConnectionStatus::Error {
                    kind: ConnectionErrorKind::Http,
                    message: "The server returned an unexpected HTTP response".to_string(),
                },
            ),
        );

        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged(
                "conn-1".to_string(),
                ConnectionStatus::Retrying {
                    attempt: 4,
                    max_attempts: 10,
                    next_retry_in_secs: 5,
                },
            ),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert_eq!(conn.status.name(), "Retrying");
        assert_eq!(conn.attempt, Some(4));
        assert_eq!(conn.max_attempts, Some(10));
        assert_eq!(conn.next_retry_in_secs, Some(5));
        // A retry supersedes the previous terminal error detail.
        assert_eq!(conn.error_kind, None);
        assert!(conn.error_message.is_none());
    }

    #[test]
    fn connected_resets_retry_and_error_fields() {
        let conns = connections();
        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged(
                "conn-1".to_string(),
                ConnectionStatus::Retrying {
                    attempt: 2,
                    max_attempts: 10,
                    next_retry_in_secs: 5,
                },
            ),
        );
        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged(
                "conn-1".to_string(),
                ConnectionStatus::Error {
                    kind: ConnectionErrorKind::Network,
                    message: "Could not reach the server".to_string(),
                },
            ),
        );
        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged("conn-1".to_string(), ConnectionStatus::Connected),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert_eq!(conn.status, ConnectionStatus::Connected);
        assert_eq!(conn.error_kind, None);
        assert!(conn.error_message.is_none());
        assert_eq!(conn.attempt, None);
        assert_eq!(conn.max_attempts, None);
        assert_eq!(conn.next_retry_in_secs, None);
    }

    #[test]
    fn retrying_status_clears_typing() {
        let conns = connections();
        insert_connected(&conns, "conn-1");
        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, Some("preview...".to_string())),
        );

        // `Retrying` replaced the intermediate `Disconnected` of the retry
        // loop, so it must keep clearing the typing indicator.
        update_runtime(
            &conns,
            &AppEvent::ConnectionStatusChanged(
                "conn-1".to_string(),
                ConnectionStatus::Retrying {
                    attempt: 2,
                    max_attempts: 10,
                    next_retry_in_secs: 5,
                },
            ),
        );

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(!conn.is_typing);
        assert!(conn.preview_text.is_none());
    }

    #[test]
    fn connection_removed_clears_typing_before_removal_and_leaves_no_entry() {
        let conns = connections();
        insert_connected(&conns, "conn-1");
        conns.write().get_mut("conn-1").unwrap().is_typing = true;

        let emitted = process_connection_runtime(
            &mut conns.write(),
            &AppEvent::ConnectionRemoved("conn-1".to_string()),
        );

        // The final typing-clear is derived and delivered before the removal so
        // the frontend never sees `typing-changed` after `connection-removed`.
        assert!(
            matches!(
                emitted.as_slice(),
                [
                    AppEvent::TypingChanged(clear_id, false, None),
                    AppEvent::ConnectionRemoved(removed_id),
                ] if clear_id == "conn-1" && removed_id == "conn-1"
            ),
            "expected [typing-changed false, connection-removed], got {emitted:?}"
        );

        // Removal must not leave a runtime entry that a stale timeout could
        // later observe and emit from.
        assert!(
            conns.read().get("conn-1").is_none(),
            "removal must leave no runtime state for the id"
        );
    }

    #[tokio::test]
    async fn removed_connection_has_no_state_for_stale_timeout() {
        tokio::time::pause();
        let conns = connections();
        insert_connected(&conns, "conn-1");
        {
            let mut guard = conns.write();
            let entry = guard.get_mut("conn-1").unwrap();
            entry.is_typing = true;
            entry.typing_generation = 1;
        }

        // A typing timeout that was scheduled while typing was active.
        let conns_clone = Arc::clone(&conns);
        let id = "conn-1".to_string();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS)).await;
            if let Some(entry) = conns_clone.write().get_mut(&id) {
                if entry.typing_generation == 1 {
                    entry.is_typing = false;
                    entry.preview_text = None;
                }
            }
        });

        // Removal happens before the timeout fires and must leave no entry.
        process_connection_runtime(
            &mut conns.write(),
            &AppEvent::ConnectionRemoved("conn-1".to_string()),
        );
        assert!(conns.read().get("conn-1").is_none());

        tokio::time::advance(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS + 1)).await;
        handle.await.unwrap();

        assert!(
            conns.read().get("conn-1").is_none(),
            "a stale timeout must find no state and emit nothing"
        );
    }

    #[test]
    fn typing_generation_increments_on_each_update() {
        let conns = connections();
        insert_connected(&conns, "conn-1");

        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, None),
        );
        let gen1 = conns.read().get("conn-1").unwrap().typing_generation;

        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, None),
        );
        let gen2 = conns.read().get("conn-1").unwrap().typing_generation;

        assert_ne!(gen1, gen2, "typing_generation must change on each update");
    }

    #[test]
    fn message_received_increments_typing_generation() {
        let conns = connections();
        insert_connected(&conns, "conn-1");

        update_runtime(
            &conns,
            &AppEvent::TypingChanged("conn-1".to_string(), true, None),
        );
        let gen_before = conns.read().get("conn-1").unwrap().typing_generation;

        update_runtime(
            &conns,
            &AppEvent::MessageReceived("conn-1".to_string(), "msg".to_string()),
        );
        let gen_after = conns.read().get("conn-1").unwrap().typing_generation;

        assert_ne!(
            gen_before, gen_after,
            "MessageReceived must invalidate any pending typing timeout"
        );
    }

    // Timeout-related tests require a full AppState with a tokio runtime, so
    // they exercise the real emit_event / schedule_typing_timeout path.
    // #[tokio::test] tests below use `tokio::time::pause()` for deterministic
    // time advancement.

    #[tokio::test]
    async fn typing_timeout_clears_state() {
        tokio::time::pause();
        let conns = connections();
        insert_connected(&conns, "conn-1");

        // Simulate what emit_event+schedule_typing_timeout does.
        let event =
            AppEvent::TypingChanged("conn-1".to_string(), true, Some("preview...".to_string()));
        update_runtime(&conns, &event);

        let gen = conns.read().get("conn-1").unwrap().typing_generation;
        let conns_clone = Arc::clone(&conns);
        let id = "conn-1".to_string();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS)).await;
            let mut c = conns_clone.write();
            if let Some(entry) = c.get_mut(&id) {
                if entry.typing_generation == gen {
                    entry.is_typing = false;
                    entry.preview_text = None;
                }
            }
        });

        tokio::time::advance(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS + 1)).await;
        handle.await.unwrap();

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(!conn.is_typing);
        assert!(conn.preview_text.is_none());
    }

    #[tokio::test]
    async fn repeated_typing_true_refreshes_timeout() {
        tokio::time::pause();
        let conns = connections();
        insert_connected(&conns, "conn-1");

        // First typing true
        let event1 = AppEvent::TypingChanged("conn-1".to_string(), true, None);
        update_runtime(&conns, &event1);
        let gen1 = conns.read().get("conn-1").unwrap().typing_generation;

        let conns_clone = Arc::clone(&conns);
        let id = "conn-1".to_string();
        let handle1 = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS)).await;
            if let Some(entry) = conns_clone.write().get_mut(&id) {
                if entry.typing_generation == gen1 {
                    entry.is_typing = false;
                    entry.preview_text = None;
                }
            }
        });

        // Advance halfway, then send another typing true
        tokio::time::advance(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS / 2)).await;
        tokio::task::yield_now().await;

        let event2 = AppEvent::TypingChanged("conn-1".to_string(), true, None);
        update_runtime(&conns, &event2);
        let gen2 = conns.read().get("conn-1").unwrap().typing_generation;

        let conns_clone2 = Arc::clone(&conns);
        let id2 = "conn-1".to_string();
        let handle2 = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS)).await;
            if let Some(entry) = conns_clone2.write().get_mut(&id2) {
                if entry.typing_generation == gen2 {
                    entry.is_typing = false;
                    entry.preview_text = None;
                }
            }
        });

        // Advance past original timeout but before refreshed one.
        tokio::time::advance(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS / 2)).await;
        // First handle should complete (gen doesn't match), second should not.
        handle1.await.unwrap();
        tokio::task::yield_now().await;

        {
            let conn = conns.read().get("conn-1").cloned().unwrap();
            assert!(
                conn.is_typing,
                "typing must still be true after original timeout would have fired"
            );
        }

        // Advance past refreshed timeout.
        tokio::time::advance(std::time::Duration::from_secs(TYPING_TIMEOUT_SECS / 2 + 1)).await;
        handle2.await.unwrap();

        let conn = conns.read().get("conn-1").cloned().unwrap();
        assert!(!conn.is_typing);
    }
}
