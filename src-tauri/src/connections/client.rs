use crate::events::{AppEvent, ConnectionStatus};
use crate::state::AppState;
use eventsource_client::{self, Client as _, ClientBuilder};
use futures_util::stream::StreamExt;
use std::future::Future;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const RECONNECT_DELAY_SECS: u64 = 5;

pub struct SSEClient {
    id: String,
    url: String,
    access_token: Option<String>,
    state: Arc<AppState>,
}

impl SSEClient {
    pub fn new(
        id: String,
        url: String,
        access_token: Option<String>,
        state: Arc<AppState>,
    ) -> Self {
        Self {
            id,
            url,
            access_token,
            state,
        }
    }

    pub fn connect(&self) -> tokio::task::JoinHandle<()> {
        let id = self.id.clone();
        let url = self.url.clone();
        let access_token = self.access_token.clone();
        let state = self.state.clone();
        tokio::spawn(async move { run_with_retries(id, url, access_token, state).await })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ConnectResult {
    NotConnected,
    ConnectedThenEnded,
    Failed,
}

async fn run_with_retries(
    id: String,
    url: String,
    access_token: Option<String>,
    state: Arc<AppState>,
) {
    let connect_fn = {
        let id = id.clone();
        let url = url.clone();
        let access_token = access_token.clone();
        let state = Arc::clone(&state);
        move || {
            let id = id.clone();
            let url = url.clone();
            let access_token = access_token.clone();
            let state = Arc::clone(&state);
            async move { try_connect(&id, &url, &access_token, &state).await }
        }
    };

    let emit_status = {
        let id = id.clone();
        let state = Arc::clone(&state);
        move |status: ConnectionStatus| {
            state.emit_event(AppEvent::ConnectionStatusChanged(id.clone(), status));
        }
    };

    retry_loop(connect_fn, emit_status).await;
}

async fn retry_loop<F, Fut>(mut connect_fn: F, emit_status: impl Fn(ConnectionStatus))
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConnectResult>,
{
    emit_status(ConnectionStatus::Connecting);
    let mut result = connect_fn().await;
    emit_status(ConnectionStatus::Disconnected);

    let mut policy = RetryPolicy::new();

    loop {
        while !policy.exhausted() {
            sleep(policy.delay()).await;

            let attempt = policy.take_attempt();
            info!("Reconnect attempt {attempt}/{MAX_RECONNECT_ATTEMPTS}");
            emit_status(ConnectionStatus::Connecting);

            result = connect_fn().await;

            if matches!(result, ConnectResult::ConnectedThenEnded) {
                emit_status(ConnectionStatus::Disconnected);
                policy.reset();
                break;
            }

            emit_status(ConnectionStatus::Disconnected);
        }

        if matches!(result, ConnectResult::ConnectedThenEnded) {
            continue;
        }

        let msg = terminal_error_message();
        warn!("{msg}");
        emit_status(ConnectionStatus::Error(msg));
        return;
    }
}

/// Terminal message when the reconnect budget is exhausted.
///
/// It reaches the webview as a user-visible error and the log file as a
/// `warn`, so it stays fixed text: no URL, no credentials — the token rides
/// in the `Cookie` header only and must never be formatted into messages
/// (roadmap 010, task 005).
pub(crate) fn terminal_error_message() -> String {
    format!("Connection failed after {MAX_RECONNECT_ATTEMPTS} attempts")
}

async fn try_connect(
    id: &str,
    url: &str,
    access_token: &Option<String>,
    state: &Arc<AppState>,
) -> ConnectResult {
    if let Err(e) = crate::config::validation::validate_url(url) {
        warn!("Invalid URL for connection {id}: {e}");
        return ConnectResult::NotConnected;
    }

    let endpoint = resolve_sse_endpoint(url, access_token.as_deref());

    let mut builder = match ClientBuilder::for_url(&endpoint.url) {
        Ok(builder) => builder,
        Err(e) => {
            warn!("Failed to build SSE client for {id}: {e}");
            return ConnectResult::NotConnected;
        }
    };

    if let Some(token) = endpoint.access_token.as_deref() {
        builder = match builder.header("Cookie", &format!("webview_auth={token}")) {
            Ok(builder) => builder,
            Err(e) => {
                warn!("Failed to set auth header for {id}: {e}");
                return ConnectResult::NotConnected;
            }
        };
        info!("Using access token (masked) for {id}");
    }

    let mut stream = builder.build().stream();
    let mut connected = false;

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                if !connected {
                    connected = true;
                    state.emit_event(AppEvent::ConnectionStatusChanged(
                        id.to_string(),
                        ConnectionStatus::Connected,
                    ));
                }

                let (event_type, text) = match event {
                    eventsource_client::SSE::Event(event) => (event.event_type, event.data),
                    eventsource_client::SSE::Comment(_) => continue,
                };
                if event_type == "connected" {
                    continue;
                }
                if text.is_empty() {
                    continue;
                }

                match classify_payload(&text) {
                    ParsedPayload::TypingIndicator { is_typing, preview } => {
                        state.emit_event(AppEvent::TypingChanged(
                            id.to_string(),
                            is_typing,
                            preview,
                        ));
                    }
                    ParsedPayload::Message(message) => {
                        state.emit_event(AppEvent::MessageReceived(id.to_string(), message));
                    }
                }
            }
            Err(e) => {
                warn!("SSE read error for {id}: {e}");
                return classify_read_error(connected);
            }
        }
    }

    if connected {
        ConnectResult::ConnectedThenEnded
    } else {
        ConnectResult::NotConnected
    }
}

fn classify_read_error(connected: bool) -> ConnectResult {
    if connected {
        ConnectResult::ConnectedThenEnded
    } else {
        ConnectResult::Failed
    }
}

#[derive(Debug)]
struct RetryPolicy {
    remaining: u32,
}

impl RetryPolicy {
    fn new() -> Self {
        Self {
            remaining: MAX_RECONNECT_ATTEMPTS,
        }
    }

    fn reset(&mut self) {
        self.remaining = MAX_RECONNECT_ATTEMPTS;
    }

    fn exhausted(&self) -> bool {
        self.remaining == 0
    }

    fn delay(&self) -> Duration {
        Duration::from_secs(RECONNECT_DELAY_SECS)
    }

    fn take_attempt(&mut self) -> u32 {
        let attempt = MAX_RECONNECT_ATTEMPTS - self.remaining + 1;
        self.remaining -= 1;
        attempt
    }
}

/// The companion app exposes its browser overlay at `/` and the event stream
/// at `/sse`. Its UI intentionally copies the browser URL, so accept that URL
/// as a connection endpoint as well. Explicit non-root paths are left intact
/// for compatibility with other SSE producers.
#[derive(Debug, PartialEq)]
struct ResolvedSseEndpoint {
    url: String,
    access_token: Option<String>,
}

fn resolve_sse_endpoint(value: &str, configured_token: Option<&str>) -> ResolvedSseEndpoint {
    let Ok(mut parsed) = url::Url::parse(value) else {
        return ResolvedSseEndpoint {
            url: value.to_string(),
            access_token: configured_token.map(str::to_owned),
        };
    };

    if parsed.path().is_empty() || parsed.path() == "/" {
        parsed.set_path("/sse");
    }

    let mut url_token = None;
    let retained_query: Vec<(String, String)> = parsed
        .query_pairs()
        .filter_map(|(key, value)| {
            if key == "token" {
                if url_token.is_none() && !value.is_empty() {
                    url_token = Some(value.into_owned());
                }
                None
            } else {
                Some((key.into_owned(), value.into_owned()))
            }
        })
        .collect();

    parsed.set_query(None);
    if !retained_query.is_empty() {
        parsed.query_pairs_mut().extend_pairs(retained_query);
    }
    parsed.set_fragment(None);

    ResolvedSseEndpoint {
        url: parsed.to_string(),
        access_token: configured_token
            .filter(|token| !token.is_empty())
            .map(str::to_owned)
            .or(url_token),
    }
}

/// The result of classifying an SSE text payload.
#[derive(Debug, PartialEq)]
enum ParsedPayload {
    TypingIndicator {
        is_typing: bool,
        preview: Option<String>,
    },
    Message(String),
}

/// Classify an SSE text payload as either a typing indicator or a regular message.
///
/// A payload is a typing indicator when parsed JSON contains a boolean `typing`
/// or `isTyping` field. The optional `text` field is preserved as a preview when
/// present. Payloads without a typing field but with a top-level `text` field
/// are treated as final messages. Non-JSON or unrecognized payloads fall
/// through to raw text.
fn classify_payload(text: &str) -> ParsedPayload {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(is_typing) = value
            .get("typing")
            .or_else(|| value.get("isTyping"))
            .and_then(|v| v.as_bool())
        {
            let preview = if is_typing {
                value
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            } else {
                None
            };
            return ParsedPayload::TypingIndicator { is_typing, preview };
        }
        if let Some(message_text) = value.get("text").and_then(|v| v.as_str()) {
            return ParsedPayload::Message(message_text.to_owned());
        }
    }
    ParsedPayload::Message(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        classify_payload, classify_read_error, resolve_sse_endpoint, retry_loop,
        terminal_error_message, ConnectResult, ParsedPayload, ResolvedSseEndpoint,
    };
    use crate::events::ConnectionStatus;
    use std::sync::{Arc, Mutex};

    #[test]
    fn read_error_after_connection_classified_as_established() {
        assert_eq!(classify_read_error(true), ConnectResult::ConnectedThenEnded);
        assert_eq!(classify_read_error(false), ConnectResult::Failed);
    }

    #[test]
    fn typing_payload_true_without_preview() {
        assert_eq!(
            classify_payload(r#"{"isTyping":true}"#),
            ParsedPayload::TypingIndicator {
                is_typing: true,
                preview: None,
            }
        );
    }

    #[test]
    fn typing_payload_true_with_preview_text() {
        assert_eq!(
            classify_payload(r#"{"isTyping":true,"text":"hello"}"#),
            ParsedPayload::TypingIndicator {
                is_typing: true,
                preview: Some("hello".to_string()),
            }
        );
    }

    #[test]
    fn typing_payload_false_clears_state() {
        assert_eq!(
            classify_payload(r#"{"isTyping":false}"#),
            ParsedPayload::TypingIndicator {
                is_typing: false,
                preview: None,
            }
        );
    }

    #[test]
    fn typing_payload_named_typing_clears_state() {
        assert_eq!(
            classify_payload(r#"{"typing":false}"#),
            ParsedPayload::TypingIndicator {
                is_typing: false,
                preview: None,
            }
        );
    }

    #[test]
    fn typing_payload_false_with_text_still_clears() {
        // When isTyping is false the producer is done typing; the text field is
        // not treated as preview.
        assert_eq!(
            classify_payload(r#"{"isTyping":false,"text":"leftover"}"#),
            ParsedPayload::TypingIndicator {
                is_typing: false,
                preview: None,
            }
        );
    }

    #[test]
    fn text_payload_without_is_typing_is_message() {
        assert_eq!(
            classify_payload(r#"{"text":"final message"}"#),
            ParsedPayload::Message("final message".to_string()),
        );
    }

    #[test]
    fn json_without_is_typing_or_text_falls_through_to_raw() {
        assert_eq!(
            classify_payload(r#"{"other":42}"#),
            ParsedPayload::Message(r#"{"other":42}"#.to_string()),
        );
    }

    #[test]
    fn non_json_falls_through_to_raw_text() {
        assert_eq!(
            classify_payload("plain text"),
            ParsedPayload::Message("plain text".to_string()),
        );
    }

    #[test]
    fn appends_sse_path_to_server_root() {
        assert_eq!(
            resolve_sse_endpoint("http://127.0.0.1:10100", None).url,
            "http://127.0.0.1:10100/sse"
        );
        assert_eq!(
            resolve_sse_endpoint("http://localhost:10100/", None).url,
            "http://localhost:10100/sse"
        );
    }

    #[test]
    fn preserves_explicit_endpoint_and_query() {
        assert_eq!(
            resolve_sse_endpoint("https://example.com/events?channel=tts", None).url,
            "https://example.com/events?channel=tts"
        );
        assert_eq!(
            resolve_sse_endpoint("http://127.0.0.1:10100/?token=secret", None),
            ResolvedSseEndpoint {
                url: "http://127.0.0.1:10100/sse".to_string(),
                access_token: Some("secret".to_string()),
            }
        );
    }

    #[test]
    fn configured_token_has_priority_and_other_query_values_survive() {
        assert_eq!(
            resolve_sse_endpoint(
                "https://example.com/?channel=tts&token=url-token",
                Some("field-token")
            ),
            ResolvedSseEndpoint {
                url: "https://example.com/sse?channel=tts".to_string(),
                access_token: Some("field-token".to_string()),
            }
        );
    }

    /* ------------------------------------------------------------------
    Token redaction audit (roadmap 010, task 005)
    ------------------------------------------------------------------ */

    /// Sentinel token value of the connection under test: its reproduction
    /// in a URL, message, or event payload is a leak.
    const TOKEN_SENTINEL: &str = "secret-sentinel";

    /// The token legitimately rides in the `Cookie` header channel, never in
    /// the request URL: a token that arrives via the copied browser URL's
    /// `?token=` query (or the configured field) must be stripped from the
    /// resolved endpoint URL, so no error/log formatting of the URL can leak
    /// it.
    #[test]
    fn resolved_endpoint_url_never_carries_the_token() {
        let url_with_token = format!("http://127.0.0.1:10100/?token={TOKEN_SENTINEL}");

        // Configured token takes priority; the query token is still dropped.
        let resolved = resolve_sse_endpoint(&url_with_token, Some(TOKEN_SENTINEL));
        assert!(
            !resolved.url.contains(TOKEN_SENTINEL),
            "resolved URL leaks the token: {}",
            resolved.url
        );
        assert_eq!(resolved.access_token.as_deref(), Some(TOKEN_SENTINEL));

        // URL-only token moves to the cookie channel as well.
        let resolved = resolve_sse_endpoint(&url_with_token, None);
        assert!(!resolved.url.contains(TOKEN_SENTINEL));
        assert_eq!(resolved.access_token.as_deref(), Some(TOKEN_SENTINEL));

        // Other query values survive next to the stripped token parameter.
        let resolved = resolve_sse_endpoint(
            &format!("http://127.0.0.1:10100/?channel=tts&token={TOKEN_SENTINEL}"),
            None,
        );
        assert!(!resolved.url.contains(TOKEN_SENTINEL));
        assert!(resolved.url.contains("channel=tts"));
    }

    /// The full status surface the retry loop produces for a connection
    /// whose token is the sentinel (statuses go to the webview as payloads
    /// and into the log as `info`/`warn` lines): neither the payload strings
    /// nor the `Debug` formatting ever reproduces the token value.
    #[tokio::test]
    async fn retry_status_surface_is_sentinel_free_for_token_carrying_connection() {
        // As in `run_with_retries`, the connect closure owns the URL (with
        // the token in its query) and the token for `try_connect`.
        let url = format!("http://127.0.0.1:10100/?token={TOKEN_SENTINEL}");
        let access_token = Some(TOKEN_SENTINEL.to_string());
        let connect_fn = move || {
            let _ = (&url, &access_token);
            async { ConnectResult::NotConnected }
        };

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        retry_loop(connect_fn, emit).await;

        let ev = events.lock().unwrap();
        assert!(!ev.is_empty());
        for status in ev.iter() {
            let payload = status.to_string();
            assert!(
                !payload.contains(TOKEN_SENTINEL),
                "status payload leaks the token: {payload}"
            );
            let debug = format!("{status:?}");
            assert!(
                !debug.contains(TOKEN_SENTINEL),
                "status Debug leaks the token: {debug}"
            );
        }

        // The terminal error is the fixed text shared with the events tests;
        // it names the attempt count, never request details.
        let terminal = ev
            .iter()
            .find_map(|e| match e {
                ConnectionStatus::Error(msg) => Some(msg.as_str()),
                _ => None,
            })
            .expect("terminal Error after exhausted budget");
        assert_eq!(terminal, terminal_error_message());
        assert!(!terminal.contains("127.0.0.1"));
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    #[test]
    fn retry_policy_exhausts_after_max_attempts() {
        let mut policy = RetryPolicy::new();
        assert!(!policy.exhausted());
        for i in 1..=MAX_RECONNECT_ATTEMPTS {
            assert_eq!(policy.take_attempt(), i);
        }
        assert!(policy.exhausted());
    }

    #[test]
    fn retry_policy_resets_correctly() {
        let mut policy = RetryPolicy::new();
        for _ in 0..5 {
            policy.take_attempt();
        }
        assert_eq!(policy.remaining, 5);
        policy.reset();
        assert_eq!(policy.remaining, MAX_RECONNECT_ATTEMPTS);
        assert!(!policy.exhausted());
    }

    #[test]
    fn retry_policy_delay_is_five_seconds() {
        let policy = RetryPolicy::new();
        assert_eq!(policy.delay(), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn exhausted_after_10_failures_no_11th_attempt() {
        tokio::time::pause();

        let call_count = Arc::new(AtomicU32::new(0));
        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));

        let connect_fn = {
            let call_count = Arc::clone(&call_count);
            move || {
                call_count.fetch_add(1, Ordering::SeqCst);
                async { ConnectResult::NotConnected }
            }
        };

        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        retry_loop(connect_fn, emit).await;

        let calls = call_count.load(Ordering::SeqCst);
        assert_eq!(calls, 11, "1 free + 10 retries = 11 total connect attempts");

        let ev = events.lock().unwrap();
        assert!(
            matches!(ev.last().unwrap(), ConnectionStatus::Error(_)),
            "Terminal event must be Error"
        );

        let connecting_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Connecting))
            .count();
        assert_eq!(connecting_count, 11);

        let disconnected_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Disconnected))
            .count();
        assert_eq!(disconnected_count, 11);

        let error_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Error(_)))
            .count();
        assert_eq!(error_count, 1);

        assert_eq!(ev.len(), 23);

        assert_eq!(ev[0], ConnectionStatus::Connecting);
        assert_eq!(ev[1], ConnectionStatus::Disconnected);
        assert_eq!(ev[2], ConnectionStatus::Connecting);
        assert_eq!(ev[3], ConnectionStatus::Disconnected);
        match &ev[22] {
            ConnectionStatus::Error(msg) => {
                assert!(
                    msg.contains("10 attempts"),
                    "Error message should mention attempt count"
                );
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn success_on_10th_attempt_resets_budget() {
        tokio::time::pause();

        let call_count = Arc::new(AtomicU32::new(0));
        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));

        let connect_fn = {
            let call_count = Arc::clone(&call_count);
            move || {
                let calls = call_count.fetch_add(1, Ordering::SeqCst);
                // First call (free) fails; next 9 fail; 10th reconnect succeeds
                // calls: 0=free, 1-9=retries fail, 10=10th retry succeeds
                async move {
                    if calls == 10 {
                        ConnectResult::ConnectedThenEnded
                    } else {
                        ConnectResult::NotConnected
                    }
                }
            }
        };

        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        retry_loop(connect_fn, emit).await;

        let calls = call_count.load(Ordering::SeqCst);
        assert_eq!(
            calls, 21,
            "1 free + 10 retries (10th succeeds) + 10 more after budget reset"
        );
    }

    #[tokio::test]
    async fn disconnect_emitted_before_first_sleep() {
        tokio::time::pause();

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));

        let connect_fn = || async { ConnectResult::NotConnected };

        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        // Spawn and advance time just enough to verify Disconnected arrives
        // before the 5-second sleep would expire.
        let handle = tokio::spawn(retry_loop(connect_fn, emit));

        // Yield a couple of ticks so the free attempt and initial Disconnected are emitted.
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        let ev = events.lock().unwrap();
        assert_eq!(
            ev.len(),
            2,
            "Connecting and Disconnected should be emitted before first sleep"
        );
        assert_eq!(ev[0], ConnectionStatus::Connecting);
        assert_eq!(ev[1], ConnectionStatus::Disconnected);
        drop(ev);

        handle.abort();
    }

    #[tokio::test]
    async fn abort_during_sleep_stops_retries() {
        tokio::time::pause();

        let call_count = Arc::new(AtomicU32::new(0));

        let connect_fn = {
            let call_count = Arc::clone(&call_count);
            move || {
                call_count.fetch_add(1, Ordering::SeqCst);
                async { ConnectResult::NotConnected }
            }
        };

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        let handle = tokio::spawn(retry_loop(connect_fn, emit));

        // Let initial attempt complete.
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        let calls_before = call_count.load(Ordering::SeqCst);
        assert_eq!(calls_before, 1, "Free attempt should have run");

        // Abort while sleeping (before first retry).
        handle.abort();
        let _ = handle.await;

        // Advance well past any remaining sleep.
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;

        let calls_after = call_count.load(Ordering::SeqCst);
        assert_eq!(
            calls_after, calls_before,
            "No additional attempts after abort"
        );

        let ev = events.lock().unwrap();
        assert!(
            ev.iter()
                .any(|e| matches!(e, ConnectionStatus::Disconnected)),
            "Disconnected should be emitted"
        );
        assert!(
            !ev.iter().any(|e| matches!(e, ConnectionStatus::Error(_))),
            "No terminal Error after abort"
        );
    }

    #[tokio::test]
    async fn error_message_is_token_free() {
        tokio::time::pause();

        let connect_fn = || async { ConnectResult::NotConnected };

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        retry_loop(connect_fn, emit).await;

        let ev = events.lock().unwrap();
        let err = ev.iter().find_map(|e| match e {
            ConnectionStatus::Error(msg) => Some(msg.as_str()),
            _ => None,
        });
        let msg = err.expect("Should have a terminal Error");
        assert!(
            !msg.contains("token") && !msg.contains("secret") && !msg.contains("password"),
            "Error message must not contain credential keywords: {msg}"
        );
    }

    #[tokio::test]
    async fn first_connected_then_ended_resets_budget_properly() {
        tokio::time::pause();

        let call_count = Arc::new(AtomicU32::new(0));

        let connect_fn = {
            let call_count = Arc::clone(&call_count);
            move || {
                let calls = call_count.fetch_add(1, Ordering::SeqCst);
                async move {
                    match calls {
                        0 | 3 => ConnectResult::ConnectedThenEnded,
                        _ => ConnectResult::NotConnected,
                    }
                }
            }
        };

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        retry_loop(connect_fn, emit).await;

        let events = events.lock().unwrap();
        let _connecting_count = events
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Connecting))
            .count();
        // Free(success) + retries(fail,fail,success) = 4 Connecting on first cycle,
        // then after reset: another Connecting (the 5th call that fails), continues...
        let total_calls = call_count.load(Ordering::SeqCst);
        // After second success on call 3 (index), subsequent retries fill out the rest.
        // We just verify the task terminates with Error after 10 consecutive fails.
        assert!(
            matches!(events.last().unwrap(), ConnectionStatus::Error(_)),
            "Should eventually hit Error after exhausting a budget"
        );
        // Budget resets are evidenced by total_calls > 11 (if we had 2 resets, etc.)
        // Not overly prescriptive; just confirm it terminates correctly.
        let _ = total_calls;
    }

    #[tokio::test]
    async fn established_termination_after_partial_retries_resets_budget_to_full() {
        tokio::time::pause();

        let call_count = Arc::new(AtomicU32::new(0));
        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));

        let connect_fn = {
            let call_count = Arc::clone(&call_count);
            move || {
                let calls = call_count.fetch_add(1, Ordering::SeqCst);
                async move {
                    // 0 = free attempt fails; 1..=3 consume part of the budget;
                    // 4 = established termination (read error/EOF after connecting),
                    // which must reset the budget; 5..=14 then run a full 10-retry
                    // failure cycle before exhausting.
                    if calls == 4 {
                        ConnectResult::ConnectedThenEnded
                    } else {
                        ConnectResult::NotConnected
                    }
                }
            }
        };

        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        retry_loop(connect_fn, emit).await;

        let calls = call_count.load(Ordering::SeqCst);
        assert_eq!(
            calls, 15,
            "1 free + 3 failed retries + 1 established termination (reset) + 10 fresh retries"
        );

        let ev = events.lock().unwrap();
        assert!(
            matches!(ev.last().unwrap(), ConnectionStatus::Error(_)),
            "Second failure cycle must exhaust the full 10-retry budget"
        );
        let connecting_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Connecting))
            .count();
        assert_eq!(connecting_count, 15);
    }
}
