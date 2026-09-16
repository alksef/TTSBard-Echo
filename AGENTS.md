# Repository map

Echo is a Windows-first Tauri application with a Vue 3 frontend and Rust
backend. Start with [development docs](docs/development/README.md); historical
files under `docs/plans/` are not current instructions.

## Change routes

- Main/floating window lifecycle: [architecture](docs/development/architecture.md#window-lifecycle), `src/components/AppTitlebar.vue`, `src/components/floating/`, `src-tauri/src/lib.rs`, and `src-tauri/src/tray.rs`.
- Persisted settings: `src-tauri/src/config/settings.rs` → `config/dto.rs` → `commands/settings.rs` → `src/types/settings.ts` and `src/composables/useAppSettings.ts`.
- Connections and SSE: `src/composables/useConnections.ts` and `src-tauri/src/connections/`.
- IPC/events: `src-tauri/src/lib.rs`, `src-tauri/src/events.rs`, and [contract reference](docs/development/events-and-ipc.md).
- Tests: `src/**/*.spec.ts`, Rust module tests, and [testing guide](docs/development/testing.md).

## Required checks

Run frontend checks with `npm test` and `npm run build`. On Windows, run Rust
commands through `scripts/cargo.ps1`; CI uses bare Cargo in clean runners. Keep
`.work/`, build outputs, local configuration, and secrets out of commits.
