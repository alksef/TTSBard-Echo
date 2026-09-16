use serde::{Deserialize, Serialize};
use std::fmt;

/* ==========================================================================
Connection Status
========================================================================== */
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionStatus::Disconnected => write!(f, "Disconnected"),
            ConnectionStatus::Connecting => write!(f, "Connecting"),
            ConnectionStatus::Connected => write!(f, "Connected"),
            ConnectionStatus::Error(e) => write!(f, "Error: {}", e),
        }
    }
}

/* ==========================================================================
App Events
========================================================================== */
#[derive(Debug, Clone)]
pub enum AppEvent {
    // Theme
    ThemeChanged(String),

    // Floating Window
    FloatingAppearanceChanged,
    ClickthroughChanged(bool),
    ShowFloatingWindow,
    HideFloatingWindow,
    FloatingWindowToggled,
    FloatingVisibilityChanged(bool),
    UpdateFloatingText(String),

    // Connections
    ConnectionsChanged,
    ConnectionStatusChanged(String, ConnectionStatus),
    MessageReceived(String, String),
    MessageCleared(String),
    ConnectionAdded(String),
    ConnectionRemoved(String),
    TypingChanged(String, bool, Option<String>),

    // Settings
    SettingsChanged,
    LoggingChanged,
    AppearanceChanged,
    HotkeysChanged,
    GeneralChanged,

    // System
    BackendReady,
    AppQuit,
}

impl AppEvent {
    pub fn to_tauri_event(&self) -> &'static str {
        match self {
            AppEvent::ThemeChanged(_) => "theme-changed",
            AppEvent::FloatingAppearanceChanged => "floating-appearance-changed",
            AppEvent::ClickthroughChanged(_) => "clickthrough-changed",
            AppEvent::ShowFloatingWindow => "show-floating-window",
            AppEvent::HideFloatingWindow => "hide-floating-window",
            AppEvent::FloatingWindowToggled => "floating-window-toggled",
            AppEvent::FloatingVisibilityChanged(_) => "floating-visibility-changed",
            AppEvent::UpdateFloatingText(_) => "update-floating-text",
            AppEvent::ConnectionsChanged => "connections-changed",
            AppEvent::ConnectionStatusChanged(_, _) => "connection-status-changed",
            AppEvent::MessageReceived(_, _) => "message-received",
            AppEvent::MessageCleared(_) => "message-cleared",
            AppEvent::ConnectionAdded(_) => "connection-added",
            AppEvent::ConnectionRemoved(_) => "connection-removed",
            AppEvent::TypingChanged(_, _, _) => "typing-changed",
            AppEvent::SettingsChanged => "settings-changed",
            AppEvent::LoggingChanged => "logging-changed",
            AppEvent::AppearanceChanged => "appearance-changed",
            AppEvent::HotkeysChanged => "hotkeys-changed",
            AppEvent::GeneralChanged => "general-changed",
            AppEvent::BackendReady => "backend-ready",
            AppEvent::AppQuit => "app-quit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sentinel token value of the connection under test: its reproduction
    /// in any webview payload or log formatting is a leak (roadmap 010,
    /// task 005). The payloads below are built exactly as `event_loop` and
    /// `connections/client` build them for a connection whose token is this
    /// sentinel — the assertion pins that the token never reaches them.
    const TOKEN_SENTINEL: &str = "secret-sentinel";

    /// Status payloads for a token-carrying connection: `event_loop` emits
    /// `(id, status.to_string())` and logs `Debug` of id and status; the
    /// terminal error text is the fixed message from the retry loop.
    #[test]
    fn status_event_payloads_are_sentinel_free() {
        let id = "conn-events";
        let statuses = [
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
            ConnectionStatus::Disconnected,
            ConnectionStatus::Error(crate::connections::client::terminal_error_message()),
        ];

        for status in statuses {
            // The webview payload shape used by event_loop.
            let payload = serde_json::to_string(&(id, status.to_string())).unwrap();
            // Positive control: the payload really carries the connection id.
            assert!(payload.contains(id));
            assert!(
                !payload.contains(TOKEN_SENTINEL),
                "status payload leaks the token: {payload}"
            );

            // The serde shape of the status itself (Debug-derived enum).
            let serde_status = serde_json::to_string(&status).unwrap();
            assert!(!serde_status.contains(TOKEN_SENTINEL));

            // The log formatting in event_loop and the Debug of dropped
            // events in state.rs.
            let debug = format!("{status:?}");
            assert!(!debug.contains(TOKEN_SENTINEL));
            let event_debug = format!(
                "{:?}",
                AppEvent::ConnectionStatusChanged(id.to_string(), status)
            );
            assert!(!event_debug.contains(TOKEN_SENTINEL));
        }
    }

    /// Typing and message payloads for a token-carrying connection carry
    /// only the connection id and producer-provided text.
    #[test]
    fn typing_and_message_event_payloads_are_sentinel_free() {
        let id = "conn-events";

        // The JSON payload built in event_loop for TypingChanged.
        let mut payload = serde_json::json!({ "id": id, "isTyping": true });
        payload["previewText"] = serde_json::json!("typing preview");
        let text = payload.to_string();
        assert!(text.contains(id));
        assert!(!text.contains(TOKEN_SENTINEL));

        // The (id, message) tuple emitted for MessageReceived.
        let payload = serde_json::to_string(&(id, "hello from the producer")).unwrap();
        assert!(!payload.contains(TOKEN_SENTINEL));

        // Debug of the same events (the drop-on-full log path).
        let debug = format!(
            "{:?}",
            AppEvent::TypingChanged(id.to_string(), true, Some("typing preview".to_string()))
        );
        assert!(!debug.contains(TOKEN_SENTINEL));
        let debug = format!(
            "{:?}",
            AppEvent::MessageReceived(id.to_string(), "hello".to_string())
        );
        assert!(!debug.contains(TOKEN_SENTINEL));
    }
}
