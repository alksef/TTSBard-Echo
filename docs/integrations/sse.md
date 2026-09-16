# SSE connection contract

Each configured connection is an HTTP(S) endpoint consumed as an SSE stream. The optional access token is sent as the configured authentication credential by the connection client; it is persisted in settings but excluded from UI runtime snapshots.

The backend owns the lifecycle: `Disconnected` → `Connecting` → `Connected`.
A failed attempt enters `Retrying` with the upcoming attempt number, retry budget,
and delay. An exhausted or non-retryable failure enters `Error` with a stable
machine-readable category and sanitized message. Connect/disconnect commands
start or abort the managed task. Reconnection and keepalive handling are
implemented by the client task; the UI receives status/message events and
reloads the authoritative snapshot after mutations.

Connection events are `connection-status-changed`, `message-received`,
`message-cleared`, `typing-changed`, `connections-changed`, `connection-added`,
and `connection-removed`. Window and settings invalidation events are documented
with their payload shapes in the [events reference](../reference/events.md).

Only HTTP(S) URLs with a non-empty host are accepted. Do not put tokens in logs, screenshots, issue reports, or documentation.
