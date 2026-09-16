import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import SettingsInterface from '@/components/settings/SettingsInterface.vue'
import { appSettingsDto } from '@/test/helpers/fixtures'
import { invokeReturns } from '@/test/helpers/tauri'
import { APP_SETTINGS_KEY, type AppSettingsContext } from '@/types'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))

function mountSettings() {
  const context: AppSettingsContext = {
    settings: ref(appSettingsDto()),
    isLoading: ref(false),
    error: ref(null),
    reload: async () => {},
  }
  invokeReturns('get_floating_appearance', {
    opacity: 0.9,
    bg_color: '#101014',
    use_custom_color: true,
  })
  return mount(SettingsInterface, {
    global: { provide: { [APP_SETTINGS_KEY as symbol]: context } },
  })
}

afterEach(() => vi.useRealTimers())

describe('interface status message', () => {
  it('clears an auto-hidden validation error in the parent', async () => {
    vi.useFakeTimers()
    const wrapper = mountSettings()
    await flushPromises()
    const colorText = wrapper.get('input[type="text"]')

    await colorText.setValue('invalid')
    await colorText.trigger('blur')
    expect(wrapper.text()).toContain('Цвет должен быть')

    await vi.advanceTimersByTimeAsync(3000)
    await flushPromises()
    expect(wrapper.text()).not.toContain('Цвет должен быть')
  })
})
