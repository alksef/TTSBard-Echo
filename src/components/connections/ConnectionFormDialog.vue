<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ConnectionConfig } from '@/types/settings'
import type { ConnectionTestResultDto } from '@/types/types'
import { connectionErrorKindLabel } from '@/lib/connectionStatusLabels'

const DEFAULT_CONNECTION_URL = ''

const props = withDefaults(
  defineProps<{ open: boolean; connection: ConnectionConfig | null; saving?: boolean }>(),
  { saving: false },
)
const emit = defineEmits<{ 'update:open': [value: boolean]; save: [config: ConnectionConfig] }>()

const name = ref('')
const url = ref('')
const token = ref('')
const formError = ref<string | null>(null)
const submitting = ref(false)
// Pre-save endpoint probe state: separate from the save lock, its own request
// id so a stale answer can never paint a result into a reopened dialog.
const testing = ref(false)
const testResult = ref<{ ok: boolean; text: string } | null>(null)
let testRequest = 0
const isEdit = computed(() => props.connection !== null)
// The parent owns the async IPC mutation and reports it through `saving`;
// `submitting` covers the synchronous emit itself.
const locked = computed(() => submitting.value || props.saving)

function reset() {
  name.value = props.connection?.name ?? ''
  url.value = props.connection?.url ?? DEFAULT_CONNECTION_URL
  token.value = props.connection?.access_token ?? ''
  formError.value = null
  submitting.value = false
  testRequest += 1
  testResult.value = null
  testing.value = false
}

watch(() => [props.open, props.connection], () => { if (props.open) void nextTick(reset) }, { immediate: true })

// A failed mutation releases the lock (the parent flips `saving` back) so the
// user can retry; a successful one closes the dialog via `open`.
watch(() => props.saving, (now, was) => {
  if (was && !now && props.open) submitting.value = false
})

function close() {
  if (!locked.value) emit('update:open', false)
}

/** RU text of a probe result: success with latency, otherwise the category
 * label; unknown or absent kinds fall back to the backend's fixed message. */
function describeTestResult(result: ConnectionTestResultDto): { ok: boolean; text: string } {
  if (result.ok) return { ok: true, text: `Подключение установлено (${result.latency_ms ?? 0} мс)` }
  const kindLabel = result.error_kind !== undefined ? connectionErrorKindLabel(result.error_kind) : undefined
  return { ok: false, text: kindLabel ?? result.error_message ?? 'Ошибка подключения' }
}

async function runTest() {
  if (locked.value || testing.value) return
  testResult.value = null
  const trimmedUrl = url.value.trim()
  if (!trimmedUrl) { formError.value = 'Введите корректный URL'; return }
  // The raw field values go as-is: the backend resolves the endpoint the same
  // way as the live path (/sse default, token out of the query into the
  // cookie channel) and classifies the outcome with the shared taxonomy.
  const request = ++testRequest
  testing.value = true
  try {
    const result = await invoke<ConnectionTestResultDto>('test_connection', {
      url: trimmedUrl,
      accessToken: token.value.trim() || undefined,
    })
    if (request !== testRequest) return
    testResult.value = describeTestResult(result)
  } catch (reason) {
    if (request !== testRequest) return
    testResult.value = { ok: false, text: reason instanceof Error ? reason.message : String(reason) }
  } finally {
    if (request === testRequest) testing.value = false
  }
}

async function submit() {
  if (locked.value) return
  const trimmedName = name.value.trim()
  const trimmedUrl = url.value.trim()
  if (!trimmedName) { formError.value = 'Введите название подключения'; return }
  if (trimmedName.length > 256) { formError.value = 'Название слишком длинное'; return }
  let parsed: URL
  try {
    parsed = new URL(trimmedUrl)
    if (!['http:', 'https:'].includes(parsed.protocol) || !parsed.hostname) throw new Error('Нужен полный внешний адрес TTSBard')
  } catch (reason) {
    formError.value = reason instanceof Error ? reason.message : 'Введите корректный URL'
    return
  }
  if (parsed.pathname === '' || parsed.pathname === '/') parsed.pathname = '/sse'
  const tokenFromUrl = parsed.searchParams.get('token')?.trim()
  parsed.searchParams.delete('token')
  parsed.hash = ''
  submitting.value = true
  formError.value = null
  try {
    emit('save', {
      id: props.connection?.id ?? crypto.randomUUID(),
      name: trimmedName,
      url: parsed.toString(),
      enabled: props.connection?.enabled ?? true,
      access_token: token.value.trim() || tokenFromUrl || undefined,
    })
    // The parent owns the async IPC mutation. Keep the dialog open and locked
    // until the parent closes it (success) or releases `saving` (failure,
    // retry allowed).
  } catch (reason) {
    formError.value = reason instanceof Error ? reason.message : String(reason)
    submitting.value = false
  }
}
</script>

<template>
  <Transition name="modal">
    <div v-if="open" class="dialog-backdrop" @click.self="close">
      <section class="dialog" role="dialog" aria-modal="true" aria-label="Настройка подключения">
        <form class="dialog-body" @submit.prevent="submit">
          <label>Название<input v-model="name" autofocus maxlength="256" placeholder="Мой TTSBard" /></label>
          <label>Внешний адрес<input v-model="url" type="url" placeholder="Вставьте внешний адрес из TTSBard" /></label>
          <label>Токен доступа <span>(необязательно)</span><input v-model="token" type="password" autocomplete="off" placeholder="Токен для авторизации" /></label>
          <div class="test-row">
            <button class="test-action" type="button" :disabled="locked || testing" @click="runTest">{{ testing ? 'Проверка…' : 'Проверить' }}</button>
            <p v-if="testResult" class="test-result" :class="testResult.ok ? 'test-ok' : 'test-fail'" role="status">{{ testResult.text }}</p>
          </div>
          <p v-if="formError" class="form-error">{{ formError }}</p>
          <footer class="dialog-actions">
            <button class="secondary-action" type="button" :disabled="locked" @click="close">Отмена</button>
            <button class="primary-action" type="submit" :disabled="locked">{{ isEdit ? 'Сохранить' : 'Добавить' }}</button>
          </footer>
        </form>
      </section>
    </div>
  </Transition>
</template>

<style scoped>
.dialog-backdrop { position: fixed; inset: 0; z-index: 1001; display: grid; place-items: center; padding: 1rem; background: rgba(0,0,0,.48); backdrop-filter: blur(4px); }
.dialog { width: min(100%, 480px); overflow: hidden; background: var(--color-bg-panel); border: 1px solid var(--color-border); border-radius: 14px; box-shadow: var(--shadow-soft); }
.dialog-actions { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
.dialog-body { display: grid; gap: .9rem; padding: 1.15rem; }
label { display: grid; gap: .35rem; color: var(--color-text-secondary); font-size: .8rem; font-weight: 600; }
label span { font-weight: 400; color: var(--color-text-muted); }
input { width: 100%; box-sizing: border-box; padding: .65rem .75rem; color: var(--color-text-primary); background: var(--color-bg-field); border: 1px solid var(--color-border); border-radius: 8px; }
input:focus { outline: none; border-color: var(--color-accent); box-shadow: 0 0 0 3px rgba(var(--rgb-accent), .12); }
.form-error { margin: 0; color: var(--color-danger); font-size: .8rem; }
.test-row { display: flex; align-items: center; flex-wrap: wrap; gap: .6rem; }
.test-action { padding: .5rem .8rem; color: var(--color-text-primary); background: var(--color-bg-field); border: 1px solid var(--color-border); border-radius: 8px; cursor: pointer; font-weight: 600; }
.test-action:disabled { opacity: .55; cursor: wait; }
.test-result { margin: 0; font-size: .8rem; }
.test-ok { color: var(--color-success); }
.test-fail { color: var(--color-danger); }
.dialog-actions { justify-content: flex-end; margin-top: .2rem; }
.primary-action, .secondary-action { padding: .6rem .9rem; border-radius: 8px; border: 1px solid var(--color-border); cursor: pointer; font-weight: 600; }
.primary-action { color: white; background: var(--color-accent); border-color: var(--color-accent); }
.secondary-action { color: var(--color-text-primary); background: var(--color-bg-field); }
button:disabled { opacity: .55; cursor: wait; }
.modal-enter-active, .modal-leave-active { transition: opacity .15s ease; }
.modal-enter-from, .modal-leave-to { opacity: 0; }
</style>
