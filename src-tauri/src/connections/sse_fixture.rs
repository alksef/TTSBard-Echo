//! Test-only SSE fixture server (roadmap 011, decision G).
//!
//! A minimal HTTP/1.1 + Server-Sent-Events server on a plain
//! `tokio::net::TcpListener` — raw protocol bytes, no new dependencies.
//! Every accepted TCP connection is served the same [`Script`], so reconnect
//! tests see a consistent server. Tests bind the fixture on `127.0.0.1:0`
//! and drive the production client path against `http://127.0.0.1:<port>`.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// Response head of a successful event-stream fixture (body follows until
/// EOF, like a real SSE producer).
const SSE_RESPONSE_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\n\
    Content-Type: text/event-stream\r\n\
    Cache-Control: no-cache\r\n\
    \r\n";

/// Authentication-failure response with a small body.
const UNAUTHORIZED_RESPONSE: &[u8] = b"HTTP/1.1 401 Unauthorized\r\n\
    Content-Type: text/plain\r\n\
    Content-Length: 12\r\n\
    \r\n\
    Unauthorized";

/// The scripted response served to every accepted connection.
#[derive(Clone)]
pub(crate) enum Script {
    /// `200 + text/event-stream`, the raw SSE `body`, then the socket is
    /// closed: the client observes the events and then the stream end.
    Events { body: Vec<u8> },
    /// Accept the TCP connection and never answer: no response bytes at all.
    Silent,
    /// `200 + text/event-stream` headers, then no body bytes: the SSE parser
    /// waits for data that never comes.
    HeadersThenSilence,
    /// `401` with a body, then the socket is closed.
    Unauthorized,
    /// Slow stream: `200 + text/event-stream` + `first`, then a `pause`
    /// (the scenario itself, not a test-side sleep), then `second`, then the
    /// socket is held open until the peer hangs up.
    SlowStream {
        first: Vec<u8>,
        second: Vec<u8>,
        pause: Duration,
    },
    /// `200 + text/event-stream` + `first`; then the fixture waits for
    /// [`FixtureServer::release`] from the test; only then `then` is written
    /// and the socket is held open. The trigger makes "more bytes after an
    /// event the client already observed" deterministic (event-driven, no
    /// sleeps).
    AfterFirstEvent { first: Vec<u8>, then: Vec<u8> },
}

/// A running fixture server: `127.0.0.1` on an ephemeral port, serving
/// `script` to every connection until dropped with the test's runtime.
pub(crate) struct FixtureServer {
    addr: SocketAddr,
    release: UnboundedSender<()>,
    connections: Arc<AtomicUsize>,
}

impl FixtureServer {
    /// Bind on `127.0.0.1:0` and serve `script` to every connection.
    pub(crate) async fn spawn(script: Script) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture binds on 127.0.0.1:0");
        let addr = listener.local_addr().expect("fixture local address");

        let (release, release_rx) = unbounded_channel();
        let release_slot = Arc::new(tokio::sync::Mutex::new(Some(release_rx)));
        let connections = Arc::new(AtomicUsize::new(0));

        let served_connections = Arc::clone(&connections);
        let served_release_slot = Arc::clone(&release_slot);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                served_connections.fetch_add(1, Ordering::SeqCst);
                // Only the first connection can consume the release trigger.
                let trigger = served_release_slot.lock().await.take();
                let script = script.clone();
                tokio::spawn(serve_connection(script, stream, trigger));
            }
        });

        Self {
            addr,
            release,
            connections,
        }
    }

    /// Base URL of the fixture: `http://127.0.0.1:<port>` (no path — the
    /// production resolver appends `/sse`, which the fixture ignores).
    pub(crate) fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Release a [`Script::AfterFirstEvent`] connection: the fixture writes
    /// its `then` bytes.
    pub(crate) fn release(&self) {
        let _ = self.release.send(());
    }

    /// TCP connections accepted so far.
    pub(crate) fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

async fn serve_connection(
    script: Script,
    mut stream: TcpStream,
    release: Option<UnboundedReceiver<()>>,
) {
    match script {
        Script::Silent => {
            // Hold the socket open without answering; exit when the peer
            // hangs up.
            hold_until_peer_quits(&mut stream).await;
        }
        Script::HeadersThenSilence => {
            if !drain_request_head(&mut stream).await {
                return;
            }
            if stream.write_all(SSE_RESPONSE_HEAD).await.is_err() {
                return;
            }
            hold_until_peer_quits(&mut stream).await;
        }
        Script::Events { body } => {
            if !drain_request_head(&mut stream).await {
                return;
            }
            if stream.write_all(SSE_RESPONSE_HEAD).await.is_err() {
                return;
            }
            if stream.write_all(&body).await.is_err() {
                return;
            }
            // The scripted end of the stream: the client observes its events
            // and then the stream end (an `eof` read error).
            let _ = stream.shutdown().await;
        }
        Script::Unauthorized => {
            if !drain_request_head(&mut stream).await {
                return;
            }
            let _ = stream.write_all(UNAUTHORIZED_RESPONSE).await;
            let _ = stream.shutdown().await;
        }
        Script::SlowStream {
            first,
            second,
            pause,
        } => {
            if !drain_request_head(&mut stream).await {
                return;
            }
            if stream.write_all(SSE_RESPONSE_HEAD).await.is_err() {
                return;
            }
            if stream.write_all(&first).await.is_err() {
                return;
            }
            tokio::time::sleep(pause).await;
            let _ = stream.write_all(&second).await;
            hold_until_peer_quits(&mut stream).await;
        }
        Script::AfterFirstEvent { first, then } => {
            if !drain_request_head(&mut stream).await {
                return;
            }
            if stream.write_all(SSE_RESPONSE_HEAD).await.is_err() {
                return;
            }
            if stream.write_all(&first).await.is_err() {
                return;
            }
            match release {
                // The test observed the first event and released the trigger.
                Some(mut release) => {
                    let _ = release.recv().await;
                }
                // No trigger left (the server was dropped or this is a
                // repeated connection): hold the socket, answer nothing.
                None => hold_until_peer_quits(&mut stream).await,
            }
            let _ = stream.write_all(&then).await;
            hold_until_peer_quits(&mut stream).await;
        }
    }
}

/// Keep the socket open, discarding whatever the peer sends, until the peer
/// hangs up.
async fn hold_until_peer_quits(stream: &mut TcpStream) {
    let mut buf = [0u8; 512];
    while matches!(stream.read(&mut buf).await, Ok(n) if n > 0) {}
}

/// Read and discard the client's request head (everything up to the blank
/// line). Returns `false` if the peer quit first.
async fn drain_request_head(stream: &mut TcpStream) -> bool {
    let mut seen = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let Ok(n) = stream.read(&mut buf).await else {
            return false;
        };
        if n == 0 {
            return false;
        }
        seen.extend_from_slice(&buf[..n]);
        if seen.windows(4).any(|window| window == b"\r\n\r\n") {
            return true;
        }
    }
}

/// One SSE event frame carrying `data` (default `message` event type).
pub(crate) fn event(data: &str) -> Vec<u8> {
    format!("data: {data}\n\n").into_bytes()
}

/// One SSE comment frame (a keep-alive line).
pub(crate) fn comment(text: &str) -> Vec<u8> {
    format!(": {text}\n\n").into_bytes()
}

/// An SSE data line with invalid UTF-8 payload: the event parser must reject
/// it as a malformed line.
pub(crate) fn invalid_utf8_line() -> Vec<u8> {
    b"data: broken \xff\xfe bytes\n\n".to_vec()
}
