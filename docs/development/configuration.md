# Configuration

Settings are stored under the platform config directory in the `ttsbard-echo` application folder: `settings.json` (connections and app settings) and `windows.json` (main/floating positions, floating opacity, background color, click-through). Connection settings include endpoint/name/enabled state and an optional access token. `general.hide_on_minimize` defaults to `false`; when enabled, minimizing the main window also hides it from the taskbar.

Defaults and serde behavior are defined in `src-tauri/src/config/` and parity is checked by `npm run check:settings`.

Secrets are never part of runtime snapshots or presentation cards.

## Atomic writes and backups

Every configuration file write goes through `write_atomic`
(`src-tauri/src/config/atomic.rs`) — the single write point for all config
files. The sequence is:

1. The new content is written to a temporary file (`<name>.<pid>.<n>.tmp`) in
   the same directory (same volume, so the final rename is atomic), flushed
   and synced to disk.
2. If the target exists, its current content is copied to `<name>.bak`
   (exactly one previous version is kept).
3. The temporary file is renamed over the target.

A failure at any point leaves the existing target untouched and removes the
temporary file.

## Corruption recovery

Config files are loaded through `load_with_recovery`
(`src-tauri/src/config/recovery.rs`); startup never depends on the validity of
a file on disk:

- Missing file → defaults (the caller writes the initial file).
- Readable and parseable file → the parsed value.
- Unreadable or unparseable file → the corrupted file is quarantined as
  `<name>.corrupt-<timestamp>` next to the original (never deleted, kept for
  analysis), the previous version from `<name>.bak` is restored when it is
  valid, otherwise defaults are used.

The recovered state is rewritten to the main file name so the on-disk state
matches the in-memory state. Diagnostics are `warn`-level, name the file and
the recovery applied, and never include file content.

## Schema version and migration

The on-disk `settings.json` envelope (`StoredSettings`) carries a
`schema_version`; it is a backend file-format detail and does not appear in
frontend DTOs.

- `v1` — the v0.1.0 format: no `schema_version` field, plaintext token.
- `v2` — the current format (written by all saves since roadmap 010).

A file without the field is treated as `v1`. The first load of a `v1` file
runs a one-time, idempotent migration: tokens are encrypted (see below) and
the file is rewritten as `v2`. Migration happens at load; subsequent loads of
a `v2` file are pass-through.

## Access token storage

The scheme is fixed by
[ADR-0024](../decisions/0024-roadmap-010-secret-storage.md).

- **Windows:** the token is encrypted with DPAPI
  (`CryptProtectData`/`CryptUnprotectData`) in the current user's context.
  The base64-encoded value is stored at the same `access_token` position in
  `settings.json` — the file stays self-contained, no external store.
- **Non-Windows:** no encryption; the token is stored in the JSON in
  plaintext, as at baseline. The application is Windows-first; all DPAPI code
  sits behind `cfg(windows)`.
- An empty token is stored as an empty string on every platform.

A stored value that cannot be decrypted (the file was moved to another
Windows profile or machine, or the blob is corrupted) is treated as «token
unavailable»: no panic, a fixed content-free error, and the token is
re-entered through the connection edit form. Every decryption failure
collapses into the same message, so no part of the stored value can leak.

The settings DTOs (`get_connections`, `get_all_app_settings`) are the only
legitimate token channel to the webview (they feed the edit form). The
`Debug`/`Display` rendering of `ConnectionConfig` masks the token as
`[masked]`, and sentinel tests cover snapshots, events, errors, and logs.

## File permissions on Windows

On Windows every configuration artifact (`settings.json`, `windows.json`,
their `.bak` backups, `.tmp` temporaries, `*.corrupt-*` quarantine copies)
carries a protected DACL granting access only to the current user plus
`SYSTEM` and the local Administrators group (`src-tauri/src/config/acl.rs`).
The DACL is written protected against inheritance, so broad inherited ACEs
("Everyone", "Authenticated Users", "Users") are discarded and cannot reach
the file contents.

The restriction is applied:

- at write time in `atomic.rs` — to the temporary file before the rename and
  to the `.bak` copy right after it is made; a failure there aborts the write
  atomically;
- at load/recovery time in `recovery.rs` — for files created earlier with
  broad rights and for quarantined copies; there a failure is logged and
  never blocks startup.

On other platforms this is a no-op.

## Content Security Policy

`src-tauri/tauri.conf.json` defines the webview CSP:

- Production (`csp`):
  `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'`
- Development (`devCsp`): the same plus `connect-src 'self' ws://localhost:5173`
  for the Vite dev server.

Remote fonts (Google Fonts `@import`) were removed from the frontend;
typography uses system fallbacks (Segoe UI / Cascadia Code). Self-hosting
Manrope / JetBrains Mono is a separate follow-up.
