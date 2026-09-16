/* ============================================================================
   useConnections behavioural tests: initial snapshot, event handling,
   reload, mutation timeout, post-error state resync and listener cleanup.
   ============================================================================ */
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'

import {
  emitTauriEvent,
  invokeCalls,
  invokePending,
  invokeRejects,
  invokeReturns,
  listen,
  tauriListenerCount,
} from '@/test/helpers/tauri'
import { connectionConfig, runtimeSnapshotDto } from '@/test/helpers/fixtures'
import { withSetup } from '@/test/helpers/withSetup'
import {
  mapConnectionRuntimeSnapshot,
  normalizeConnectionError,
  useConnections,
} from '@/composables/useConnections'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/event', async () => await import('@/test/helpers/tauri'))

const c1 = connectionConfig('c1')
const c2 = connectionConfig('c2')

function primeBackend(): void {
  invokeReturns('get_connections', [c1, c2])
  invokeReturns('get_connection_runtime_snapshot', [
    runtimeSnapshotDto('c1', 'Connected'),
    runtimeSnapshotDto('c2', 'Connecting'),
  ])
}

/** Mount the composable and wait for subscribe + initial reload. */
async function setupConnections() {
  const handle = withSetup(() => useConnections())
  await flushPromises()
  return handle
}

beforeEach(() => {
  primeBackend()
})

describe('normalizeConnectionError', () => {
  it('unwraps Error message, strings and unknown shapes', () => {
    expect(normalizeConnectionError(new Error('boom'))).toBe('boom')
    expect(normalizeConnectionError('plain')).toBe('plain')
    expect(normalizeConnectionError({ code: 42 })).toBe('Неизвестная ошибка подключения')
  })
})

describe('mapConnectionRuntimeSnapshot', () => {
  it('maps the snake_case wire DTO to camelCase runtime state', () => {
    expect(mapConnectionRuntimeSnapshot(runtimeSnapshotDto('c1', 'Connected', { last_message: 'hello' }))).toEqual({
      id: 'c1',
      status: 'Connected',
      lastMessage: 'hello',
      errorKind: undefined,
      errorMessage: undefined,
      attempt: undefined,
      maxAttempts: undefined,
      nextRetryInSecs: undefined,
      isTyping: false,
      previewText: undefined,
    })
  })

  it('maps retry progress and classified error detail fields', () => {
    const retrying = mapConnectionRuntimeSnapshot(runtimeSnapshotDto('c1', 'Retrying', {
      attempt: 3,
      max_attempts: 10,
      next_retry_in_secs: 5,
    }))
    expect(retrying.status).toBe('Retrying')
    expect(retrying.attempt).toBe(3)
    expect(retrying.maxAttempts).toBe(10)
    expect(retrying.nextRetryInSecs).toBe(5)
    expect(retrying.errorKind).toBeUndefined()
    expect(retrying.errorMessage).toBeUndefined()

    const failed = mapConnectionRuntimeSnapshot(runtimeSnapshotDto('c1', 'Error', {
      error_kind: 'authentication',
      error_message: 'The server rejected the credentials',
    }))
    expect(failed.status).toBe('Error')
    expect(failed.errorKind).toBe('authentication')
    expect(failed.errorMessage).toBe('The server rejected the credentials')
    expect(failed.attempt).toBeUndefined()
    expect(failed.maxAttempts).toBeUndefined()
    expect(failed.nextRetryInSecs).toBeUndefined()
  })
})

describe('useConnections initial snapshot', () => {
  it('merges configs with runtime snapshot and reports loading state', async () => {
    const { result } = await setupConnections()

    expect(result.loading.value).toBe(false)
    expect(result.error.value).toBeNull()
    expect(result.configs.value).toEqual([c1, c2])
    expect(result.connections.value.map((view) => [view.id, view.runtime.status])).toEqual([
      ['c1', 'Connected'],
      ['c2', 'Connecting'],
    ])
  })

  it('falls back to a Disconnected runtime when snapshot has no entry', async () => {
    invokeReturns('get_connection_runtime_snapshot', [])
    const { result } = await setupConnections()

    expect(result.connections.value[0].runtime.status).toBe('Disconnected')
    expect(result.connections.value[0].runtime.isTyping).toBe(false)
  })
})

describe('useConnections error state', () => {
  it('surfaces reload failure and clears loading', async () => {
    invokeRejects('get_connections', new Error('backend unreachable'))
    const { result } = await setupConnections()

    expect(result.error.value).toBe('backend unreachable')
    expect(result.loading.value).toBe(false)
    expect(result.configs.value).toEqual([])
  })

  it('swallows initial load failure into error instead of unhandled rejection', async () => {
    invokeRejects('get_connection_runtime_snapshot', 'ipc down')
    const { result } = await setupConnections()

    expect(result.error.value).toBe('ipc down')
  })
})

describe('useConnections manual reload', () => {
  it('refetches configs and snapshot and toggles loading', async () => {
    const { result } = await setupConnections()
    const initialCalls = invokeCalls('get_connections')

    const pendingSnapshot = invokePending('get_connection_runtime_snapshot')
    const reloading = result.reload()
    expect(result.loading.value).toBe(true)

    pendingSnapshot.resolve([runtimeSnapshotDto('c1', 'Error', {
      error_kind: 'network',
      error_message: 'Could not reach the server',
    })])
    await reloading
    await flushPromises()

    expect(invokeCalls('get_connections')).toBe(initialCalls + 1)
    expect(result.loading.value).toBe(false)
    expect(result.connections.value[0].runtime.status).toBe('Error')
    expect(result.connections.value[0].runtime.errorKind).toBe('network')
    expect(result.connections.value[0].runtime.errorMessage).toBe('Could not reach the server')
  })

  it('rejects and keeps loading=false when reload fails', async () => {
    const { result } = await setupConnections()
    invokeRejects('get_connections', 'nope')

    await expect(result.reload()).rejects.toBe('nope')
    expect(result.loading.value).toBe(false)
    expect(result.error.value).toBe('nope')
  })
})

describe('useConnections event handling', () => {
  it('applies structured status payloads including Error and Retrying details', async () => {
    const { result } = await setupConnections()

    emitTauriEvent('connection-status-changed', { id: 'c1', status: 'Connected' })
    expect(result.connections.value[0].runtime.status).toBe('Connected')

    emitTauriEvent('connection-status-changed', {
      id: 'c1',
      status: 'Error',
      errorKind: 'authentication',
      errorMessage: 'The server rejected the credentials',
    })
    const failed = result.connections.value[0].runtime
    expect(failed.status).toBe('Error')
    expect(failed.errorKind).toBe('authentication')
    expect(failed.errorMessage).toBe('The server rejected the credentials')

    emitTauriEvent('connection-status-changed', {
      id: 'c1',
      status: 'Retrying',
      attempt: 2,
      maxAttempts: 10,
      nextRetryInSecs: 5,
    })
    const retrying = result.connections.value[0].runtime
    expect(retrying.status).toBe('Retrying')
    expect(retrying.attempt).toBe(2)
    expect(retrying.maxAttempts).toBe(10)
    expect(retrying.nextRetryInSecs).toBe(5)
  })

  it('clears stale error and retry details when a plain status arrives', async () => {
    const { result } = await setupConnections()

    emitTauriEvent('connection-status-changed', {
      id: 'c1',
      status: 'Error',
      errorKind: 'tls',
      errorMessage: 'Secure connection (TLS) failed',
    })
    expect(result.connections.value[0].runtime.errorKind).toBe('tls')

    emitTauriEvent('connection-status-changed', { id: 'c1', status: 'Retrying', attempt: 1, maxAttempts: 10, nextRetryInSecs: 5 })
    emitTauriEvent('connection-status-changed', { id: 'c1', status: 'Connected' })
    const cleared = result.connections.value[0].runtime
    expect(cleared.status).toBe('Connected')
    expect(cleared.errorKind).toBeUndefined()
    expect(cleared.errorMessage).toBeUndefined()
    expect(cleared.attempt).toBeUndefined()
    expect(cleared.maxAttempts).toBeUndefined()
    expect(cleared.nextRetryInSecs).toBeUndefined()
  })

  it('stores message, stops typing and clears the message again', async () => {
    const { result } = await setupConnections()

    emitTauriEvent('typing-changed', { id: 'c1', isTyping: true, previewText: 'hel' })
    expect(result.connections.value[0].runtime.isTyping).toBe(true)
    expect(result.connections.value[0].runtime.previewText).toBe('hel')

    emitTauriEvent('message-received', ['c1', 'hello world'])
    const runtime = result.connections.value[0].runtime
    expect(runtime.lastMessage).toBe('hello world')
    expect(runtime.isTyping).toBe(false)
    expect(runtime.previewText).toBeUndefined()

    emitTauriEvent('message-cleared', 'c1')
    expect(result.connections.value[0].runtime.lastMessage).toBeUndefined()
  })

  it('drops runtime state for removed connections', async () => {
    const { result } = await setupConnections()
    expect(result.runtimeStates.value.has('c2')).toBe(true)

    emitTauriEvent('connection-removed', 'c2')
    expect(result.runtimeStates.value.has('c2')).toBe(false)
  })

  it('reloads when the backend announces connections-changed', async () => {
    const { result } = await setupConnections()
    const initialCalls = invokeCalls('get_connections')
    invokeReturns('get_connections', [c1])

    emitTauriEvent('connections-changed')
    await flushPromises()

    expect(invokeCalls('get_connections')).toBe(initialCalls + 1)
    expect(result.configs.value).toEqual([c1])
  })
})

describe('useConnections mutation timeout', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('rejects with a timeout message and still resyncs state afterwards', async () => {
    const { result } = await setupConnections()
    const callsBefore = invokeCalls('get_connections')

    vi.useFakeTimers()
    invokePending('connect_connection')

    const mutation = result.connect('c1')
    const assertion = expect(mutation).rejects.toThrow('Операция connect_connection: превышено время ожидания')

    await vi.advanceTimersByTimeAsync(10_000)
    await assertion
    await flushPromises()

    // The backend event/snapshot stays authoritative even after the timeout.
    expect(invokeCalls('get_connections')).toBeGreaterThan(callsBefore)
    expect(result.loading.value).toBe(false)
    // The timeout timer was cleared, no dangling fake timers remain.
    expect(vi.getTimerCount()).toBe(0)
  })

  it('propagates mutation failure after reloading fresh state', async () => {
    const { result } = await setupConnections()
    invokeReturns('get_connections', [c1])
    invokeRejects('disconnect_connection', new Error('socket stuck'))

    await expect(result.disconnect('c1')).rejects.toThrow('socket stuck')
    await flushPromises()

    expect(result.configs.value).toEqual([c1])
  })
})

describe('useConnections cleanup', () => {
  it('unsubscribes all event listeners on unmount', async () => {
    const { unmount } = await setupConnections()
    expect(tauriListenerCount()).toBeGreaterThan(0)

    unmount()
    expect(tauriListenerCount()).toBe(0)
  })

  it('ignores events after unmount instead of mutating state', async () => {
    const { result, unmount } = await setupConnections()
    unmount()

    emitTauriEvent('message-received', ['c1', 'late'])
    expect(result.connections.value[0].runtime.lastMessage).toBeUndefined()
  })

  it('unlistens late-resolving subscriptions when unmount happens mid-subscribe', async () => {
    const resolvers: Array<(unlisten: () => void) => void> = []
    listen.mockImplementation(() => new Promise<() => void>((resolve) => resolvers.push(resolve)))

    const handle = withSetup(() => useConnections())
    await flushPromises()
    handle.unmount()

    // Subscriptions resolve after disposal; the composable must not keep them.
    for (const resolve of resolvers.splice(0)) resolve(() => {})
    await flushPromises()

    expect(tauriListenerCount()).toBe(0)
    listen.mockRestore()
  })
})
