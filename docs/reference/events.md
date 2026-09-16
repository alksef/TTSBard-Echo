# Events

Event names are defined in `src-tauri/src/events.rs`; payload serialization is
owned by `src-tauri/src/event_loop.rs`.

- `connection-status-changed`: `{ id, status }`; `Retrying` additionally carries
  `attempt`, `maxAttempts`, and `nextRetryInSecs`, while `Error` carries
  `errorKind` and `errorMessage`.
- `message-received`: tuple `[connection_id, message]`.
- `message-cleared`, `connection-added`, and `connection-removed`: connection ID.
- `typing-changed`: `{ id, isTyping, previewText? }`.
- `floating-visibility-changed`: `{ visible }`.
- `theme-changed`: theme string; `clickthrough-changed`: boolean.
- `connections-changed`, `floating-appearance-changed`, `settings-changed`,
  `logging-changed`, `hotkeys-changed`, and `general-changed`: empty invalidation
  payloads.
