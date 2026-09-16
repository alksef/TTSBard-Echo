/* ============================================================================
   AppTitlebar behavioural tests: floating visibility sync (snapshot + events +
   stale-response guard), click-through optimistic update and rollback, window
   control buttons and listener cleanup.
   ============================================================================ */
import { describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

import {
  emitTauriEvent,
  invokeCalls,
  invokePending,
  invokeRejects,
  invokeReturns,
  tauriListenerCount,
  windowApi,
} from '@/test/helpers/tauri'
import { appSettingsDto } from '@/test/helpers/fixtures'
import AppTitlebar from '@/components/AppTitlebar.vue'
import { APP_SETTINGS_KEY, type AppSettingsContext, type AppSettingsDto } from '@/types'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/event', async () => await import('@/test/helpers/tauri'))
vi.mock('@tauri-apps/api/window', async () => await import('@/test/helpers/tauri'))

function makeContext(settings: AppSettingsDto | null = null) {
  const settingsRef = ref<AppSettingsDto | null>(settings)
  const context: AppSettingsContext = {
    settings: settingsRef,
    isLoading: ref(false),
    error: ref(null),
    reload: async () => {},
    cleanup: () => {},
  }
  return { context, settings: settingsRef }
}

function mountTitlebar(settings: AppSettingsDto | null = null) {
  const { context, settings: settingsRef } = makeContext(settings)
  const wrapper = mount(AppTitlebar, {
    global: { provide: { [APP_SETTINGS_KEY as symbol]: context } },
  })
  return { wrapper, settings: settingsRef }
}

describe('floating visibility', () => {
  it('reflects the backend snapshot on mount', async () => {
    invokeReturns('get_floating_visibility', true)
    const { wrapper } = mountTitlebar()
    await flushPromises()

    expect(wrapper.findAll('button')[1].classes()).toContain('active')
  })

  it('follows floating-visibility-changed events and updates the label', async () => {
    invokeReturns('get_floating_visibility', false)
    const { wrapper } = mountTitlebar()
    await flushPromises()
    expect(wrapper.findAll('button')[1].classes()).not.toContain('active')

    emitTauriEvent('floating-visibility-changed', { visible: true })
    await flushPromises()

    expect(wrapper.findAll('button')[1].classes()).toContain('active')
    expect(wrapper.findAll('button')[1].attributes('aria-label')).toBe('Скрыть плавающее окно')
  })

  it('ignores a stale visibility snapshot once a newer event arrived', async () => {
    const pending = invokePending('get_floating_visibility')
    const { wrapper } = mountTitlebar()
    await flushPromises()

    emitTauriEvent('floating-visibility-changed', { visible: true })
    pending.resolve(false)
    await flushPromises()

    expect(wrapper.findAll('button')[1].classes()).toContain('active')
  })
})

describe('floating toggle', () => {
  it('guards against double activation while a toggle is in flight', async () => {
    const pending = invokePending('toggle_floating_window')
    const { wrapper } = mountTitlebar()
    await flushPromises()

    const floating = wrapper.findAll('button')[1]
    await floating.trigger('click')
    await floating.trigger('click')

    expect(invokeCalls('toggle_floating_window')).toBe(1)
    expect(floating.attributes('disabled')).toBeDefined()

    pending.resolve({ visible: true })
    await flushPromises()

    expect(wrapper.findAll('button')[1].classes()).toContain('active')
    expect(wrapper.findAll('button')[1].attributes('disabled')).toBeUndefined()
  })

  it('falls back to a backend refresh when the toggle fails', async () => {
    invokeRejects('toggle_floating_window', new Error('window gone'))
    invokeReturns('get_floating_visibility', false)
    const { wrapper } = mountTitlebar()
    await flushPromises()

    await wrapper.findAll('button')[1].trigger('click')
    await flushPromises()

    expect(invokeCalls('get_floating_visibility')).toBeGreaterThan(0)
    expect(wrapper.findAll('button')[1].classes()).not.toContain('active')
  })
})

describe('click-through', () => {
  it('starts from the windows settings snapshot', async () => {
    const settings = appSettingsDto()
    settings.windows.floating.clickthrough = true
    const { wrapper } = mountTitlebar(settings)

    expect(wrapper.findAll('button')[0].classes()).toContain('active')
  })

  it('updates optimistically and settles on the backend answer', async () => {
    invokeReturns('get_floating_visibility', false)
    const pending = invokePending('set_clickthrough')
    const { wrapper } = mountTitlebar()
    await flushPromises()

    await wrapper.findAll('button')[0].trigger('click')
    expect(wrapper.findAll('button')[0].classes()).toContain('active')

    pending.resolve(false)
    await flushPromises()
    expect(wrapper.findAll('button')[0].classes()).not.toContain('active')
  })

  it('rolls back to the previous value when the backend rejects', async () => {
    invokeReturns('get_floating_visibility', false)
    invokeRejects('set_clickthrough', new Error('denied'))
    const { wrapper } = mountTitlebar()
    await flushPromises()

    await wrapper.findAll('button')[0].trigger('click')
    await flushPromises()

    expect(wrapper.findAll('button')[0].classes()).not.toContain('active')
  })

  it('applies clickthrough-changed events', async () => {
    invokeReturns('get_floating_visibility', false)
    const { wrapper } = mountTitlebar()
    await flushPromises()

    emitTauriEvent('clickthrough-changed', true)
    await flushPromises()
    expect(wrapper.findAll('button')[0].classes()).toContain('active')
  })
})

describe('window controls and cleanup', () => {
  it('minimizes and closes to tray without exposing the full exit action', async () => {
    const { wrapper } = mountTitlebar()
    await flushPromises()
    const buttons = wrapper.findAll('button')

    expect(buttons).toHaveLength(4)
    await buttons[2].trigger('click')
    await buttons[3].trigger('click')

    expect(windowApi.minimize).toHaveBeenCalledTimes(1)
    expect(windowApi.close).toHaveBeenCalledTimes(1)
    expect(invokeCalls('quit_app')).toBe(0)
  })

  it('unsubscribes event listeners on unmount', async () => {
    const { wrapper } = mountTitlebar()
    await flushPromises()
    expect(tauriListenerCount()).toBeGreaterThan(0)

    wrapper.unmount()
    expect(tauriListenerCount()).toBe(0)
  })
})
