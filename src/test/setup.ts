/* ============================================================================
   Global Vitest setup: happy-dom shims + per-test isolation.
   Tauri API mocking is declared per test file via vi.mock (see
   src/test/helpers/tauri.ts).
   ============================================================================ */
import { afterEach, beforeEach, vi } from 'vitest'
import { ResizeObserverMock, resetTauriMocks } from './helpers/tauri'

// Trackable stub replaces the environment's opaque ResizeObserver.
globalThis.ResizeObserver = ResizeObserverMock as unknown as typeof ResizeObserver

// In the happy-dom environment window === globalThis, so vi.useFakeTimers()
// automatically intercepts code that calls window.setTimeout as well.

// Restores per-test mockImplementation overrides (and clears call history)
// so specs can never leak configuration into each other.
afterEach(() => {
  vi.restoreAllMocks()
})

beforeEach(() => {
  resetTauriMocks()
  localStorage.clear()
  sessionStorage.clear()
  document.documentElement.removeAttribute('data-theme')
  document.documentElement.style.cssText = ''
})
