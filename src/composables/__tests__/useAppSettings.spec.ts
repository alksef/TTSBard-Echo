/* ============================================================================
   useAppSettings / createAppSettings behavioural tests: backend readiness,
   concurrent reload invalidation, load errors, theme events and listener
   cleanup.
   ============================================================================ */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'

import {
  emitTauriEvent,
  invokeCalls,
  invokeRejects,
  invokeReturns,
  onInvoke,
  tauriListenerCount,
} from '@/test/helpers/tauri'
import { backendSettingsDto } from '@/test/helpers/fixtures'
import { withSetup } from '@/test/helpers/withSetup'
import { createAppSettings, useAppSettings } from '@/composables/useAppSettings'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/event', async () => await import('@/test/helpers/tauri'))

beforeEach(() => {
  invokeReturns('is_backend_ready', true)
  invokeReturns('get_all_app_settings', backendSettingsDto('dark'))
})

afterEach(() => {
  vi.useRealTimers()
})

describe('backend readiness', () => {
  it('loads settings as soon as the backend reports ready', async () => {
    const { result } = withSetup(() => createAppSettings())
    await flushPromises()

    expect(result.settings.value?.general.theme).toBe('dark')
    expect(result.settings.value?.windows.floating.opacity).toBe(90)
    expect(result.settings.value?.windows.floating.visible).toBe(false)
    expect(result.isLoading.value).toBe(false)
    expect(result.error.value).toBeNull()
    // Ready on the first probe: no confirmation round-trip needed.
    expect(invokeCalls('confirm_backend_ready')).toBe(0)
  })

  it('retries until the backend becomes ready', async () => {
    let probes = 0
    onInvoke('is_backend_ready', () => ++probes >= 3)
    invokeReturns('get_all_app_settings', backendSettingsDto('light'))

    vi.useFakeTimers()
    const { result } = withSetup(() => createAppSettings())
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()

    expect(result.settings.value?.general.theme).toBe('light')
    expect(result.error.value).toBeNull()
    expect(invokeCalls('confirm_backend_ready')).toBe(2)
  })

  it('sets an error and stays empty when the backend never becomes ready', async () => {
    invokeReturns('is_backend_ready', false)

    vi.useFakeTimers()
    const { result } = withSetup(() => createAppSettings())
    await vi.advanceTimersByTimeAsync(50 * 100 + 50)
    await flushPromises()

    expect(result.error.value).toBe('Backend not ready after timeout')
    expect(result.settings.value).toBeNull()
    expect(result.isLoading.value).toBe(false)
    expect(invokeCalls('confirm_backend_ready')).toBe(50)
  })
})

describe('load errors and recovery', () => {
  it('surfaces get_all_app_settings failures and keeps old values', async () => {
    const { result } = withSetup(() => createAppSettings())
    await flushPromises()
    expect(result.settings.value?.general.theme).toBe('dark')

    invokeRejects('get_all_app_settings', new Error('storage locked'))
    emitTauriEvent('settings-changed', null)
    await flushPromises()

    expect(result.error.value).toBe('storage locked')
    expect(result.settings.value?.general.theme).toBe('dark')
  })

  it('retries the initial load when backend-ready arrives later', async () => {
    invokeRejects('get_all_app_settings', new Error('not yet'))
    const { result } = withSetup(() => createAppSettings())
    await flushPromises()
    expect(result.settings.value).toBeNull()

    invokeReturns('get_all_app_settings', backendSettingsDto('light'))
    emitTauriEvent('backend-ready', null)
    await flushPromises()

    expect(result.settings.value?.general.theme).toBe('light')
    expect(result.error.value).toBeNull()
  })

  it('ignores backend-ready once settings are already loaded', async () => {
    const { result } = withSetup(() => createAppSettings())
    await flushPromises()

    const reads = invokeCalls('get_all_app_settings')
    emitTauriEvent('backend-ready', null)
    await flushPromises()

    expect(invokeCalls('get_all_app_settings')).toBe(reads)
    expect(result.settings.value?.general.theme).toBe('dark')
  })
})

describe('concurrent reloads', () => {
  it('re-reads settings after a reload was requested mid-flight', async () => {
    let resolveFirst!: (value: unknown) => void
    const firstRead = new Promise((resolve) => { resolveFirst = resolve })
    let reads = 0
    onInvoke('get_all_app_settings', () => {
      reads += 1
      return reads === 1 ? firstRead : backendSettingsDto('light')
    })

    const { result } = withSetup(() => createAppSettings())
    await flushPromises()
    expect(result.isLoading.value).toBe(true)

    // Two overlapping reloads while the first read is still pending: both are
    // collapsed into one queued re-read that must observe the newer value.
    void result.reload()
    void result.reload()

    resolveFirst(backendSettingsDto('dark'))
    await flushPromises()

    expect(reads).toBe(2)
    expect(result.settings.value?.general.theme).toBe('light')
    expect(result.isLoading.value).toBe(false)
    expect(result.error.value).toBeNull()
  })

  it('reloads on settings-changed events', async () => {
    const { result } = withSetup(() => createAppSettings())
    await flushPromises()

    invokeReturns('get_all_app_settings', backendSettingsDto('light'))
    emitTauriEvent('settings-changed', null)
    await flushPromises()

    expect(result.settings.value?.general.theme).toBe('light')
    expect(result.error.value).toBeNull()
  })
})

describe('theme handling', () => {
  it('applies valid theme-changed payloads and ignores the rest', async () => {
    withSetup(() => createAppSettings())
    await flushPromises()

    emitTauriEvent('theme-changed', 'light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
    expect(localStorage.getItem('app-theme')).toBe('light')

    emitTauriEvent('theme-changed', 'blue')
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
  })
})

describe('listener cleanup', () => {
  it('unsubscribes every listener on scope dispose and stops reacting', async () => {
    const { result, unmount } = withSetup(() => createAppSettings())
    await flushPromises()
    expect(tauriListenerCount()).toBeGreaterThan(0)

    unmount()
    expect(tauriListenerCount()).toBe(0)

    const reads = invokeCalls('get_all_app_settings')
    emitTauriEvent('settings-changed', null)
    emitTauriEvent('backend-ready', null)
    await flushPromises()
    expect(invokeCalls('get_all_app_settings')).toBe(reads)
    // cleanup() stays callable after dispose.
    expect(() => result.cleanup?.()).not.toThrow()
  })
})

describe('useAppSettings injection fallback', () => {
  it('returns a inert default context outside of a provider', async () => {
    const { result } = withSetup(() => useAppSettings())

    expect(result.settings.value).toBeNull()
    expect(result.isLoading.value).toBe(false)
    await expect(result.reload()).resolves.toBeUndefined()
  })
})
