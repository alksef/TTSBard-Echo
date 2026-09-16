/* ============================================================================
   ConnectionsPanel behavioural tests: structured status rendering (Retrying,
   error categories) via the shared label helper, and the floating-mode window
   height watch staying item-count based when a connection is Retrying
   (roadmap 011, task 003).
   ============================================================================ */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import {
  emitTauriEvent,
  invokeReturns,
  windowApi,
} from '@/test/helpers/tauri'
import { connectionConfig, runtimeSnapshotDto } from '@/test/helpers/fixtures'
import ConnectionsPanel from '@/components/ConnectionsPanel.vue'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/event', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/window', async () => await import('@/test/helpers/tauri'))

function primeBackend(
  snapshotOverrides: Record<string, Partial<ReturnType<typeof runtimeSnapshotDto>>>,
  ids = Object.keys(snapshotOverrides),
): void {
  invokeReturns('get_connections', ids.map((id) => connectionConfig(id)))
  invokeReturns('get_connection_runtime_snapshot', ids.map((id) => runtimeSnapshotDto(id, 'Disconnected', snapshotOverrides[id])))
}

async function mountPanel(props: { floating?: boolean } = {}) {
  const wrapper = mount(ConnectionsPanel, { props })
  await flushPromises()
  return wrapper
}

/** Last LogicalSize the floating window was resized to. */
function lastSize(): { width: number; height: number } {
  const calls = windowApi.setSize.mock.calls
  return calls[calls.length - 1][0] as { width: number; height: number }
}

beforeEach(() => {
  invokeReturns('get_connections', [])
  invokeReturns('get_connection_runtime_snapshot', [])
})

describe('ConnectionsPanel status rendering', () => {
  it('renders the retry label with wait and attempt progress and a spinner', async () => {
    primeBackend({
      c1: { status: 'Retrying', attempt: 3, max_attempts: 10, next_retry_in_secs: 5 },
    })
    const wrapper = await mountPanel()
    const status = wrapper.find('.connection-card .status')

    expect(status.text()).toContain('Повтор через 5с (попытка 3 из 10)')
    expect(status.classes()).toContain('retrying')
    expect(status.find('.spin').exists()).toBe(true)
    wrapper.unmount()
  })

  it('renders the RU text of the error category instead of the raw message', async () => {
    primeBackend({
      c1: { status: 'Error', error_kind: 'authentication', error_message: 'The server rejected the credentials' },
    })
    const wrapper = await mountPanel()

    expect(wrapper.find('.connection-card .status').text()).toBe('Доступ запрещён — проверьте токен доступа')
    wrapper.unmount()
  })

  it('updates the retry label from structured status events', async () => {
    primeBackend({ c1: { status: 'Connected' } })
    const wrapper = await mountPanel()

    emitTauriEvent('connection-status-changed', {
      id: 'c1',
      status: 'Retrying',
      attempt: 9,
      maxAttempts: 10,
      nextRetryInSecs: 5,
    })
    await flushPromises()
    expect(wrapper.find('.connection-card .status').text()).toContain('Повтор через 5с (попытка 9 из 10)')
    wrapper.unmount()
  })
})

describe('ConnectionsPanel floating height', () => {
  it('keeps the item-count-based height while a connection is Retrying', async () => {
    primeBackend({
      c1: { status: 'Retrying', attempt: 4, max_attempts: 10, next_retry_in_secs: 5 },
      c2: { status: 'Connected' },
    })
    const wrapper = await mountPanel({ floating: true })

    // 2 cards * 60 + 80 base — the long retry label must not inflate it.
    expect(lastSize()).toEqual({ width: 350, height: 200 })
    wrapper.unmount()
  })

  it('recalculates from the connection count when retries start and caps the height', async () => {
    primeBackend({ c1: {} })
    const wrapper = await mountPanel({ floating: true })
    expect(lastSize()).toEqual({ width: 350, height: 140 })

    const ids = ['c1', 'c2', 'c3', 'c4', 'c5', 'c6']
    invokeReturns('get_connections', ids.map((id) => connectionConfig(id)))
    invokeReturns('get_connection_runtime_snapshot', ids.map((id) => runtimeSnapshotDto(id, 'Retrying', {
      attempt: 2,
      max_attempts: 10,
      next_retry_in_secs: 5,
    })))
    emitTauriEvent('connections-changed')
    await flushPromises()

    // 6 cards * 60 + 80 = 440 exceeds the 4-card cap of 320.
    expect(lastSize()).toEqual({ width: 350, height: 320 })
    wrapper.unmount()
  })
})
