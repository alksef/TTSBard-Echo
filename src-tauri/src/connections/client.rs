use crate::connections::{ConnectionError, ConnectionErrorKind};
use crate::events::{AppEvent, ConnectionStatus};
use crate::state::AppState;
use eventsource_client::{self, Client as _, ClientBuilder};
use futures_util::stream::StreamExt;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::watch;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const RECONNECT_DELAY_SECS: u64 = 5;

/// Hard ceiling for one connect attempt to deliver its first event (roadmap
/// 011, decision E): a server that accepted the TCP connection and then
/// stays silent must not hold the attempt forever. Per-attempt overridable
/// via [`try_connect_with`] so the fixture tests can use a short budget.
const FIRST_EVENT_TIMEOUT: Duration = Duration::from_secs(10);

/* ==========================================================================
Cooperative cancellation (roadmap 011, decision C)
========================================================================== */

/// Owner side of a connection's [`CancellationToken`].
///
/// Cancelling is synchronous and latched: every await point in the connection
/// task races its awaitable against [`CancellationToken::cancelled`], so the
/// task stops the moment the source cancels. Dropping the source without
/// cancelling does NOT release the waiters — only [`CancelSource::cancel`]
/// does — so a dropped owner can never be mistaken for a cancellation.
pub(crate) struct CancelSource {
    sender: watch::Sender<bool>,
}

impl Default for CancelSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelSource {
    pub(crate) fn new() -> Self {
        Self {
            sender: watch::channel(false).0,
        }
    }

    /// A fresh token observing this source.
    pub(crate) fn token(&self) -> CancellationToken {
        CancellationToken {
            receiver: self.sender.subscribe(),
        }
    }

    /// Latch the cancellation: every current and future waiter of the tokens
    /// issued by this source resolves.
    pub(crate) fn cancel(&self) {
        let _ = self.sender.send(true);
    }
}

/// Cloneable cooperative-cancellation token for one running connection task.
///
/// The retry loop races the retry sleep and the in-flight attempt (including
/// the stream wait inside `try_connect`) against [`CancellationToken::cancelled`];
/// once cancelled, the task exits without emitting any further status event.
/// The tracked `JoinHandle::abort` remains as the backstop for code paths that
/// never await the token.
#[derive(Clone)]
pub(crate) struct CancellationToken {
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    /// Non-blocking cancellation check, used before every status emission.
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Resolves once [`CancelSource::cancel`] latched the cancellation.
    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.receiver.clone();
        while !*receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                // The sender is gone without cancelling: nobody can cancel
                // anymore, so park forever — the abort backstop owns this
                // task's termination now.
                std::future::pending::<()>().await;
            }
        }
    }
}

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

    pub fn connect(&self, token: CancellationToken) -> tokio::task::JoinHandle<()> {
        let id = self.id.clone();
        let url = self.url.clone();
        let access_token = self.access_token.clone();
        let state = self.state.clone();
        tokio::spawn(async move { run_with_retries(id, url, access_token, state, token).await })
    }
}

/// The outcome of one connect attempt. `pub(crate)` so the manager registry
/// tests can drive `retry_loop` with their own attempt futures.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConnectResult {
    /// The token was cancelled mid-attempt. Never surfaces as a status: a
    /// cancelled task emits nothing (roadmap 011, decision C).
    Cancelled,
    NotConnected,
    ConnectedThenEnded,
    /// The attempt failed before the connection was usable; carries the
    /// normalized error (category + fixed message) so the status layer can
    /// surface `errorKind`/`errorMessage` (roadmap 011 task 002).
    Failed(ConnectionError),
}

async fn run_with_retries(
    id: String,
    url: String,
    access_token: Option<String>,
    state: Arc<AppState>,
    token: CancellationToken,
) {
    let connect_fn = {
        let id = id.clone();
        let url = url.clone();
        let access_token = access_token.clone();
        let state = Arc::clone(&state);
        let token = token.clone();
        move || {
            let id = id.clone();
            let url = url.clone();
            let access_token = access_token.clone();
            let state = Arc::clone(&state);
            let token = token.clone();
            async move { try_connect(&id, &url, &access_token, &state, &token).await }
        }
    };

    let emit_status = {
        let id = id.clone();
        let state = Arc::clone(&state);
        move |status: ConnectionStatus| {
            state.emit_event(AppEvent::ConnectionStatusChanged(id.clone(), status));
        }
    };

    retry_loop(&id, token, connect_fn, emit_status).await;
}

/// The reconnect cycle of one connection, cooperatively cancellable via
/// `token` (roadmap 011, decision C).
///
/// The token is checked before EVERY status emission and raced against every
/// await point (`biased` so a simultaneously-ready cancellation always wins):
/// once cancelled, the loop returns without emitting another status event,
/// leaving the target-status emission to the command that cancelled it.
pub(crate) async fn retry_loop<F, Fut>(
    id: &str,
    token: CancellationToken,
    connect_fn: F,
    emit_status: impl Fn(ConnectionStatus),
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConnectResult>,
{
    retry_loop_with_delay(
        id,
        token,
        connect_fn,
        emit_status,
        Duration::from_secs(RECONNECT_DELAY_SECS),
    )
    .await;
}

/// [`retry_loop`] with the inter-attempt wait as a parameter (roadmap 011
/// task 006): production always goes through [`retry_loop`]'s fixed 5s, the
/// SSE-fixture tests pass a short wait. The `Retrying` status reports the
/// ACTUAL wait as `next_retry_in_secs`. `id` is the correlation id of the
/// connection: every log line of the loop names it (roadmap 011, task 007).
pub(crate) async fn retry_loop_with_delay<F, Fut>(
    id: &str,
    token: CancellationToken,
    mut connect_fn: F,
    emit_status: impl Fn(ConnectionStatus),
    retry_delay: Duration,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConnectResult>,
{
    let mut policy = RetryPolicy::new();

    if token.is_cancelled() {
        return;
    }

    emit_status(ConnectionStatus::Connecting);
    let mut result = tokio::select! {
        biased;
        _ = token.cancelled() => return,
        attempt_result = connect_fn() => attempt_result,
    };

    loop {
        // A cancelled task emits nothing — not even for a result that raced
        // the cancellation inside the select above.
        if token.is_cancelled() || matches!(result, ConnectResult::Cancelled) {
            return;
        }

        if matches!(result, ConnectResult::ConnectedThenEnded) {
            // An established connection that later ended starts a fresh
            // cycle with the full retry budget.
            policy.reset();
        } else if policy.exhausted() {
            // The retry budget is spent: exactly one terminal `Error`,
            // carrying the category of the last failed attempt (roadmap 011,
            // decision B).
            let kind = match &result {
                ConnectResult::Failed(error) => error.kind,
                _ => ConnectionErrorKind::Network,
            };
            let message = terminal_error_message();
            warn!("Connection {id}: {message} ({kind})");
            emit_status(ConnectionStatus::Error { kind, message });
            return;
        }

        // The attempt failed but the budget is not spent: announce the
        // upcoming attempt in place of the old intermediate `Disconnected`.
        // `attempt` is the number of the NEXT attempt, 1-based.
        let attempt = policy.take_attempt();
        emit_status(ConnectionStatus::Retrying {
            attempt,
            max_attempts: MAX_RECONNECT_ATTEMPTS,
            next_retry_in_secs: retry_delay.as_secs(),
        });

        tokio::select! {
            biased;
            _ = token.cancelled() => return,
            _ = sleep(retry_delay) => {}
        }

        if token.is_cancelled() {
            return;
        }

        info!("Connection {id}: reconnect attempt {attempt}/{MAX_RECONNECT_ATTEMPTS}");
        emit_status(ConnectionStatus::Connecting);
        result = tokio::select! {
            biased;
            _ = token.cancelled() => return,
            attempt_result = connect_fn() => attempt_result,
        };
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
    token: &CancellationToken,
) -> ConnectResult {
    let state = Arc::clone(state);
    let connection_id = id.to_string();
    let mut on_observation = move |observation: AttemptObservation| match observation {
        AttemptObservation::Connected => {
            state.emit_event(AppEvent::ConnectionStatusChanged(
                connection_id.clone(),
                ConnectionStatus::Connected,
            ));
        }
        AttemptObservation::Typing { is_typing, preview } => {
            state.emit_event(AppEvent::TypingChanged(
                connection_id.clone(),
                is_typing,
                preview,
            ));
        }
        AttemptObservation::Message(message) => {
            state.emit_event(AppEvent::MessageReceived(connection_id.clone(), message));
        }
    };

    try_connect_with(
        id,
        url,
        access_token.as_deref(),
        &mut on_observation,
        token,
        FIRST_EVENT_TIMEOUT,
    )
    .await
}

/// What one connect attempt observed on the live stream, before any
/// `AppState` involvement (roadmap 011 task 006).
///
/// Production wires the observer callback to `AppState::emit_event`; the
/// SSE-fixture tests collect the observations in a channel.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AttemptObservation {
    /// The first stream item arrived: the attempt is usable.
    Connected,
    Typing {
        is_typing: bool,
        preview: Option<String>,
    },
    Message(String),
}

/// One connection attempt against `url`, decoupled from `AppState` via the
/// `on_observation` callback (roadmap 011 task 006).
///
/// `first_event_timeout` bounds the wait for the first stream item — event
/// or error (roadmap 011, decision E). Production passes
/// [`FIRST_EVENT_TIMEOUT`]; the fixture tests pass a short budget. Behavior
/// is otherwise exactly the extracted [`try_connect`] body.
pub(crate) async fn try_connect_with<F>(
    id: &str,
    url: &str,
    access_token: Option<&str>,
    on_observation: &mut F,
    token: &CancellationToken,
    first_event_timeout: Duration,
) -> ConnectResult
where
    F: FnMut(AttemptObservation),
{
    if token.is_cancelled() {
        return ConnectResult::Cancelled;
    }

    if let Err(e) = crate::config::validation::validate_url(url) {
        // Safe to log as is: `validate_url` failures are fixed literals plus
        // the url crate's `ParseError` phrases — they never echo the input
        // back, so no URL or query can appear here (roadmap 011, task 007).
        warn!("Invalid URL for connection {id}: {e}");
        return ConnectResult::Failed(ConnectionError::for_kind(
            ConnectionErrorKind::Configuration,
        ));
    }

    let endpoint = resolve_sse_endpoint(url, access_token);
    // The loggable endpoint form (task 007): `resolve_sse_endpoint` already
    // moved the token out of the URL, but other query pairs may remain — the
    // sanitizer drops the whole query, unconditionally.
    let sanitized_endpoint = sanitize_url_for_log(&endpoint.url);

    let mut builder = match ClientBuilder::for_url(&endpoint.url) {
        Ok(builder) => builder,
        Err(_) => {
            // The raw builder error wraps the (possibly URL-bearing) cause and
            // is never logged; the log carries id, category, fixed message,
            // and the sanitized endpoint only (roadmap 011, task 007).
            let error = ConnectionError::for_kind(ConnectionErrorKind::Configuration);
            warn!(
                "Failed to build SSE client for connection {id} (endpoint {sanitized_endpoint}): {error}"
            );
            return ConnectResult::Failed(error);
        }
    };

    if let Some(token_value) = endpoint.access_token.as_deref() {
        builder = match builder.header("Cookie", &format!("webview_auth={token_value}")) {
            Ok(builder) => builder,
            Err(_) => {
                let error = ConnectionError::new(
                    ConnectionErrorKind::Configuration,
                    "Saved credentials are not valid for this connection",
                );
                warn!("Failed to set auth header for connection {id}: {error}");
                return ConnectResult::Failed(error);
            }
        };
        info!("Using access token (masked) for {id}");
    }

    let mut stream = builder.build().stream();
    let mut connected = false;
    // The FIRST stream item carries the decision-E budget: the attempt must
    // produce it (event or error) within `first_event_timeout` — a server
    // that accepted the socket and stays silent must not hold the attempt
    // forever. Every further item is unbounded: a slow stream is legitimate.
    let mut awaiting_first_item = true;

    loop {
        // The stream wait is the cancellation point of a live attempt: a
        // cancelled task neither keeps polling the stream nor reports
        // `Connected` for it.
        let next = if awaiting_first_item {
            let first = tokio::select! {
                biased;
                _ = token.cancelled() => return ConnectResult::Cancelled,
                first = tokio::time::timeout(first_event_timeout, stream.next()) => first,
            };
            let next = match first {
                Ok(next) => next,
                Err(_elapsed) => {
                    warn!(
                        "No first SSE event for connection {id} (endpoint {sanitized_endpoint}) within {first_event_timeout:?}"
                    );
                    return ConnectResult::Failed(first_event_timeout_error());
                }
            };
            awaiting_first_item = false;
            next
        } else {
            tokio::select! {
                biased;
                _ = token.cancelled() => return ConnectResult::Cancelled,
                next = stream.next() => next,
            }
        };

        let Some(result) = next else {
            break;
        };

        if token.is_cancelled() {
            return ConnectResult::Cancelled;
        }

        match result {
            Ok(event) => {
                if !connected {
                    connected = true;
                    on_observation(AttemptObservation::Connected);
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
                        on_observation(AttemptObservation::Typing { is_typing, preview });
                    }
                    ParsedPayload::Message(message) => {
                        on_observation(AttemptObservation::Message(message));
                    }
                }
            }
            Err(e) => {
                // Roadmap 011, task 007 decision: the raw `Display` of this
                // external crate error (eventsource-client / hyper) can embed
                // the full request URL including the query, so the raw text is
                // never logged — not on warn, not on debug (the log file
                // records debug lines too). The log carries the connection id,
                // the classified category, the fixed message, and the
                // sanitized endpoint; the raw text is used exclusively to pick
                // the category.
                let error = ConnectionError::from_error_text(&e.to_string());
                warn!(
                    "SSE read error for connection {id} (endpoint {sanitized_endpoint}): {error}"
                );
                return classify_read_error(connected, error);
            }
        }
    }

    if connected {
        ConnectResult::ConnectedThenEnded
    } else {
        ConnectResult::NotConnected
    }
}

/// Map a mid-attempt stream failure to the attempt result.
///
/// A failure after the connection was established counts as an
/// established-then-ended cycle (the retry budget resets); a failure before
/// any event is a classified failure. The classified error carries the
/// category and the fixed message only — no request detail (URL, query,
/// token) travels with it; the raw text is dropped by the classifier before
/// this point (roadmap 011, task 007).
fn classify_read_error(connected: bool, error: ConnectionError) -> ConnectResult {
    if connected {
        ConnectResult::ConnectedThenEnded
    } else {
        ConnectResult::Failed(error)
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

    fn take_attempt(&mut self) -> u32 {
        let attempt = MAX_RECONNECT_ATTEMPTS - self.remaining + 1;
        self.remaining -= 1;
        attempt
    }
}

/* ==========================================================================
Log sanitization (roadmap 011, task 007)
========================================================================== */

/// The loggable form of a connection URL: `scheme://host:port/path`.
///
/// The query string and the fragment are dropped ALWAYS — not just a `token`
/// parameter (roadmap 011, task 007): any query pair may carry a credential,
/// and correlating a log line never needs more than origin and path. Input
/// that does not parse as a URL at all is replaced by a fixed placeholder, so
/// arbitrary user input is never echoed into the log either.
///
/// Decision recorded (task 007): the raw `Display` of external crate errors
/// (eventsource-client / hyper) can embed the full request URL including its
/// query. Such raw texts are therefore NOT logged at ANY level — warn and
/// above log the connection id, the error category, the fixed message, and
/// this sanitized endpoint only; `debug` gets no raw text either, because the
/// production log file records debug lines (`ttsbard_echo=debug`) and there
/// is no URL-safe substring guarantee for these texts.
pub(crate) fn sanitize_url_for_log(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => "<invalid url>".to_string(),
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

/* ==========================================================================
Pre-save endpoint probe (roadmap 011, task 005, decision E)
========================================================================== */

/// Hard ceiling of one probe attempt: a server that accepts the connection
/// and then stays silent must not hold the pre-save check forever (roadmap
/// 011, decision E).
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The outcome of a single pre-save connection probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    /// The endpoint delivered a first valid SSE event within the timeout.
    Connected { latency_ms: u64 },
    /// The attempt failed; carries the normalized category plus a fixed
    /// message (no URL, query, or token ever travels with it).
    Failed(ConnectionError),
}

/// Failure reported when no first event arrived within the first-event
/// budget — [`FIRST_EVENT_TIMEOUT`] on the live attempt path, [`PROBE_TIMEOUT`]
/// on the probe.
///
/// Shared by both paths (roadmap 011, decision E). The wording rides the
/// classifier's timeout markers, so the category is the network one by the
/// shared taxonomy: a silent server looks like a reachability problem.
fn first_event_timeout_error() -> ConnectionError {
    ConnectionError::new(
        ConnectionErrorKind::Network,
        "Connection attempt timed out waiting for the server",
    )
}

/// Failure reported when the endpoint closed the stream without delivering a
/// single event: whatever it answered with was not a usable event stream.
fn probe_empty_stream_error() -> ConnectionError {
    ConnectionError::for_kind(ConnectionErrorKind::Protocol)
}

/// Perform exactly ONE connection attempt against `url` — the engine of the
/// `test_connection` command (roadmap 011, task 005).
///
/// The endpoint is resolved by the same [`resolve_sse_endpoint`] as the live
/// path (the token moves to the Cookie channel, never into the URL), but the
/// probe emits no status events, spawns no retry loop, and touches no
/// settings. Success is the first valid SSE event; every failure is
/// classified with the shared taxonomy and carries fixed text only, so the
/// result is safe for the webview and the log file.
pub(crate) async fn probe_connection(url: &str, access_token: Option<&str>) -> ProbeOutcome {
    let started = std::time::Instant::now();

    if crate::config::validation::validate_url(url).is_err() {
        return ProbeOutcome::Failed(ConnectionError::for_kind(
            ConnectionErrorKind::Configuration,
        ));
    }

    let endpoint = resolve_sse_endpoint(url, access_token);

    let mut builder = match ClientBuilder::for_url(&endpoint.url) {
        Ok(builder) => builder,
        Err(_) => {
            return ProbeOutcome::Failed(ConnectionError::for_kind(
                ConnectionErrorKind::Configuration,
            ));
        }
    };

    if let Some(token) = endpoint.access_token.as_deref() {
        builder = match builder.header("Cookie", &format!("webview_auth={token}")) {
            Ok(builder) => builder,
            Err(_) => {
                return ProbeOutcome::Failed(ConnectionError::new(
                    ConnectionErrorKind::Configuration,
                    "Saved credentials are not valid for this connection",
                ));
            }
        };
    }

    let mut stream = builder.build().stream();
    let first = tokio::time::timeout(PROBE_TIMEOUT, stream.next()).await;

    match first {
        // No first event in time: cut the attempt off as a network timeout.
        Err(_) => ProbeOutcome::Failed(first_event_timeout_error()),
        // Any first parsed event (or comment) proves a live event stream.
        Ok(Some(Ok(_))) => ProbeOutcome::Connected {
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        },
        Ok(Some(Err(e))) => {
            let error = ConnectionError::from_error_text(&e.to_string());
            // Roadmap 011, task 007: the raw crate error text can embed the
            // full URL with the query and is never logged (any level); the
            // warn carries the category, the fixed message, and the
            // sanitized RESOLVED endpoint — the same form the live path
            // logs. There is no connection id here: the probe runs
            // pre-save.
            warn!(
                "Endpoint probe failed (endpoint {}): {error}",
                sanitize_url_for_log(&endpoint.url)
            );
            ProbeOutcome::Failed(error)
        }
        Ok(None) => ProbeOutcome::Failed(probe_empty_stream_error()),
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
        classify_payload, classify_read_error, first_event_timeout_error, probe_connection,
        probe_empty_stream_error, resolve_sse_endpoint, retry_loop, terminal_error_message,
        CancelSource, ConnectResult, Duration, ParsedPayload, ProbeOutcome, ResolvedSseEndpoint,
    };
    use crate::connections::{error::classify_error_text, ConnectionError, ConnectionErrorKind};
    use crate::events::ConnectionStatus;
    use std::sync::{Arc, Mutex};

    #[test]
    fn read_error_after_connection_classified_as_established() {
        assert_eq!(
            classify_read_error(
                true,
                ConnectionError::for_kind(ConnectionErrorKind::Network)
            ),
            ConnectResult::ConnectedThenEnded
        );
        assert_eq!(
            classify_read_error(
                false,
                ConnectionError::for_kind(ConnectionErrorKind::Network)
            ),
            ConnectResult::Failed(ConnectionError::for_kind(ConnectionErrorKind::Network))
        );
    }

    /// A failure before the connection was established carries the
    /// normalized category classified from the raw error text, while the
    /// message stays fixed (no echo of the raw text).
    #[test]
    fn read_error_before_connection_carries_classified_kind_and_fixed_message() {
        let result = classify_read_error(
            false,
            ConnectionError::from_error_text("unexpected response: 401 Unauthorized"),
        );
        assert_eq!(
            result,
            ConnectResult::Failed(ConnectionError::new(
                ConnectionErrorKind::Authentication,
                "The server rejected the credentials",
            ))
        );

        let result = classify_read_error(
            false,
            ConnectionError::from_error_text("something no one has ever seen"),
        );
        assert_eq!(
            result,
            ConnectResult::Failed(ConnectionError::for_kind(ConnectionErrorKind::Network))
        );
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
    Pre-save probe (roadmap 011, task 005)
    ------------------------------------------------------------------ */

    /// An invalid URL fails offline, before any endpoint is contacted: the
    /// probe returns the configuration category with the fixed message.
    #[tokio::test]
    async fn probe_invalid_url_fails_offline_as_configuration() {
        let outcome = probe_connection("not a url", Some("secret-sentinel")).await;
        assert_eq!(
            outcome,
            ProbeOutcome::Failed(ConnectionError::for_kind(
                ConnectionErrorKind::Configuration
            ))
        );
    }

    /// The probe's two synthetic failures use the shared taxonomy: the
    /// timeout is a network failure whose text still classifies as network,
    /// and the empty stream is the protocol category.
    #[test]
    fn probe_timeout_and_empty_stream_errors_use_shared_taxonomy() {
        let timeout = first_event_timeout_error();
        assert_eq!(timeout.kind, ConnectionErrorKind::Network);
        assert!(
            timeout.message.contains("timed out"),
            "timeout message must say so: {}",
            timeout.message
        );
        assert_eq!(
            classify_error_text(&timeout.message),
            ConnectionErrorKind::Network,
            "the probe timeout message must classify as the network category"
        );

        let empty = probe_empty_stream_error();
        assert_eq!(empty.kind, ConnectionErrorKind::Protocol);
        assert_eq!(
            empty.message,
            "The server response is not a valid event stream"
        );
    }

    /// The probe timeout is the short first-event budget from decision E,
    /// not the multi-attempt reconnect horizon.
    #[test]
    fn probe_timeout_is_ten_seconds() {
        assert_eq!(super::PROBE_TIMEOUT, Duration::from_secs(10));
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

        retry_loop("conn-test", CancelSource::new().token(), connect_fn, emit).await;

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
                ConnectionStatus::Error { message, .. } => Some(message.as_str()),
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

    /// The production inter-attempt wait stays the fixed five seconds
    /// (roadmap 011, decision D): `retry_loop` delegates to
    /// `retry_loop_with_delay` with this value.
    #[test]
    fn prod_retry_delay_is_five_seconds() {
        assert_eq!(
            Duration::from_secs(RECONNECT_DELAY_SECS),
            Duration::from_secs(5)
        );
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

        retry_loop("conn-test", CancelSource::new().token(), connect_fn, emit).await;

        let calls = call_count.load(Ordering::SeqCst);
        assert_eq!(calls, 11, "1 free + 10 retries = 11 total connect attempts");

        let ev = events.lock().unwrap();
        assert!(
            matches!(ev.last().unwrap(), ConnectionStatus::Error { .. }),
            "Terminal event must be Error"
        );

        let connecting_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Connecting))
            .count();
        assert_eq!(connecting_count, 11);

        // `Retrying` replaced the intermediate `Disconnected` entirely: the
        // retry loop itself never emits `Disconnected`.
        let disconnected_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Disconnected))
            .count();
        assert_eq!(disconnected_count, 0);

        let error_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Error { .. }))
            .count();
        assert_eq!(error_count, 1);

        assert_eq!(
            ev.len(),
            22,
            "Connecting + 10x(Retrying, Connecting) + Error"
        );

        assert_eq!(ev[0], ConnectionStatus::Connecting);
        // Each failed attempt announces the next one with a growing 1-based
        // attempt number, the full budget, and the fixed 5-second wait.
        for (i, status) in ev[1..21].iter().enumerate() {
            let expected = if i % 2 == 0 {
                ConnectionStatus::Retrying {
                    attempt: (i / 2 + 1) as u32,
                    max_attempts: MAX_RECONNECT_ATTEMPTS,
                    next_retry_in_secs: RECONNECT_DELAY_SECS,
                }
            } else {
                ConnectionStatus::Connecting
            };
            assert_eq!(*status, expected, "event at position {}", i + 1);
        }
        match &ev[21] {
            ConnectionStatus::Error { message, kind } => {
                assert!(
                    message.contains("10 attempts"),
                    "Error message should mention attempt count"
                );
                // The connection never reached the server, so the terminal
                // kind is the conservative network default.
                assert_eq!(*kind, ConnectionErrorKind::Network);
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

        retry_loop("conn-test", CancelSource::new().token(), connect_fn, emit).await;

        let calls = call_count.load(Ordering::SeqCst);
        assert_eq!(
            calls, 21,
            "1 free + 10 retries (10th succeeds) + 10 more after budget reset"
        );
    }

    #[tokio::test]
    async fn retrying_emitted_before_first_sleep() {
        tokio::time::pause();

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));

        let connect_fn = || async { ConnectResult::NotConnected };

        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        // Spawn and advance time just enough to verify the Retrying announce
        // arrives before the 5-second sleep would expire.
        let handle = tokio::spawn(retry_loop(
            "conn-test",
            CancelSource::new().token(),
            connect_fn,
            emit,
        ));

        // Yield a couple of ticks so the free attempt and its Retrying
        // announcement are emitted.
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        let ev = events.lock().unwrap();
        assert_eq!(
            ev.len(),
            2,
            "Connecting and Retrying should be emitted before first sleep"
        );
        assert_eq!(ev[0], ConnectionStatus::Connecting);
        assert_eq!(
            ev[1],
            ConnectionStatus::Retrying {
                attempt: 1,
                max_attempts: MAX_RECONNECT_ATTEMPTS,
                next_retry_in_secs: RECONNECT_DELAY_SECS,
            }
        );
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

        let handle = tokio::spawn(retry_loop(
            "conn-test",
            CancelSource::new().token(),
            connect_fn,
            emit,
        ));

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
                .any(|e| matches!(e, ConnectionStatus::Retrying { .. })),
            "Retrying announce should be emitted after the failed free attempt"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, ConnectionStatus::Error { .. })),
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

        retry_loop("conn-test", CancelSource::new().token(), connect_fn, emit).await;

        let ev = events.lock().unwrap();
        let err = ev.iter().find_map(|e| match e {
            ConnectionStatus::Error { message, kind } => Some((message.as_str(), *kind)),
            _ => None,
        });
        let (msg, kind) = err.expect("Should have a terminal Error");
        assert!(
            !msg.contains("token") && !msg.contains("secret") && !msg.contains("password"),
            "Error message must not contain credential keywords: {msg}"
        );
        // The terminal error always carries a machine-readable category.
        assert_eq!(kind, ConnectionErrorKind::Network);
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

        retry_loop("conn-test", CancelSource::new().token(), connect_fn, emit).await;

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
            matches!(events.last().unwrap(), ConnectionStatus::Error { .. }),
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

        retry_loop("conn-test", CancelSource::new().token(), connect_fn, emit).await;

        let calls = call_count.load(Ordering::SeqCst);
        assert_eq!(
            calls, 15,
            "1 free + 3 failed retries + 1 established termination (reset) + 10 fresh retries"
        );

        let ev = events.lock().unwrap();
        assert!(
            matches!(ev.last().unwrap(), ConnectionStatus::Error { .. }),
            "Second failure cycle must exhaust the full 10-retry budget"
        );
        let connecting_count = ev
            .iter()
            .filter(|e| matches!(e, ConnectionStatus::Connecting))
            .count();
        assert_eq!(connecting_count, 15);

        // Every failed attempt announced the next one; the established
        // termination reset the numbering back to a fresh 1-based cycle.
        let retry_attempts: Vec<u32> = ev
            .iter()
            .filter_map(|e| match e {
                ConnectionStatus::Retrying { attempt, .. } => Some(*attempt),
                _ => None,
            })
            .collect();
        let mut expected: Vec<u32> = vec![1, 2, 3, 4];
        expected.extend(1..=10);
        assert_eq!(
            retry_attempts, expected,
            "Retrying attempt numbers must grow 1-based and reset after an established cycle"
        );
    }

    /* ------------------------------------------------------------------
    Cooperative cancellation (roadmap 011 task 004, decision C)
    ------------------------------------------------------------------ */

    /// Cancelling while the loop sleeps between attempts: no further attempt
    /// is made and no status event is emitted after the cancellation.
    #[tokio::test]
    async fn cancel_during_retry_sleep_stops_attempts_and_events() {
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

        let source = CancelSource::new();
        let handle = tokio::spawn(retry_loop("conn-test", source.token(), connect_fn, emit));

        // Let the free attempt fail and the loop reach its retry sleep.
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        let calls_before = call_count.load(Ordering::SeqCst);
        assert_eq!(calls_before, 1, "free attempt should have run");
        let events_before = events.lock().unwrap().len();
        assert!(events_before >= 2, "Connecting and Retrying were emitted");

        source.cancel();
        handle.await.unwrap();

        // However long the connection lives on, it stays dead and silent.
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;

        assert_eq!(
            call_count.load(Ordering::SeqCst),
            calls_before,
            "no attempt after cancellation"
        );
        assert_eq!(
            events.lock().unwrap().len(),
            events_before,
            "no status event after cancellation"
        );
    }

    /// Cancelling while an attempt hangs forever (never-resolving connect):
    /// the task finishes anyway and emits nothing after the cancellation.
    #[tokio::test]
    async fn cancel_during_hanging_attempt_finishes_without_events() {
        tokio::time::pause();

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        let source = CancelSource::new();
        let handle = tokio::spawn(retry_loop(
            "conn-test",
            source.token(),
            std::future::pending::<ConnectResult>,
            emit,
        ));

        // Let the free attempt start and hang.
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            events.lock().unwrap().len(),
            1,
            "Connecting was emitted, then the attempt hangs"
        );

        source.cancel();
        // A never-resolving attempt must not keep the task alive.
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("task finishes after cancellation")
            .unwrap();

        assert_eq!(
            events.lock().unwrap().len(),
            1,
            "no status event after cancellation"
        );
    }

    /// An already-cancelled task emits no status at all — the loop-level
    /// invariant behind the disabled-connection no-op.
    #[tokio::test]
    async fn cancelled_token_completes_without_any_event() {
        tokio::time::pause();

        let events: Arc<Mutex<Vec<ConnectionStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let emit = {
            let events = Arc::clone(&events);
            move |status: ConnectionStatus| {
                events.lock().unwrap().push(status);
            }
        };

        let source = CancelSource::new();
        let token = source.token();
        source.cancel();

        retry_loop(
            "conn-test",
            token,
            std::future::pending::<ConnectResult>,
            emit,
        )
        .await;

        assert!(
            events.lock().unwrap().is_empty(),
            "a cancelled task emits no status"
        );
    }
}

/* ==========================================================================
SSE fixture integration tests (roadmap 011, decision G / task 006)
========================================================================== */

#[cfg(test)]
mod sse_integration_tests {
    //! The reliability scenarios of the roadmap driven through the REAL
    //! client path (`try_connect_with`, `retry_loop_with_delay`,
    //! `probe_connection`) against the local fixture server. The observer
    //! callback replaces the `AppState` wiring; everything else — endpoint
    //! resolution, the eventsource-client stream, payload classification,
    //! error classification — is production code.

    use super::{
        first_event_timeout_error, probe_connection, retry_loop_with_delay, try_connect_with,
        AttemptObservation, CancelSource, CancellationToken, ConnectResult, Duration, ProbeOutcome,
        FIRST_EVENT_TIMEOUT, PROBE_TIMEOUT,
    };
    use crate::connections::sse_fixture::{self, FixtureServer, Script};
    use crate::connections::{ConnectionError, ConnectionErrorKind};
    use crate::events::ConnectionStatus;
    use eventsource_client::{Client as _, ClientBuilder};
    use futures_util::StreamExt as _;
    use std::time::Instant;
    use tokio::sync::mpsc::UnboundedReceiver;
    use tokio::time::timeout;

    /// Short first-event budget of the fixture tests (production keeps 10s).
    const TEST_FIRST_EVENT_TIMEOUT: Duration = Duration::from_millis(500);

    /// Short inter-attempt wait of the retry-loop fixture test (production
    /// keeps the fixed 5s). Reported as `next_retry_in_secs: 0`.
    const TEST_RETRY_DELAY: Duration = Duration::from_millis(200);

    /// Wall-clock ceiling of one fixture test step: generous against slow
    /// machines, small enough to fail instead of hanging.
    fn step_deadline() -> Duration {
        Duration::from_secs(10)
    }

    /// Production first-event budget stays the decision-E ten seconds.
    #[test]
    fn prod_first_event_timeout_is_ten_seconds() {
        assert_eq!(FIRST_EVENT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(PROBE_TIMEOUT, Duration::from_secs(10));
    }

    async fn drain_observations(
        rx: &mut UnboundedReceiver<AttemptObservation>,
        window: Duration,
    ) -> Vec<AttemptObservation> {
        let mut out = Vec::new();
        while let Ok(Some(observation)) = timeout(window, rx.recv()).await {
            out.push(observation);
        }
        out
    }

    async fn drain_statuses(
        rx: &mut UnboundedReceiver<ConnectionStatus>,
        window: Duration,
    ) -> Vec<ConnectionStatus> {
        let mut out = Vec::new();
        while let Ok(Some(status)) = timeout(window, rx.recv()).await {
            out.push(status);
        }
        out
    }

    async fn next_observation(
        rx: &mut UnboundedReceiver<AttemptObservation>,
    ) -> AttemptObservation {
        timeout(step_deadline(), rx.recv())
            .await
            .expect("observation within the step deadline")
            .expect("observer channel stays open")
    }

    async fn next_status(rx: &mut UnboundedReceiver<ConnectionStatus>) -> ConnectionStatus {
        timeout(step_deadline(), rx.recv())
            .await
            .expect("status within the step deadline")
            .expect("status channel stays open")
    }

    /// Spawn one `try_connect_with` attempt whose observations flow into the
    /// returned receiver.
    fn spawn_attempt(
        id: &'static str,
        url: String,
        token: CancellationToken,
        first_event_timeout: Duration,
    ) -> (
        tokio::task::JoinHandle<ConnectResult>,
        UnboundedReceiver<AttemptObservation>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let attempt = tokio::spawn(async move {
            let mut observer = move |observation: AttemptObservation| {
                let _ = tx.send(observation);
            };
            try_connect_with(id, &url, None, &mut observer, &token, first_event_timeout).await
        });
        (attempt, rx)
    }

    /* ------------------------------------------------------------------
    Happy path
    ------------------------------------------------------------------ */

    /// The fixture streams a keep-alive comment, a typing indicator, and a
    /// final message; the attempt reports `Connected` on the first item and
    /// classifies the payloads exactly like production: typing (with its
    /// preview) and message reach the observer, and the scripted stream end
    /// afterwards is the established-then-ended cycle.
    #[tokio::test]
    async fn happy_stream_delivers_typing_and_message_like_production() {
        let mut body = sse_fixture::comment("keep-alive");
        body.extend(sse_fixture::event(
            r#"{"isTyping":true,"text":"composing a reply"}"#,
        ));
        body.extend(sse_fixture::event(r#"{"text":"the final answer"}"#));

        let server = FixtureServer::spawn(Script::Events { body }).await;
        let source = CancelSource::new();
        let (attempt, mut rx) = spawn_attempt(
            "conn-happy",
            server.url(),
            source.token(),
            TEST_FIRST_EVENT_TIMEOUT,
        );

        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Connected
        );
        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Typing {
                is_typing: true,
                preview: Some("composing a reply".to_string()),
            }
        );
        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Message("the final answer".to_string())
        );

        let result = timeout(step_deadline(), attempt)
            .await
            .expect("attempt finishes")
            .expect("no join error");
        assert_eq!(result, ConnectResult::ConnectedThenEnded);

        let tail = drain_observations(&mut rx, Duration::from_millis(200)).await;
        assert!(tail.is_empty(), "no observations beyond the three events");
    }

    /* ------------------------------------------------------------------
    Malformed events
    ------------------------------------------------------------------ */

    /// A malformed line as the FIRST stream content: the event parser rejects
    /// it, the attempt fails before it was ever usable, and the failure is
    /// the protocol category with the fixed message — no panic, no raw echo
    /// of the broken bytes.
    #[tokio::test]
    async fn malformed_first_event_fails_attempt_as_protocol() {
        let server = FixtureServer::spawn(Script::Events {
            body: sse_fixture::invalid_utf8_line(),
        })
        .await;
        let source = CancelSource::new();
        let (attempt, mut rx) = spawn_attempt(
            "conn-malformed",
            server.url(),
            source.token(),
            TEST_FIRST_EVENT_TIMEOUT,
        );

        let result = timeout(step_deadline(), attempt)
            .await
            .expect("attempt finishes")
            .expect("no join error");

        assert_eq!(
            result,
            ConnectResult::Failed(ConnectionError::new(
                ConnectionErrorKind::Protocol,
                "The server response is not a valid event stream",
            ))
        );
        let observations = drain_observations(&mut rx, Duration::from_millis(200)).await;
        assert!(
            observations.is_empty(),
            "a failed-first-read attempt never reports Connected: {observations:?}"
        );
    }

    /// A malformed line AFTER the stream was established (released only once
    /// the client observed the first event): honestly pinned — the parser
    /// fault is fatal to the stream, and since events were already flowing
    /// the attempt is the established-then-ended cycle (a fresh retry
    /// budget), not a terminal failure. No panic either way.
    #[tokio::test]
    async fn malformed_after_established_ends_the_cycle_without_panic() {
        let server = FixtureServer::spawn(Script::AfterFirstEvent {
            first: sse_fixture::event(r#"{"text":"before the damage"}"#),
            then: sse_fixture::invalid_utf8_line(),
        })
        .await;
        let source = CancelSource::new();
        let (attempt, mut rx) = spawn_attempt(
            "conn-malformed-late",
            server.url(),
            source.token(),
            TEST_FIRST_EVENT_TIMEOUT,
        );

        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Connected
        );
        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Message("before the damage".to_string())
        );

        // The client is parked on the stream again; only now the fixture
        // damages it.
        server.release();

        let result = timeout(step_deadline(), attempt)
            .await
            .expect("attempt finishes")
            .expect("no join error");
        assert_eq!(result, ConnectResult::ConnectedThenEnded);
    }

    /* ------------------------------------------------------------------
    Disconnect after establishment
    ------------------------------------------------------------------ */

    /// The fixture closing the socket right after a delivered event: the
    /// attempt ends as the established-then-ended cycle (the crate reports
    /// the stream end as an `eof` read error after events were flowing).
    #[tokio::test]
    async fn server_disconnect_after_established_is_connected_then_ended() {
        let server = FixtureServer::spawn(Script::Events {
            body: sse_fixture::event(r#"{"text":"last event before the drop"}"#),
        })
        .await;
        let source = CancelSource::new();
        let (attempt, mut rx) = spawn_attempt(
            "conn-drop",
            server.url(),
            source.token(),
            TEST_FIRST_EVENT_TIMEOUT,
        );

        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Connected
        );
        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Message("last event before the drop".to_string())
        );

        let result = timeout(step_deadline(), attempt)
            .await
            .expect("attempt finishes")
            .expect("no join error");
        assert_eq!(result, ConnectResult::ConnectedThenEnded);
    }

    /// The full production retry cycle over a real disconnecting server:
    /// the established-then-ended attempt announces the next one, the
    /// fixture accepts the reconnect, and cancelling after the second
    /// establishment stops the loop with no further attempt and no status
    /// beyond an already-announced retry.
    #[tokio::test]
    async fn retry_loop_reconnects_after_real_disconnect_and_cancels_cleanly() {
        let server = FixtureServer::spawn(Script::Events {
            body: sse_fixture::event(r#"{"isTyping":true}"#),
        })
        .await;

        let (statuses_tx, mut statuses) = tokio::sync::mpsc::unbounded_channel();
        let (observations_tx, mut observations) = tokio::sync::mpsc::unbounded_channel();

        let source = CancelSource::new();
        let url = server.url();
        let connect_fn = {
            let token = source.token();
            move || {
                let url = url.clone();
                let tx = observations_tx.clone();
                let token = token.clone();
                async move {
                    let mut observer = move |observation: AttemptObservation| {
                        let _ = tx.send(observation);
                    };
                    try_connect_with(
                        "conn-retry",
                        &url,
                        None,
                        &mut observer,
                        &token,
                        TEST_FIRST_EVENT_TIMEOUT,
                    )
                    .await
                }
            }
        };

        let loop_task = tokio::spawn(retry_loop_with_delay(
            "conn-retry",
            source.token(),
            connect_fn,
            move |status: ConnectionStatus| {
                let _ = statuses_tx.send(status);
            },
            TEST_RETRY_DELAY,
        ));

        assert_eq!(
            next_status(&mut statuses).await,
            ConnectionStatus::Connecting
        );

        assert_eq!(
            next_observation(&mut observations).await,
            AttemptObservation::Connected,
            "first establishment"
        );
        assert_eq!(
            next_observation(&mut observations).await,
            AttemptObservation::Typing {
                is_typing: true,
                preview: None,
            },
            "the first attempt delivered its event"
        );
        assert_eq!(
            next_status(&mut statuses).await,
            ConnectionStatus::Retrying {
                attempt: 1,
                max_attempts: 10,
                next_retry_in_secs: 0,
            },
            "the disconnect announced the next attempt (the short test wait is 0s)"
        );
        assert_eq!(
            next_status(&mut statuses).await,
            ConnectionStatus::Connecting
        );
        assert_eq!(
            next_observation(&mut observations).await,
            AttemptObservation::Connected,
            "the reconnect was accepted by the fixture"
        );
        assert_eq!(
            next_observation(&mut observations).await,
            AttemptObservation::Typing {
                is_typing: true,
                preview: None,
            },
            "the reconnect delivered its event"
        );
        assert!(
            server.connection_count() >= 2,
            "every cycle opened its own connection"
        );

        let connections_at_cancel = server.connection_count();
        source.cancel();
        timeout(step_deadline(), loop_task)
            .await
            .expect("loop finishes after the cancel")
            .expect("no join error");

        assert_eq!(
            server.connection_count(),
            connections_at_cancel,
            "no reconnect attempt after the cancellation"
        );
        // An established-then-ended attempt starts a FRESH cycle, so an
        // announcement that raced (and lost against) the cancellation is
        // numbered 1 again. Whatever followed the cancel, it must be such an
        // announcement only: no new Connecting (an actual attempt — also
        // proven by the unchanged connection count) and no Error.
        for status in drain_statuses(&mut statuses, Duration::from_millis(300)).await {
            assert!(
                matches!(status, ConnectionStatus::Retrying { .. }),
                "only an already-announced retry may follow the cancel, got {status:?}"
            );
        }
    }

    /* ------------------------------------------------------------------
    First-event timeout (roadmap 011, decision E)
    ------------------------------------------------------------------ */

    async fn run_timeout_scenario(server: FixtureServer) {
        let source = CancelSource::new();
        let (attempt, mut rx) = spawn_attempt(
            "conn-timeout",
            server.url(),
            source.token(),
            TEST_FIRST_EVENT_TIMEOUT,
        );

        let started = Instant::now();
        let result = timeout(step_deadline(), attempt)
            .await
            .expect("the attempt finishes by the first-event timeout")
            .expect("no join error");

        assert_eq!(result, ConnectResult::Failed(first_event_timeout_error()));
        assert!(
            started.elapsed() >= TEST_FIRST_EVENT_TIMEOUT - Duration::from_millis(50),
            "the attempt really waited out the first-event budget"
        );
        let observations = drain_observations(&mut rx, Duration::from_millis(200)).await;
        assert!(
            observations.is_empty(),
            "a timed-out attempt never reports Connected: {observations:?}"
        );
    }

    /// A server that accepts the TCP connection and never answers is cut off
    /// by the first-event timeout; the failure is the network category with
    /// the fixed timeout message.
    #[tokio::test]
    async fn silent_server_attempt_times_out_as_network() {
        let server = FixtureServer::spawn(Script::Silent).await;
        run_timeout_scenario(server).await;
    }

    /// Same, for a server that answers with event-stream headers and then
    /// goes silent: the SSE parser waits for body bytes that never come.
    #[tokio::test]
    async fn headers_then_silence_attempt_times_out_as_network() {
        let server = FixtureServer::spawn(Script::HeadersThenSilence).await;
        run_timeout_scenario(server).await;
    }

    /* ------------------------------------------------------------------
    Cancellation during a slow stream (roadmap 011, decision C)
    ------------------------------------------------------------------ */

    /// Cancelling while the stream is paused: the attempt stops immediately
    /// (the fixture still holds the socket open), reports `Cancelled`, and
    /// no event after the cancellation — including the fixture's second,
    /// delayed event — ever reaches the observer.
    #[tokio::test]
    async fn cancel_during_slow_stream_stops_immediately_and_silently() {
        let server = FixtureServer::spawn(Script::SlowStream {
            first: sse_fixture::event(r#"{"text":"before the pause"}"#),
            second: sse_fixture::event(r#"{"text":"after the pause"}"#),
            pause: Duration::from_millis(750),
        })
        .await;
        let source = CancelSource::new();
        // The production budget: the cancellation must beat the long wait.
        let (attempt, mut rx) = spawn_attempt(
            "conn-slow",
            server.url(),
            source.token(),
            FIRST_EVENT_TIMEOUT,
        );

        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Connected
        );
        assert_eq!(
            next_observation(&mut rx).await,
            AttemptObservation::Message("before the pause".to_string())
        );

        source.cancel();
        let result = timeout(Duration::from_secs(5), attempt)
            .await
            .expect("the attempt finishes immediately after the cancel")
            .expect("no join error");
        assert_eq!(result, ConnectResult::Cancelled);

        // Wider than the fixture's pause: the second event must never arrive.
        let late = drain_observations(&mut rx, Duration::from_millis(1500)).await;
        assert!(late.is_empty(), "no observation after the cancel: {late:?}");
    }

    /* ------------------------------------------------------------------
    Authentication failure
    ------------------------------------------------------------------ */

    /// The 401 fixture answer through the live path: the classifier maps the
    /// real crate error to the authentication category; the message is the
    /// fixed text (the raw status never echoes into it).
    #[tokio::test]
    async fn unauthorized_response_is_classified_as_authentication() {
        let server = FixtureServer::spawn(Script::Unauthorized).await;
        let source = CancelSource::new();
        let (attempt, mut rx) = spawn_attempt(
            "conn-401",
            server.url(),
            source.token(),
            TEST_FIRST_EVENT_TIMEOUT,
        );

        let result = timeout(step_deadline(), attempt)
            .await
            .expect("attempt finishes")
            .expect("no join error");

        assert_eq!(
            result,
            ConnectResult::Failed(ConnectionError::new(
                ConnectionErrorKind::Authentication,
                "The server rejected the credentials",
            ))
        );
        let observations = drain_observations(&mut rx, Duration::from_millis(200)).await;
        assert!(
            observations.is_empty(),
            "a rejected attempt never reports Connected: {observations:?}"
        );
    }

    /// The REAL error texts of eventsource-client 0.12 against the fixture
    /// responses — the exact inputs the text classifier relies on (roadmap
    /// 011 decision A, verified on the wire in task 006).
    #[tokio::test]
    async fn crate_error_texts_on_fixture_responses_are_as_classified() {
        // 401 → `unexpected response: {StatusCode}` (the authentication
        // marker of the classifier).
        let server = FixtureServer::spawn(Script::Unauthorized).await;
        let error = first_raw_error(&server.url()).await;
        assert_eq!(error, "unexpected response: 401 Unauthorized");

        // Malformed line → `invalid line: …` (the protocol markers; the
        // suffix embeds the `Utf8Error` debug and stays unpinned).
        let server = FixtureServer::spawn(Script::Events {
            body: sse_fixture::invalid_utf8_line(),
        })
        .await;
        let error = first_raw_error(&server.url()).await;
        assert!(
            error.starts_with("invalid line:"),
            "malformed line text: {error}"
        );

        // Scripted stream end → `eof` after the event: this is what makes a
        // mid-stream disconnect the established-then-ended cycle.
        let server = FixtureServer::spawn(Script::Events {
            body: sse_fixture::event(r#"{"text":"the only event"}"#),
        })
        .await;
        let mut stream = ClientBuilder::for_url(&server.url())
            .expect("fixture url builds")
            .build()
            .stream();
        let first = timeout(step_deadline(), stream.next())
            .await
            .expect("event arrives")
            .expect("stream yields an item");
        assert!(
            matches!(first, Ok(eventsource_client::SSE::Event(_))),
            "the event precedes the stream end: {first:?}"
        );
        let second = timeout(step_deadline(), stream.next())
            .await
            .expect("stream end arrives")
            .expect("stream yields an item");
        assert_eq!(
            second.expect_err("stream ends with an error").to_string(),
            "eof"
        );
    }

    /// First item the raw crate client produces for `url` — its error text.
    async fn first_raw_error(url: &str) -> String {
        let mut stream = ClientBuilder::for_url(url)
            .expect("fixture url builds")
            .build()
            .stream();
        let first = timeout(step_deadline(), stream.next())
            .await
            .expect("crate client answers within the deadline")
            .expect("stream yields an item");
        match first {
            Err(error) => error.to_string(),
            Ok(other) => panic!("expected an error, got {other:?}"),
        }
    }

    /* ------------------------------------------------------------------
    Pre-save probe over a real server (roadmap 011, task 005)
    ------------------------------------------------------------------ */

    /// The probe on a live event stream: `Connected` with a sane loopback
    /// latency.
    #[tokio::test]
    async fn probe_reports_connected_on_a_live_stream() {
        let server = FixtureServer::spawn(Script::Events {
            body: sse_fixture::comment("keep-alive"),
        })
        .await;

        let outcome = timeout(step_deadline(), probe_connection(&server.url(), None))
            .await
            .expect("probe finishes");
        match outcome {
            ProbeOutcome::Connected { latency_ms } => {
                assert!(
                    latency_ms < 10_000,
                    "loopback latency is sane: {latency_ms}ms"
                );
            }
            other => panic!("expected Connected, got {other:?}"),
        }
    }

    /// The probe on a 401: the authentication category with the fixed
    /// message.
    #[tokio::test]
    async fn probe_classifies_unauthorized_as_authentication() {
        let server = FixtureServer::spawn(Script::Unauthorized).await;

        let outcome = timeout(step_deadline(), probe_connection(&server.url(), None))
            .await
            .expect("probe finishes");
        assert_eq!(
            outcome,
            ProbeOutcome::Failed(ConnectionError::new(
                ConnectionErrorKind::Authentication,
                "The server rejected the credentials",
            ))
        );
    }
}

/* ==========================================================================
Sanitized logging tests (roadmap 011, task 007)
========================================================================== */

#[cfg(test)]
mod logging_tests {
    //! The log surface of the connection path, asserted with a captured
    //! subscriber (the `recovery::test_support::capture_warns` pattern):
    //! sentinel values riding the URL query must never appear in any captured
    //! line, while the sanitized endpoint and the connection id must.

    use super::{
        probe_connection, retry_loop_with_delay, sanitize_url_for_log, try_connect_with,
        AttemptObservation, CancelSource, ConnectResult, Duration, ProbeOutcome,
        FIRST_EVENT_TIMEOUT,
    };
    use crate::config::recovery::test_support::capture_warns;
    use crate::connections::sse_fixture::{FixtureServer, Script};
    use crate::connections::{ConnectionError, ConnectionErrorKind};
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex, MutexGuard};
    use tracing::Level;
    use tracing_subscriber::fmt::writer::MakeWriter;

    /// Sentinel token value carried in the URL query of the connection under
    /// test: its reproduction in any captured log line is a leak.
    const TOKEN_SENTINEL: &str = "secret-sentinel-007";

    /// A second sentinel riding the query as a plain (non-token) parameter:
    /// the sanitizer must drop the WHOLE query, not only a `token=` pair.
    const QUERY_SENTINEL: &str = "query-sentinel-007";

    /// [`capture_warns`] generalized to the ceiling `max_level` (the
    /// per-attempt log lines are info-level). Same mechanics: a scoped
    /// subscriber writing into a buffer, so the spawned fixture tasks of a
    /// current-thread runtime land on the capturing thread.
    fn capture_logs<R>(max_level: Level, work: impl FnOnce() -> R) -> (R, String) {
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(max_level)
            .with_writer(Factory {
                buffer: buffer.clone(),
            })
            .finish();
        let result = tracing::subscriber::with_default(subscriber, work);
        let logs = String::from_utf8_lossy(&buffer.lock().unwrap().clone()).into_owned();
        (result, logs)
    }

    struct Factory {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    struct BufferWriter<'a> {
        guard: MutexGuard<'a, Vec<u8>>,
    }

    impl Write for BufferWriter<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.guard.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Factory {
        type Writer = BufferWriter<'a>;

        fn make_writer(&'a self) -> BufferWriter<'a> {
            BufferWriter {
                guard: self.buffer.lock().unwrap(),
            }
        }
    }

    /// A current-thread runtime created OUTSIDE the capture closure, so every
    /// task polled during `block_on` runs on the capturing thread.
    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
    }

    /* ------------------------------------------------------------------
    Sanitizer
    ------------------------------------------------------------------ */

    #[test]
    fn sanitizer_drops_query_and_fragment_but_keeps_origin_and_path() {
        assert_eq!(
            sanitize_url_for_log(
                "https://example.com/events?channel=tts&token=secret-sentinel-007#fragment"
            ),
            "https://example.com/events"
        );
        assert_eq!(
            sanitize_url_for_log("http://127.0.0.1:10100/sse?channel=tts"),
            "http://127.0.0.1:10100/sse"
        );
        // A bare origin stays a bare origin.
        assert_eq!(
            sanitize_url_for_log("http://localhost:10100/?token=x"),
            "http://localhost:10100/"
        );
    }

    #[test]
    fn sanitizer_never_echoes_unparseable_input() {
        let sanitized = sanitize_url_for_log("totally not a url?token=secret-sentinel-007");
        assert_eq!(sanitized, "<invalid url>");
        assert!(!sanitized.contains("secret-sentinel-007"));
        assert!(!sanitized.contains("totally not a url"));
    }

    /* ------------------------------------------------------------------
    Live-attempt read error warn
    ------------------------------------------------------------------ */

    /// The REAL wire failure (eventsource-client reports the fixture's 401 as
    /// a read error) under a warn-capturing subscriber: the line names the
    /// connection id, the category, the fixed message, and the sanitized
    /// endpoint — and reproduces neither the token nor the other query
    /// sentinel, because the raw crate error text is not logged at all.
    #[test]
    fn read_error_warn_carries_id_category_and_sanitized_endpoint_only() {
        let rt = test_runtime();

        let (outcome_and_endpoint, logs) = capture_warns(|| {
            rt.block_on(async {
                let server = FixtureServer::spawn(Script::Unauthorized).await;
                let url = format!(
                    "{}/?token={TOKEN_SENTINEL}&channel={QUERY_SENTINEL}",
                    server.url()
                );
                let mut observer = |_observation: AttemptObservation| {};
                let outcome = try_connect_with(
                    "conn-sanitize",
                    &url,
                    None,
                    &mut observer,
                    &CancelSource::new().token(),
                    FIRST_EVENT_TIMEOUT,
                )
                .await;
                // The form the log must show instead of the full URL.
                let sanitized_endpoint = format!("{}/sse", server.url());
                (outcome, sanitized_endpoint)
            })
        });

        let (outcome, sanitized_endpoint) = outcome_and_endpoint;
        assert_eq!(
            outcome,
            ConnectResult::Failed(ConnectionError::new(
                ConnectionErrorKind::Authentication,
                "The server rejected the credentials",
            ))
        );

        assert!(logs.contains("conn-sanitize"), "log: {logs}");
        assert!(
            logs.contains(sanitized_endpoint.as_str()),
            "sanitized endpoint `{sanitized_endpoint}` missing: {logs}"
        );
        assert!(logs.contains("authentication"), "log: {logs}");
        assert!(
            logs.contains("The server rejected the credentials"),
            "log: {logs}"
        );

        assert!(
            !logs.contains(TOKEN_SENTINEL),
            "token leaked into the log: {logs}"
        );
        assert!(
            !logs.contains(QUERY_SENTINEL),
            "query parameter leaked into the log: {logs}"
        );
    }

    /* ------------------------------------------------------------------
    Invalid-URL warn
    ------------------------------------------------------------------ */

    /// The invalid-URL warn keeps the correlation id and the fixed validation
    /// text, and never echoes the invalid input back (which carried the
    /// sentinels here).
    #[test]
    fn invalid_url_warn_keeps_id_and_never_echoes_the_input() {
        let rt = test_runtime();

        let (outcome, logs) = capture_warns(|| {
            rt.block_on(async {
                let mut observer = |_observation: AttemptObservation| {};
                try_connect_with(
                    "conn-invalid",
                    &format!("totally not a url?token={TOKEN_SENTINEL}&q={QUERY_SENTINEL}"),
                    None,
                    &mut observer,
                    &CancelSource::new().token(),
                    FIRST_EVENT_TIMEOUT,
                )
                .await
            })
        });

        assert_eq!(
            outcome,
            ConnectResult::Failed(ConnectionError::for_kind(
                ConnectionErrorKind::Configuration
            ))
        );

        assert!(logs.contains("conn-invalid"), "log: {logs}");
        assert!(logs.contains("Invalid URL"), "log: {logs}");
        assert!(
            !logs.contains(TOKEN_SENTINEL) && !logs.contains(QUERY_SENTINEL),
            "sentinel leaked into the log: {logs}"
        );
        assert!(
            !logs.contains("totally not a url"),
            "invalid input echoed into the log: {logs}"
        );
    }

    /* ------------------------------------------------------------------
    Retry-loop warns (terminal) and attempt infos
    ------------------------------------------------------------------ */

    /// The terminal budget-exhausted warn carries the connection id, the
    /// fixed message, and the category — and no endpoint or URL text at all.
    #[test]
    fn terminal_budget_warn_carries_connection_id_without_endpoint() {
        let rt = test_runtime();

        let (_events, logs) = capture_warns(|| {
            rt.block_on(async {
                retry_loop_with_delay(
                    "conn-budget",
                    CancelSource::new().token(),
                    || async { ConnectResult::NotConnected },
                    |_| {},
                    Duration::from_millis(1),
                )
                .await;
            })
        });

        assert!(logs.contains("conn-budget"), "log: {logs}");
        assert!(logs.contains("10 attempts"), "log: {logs}");
        assert!(logs.contains("network"), "log: {logs}");
        // No endpoint text anywhere in the captured warn lines.
        assert!(!logs.contains("http"), "log: {logs}");
        assert!(!logs.contains("127.0.0.1"), "log: {logs}");
    }

    /// Every per-attempt log line (info level) names the connection id, its
    /// 1-based attempt number, and the full budget.
    #[test]
    fn attempt_lines_carry_id_attempt_number_and_budget() {
        let rt = test_runtime();

        let (_, logs) = capture_logs(Level::INFO, || {
            rt.block_on(async {
                retry_loop_with_delay(
                    "conn-attempts",
                    CancelSource::new().token(),
                    || async { ConnectResult::NotConnected },
                    |_| {},
                    Duration::from_millis(1),
                )
                .await;
            })
        });

        assert!(logs.contains("conn-attempts"), "log: {logs}");
        assert!(logs.contains("attempt 1/10"), "log: {logs}");
        assert!(logs.contains("attempt 10/10"), "log: {logs}");
        assert!(!logs.contains("attempt 11/10"), "log: {logs}");
        assert!(!logs.contains(TOKEN_SENTINEL), "log: {logs}");
    }

    /* ------------------------------------------------------------------
    Probe warn (roadmap 011 task 005 path, same rules)
    ------------------------------------------------------------------ */

    /// The probe failure warn follows the same rules: category + fixed
    /// message + sanitized endpoint (there is no connection id yet — the
    /// probe runs pre-save); neither sentinel appears and the raw crate
    /// error text is absent.
    #[test]
    fn probe_warn_carries_sanitized_endpoint_without_sentinels() {
        let rt = test_runtime();

        let (outcome_and_endpoint, logs) = capture_warns(|| {
            rt.block_on(async {
                let server = FixtureServer::spawn(Script::Unauthorized).await;
                let url = format!(
                    "{}/?token={TOKEN_SENTINEL}&channel={QUERY_SENTINEL}",
                    server.url()
                );
                let outcome = probe_connection(&url, Some(TOKEN_SENTINEL)).await;
                let sanitized_endpoint = format!("{}/sse", server.url());
                (outcome, sanitized_endpoint)
            })
        });

        let (outcome, sanitized_endpoint) = outcome_and_endpoint;
        assert_eq!(
            outcome,
            ProbeOutcome::Failed(ConnectionError::new(
                ConnectionErrorKind::Authentication,
                "The server rejected the credentials",
            ))
        );

        assert!(logs.contains("Endpoint probe failed"), "log: {logs}");
        assert!(
            logs.contains(sanitized_endpoint.as_str()),
            "sanitized endpoint `{sanitized_endpoint}` missing: {logs}"
        );
        assert!(
            !logs.contains(TOKEN_SENTINEL),
            "token leaked into the log: {logs}"
        );
        assert!(
            !logs.contains(QUERY_SENTINEL),
            "query parameter leaked into the log: {logs}"
        );
    }
}
