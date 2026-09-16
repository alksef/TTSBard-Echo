//! Normalized connection error taxonomy (roadmap 011, decision A).
//!
//! Exactly seven machine-readable categories describe why a connection
//! attempt failed. The backend pairs a category with a fixed English message
//! so the webview can render localized text per category and fall back to the
//! message for unknown kinds. Messages are built from literals only: no URL,
//! query string, or token ever enters them (roadmap 010 redaction rules).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Machine-readable category of a connection failure.
///
/// Serializes to snake_case (`"configuration"`, `"network"`, …). The
/// `cancelled` category is recognized by the classifier now; it is produced
/// actively starting from roadmap 011 task 004 (cooperative cancellation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionErrorKind {
    Configuration,
    Network,
    Tls,
    Authentication,
    Http,
    Protocol,
    Cancelled,
}

impl ConnectionErrorKind {
    /// The category's fixed English message.
    ///
    /// Fixed means literal text only — safe to show in the webview and write
    /// to the log file, because no request detail (URL, query, token) can be
    /// formatted into it.
    pub fn fixed_message(self) -> &'static str {
        match self {
            Self::Configuration => "Connection configuration is invalid",
            Self::Network => "Could not reach the server",
            Self::Tls => "Secure connection (TLS) failed",
            Self::Authentication => "The server rejected the credentials",
            Self::Http => "The server returned an unexpected HTTP response",
            Self::Protocol => "The server response is not a valid event stream",
            Self::Cancelled => "The connection attempt was cancelled",
        }
    }
}

impl fmt::Display for ConnectionErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Same snake_case spelling the serde serialization produces.
        let name = match self {
            Self::Configuration => "configuration",
            Self::Network => "network",
            Self::Tls => "tls",
            Self::Authentication => "authentication",
            Self::Http => "http",
            Self::Protocol => "protocol",
            Self::Cancelled => "cancelled",
        };
        f.write_str(name)
    }
}

/// A classified connection failure: machine category plus a fixed English
/// message.
///
/// The raw error text that produced the category is deliberately dropped: it
/// may embed request details, so only the fixed message travels with the
/// error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionError {
    pub kind: ConnectionErrorKind,
    pub message: String,
}

impl ConnectionError {
    pub fn new(kind: ConnectionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// An error carrying the category's fixed message.
    pub fn for_kind(kind: ConnectionErrorKind) -> Self {
        Self::new(kind, kind.fixed_message())
    }

    /// Classify a raw failure text and attach the category's fixed message.
    ///
    /// The raw text never reaches the result.
    pub fn from_error_text(text: &str) -> Self {
        Self::for_kind(classify_error_text(text))
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

/* --------------------------------------------------------------------------
Classification
-------------------------------------------------------------------------- */

/// `ClientBuilder::for_url` / `header` failures: `invalid parameter: {cause}`.
const CONFIGURATION_MARKERS: &[&str] = &[
    "invalid parameter",
    "invalid uri",
    "invalid url",
    "relative url without a base",
];

/// hyper/rustls certificate faults surfacing through the connect error text.
///
/// Checked before the network markers so `invalid dnsname` (a TLS certificate
/// name mismatch) is not swallowed by the `dns` pattern of DNS resolution
/// failures.
const TLS_MARKERS: &[&str] = &[
    "invalid peer certificate",
    "invalid dnsname",
    "certificate",
    "handshake",
    "fatal alert",
    "tls",
    "ssl",
];

/// Non-2xx HTTP statuses the crate reports as
/// `unexpected response: {status}` (e.g. `500 Internal Server Error`,
/// `404 Not Found`) and the redirect-limit exhaustion.
const HTTP_MARKERS: &[&str] = &[
    "unexpected response",
    "invalid status code",
    "maximum redirect limit",
];

/// Credential failures: `unexpected response: 401 Unauthorized` /
/// `unexpected response: 403 Forbidden`.
const AUTHENTICATION_MARKERS: &[&str] = &[
    "unexpected response: 401",
    "unexpected response: 403",
    "unauthorized",
    "forbidden",
];

/// Server violated the SSE/HTTP framing: `invalid line: malformed key/value:
/// {Utf8Error}`, `invalid event`, `malformed header: {cause}`.
///
/// Matched first: these texts embed server-controlled data, so a marker of
/// another category inside that data (e.g. an `invalid line` echo containing
/// "unexpected response") must not reclassify the parser fault.
const PROTOCOL_MARKERS: &[&str] = &["invalid line", "invalid event", "malformed header"];

/// Client-side cancellation markers (`cancelled` covers both spellings).
/// Recognized now; actively produced from task 004.
const CANCELLED_MARKERS: &[&str] = &["canceled", "cancelled"];

/// Transport-level failures: hyper connect/DNS errors (`client error
/// (Connect): tcp connect error` / `dns error`), timeouts (`timed out`), and
/// the stream-end family (`eof`, `unexpected eof`, `stream closed`).
const NETWORK_MARKERS: &[&str] = &[
    "timed out",
    "timeout",
    "dns",
    "connect",
    "connection refused",
    "connection reset",
    "connection closed",
    "stream closed",
    "broken pipe",
    "unreachable",
    "eof",
    "network",
];

/// Classify a raw failure text into a [`ConnectionErrorKind`].
///
/// eventsource-client 0.12 reports failures as `Display` texts only (e.g.
/// `unexpected response: 401 Unauthorized`, `http error: {hyper error}`), so
/// classification is textual. Matching is deliberately conservative: the
/// single function below is the only place that maps texts to categories, and
/// anything unrecognized falls back to [`ConnectionErrorKind::Network`] — an
/// unknown failure looks like a connection problem, not like something the
/// user must fix in the configuration.
pub(crate) fn classify_error_text(text: &str) -> ConnectionErrorKind {
    let lower = text.to_ascii_lowercase();

    let groups = [
        (PROTOCOL_MARKERS, ConnectionErrorKind::Protocol),
        (CANCELLED_MARKERS, ConnectionErrorKind::Cancelled),
        (AUTHENTICATION_MARKERS, ConnectionErrorKind::Authentication),
        (HTTP_MARKERS, ConnectionErrorKind::Http),
        (TLS_MARKERS, ConnectionErrorKind::Tls),
        (CONFIGURATION_MARKERS, ConnectionErrorKind::Configuration),
        (NETWORK_MARKERS, ConnectionErrorKind::Network),
    ];

    for (markers, kind) in groups {
        if markers.iter().any(|marker| lower.contains(marker)) {
            return kind;
        }
    }

    ConnectionErrorKind::Network
}

#[cfg(test)]
mod tests {
    use super::{classify_error_text, ConnectionError, ConnectionErrorKind};

    /// Sentinel token value: its reproduction in any fixed message is a leak
    /// (roadmap 010, task 005).
    const TOKEN_SENTINEL: &str = "secret-sentinel";

    #[test]
    fn kind_serializes_as_snake_case() {
        let cases = [
            (ConnectionErrorKind::Configuration, "configuration"),
            (ConnectionErrorKind::Network, "network"),
            (ConnectionErrorKind::Tls, "tls"),
            (ConnectionErrorKind::Authentication, "authentication"),
            (ConnectionErrorKind::Http, "http"),
            (ConnectionErrorKind::Protocol, "protocol"),
            (ConnectionErrorKind::Cancelled, "cancelled"),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{expected}\""),
                "serde form of {kind:?}"
            );
            // Display matches the serde spelling, so logs and payloads agree.
            assert_eq!(kind.to_string(), expected, "Display of {kind:?}");
        }
    }

    /// Every category is reachable from real error texts produced by
    /// eventsource-client 0.12 / hyper / rustls (Display of
    /// `eventsource_client::Error` as handed to the stream consumer).
    #[test]
    fn classification_covers_all_seven_categories() {
        let cases = [
            // configuration — invalid URL or header rejected by the builder:
            // `Error::InvalidParameter` → "invalid parameter: {cause}"
            (
                "invalid parameter: invalid uri character",
                ConnectionErrorKind::Configuration,
            ),
            (
                "invalid parameter: relative URL without a base",
                ConnectionErrorKind::Configuration,
            ),
            // network — hyper connect/DNS errors, crate timeout, stream-end family
            (
                "http error: client error (Connect): tcp connect error",
                ConnectionErrorKind::Network,
            ),
            (
                "http error: client error (Connect): dns error",
                ConnectionErrorKind::Network,
            ),
            (
                "http error: operation timed out",
                ConnectionErrorKind::Network,
            ),
            ("timed out", ConnectionErrorKind::Network),
            ("eof", ConnectionErrorKind::Network),
            ("unexpected eof", ConnectionErrorKind::Network),
            // tls — rustls certificate faults behind hyper connect errors
            (
                "http error: client error (Connect): invalid peer certificate: UnknownIssuer",
                ConnectionErrorKind::Tls,
            ),
            (
                "http error: client error (Connect): invalid dnsname",
                ConnectionErrorKind::Tls,
            ),
            // authentication — `Error::UnexpectedResponse(401|403)` →
            // "unexpected response: {status}" (StatusCode Display)
            (
                "unexpected response: 401 Unauthorized",
                ConnectionErrorKind::Authentication,
            ),
            (
                "unexpected response: 403 Forbidden",
                ConnectionErrorKind::Authentication,
            ),
            // http — other non-2xx statuses and redirect loops
            (
                "unexpected response: 500 Internal Server Error",
                ConnectionErrorKind::Http,
            ),
            (
                "unexpected response: 503 Service Unavailable",
                ConnectionErrorKind::Http,
            ),
            (
                "unexpected response: 404 Not Found",
                ConnectionErrorKind::Http,
            ),
            (
                "maximum redirect limit reached: 16",
                ConnectionErrorKind::Http,
            ),
            // protocol — SSE parser faults
            (
                "invalid line: malformed value: invalid utf-8 sequence of 1 bytes from index 0",
                ConnectionErrorKind::Protocol,
            ),
            ("invalid event", ConnectionErrorKind::Protocol),
            (
                "malformed header: missing Location header",
                ConnectionErrorKind::Protocol,
            ),
            // cancelled — hyper's client-side cancellation marker
            // ("operation was canceled"); actively produced from task 004.
            (
                "http error: operation was canceled",
                ConnectionErrorKind::Cancelled,
            ),
            (
                "connection cancelled by user",
                ConnectionErrorKind::Cancelled,
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(classify_error_text(text), expected, "text: {text}");
        }
    }

    #[test]
    fn unrecognized_text_defaults_to_network() {
        assert_eq!(
            classify_error_text("some totally novel failure mode"),
            ConnectionErrorKind::Network
        );
    }

    /// The suffix of an `invalid line: …` text is server-controlled data;
    /// markers of other categories embedded there must not reclassify the
    /// parser fault. Protocol markers are therefore matched first.
    #[test]
    fn protocol_markers_win_over_data_embedded_in_invalid_line() {
        assert_eq!(
            classify_error_text("invalid line: unexpected response: 401 Unauthorized"),
            ConnectionErrorKind::Protocol
        );
        assert_eq!(
            classify_error_text("invalid line: certificate revoked"),
            ConnectionErrorKind::Protocol
        );
    }

    #[test]
    fn every_kind_has_fixed_sentinel_free_message() {
        let kinds = [
            ConnectionErrorKind::Configuration,
            ConnectionErrorKind::Network,
            ConnectionErrorKind::Tls,
            ConnectionErrorKind::Authentication,
            ConnectionErrorKind::Http,
            ConnectionErrorKind::Protocol,
            ConnectionErrorKind::Cancelled,
        ];
        for kind in kinds {
            let error = ConnectionError::for_kind(kind);
            assert!(!error.message.is_empty(), "message for {kind:?}");
            assert!(
                !error.message.contains(TOKEN_SENTINEL),
                "fixed message for {kind:?} leaks the sentinel: {}",
                error.message
            );
        }
    }

    /// Classification never copies the raw text into the message, even when
    /// the raw text carries the sentinel (e.g. an echo of request data): the
    /// result carries the category and the fixed message only.
    #[test]
    fn from_error_text_drops_raw_text_and_sentinel() {
        let raw = format!("http error: client error (Connect): bad peer {TOKEN_SENTINEL}");
        let error = ConnectionError::from_error_text(&raw);
        assert_eq!(error.kind, ConnectionErrorKind::Network);
        assert_eq!(error.message, ConnectionErrorKind::Network.fixed_message());
        assert!(!error.to_string().contains(TOKEN_SENTINEL));
    }

    #[test]
    fn from_error_text_pairs_status_texts_with_kinds() {
        let error = ConnectionError::from_error_text("unexpected response: 401 Unauthorized");
        assert_eq!(error.kind, ConnectionErrorKind::Authentication);
        assert_eq!(error.message, "The server rejected the credentials");

        let error =
            ConnectionError::from_error_text("unexpected response: 500 Internal Server Error");
        assert_eq!(error.kind, ConnectionErrorKind::Http);
        assert_eq!(
            error.message,
            "The server returned an unexpected HTTP response"
        );
    }
}
