# Testing

Run the repository checks from PowerShell:

```powershell
npm test              # contract checks + frontend unit tests
npm run test:contracts
npm run test:unit
npm run build         # vue-tsc type check (includes test files) + vite build
scripts/cargo.ps1 --% check --manifest-path src-tauri/Cargo.toml --locked
scripts/cargo.ps1 --% test --manifest-path src-tauri/Cargo.toml --locked --all-targets
```

## Contract checks

`check:ipc` verifies frontend invokes against registered Tauri commands.
`check:settings` verifies the Rust/TypeScript settings fields and appearance
conversion helpers.

## Frontend unit tests (Vitest)

`npm run test:unit` runs Vitest in happy-dom against
`src/**/*.spec.ts`. The suite covers the behaviour of the critical
composables and components (`useConnections`, `useAppSettings`,
`ConnectionFormDialog`, `AppTitlebar`, floating UI, `StatusMessage`):
success, error and cleanup scenarios, async lifecycle, stale-response
guards, double-submit protection and listener/timer teardown.

- Tauri APIs are replaced by `src/test/helpers/tauri.ts` (invoke routed by
  command name and failing loudly on unregistered commands, an in-memory
  event bus, window-API and ResizeObserver stubs). Tests never touch a real
  WebView, IPC channel or the network.
- Time-dependent behaviour uses fake timers
  (`vi.useFakeTimers()` + `vi.advanceTimersByTimeAsync`).
- Backend payloads come from `src/test/helpers/fixtures.ts`; components are
  mounted with `@vue/test-utils`, composables run inside a real component
  via `withSetup` so lifecycle hooks work.

Run a single file while iterating:

```powershell
npx vitest run src/composables/__tests__/useConnections.spec.ts
```

Test files are type-checked by `vue-tsc` as part of `npm run build`.

## Rust

On Windows use `scripts/cargo.ps1` so the checks run with the MSVC toolchain
prepared by the repository. Bare `cargo` is used by CI and non-Windows clean
runners. Windows WebView behavior remains a manual smoke-test surface.

The complete local Rust gate is:

```powershell
scripts/cargo.ps1 --% fmt --manifest-path src-tauri/Cargo.toml --all -- --check
scripts/cargo.ps1 --% clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets --all-features -- -D warnings
scripts/cargo.ps1 --% test --manifest-path src-tauri/Cargo.toml --locked --all-targets
```
