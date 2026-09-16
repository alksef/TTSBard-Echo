use crate::connections::ConnectionErrorKind;
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
    /// A failed attempt announced the next one: `attempt` is the number of
    /// the upcoming (not yet made) attempt, 1-based, out of `max_attempts`;
    /// `next_retry_in_secs` is the wait before it starts (roadmap 011,
    /// decision B).
    Retrying {
        attempt: u32,
        max_attempts: u32,
        next_retry_in_secs: u64,
    },
    /// A classified terminal failure: machine category plus the fixed
    /// English message (no URL, query, or token can enter the message).
    Error {
        kind: ConnectionErrorKind,
        message: String,
    },
}

impl ConnectionStatus {
    /// The simple status name used as the `status` field of the wire payload.
    ///
    /// The structured detail (error kind/message, attempt counters) travels
    /// in its own payload fields, not in this name.
    pub fn name(&self) -> &'static str {
        match self {
            ConnectionStatus::Disconnected => "Disconnected",
            ConnectionStatus::Connecting => "Connecting",
            ConnectionStatus::Connected => "Connected",
            ConnectionStatus::Retrying { .. } => "Retrying",
            ConnectionStatus::Error { .. } => "Error",
        }
    }
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionStatus::Disconnected => write!(f, "Disconnected"),
            ConnectionStatus::Connecting => write!(f, "Connecting"),
            ConnectionStatus::Connected => write!(f, "Connected"),
            ConnectionStatus::Retrying { .. } => write!(f, "Retrying"),
            ConnectionStatus::Error { message, .. } => write!(f, "Error: {message}"),
        }
    }
}

/// The structured webview payload for `connection-status-changed`:
/// `{id, status, errorKind?, errorMessage?, attempt?, maxAttempts?,
/// nextRetryInSecs?}` (roadmap 011, decision B).
///
/// Wire field names are camelCase; the optional detail fields are present
/// only when the status carries them — plain statuses serialize exactly
/// `{id, status}`.
pub fn connection_status_payload(id: &str, status: &ConnectionStatus) -> serde_json::Value {
    let mut payload = serde_json::json!({ "id": id, "status": status.name() });
    match status {
        ConnectionStatus::Retrying {
            attempt,
            max_attempts,
            next_retry_in_secs,
        } => {
            payload["attempt"] = serde_json::json!(attempt);
            payload["maxAttempts"] = serde_json::json!(max_attempts);
            payload["nextRetryInSecs"] = serde_json::json!(next_retry_in_secs);
        }
        ConnectionStatus::Error { kind, message } => {
            payload["errorKind"] = serde_json::json!(kind);
            payload["errorMessage"] = serde_json::json!(message);
        }
        _ => {}
    }
    payload
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
    FloatingVisibilityChanged(bool),

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
    HotkeysChanged,
    GeneralChanged,
}

impl AppEvent {
    pub fn to_tauri_event(&self) -> &'static str {
        match self {
            AppEvent::ThemeChanged(_) => "theme-changed",
            AppEvent::FloatingAppearanceChanged => "floating-appearance-changed",
            AppEvent::ClickthroughChanged(_) => "clickthrough-changed",
            AppEvent::FloatingVisibilityChanged(_) => "floating-visibility-changed",
            AppEvent::ConnectionsChanged => "connections-changed",
            AppEvent::ConnectionStatusChanged(_, _) => "connection-status-changed",
            AppEvent::MessageReceived(_, _) => "message-received",
            AppEvent::MessageCleared(_) => "message-cleared",
            AppEvent::ConnectionAdded(_) => "connection-added",
            AppEvent::ConnectionRemoved(_) => "connection-removed",
            AppEvent::TypingChanged(_, _, _) => "typing-changed",
            AppEvent::SettingsChanged => "settings-changed",
            AppEvent::LoggingChanged => "logging-changed",
            AppEvent::HotkeysChanged => "hotkeys-changed",
            AppEvent::GeneralChanged => "general-changed",
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

    /// The structured payload shape: plain statuses serialize exactly
    /// `{id, status}`, while `Retrying`/`Error` add their detail fields —
    /// and nothing else (roadmap 011, decision B).
    #[test]
    fn status_payload_is_structural_with_fields_only_when_applicable() {
        let id = "conn-events";

        for status in [
            ConnectionStatus::Disconnected,
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
        ] {
            let payload = connection_status_payload(id, &status);
            assert_eq!(
                payload,
                serde_json::json!({ "id": id, "status": status.name() }),
                "payload of {status:?}"
            );
        }

        let payload = connection_status_payload(
            id,
            &ConnectionStatus::Retrying {
                attempt: 3,
                max_attempts: 10,
                next_retry_in_secs: 5,
            },
        );
        assert_eq!(
            payload,
            serde_json::json!({
                "id": id,
                "status": "Retrying",
                "attempt": 3,
                "maxAttempts": 10,
                "nextRetryInSecs": 5,
            })
        );

        let payload = connection_status_payload(
            id,
            &ConnectionStatus::Error {
                kind: ConnectionErrorKind::Authentication,
                message: ConnectionErrorKind::Authentication
                    .fixed_message()
                    .to_string(),
            },
        );
        assert_eq!(
            payload,
            serde_json::json!({
                "id": id,
                "status": "Error",
                "errorKind": "authentication",
                "errorMessage": "The server rejected the credentials",
            })
        );
    }

    /// Status payloads for a token-carrying connection: `event_loop` emits
    /// `connection_status_payload(id, status)` and logs `Debug` of id and
    /// status; the terminal error carries the fixed kind/message pair from
    /// the retry loop.
    #[test]
    fn status_event_payloads_are_sentinel_free() {
        let id = "conn-events";
        let statuses = [
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
            ConnectionStatus::Disconnected,
            ConnectionStatus::Retrying {
                attempt: 2,
                max_attempts: 10,
                next_retry_in_secs: 5,
            },
            ConnectionStatus::Error {
                kind: ConnectionErrorKind::Network,
                message: crate::connections::client::terminal_error_message(),
            },
        ];

        for status in statuses {
            // The webview payload shape used by event_loop.
            let payload = connection_status_payload(id, &status).to_string();
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
                AppEvent::ConnectionStatusChanged(id.to_string(), status.clone())
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
