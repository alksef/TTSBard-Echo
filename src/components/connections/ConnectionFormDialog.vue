<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import type { ConnectionConfig } from '@/types/settings'

const DEFAULT_CONNECTION_URL = 'http://127.0.0.1:10100/sse'

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

async function submit() {
  if (locked.value) return
  const trimmedName = name.value.trim()
  const trimmedUrl = url.value.trim()
  if (!trimmedName) { formError.value = 'Введите название подключения'; return }
  if (trimmedName.length > 256) { formError.value = 'Название слишком длинное'; return }
  let parsed: URL
  try {
    parsed = new URL(trimmedUrl)
    if (!['http:', 'https:'].includes(parsed.protocol) || !parsed.hostname) throw new Error('Нужен полный HTTP(S)-URL с адресом сервера')
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
          <label>Название<input v-model="name" autofocus maxlength="256" placeholder="Мой SSE-сервер" /></label>
          <label>URL<input v-model="url" type="url" :placeholder="DEFAULT_CONNECTION_URL" /></label>
          <label>Токен доступа <span>(необязательно)</span><input v-model="token" type="password" autocomplete="off" placeholder="Токен для авторизации" /></label>
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
.dialog-actions { justify-content: flex-end; margin-top: .2rem; }
.primary-action, .secondary-action { padding: .6rem .9rem; border-radius: 8px; border: 1px solid var(--color-border); cursor: pointer; font-weight: 600; }
.primary-action { color: white; background: var(--color-accent); border-color: var(--color-accent); }
.secondary-action { color: var(--color-text-primary); background: var(--color-bg-field); }
button:disabled { opacity: .55; cursor: wait; }
.modal-enter-active, .modal-leave-active { transition: opacity .15s ease; }
.modal-enter-from, .modal-leave-to { opacity: 0; }
</style>
