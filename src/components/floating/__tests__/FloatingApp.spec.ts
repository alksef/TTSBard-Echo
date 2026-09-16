/* ============================================================================
   FloatingApp + FloatingConnectionList behavioural tests: loading/empty/error
   states, connection rendering with typing/message lifecycle, appearance and
   theme sync with stale-response guard, window sizing and cleanup.
   ============================================================================ */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import type { ConnectionErrorKind } from '@/types/types'

import {
  createdResizeObservers,
  emitTauriEvent,
  invoke,
  invokeCalls,
  invokePending,
  invokeRejects,
  invokeReturns,
  listen,
  onInvoke,
  tauriListenerCount,
  windowApi,
} from '@/test/helpers/tauri'
import { connectionConfig, runtimeSnapshot, runtimeSnapshotDto } from '@/test/helpers/fixtures'
import FloatingApp from '@/components/floating/FloatingApp.vue'
import FloatingConnectionList from '@/components/floating/FloatingConnectionList.vue'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/event', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/window', async () => await import('@/test/helpers/tauri'))

function primeBackend(): void {
  invokeReturns('get_connections', [])
  invokeReturns('get_connection_runtime_snapshot', [])
  invokeReturns('get_floating_appearance', { opacity: 80, bg_color: '#101014', use_custom_color: false, clickthrough: false })
  invokeReturns('get_theme', 'dark')
  invokeReturns('restore_floating_window', undefined)
}

async function mountFloatingApp() {
  // attachTo: fitToContent() and the ResizeObserver look up
  // document.querySelector('.connection-list').
  const wrapper = mount(FloatingApp, { attachTo: document.body })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  primeBackend()
})

afterEach(() => {
  document.body.innerHTML = ''
  vi.useRealTimers()
})

describe('FloatingConnectionList states', () => {
  const base = { loading: false, error: null as string | null }

  it('shows the loading state', () => {
    const wrapper = mount(FloatingConnectionList, { props: { ...base, loading: true, connections: [] } })
    expect(wrapper.text()).toContain('Загрузка подключений…')
  })

  it('shows the error state', () => {
    const wrapper = mount(FloatingConnectionList, { props: { ...base, error: 'backend down', connections: [] } })
    expect(wrapper.find('.state.error').text()).toBe('backend down')
  })

  it('shows the empty state', () => {
    const wrapper = mount(FloatingConnectionList, { props: { ...base, connections: [] } })
    expect(wrapper.text()).toContain('Нет активных подключений')
  })

  it('renders a connection with status, error message, typing and last message', () => {
    const connections = [{
      ...connectionConfig('c1', { name: 'Main' }),
      runtime: runtimeSnapshot('c1', 'Connected', { lastMessage: 'hello' }),
    }]
    const wrapper = mount(FloatingConnectionList, { props: { ...base, connections } })

    expect(wrapper.text()).toContain('Main')
    expect(wrapper.find('.status').attributes('data-status')).toBe('Connected')
    expect(wrapper.find('.message').text()).toBe('hello')

    const typingOnly = [{ ...connections[0], runtime: runtimeSnapshot('c1', 'Connected', { isTyping: true, previewText: 'he' }) }]
    expect(mount(FloatingConnectionList, { props: { ...base, connections: typingOnly } }).find('.typing').exists()).toBe(true)

    // A failure renders the RU category label from the shared helper, never
    // the raw English backend message (roadmap 011, task 003 defect fix).
    const errored = [{ ...connections[0], runtime: runtimeSnapshot('c1', 'Error', { errorKind: 'network', errorMessage: 'Could not reach the server', isTyping: true }) }]
    const errorWrapper = mount(FloatingConnectionList, { props: { ...base, connections: errored } })
    expect(errorWrapper.find('p.error').text()).toBe('Сервер недоступен (сеть или DNS)')
    expect(errorWrapper.find('.typing').exists()).toBe(false)

    // An unknown wire kind falls back to the fixed backend message...
    const unknownKind = [{ ...connections[0], runtime: runtimeSnapshot('c1', 'Error', { errorKind: 'mystery' as ConnectionErrorKind, errorMessage: 'Novel failure text' }) }]
    expect(mount(FloatingConnectionList, { props: { ...base, connections: unknownKind } }).find('p.error').text()).toBe('Novel failure text')

    // ...as does an Error status without a kind at all.
    const kindless = [{ ...connections[0], runtime: runtimeSnapshot('c1', 'Error', { errorMessage: 'The server rejected the credentials' }) }]
    expect(mount(FloatingConnectionList, { props: { ...base, connections: kindless } }).find('p.error').text()).toBe('The server rejected the credentials')
  })
})

describe('FloatingApp states', () => {
  it('moves from loading to empty when there are no connections', async () => {
    invokePending('get_connections')
    const wrapper = mount(FloatingApp, { attachTo: document.body })
    await flushPromises()
    expect(wrapper.text()).toContain('Загрузка подключений…')

    invokeReturns('get_connections', [])
    emitTauriEvent('connections-changed')
    await flushPromises()
    expect(wrapper.text()).toContain('Нет активных подключений')
    wrapper.unmount()
  })

  it('shows the reload error when the backend fails', async () => {
    invokeRejects('get_connections', 'ipc refused')
    const wrapper = mount(FloatingApp, { attachTo: document.body })
    await flushPromises()

    expect(wrapper.find('.state.error').text()).toBe('ipc refused')
    wrapper.unmount()
  })

  it('renders connections and reacts to typing and message events', async () => {
    invokeReturns('get_connections', [connectionConfig('c1', { name: 'Main' })])
    invokeReturns('get_connection_runtime_snapshot', [runtimeSnapshotDto('c1', 'Connected')])
    const wrapper = await mountFloatingApp()

    expect(wrapper.find('.connection-card').exists()).toBe(true)
    expect(wrapper.find('.status').attributes('data-status')).toBe('Connected')

    emitTauriEvent('typing-changed', { id: 'c1', isTyping: true, previewText: 'he' })
    await flushPromises()
    expect(wrapper.find('.typing').exists()).toBe(true)

    emitTauriEvent('message-received', ['c1', 'hello'])
    await flushPromises()
    expect(wrapper.find('.typing').exists()).toBe(false)
    expect(wrapper.find('.message').text()).toBe('hello')

    emitTauriEvent('message-cleared', 'c1')
    await flushPromises()
    expect(wrapper.find('.message').exists()).toBe(false)
    wrapper.unmount()
  })
})

describe('FloatingApp appearance and theme sync', () => {
  it('applies appearance updates and ignores stale responses', async () => {
    let resolveFirst!: (value: unknown) => void
    const firstRead = new Promise((resolve) => { resolveFirst = resolve })
    let reads = 0
    primeBackend()
    onInvoke('get_floating_appearance', () => {
      reads += 1
      return reads === 1 ? firstRead : { opacity: 80, bg_color: '#101014', use_custom_color: false, clickthrough: false }
    })

    const wrapper = mount(FloatingApp, { attachTo: document.body })
    await flushPromises()
    expect(document.documentElement.style.getPropertyValue('--floating-opacity')).toBe('95%')

    emitTauriEvent('floating-appearance-update')
    await flushPromises()
    expect(document.documentElement.style.getPropertyValue('--floating-opacity')).toBe('80%')

    // The first read resolves late with an older snapshot: must be dropped.
    resolveFirst({ opacity: 50, bg_color: '#000000', use_custom_color: false, clickthrough: false })
    await flushPromises()
    expect(document.documentElement.style.getPropertyValue('--floating-opacity')).toBe('80%')
    wrapper.unmount()
  })

  it('reloads appearance on clickthrough changes', async () => {
    const wrapper = await mountFloatingApp()
    const reads = invokeCalls('get_floating_appearance')

    emitTauriEvent('clickthrough-changed', true)
    await flushPromises()

    expect(invokeCalls('get_floating_appearance')).toBeGreaterThan(reads)
    wrapper.unmount()
  })

  it('applies theme-changed events', async () => {
    const wrapper = await mountFloatingApp()
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')

    emitTauriEvent('theme-changed', 'light')
    await flushPromises()
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
    wrapper.unmount()
  })

  it('sizes the window to the content and observes resizes', async () => {
    invokeReturns('get_connections', [connectionConfig('c1')])
    const wrapper = await mountFloatingApp()
    await flushPromises()

    expect(windowApi.setSize).toHaveBeenCalled()
    const size = windowApi.setSize.mock.calls[0][0] as { width: number; height: number }
    expect(size.height).toBeGreaterThanOrEqual(64)
    expect(size.width).toBeGreaterThanOrEqual(300)
    expect(windowApi.setMinSize).toHaveBeenCalled()
    expect(windowApi.setMaxSize).toHaveBeenCalled()
    const minSize = windowApi.setMinSize.mock.calls[windowApi.setMinSize.mock.calls.length - 1][0] as { width: number; height: number }
    const maxSize = windowApi.setMaxSize.mock.calls[windowApi.setMaxSize.mock.calls.length - 1][0] as { width: number; height: number }
    expect(minSize.height).toBe(size.height)
    expect(maxSize.height).toBe(size.height)
    expect(minSize.width).toBe(300)
    expect(maxSize.width).toBe(10_000)
    expect(createdResizeObservers()).toHaveLength(1)
    expect(createdResizeObservers()[0].observe).toHaveBeenCalled()
    expect(windowApi.onResized).toHaveBeenCalledOnce()
    expect(invokeCalls('restore_floating_window')).toBe(1)
    const restoreCall = invoke.mock.calls.findIndex(([command]) => command === 'restore_floating_window')
    expect(windowApi.setSize.mock.invocationCallOrder[0])
      .toBeLessThan(invoke.mock.invocationCallOrder[restoreCall])
    wrapper.unmount()
  })

  it('preserves a manually resized width while keeping content-driven height constrained', async () => {
    const wrapper = await mountFloatingApp()
    windowApi.setSize.mockClear()
    windowApi.innerSize.mockResolvedValue({ width: 860, height: 500 })

    const resizeHandler = windowApi.onResized.mock.calls[0][0]
    resizeHandler({ payload: { width: 860, height: 500 } })
    await flushPromises()

    expect(windowApi.setSize).toHaveBeenCalled()
    const lastCall = windowApi.setSize.mock.calls[windowApi.setSize.mock.calls.length - 1]
    const size = lastCall[0] as { width: number; height: number }
    expect(size.width).toBe(860)
    expect(size.height).toBe(64)
    const minSize = windowApi.setMinSize.mock.calls[windowApi.setMinSize.mock.calls.length - 1][0] as { height: number }
    const maxSize = windowApi.setMaxSize.mock.calls[windowApi.setMaxSize.mock.calls.length - 1][0] as { height: number }
    expect(minSize.height).toBe(64)
    expect(maxSize.height).toBe(64)
    wrapper.unmount()
  })
})

describe('FloatingApp cleanup', () => {
  it('unsubscribes listeners and disconnects the resize observer on unmount', async () => {
    const wrapper = await mountFloatingApp()
    expect(tauriListenerCount()).toBeGreaterThan(0)

    wrapper.unmount()
    expect(tauriListenerCount()).toBe(0)
    expect(createdResizeObservers()[0].disconnect).toHaveBeenCalled()
  })

  it('stops reacting to events after unmount', async () => {
    const wrapper = await mountFloatingApp()
    const reads = invokeCalls('get_floating_appearance')
    wrapper.unmount()

    emitTauriEvent('floating-appearance-update')
    await flushPromises()
    expect(invokeCalls('get_floating_appearance')).toBe(reads)
  })

  it('unlistens late-resolving subscriptions after unmount', async () => {
    primeBackend()
    const resolvers: Array<(unlisten: () => void) => void> = []
    listen.mockImplementation(() => new Promise<() => void>((resolve) => resolvers.push(resolve)))

    const wrapper = mount(FloatingApp, { attachTo: document.body })
    await flushPromises()
    wrapper.unmount()

    for (const resolve of resolvers.splice(0)) resolve(() => {})
    await flushPromises()

    expect(tauriListenerCount()).toBe(0)
    listen.mockRestore()
  })
})
