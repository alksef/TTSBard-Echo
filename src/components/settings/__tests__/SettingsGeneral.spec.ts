import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { ref } from 'vue'

import SettingsGeneral from '@/components/settings/SettingsGeneral.vue'
import { appSettingsDto } from '@/test/helpers/fixtures'
import { invokeCalls, invokePending, invokeRejects, invokeReturns } from '@/test/helpers/tauri'
import { APP_SETTINGS_KEY, type AppSettingsContext } from '@/types'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))

function mountSettings(hideOnMinimize = false) {
  const settings = appSettingsDto()
  settings.general.hide_on_minimize = hideOnMinimize
  const context: AppSettingsContext = {
    settings: ref(settings),
    isLoading: ref(false),
    error: ref(null),
    reload: async () => {},
  }
  return mount(SettingsGeneral, {
    global: { provide: { [APP_SETTINGS_KEY as symbol]: context } },
  })
}

describe('hide on minimize setting', () => {
  it('is off by default and blocks repeated changes while saving', async () => {
    const pending = invokePending('set_hide_on_minimize')
    const wrapper = mountSettings()
    const checkbox = wrapper.get('input[type="checkbox"]')

    expect((checkbox.element as HTMLInputElement).checked).toBe(false)
    await checkbox.trigger('change')
    await checkbox.trigger('change')

    expect(invokeCalls('set_hide_on_minimize')).toBe(1)
    expect(checkbox.attributes('disabled')).toBeDefined()

    pending.resolve(null)
    await flushPromises()
    expect((checkbox.element as HTMLInputElement).checked).toBe(true)
  })

  it('restores the previous value when persistence fails', async () => {
    invokeRejects('set_hide_on_minimize', new Error('write failed'))
    const wrapper = mountSettings(true)
    const checkbox = wrapper.get('input[type="checkbox"]')

    await checkbox.trigger('change')
    await flushPromises()

    expect((checkbox.element as HTMLInputElement).checked).toBe(true)
    expect(wrapper.text()).toContain('write failed')
  })
})

describe('general setting persistence', () => {
  it('renders a successful save as success', async () => {
    invokeReturns('set_logging_enabled', null)
    const wrapper = mountSettings()
    const checkbox = wrapper.findAll('input[type="checkbox"]')[2]

    await checkbox.trigger('change')
    await flushPromises()

    expect(wrapper.get('.status-message').classes()).toContain('success')
  })

  it('restores the capture setting when persistence fails', async () => {
    invokeRejects('set_exclude_from_capture', new Error('capture write failed'))
    const wrapper = mountSettings()
    const checkbox = wrapper.findAll('input[type="checkbox"]')[1]

    expect((checkbox.element as HTMLInputElement).checked).toBe(false)
    await checkbox.trigger('change')
    await flushPromises()

    expect((checkbox.element as HTMLInputElement).checked).toBe(false)
    expect(wrapper.text()).toContain('capture write failed')
  })

  it('groups settings into meaningful sections', () => {
    const wrapper = mountSettings()

    expect(wrapper.findAll('.section-title').map(title => title.text())).toEqual([
      'Поведение окон',
      'Диагностика',
      'Сообщения',
    ])
    expect(wrapper.findAll('.settings-section')).toHaveLength(3)
  })
})
