# Decision 0024 — Roadmap 010 access token storage scheme

- **Status:** accepted
- **Date:** 2026-09-14
- **Scope:** roadmap 010 (settings integrity and secrets), task 004

## Context

`settings.json` is overwritten in place (`std::fs::write` in
`SettingsManager::save`), and `ConnectionConfig.access_token: Option<String>` is
stored in it in plaintext. Roadmap 010 (P0) requires that access tokens are not
kept in plaintext, and explicitly requires the storage format to be fixed by an
ADR before the migration is implemented.

Two candidate schemes were considered:

1. **DPAPI** (`CryptProtectData`/`CryptUnprotectData`) — encrypt the token value
   directly in `settings.json`.
2. **Windows Credential Manager** — keep the token in the system credential
   vault and leave only a reference/identifier in `settings.json`.

The settings DTOs (`get_connections`, `get_all_app_settings`) pass the token to
the webview because the connection edit form needs it; that channel must keep
working without changing the frontend contract.

## Decision

1. **Storage: DPAPI, encrypted value directly in `settings.json`.** The token
   value is encrypted with DPAPI (`CryptProtectData`/`CryptUnprotectData`) in the
   current user's context; the encrypted value, base64-encoded, is stored at the
   same `access_token` position in `settings.json`. The alternative
   «Windows Credential Manager + reference in JSON» is rejected: two independent
   stores cause synchronization drift and dangling references (the credential
   deleted from the vault while the file is kept, or the file moved to another
   profile/machine while the credential stays behind), and it adds an extra
   dependency. A single self-contained file keeps backup/copy/restore of the
   settings moving the token together with them, with no external state.
2. **The settings DTO is the only legitimate token channel to the webview.**
   `get_connections` and `get_all_app_settings` continue to return the
   decrypted token for the edit form; the frontend contract (shape,
   `src/types/settings.ts`, `scripts/contract-checker.js`) is unchanged.
   Runtime snapshots, events, errors, and logs carry no token — the existing
   rule is preserved (`ConnectionRuntimeSnapshotDto` has no token field;
   `error_message_is_token_free`).
3. **Non-Windows platforms: no encryption.** The token stays in the JSON in
   plaintext, as at baseline — the application is Windows-first. All
   cryptographic code sits behind `cfg(windows)`, and the Linux CI job must stay
   green. A file encrypted on Windows and opened under another user profile or
   environment, where the value cannot be decrypted, is read as «token
   unavailable»: no panic, a clear error, and the token is re-entered through
   the form.
4. **`schema_version` is a backend file-format detail.** It is stored in
   `settings.json` for version tracking and the one-time plaintext→encrypted
   migration; it does not appear in frontend DTOs.

## Consequences

- After the one-time migration (task 004) no plaintext token remains in
  `settings.json`; updating, removing, and re-adding a token stays predictable
  through the same edit form (roadmap 010 acceptance).
- The token is bound to the Windows user profile: moving the file to another
  machine/profile makes the token unreadable (re-enter through the form); no
  other part of the settings is affected.
- The frontend contract is unchanged: no changes to `get_connections`,
  `get_all_app_settings`, `src/types/settings.ts`, or
  `scripts/contract-checker.js`.
- Non-Windows builds contain no cryptographic code; the Linux CI job stays
  green.
- Task 004 (schema version + DPAPI + one-time migration) must match this ADR
  exactly; a divergence between the ADR and the code is a defect.
