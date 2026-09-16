import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import { invokeCalls, invokePending, invokeRejects } from '@/test/helpers/tauri'
import Sidebar from '@/components/Sidebar.vue'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))

function mountSidebar() {
  return mount(Sidebar, { props: { panel: 'connections' } })
}

describe('sidebar exit action', () => {
  it('shows the exit label in the expanded sidebar and guards repeated activation', async () => {
    const pending = invokePending('quit_app')
    const wrapper = mountSidebar()
    await flushPromises()

    const quit = wrapper.get('.quit-button')
    expect(quit.text()).toBe('Выход')
    expect(quit.attributes('aria-label')).toBe('Выход')

    await quit.trigger('click')
    await quit.trigger('click')

    expect(invokeCalls('quit_app')).toBe(1)
    expect(quit.attributes('disabled')).toBeDefined()

    pending.resolve(null)
    await flushPromises()
  })

  it('keeps exit accessible when the sidebar is collapsed', async () => {
    localStorage.setItem('sidebar-collapsed', 'true')
    const wrapper = mountSidebar()
    await flushPromises()

    const quit = wrapper.get('.quit-button')
    expect(quit.text()).toBe('')
    expect(quit.attributes('title')).toBe('Выход')
    expect(quit.attributes('aria-label')).toBe('Выход')
  })

  it('allows retry after the backend rejects the exit request', async () => {
    invokeRejects('quit_app', new Error('shutdown failed'))
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const wrapper = mountSidebar()

    await wrapper.get('.quit-button').trigger('click')
    await flushPromises()

    expect(wrapper.get('.quit-button').attributes('disabled')).toBeUndefined()
    expect(errorSpy).toHaveBeenCalledOnce()
  })
})
