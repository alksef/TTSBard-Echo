/* ============================================================================
   ConnectionFormDialog behavioural tests: add/edit, validation, cancel and
   reset, double-submit protection (dialog lock + parent `saving` flag), and
   the pre-save endpoint probe button (roadmap 011, task 005).
   ============================================================================ */
import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import ConnectionFormDialog from '@/components/connections/ConnectionFormDialog.vue'
import { connectionConfig } from '@/test/helpers/fixtures'
import { invoke, invokeCalls, invokePending, onInvoke } from '@/test/helpers/tauri'

vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))

const existing = connectionConfig('c-42', { name: 'Prod', url: 'http://prod.example/sse', access_token: 'tok' })

async function openDialog(connection: ConnectionConfigExt | null = null, saving = false) {
  const wrapper = mount(ConnectionFormDialog, {
    props: { open: false, connection, saving },
  })
  await wrapper.setProps({ open: true })
  await flushPromises()
  return wrapper
}

type ConnectionConfigExt = ReturnType<typeof connectionConfig>

async function fillValidForm(wrapper: Awaited<ReturnType<typeof openDialog>>, name = 'Local') {
  const inputs = wrapper.findAll('input')
  await inputs[0].setValue(name)
  await inputs[1].setValue('http://127.0.0.1:10100/sse?token=secret#anchor')
}

describe('add mode', () => {
  it('opens with defaults and an add button', async () => {
    const wrapper = await openDialog()

    const inputs = wrapper.findAll('input')
    expect((inputs[0].element as HTMLInputElement).value).toBe('')
    expect((inputs[1].element as HTMLInputElement).value).toBe('')
    expect(wrapper.find('button[type="submit"]').text()).toBe('Добавить')
  })

  it('normalizes the URL and lifts the token out of the query string', async () => {
    const wrapper = await openDialog()
    await fillValidForm(wrapper)
    await wrapper.find('form').trigger('submit')

    const payload = wrapper.emitted('save')?.[0]?.[0] as Record<string, unknown>
    expect(payload).toMatchObject({
      name: 'Local',
      url: 'http://127.0.0.1:10100/sse',
      enabled: true,
      access_token: 'secret',
    })
    expect(String(payload.id)).toBeTruthy()
  })

  it('defaults a bare host URL to the /sse path', async () => {
    const wrapper = await openDialog()
    const inputs = wrapper.findAll('input')
    await inputs[0].setValue('Bare')
    await inputs[1].setValue('http://127.0.0.1:10100')
    await wrapper.find('form').trigger('submit')

    const payload = wrapper.emitted('save')?.[0]?.[0] as Record<string, unknown>
    expect(payload.url).toBe('http://127.0.0.1:10100/sse')
  })
})

describe('edit mode', () => {
  it('prefills fields, keeps identity and shows a save button', async () => {
    const wrapper = await openDialog(existing)

    const inputs = wrapper.findAll('input')
    expect((inputs[0].element as HTMLInputElement).value).toBe('Prod')
    expect((inputs[1].element as HTMLInputElement).value).toBe('http://prod.example/sse')
    expect((inputs[2].element as HTMLInputElement).value).toBe('tok')
    expect(wrapper.find('button[type="submit"]').text()).toBe('Сохранить')

    await inputs[0].setValue('Prod renamed')
    await wrapper.find('form').trigger('submit')

    const payload = wrapper.emitted('save')?.[0]?.[0] as Record<string, unknown>
    expect(payload).toMatchObject({ id: 'c-42', name: 'Prod renamed', enabled: true, access_token: 'tok' })
  })
})

describe('validation', () => {
  it.each([
    ['empty name', async (w: Awaited<ReturnType<typeof openDialog>>) => { await (await w.findAll('input'))[0].setValue('   ') }],
    ['overlong name', async (w: Awaited<ReturnType<typeof openDialog>>) => { await (await w.findAll('input'))[0].setValue('x'.repeat(257)) }],
    ['garbage url', async (w: Awaited<ReturnType<typeof openDialog>>) => { await (await w.findAll('input'))[1].setValue('not a url') }],
    ['non-http scheme', async (w: Awaited<ReturnType<typeof openDialog>>) => { await (await w.findAll('input'))[1].setValue('ftp://example.com/sse') }],
    ['missing host', async (w: Awaited<ReturnType<typeof openDialog>>) => { await (await w.findAll('input'))[1].setValue('http:///sse') }],
  ])('rejects %s without emitting save', async (_label, breakForm) => {
    const wrapper = await openDialog()
    await breakForm(wrapper)
    await wrapper.find('form').trigger('submit')

    expect(wrapper.emitted('save')).toBeUndefined()
    expect(wrapper.find('.form-error').text()).not.toBe('')
  })
})

describe('cancel and reset', () => {
  it('closes on cancel and on backdrop click', async () => {
    const wrapper = await openDialog()

    await wrapper.find('button.secondary-action').trigger('click')
    expect(wrapper.emitted('update:open')?.[0]).toEqual([false])

    await wrapper.find('.dialog-backdrop').trigger('click')
    expect(wrapper.emitted('update:open')?.[1]).toEqual([false])
  })

  it('resets fields from the connection prop on reopen', async () => {
    const wrapper = await openDialog(existing)
    await (await wrapper.findAll('input'))[0].setValue('changed')

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ connection: null })
    await wrapper.setProps({ open: true })
    await flushPromises()

    const values = (await wrapper.findAll('input')).map((i) => (i.element as HTMLInputElement).value)
    expect(values).toEqual(['', '', ''])
  })
})

describe('double submit protection', () => {
  it('emits save once for rapid repeated submits', async () => {
    const wrapper = await openDialog()
    await fillValidForm(wrapper)

    await wrapper.find('form').trigger('submit')
    await wrapper.find('form').trigger('submit')

    expect(wrapper.emitted('save')?.length).toBe(1)
    expect(wrapper.find('button[type="submit"]').attributes('disabled')).toBeDefined()
  })

  it('ignores submits while the parent reports the mutation as saving', async () => {
    const wrapper = await openDialog(null, true)
    await fillValidForm(wrapper)

    expect(wrapper.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    await wrapper.find('form').trigger('submit')

    expect(wrapper.emitted('save')).toBeUndefined()
  })

  it('blocks closing while saving', async () => {
    const wrapper = await openDialog(null, true)

    await wrapper.find('button.secondary-action').trigger('click')
    await wrapper.find('.dialog-backdrop').trigger('click')
    expect(wrapper.emitted('update:open')).toBeUndefined()
  })

  it('releases the lock after a failed mutation so the user can retry', async () => {
    const wrapper = await openDialog()
    await fillValidForm(wrapper)
    await wrapper.find('form').trigger('submit')
    expect(wrapper.emitted('save')?.length).toBe(1)

    // Parent mutation failed: saving went true and back to false, dialog open.
    await wrapper.setProps({ saving: true })
    await wrapper.setProps({ saving: false })
    await flushPromises()

    expect(wrapper.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
    await wrapper.find('form').trigger('submit')
    expect(wrapper.emitted('save')?.length).toBe(2)
  })
})

describe('endpoint probe (Проверить)', () => {
  interface TestArgs { url: string; accessToken?: string }

  function testConnectionArgs(): TestArgs | undefined {
    const call = invoke.mock.calls.find(([name]) => name === 'test_connection')
    return call?.[1] as TestArgs | undefined
  }

  it('probes with the current field values without saving', async () => {
    onInvoke('test_connection', () => ({ ok: true, latency_ms: 42 }))
    const wrapper = await openDialog()
    const inputs = await wrapper.findAll('input')
    await inputs[0].setValue('Local')
    await inputs[1].setValue('http://127.0.0.1:10100/?token=urltok')
    await inputs[2].setValue('fieldtok')

    await wrapper.find('button.test-action').trigger('click')
    await flushPromises()

    // The raw field values travel as-is (the backend resolves the endpoint),
    // the field token wins over the query token, and nothing is saved.
    expect(testConnectionArgs()).toEqual({ url: 'http://127.0.0.1:10100/?token=urltok', accessToken: 'fieldtok' })
    expect(wrapper.emitted('save')).toBeUndefined()
    expect(wrapper.find('.test-result').text()).toBe('Подключение установлено (42 мс)')
    expect(wrapper.find('.test-result').classes()).toContain('test-ok')
  })

  it('shows the RU category label for a classified failure', async () => {
    onInvoke('test_connection', () => ({ ok: false, error_kind: 'authentication', error_message: 'The server rejected the credentials' }))
    const wrapper = await openDialog()
    await (await wrapper.findAll('input'))[1].setValue('http://127.0.0.1:10100/sse')

    await wrapper.find('button.test-action').trigger('click')
    await flushPromises()

    expect(wrapper.find('.test-result').text()).toBe('Доступ запрещён — проверьте токен доступа')
    expect(wrapper.find('.test-result').classes()).toContain('test-fail')
  })

  it('falls back to the backend message for an unknown category', async () => {
    onInvoke('test_connection', () => ({ ok: false, error_kind: 'mystery', error_message: 'Novel upstream failure' }))
    const wrapper = await openDialog()
    await (await wrapper.findAll('input'))[1].setValue('http://echo.example/sse')

    await wrapper.find('button.test-action').trigger('click')
    await flushPromises()

    expect(wrapper.find('.test-result').text()).toBe('Novel upstream failure')
  })

  it('reports an IPC-level failure inline without emitting save', async () => {
    onInvoke('test_connection', () => { throw new Error('ipc refused') })
    const wrapper = await openDialog()
    await (await wrapper.findAll('input'))[1].setValue('http://echo.example/sse')

    await wrapper.find('button.test-action').trigger('click')
    await flushPromises()

    expect(wrapper.find('.test-result').text()).toBe('ipc refused')
    expect(wrapper.emitted('save')).toBeUndefined()
  })

  it('disables the button and blocks re-entry while the probe is in flight', async () => {
    const pending = invokePending('test_connection')
    const wrapper = await openDialog()
    await (await wrapper.findAll('input'))[1].setValue('http://echo.example/sse')

    await wrapper.find('button.test-action').trigger('click')
    await flushPromises()

    const button = wrapper.find('button.test-action')
    expect(button.attributes('disabled')).toBeDefined()
    expect(invokeCalls('test_connection')).toBe(1)

    await button.trigger('click')
    expect(invokeCalls('test_connection')).toBe(1)

    pending.resolve({ ok: true, latency_ms: 7 })
    await flushPromises()

    expect(wrapper.find('button.test-action').attributes('disabled')).toBeUndefined()
    expect(wrapper.find('.test-result').text()).toBe('Подключение установлено (7 мс)')
  })

  it('stays disabled while the parent reports the mutation as saving', async () => {
    onInvoke('test_connection', () => ({ ok: true, latency_ms: 1 }))
    const wrapper = await openDialog(null, true)

    expect(wrapper.find('button.test-action').attributes('disabled')).toBeDefined()
    await wrapper.find('button.test-action').trigger('click')

    expect(invokeCalls('test_connection')).toBe(0)
  })

  it('clears a stale result on reopen', async () => {
    onInvoke('test_connection', () => ({ ok: true, latency_ms: 5 }))
    const wrapper = await openDialog()
    await (await wrapper.findAll('input'))[1].setValue('http://echo.example/sse')
    await wrapper.find('button.test-action').trigger('click')
    await flushPromises()
    expect(wrapper.find('.test-result').exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(wrapper.find('.test-result').exists()).toBe(false)
  })
})
