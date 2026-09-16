/* ============================================================================
   StatusMessage behavioural tests: auto-hide timer, rescheduling on message
   change, manual dismiss and timer cleanup on unmount. Uses fake timers —
   no real waiting.
   ============================================================================ */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import StatusMessage from '@/components/shared/StatusMessage.vue'

afterEach(() => {
  vi.useRealTimers()
})

describe('StatusMessage', () => {
  it('becomes visible when a message arrives and auto-hides once', async () => {
    vi.useFakeTimers()
    const wrapper = mount(StatusMessage, { props: { message: '', autoHideDelay: 1000 } })
    expect(wrapper.find('.status-message').exists()).toBe(false)

    await wrapper.setProps({ message: 'Сохранено' })
    expect(wrapper.find('.status-message').exists()).toBe(true)

    await vi.advanceTimersByTimeAsync(1000)
    await flushPromises()
    expect(wrapper.emitted('dismiss')?.length).toBe(1)

    // No duplicate dismissals from leftover timers.
    await vi.advanceTimersByTimeAsync(5000)
    expect(wrapper.emitted('dismiss')?.length).toBe(1)
  })

  it('reschedules the timer when the message changes', async () => {
    vi.useFakeTimers()
    const wrapper = mount(StatusMessage, { props: { message: 'first', autoHideDelay: 1000 } })

    await wrapper.setProps({ message: 'second' })
    // The countdown restarted: half of the window must not fire.
    await vi.advanceTimersByTimeAsync(500)
    expect(wrapper.emitted('dismiss')).toBeUndefined()

    await vi.advanceTimersByTimeAsync(500)
    expect(wrapper.emitted('dismiss')?.length).toBe(1)
  })

  it('does not schedule auto-hide when autoHide is off', async () => {
    vi.useFakeTimers()
    const wrapper = mount(StatusMessage, { props: { message: 'kept', autoHide: false, autoHideDelay: 1000 } })

    await vi.advanceTimersByTimeAsync(10_000)
    expect(wrapper.emitted('dismiss')).toBeUndefined()
    expect(wrapper.find('.status-message').exists()).toBe(true)
  })

  it('emits dismiss when the close button is clicked', async () => {
    const wrapper = mount(StatusMessage, { props: { message: 'hello', dismissible: true } })
    await flushPromises()

    const close = wrapper.find('.status-close')
    expect(close.attributes('aria-label')).toBe('Закрыть уведомление')
    await close.trigger('click')
    expect(wrapper.emitted('dismiss')?.length).toBe(1)
  })

  it('clears the auto-hide timer on unmount', async () => {
    vi.useFakeTimers()
    const wrapper = mount(StatusMessage, { props: { message: 'bye', autoHideDelay: 1000 } })
    expect(vi.getTimerCount()).toBe(1)

    wrapper.unmount()
    await vi.advanceTimersByTimeAsync(10_000)
    expect(wrapper.emitted('dismiss')).toBeUndefined()
  })
})
