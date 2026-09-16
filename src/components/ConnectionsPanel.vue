<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Globe, Pencil, Plus, Plug, Square, Trash2, Loader2, Download } from 'lucide-vue-next'
import { invoke } from '@tauri-apps/api/core'
import ConnectionFormDialog from './connections/ConnectionFormDialog.vue'
import { useConnections } from '@/composables/useConnections'
import { connectionStatusLabel } from '@/lib/connectionStatusLabels'
import type { ConnectionConfig } from '@/types/settings'
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'

const {
  connections,
  loading,
  error,
  add,
  update,
  remove,
  connect,
  disconnect,
} = useConnections()

const dialogOpen = ref(false)
const editingConnection = ref<ConnectionConfig | null>(null)
const busyId = ref<string | null>(null)
const saving = ref(false)
const actionError = ref<string | null>(null)
const actionInfo = ref<string | null>(null)
const exporting = ref(false)

const visibleError = computed(() => error.value || actionError.value)

const props = defineProps<{ floating?: boolean }>()

watch(connections, async (items) => {
  if (!props.floating) return
  const minHeight = 80
  const panelHeight = 60
  const maxHeight = 4 * panelHeight + minHeight
  const height = Math.min(maxHeight, Math.max(minHeight, items.length * panelHeight + minHeight))
  await getCurrentWindow().setSize(new LogicalSize(350, height))
}, { immediate: true })

function openAdd() {
  editingConnection.value = null
  dialogOpen.value = true
}

function openEdit(config: ConnectionConfig) {
  editingConnection.value = config
  dialogOpen.value = true
}

// Only runs on this explicit user action — the backend never exports on its
// own (roadmap 011, task 008). The result is the written file path.
async function exportDiagnostics() {
  if (exporting.value) return
  exporting.value = true
  actionError.value = null
  actionInfo.value = null
  try {
    const path = await invoke<string>('export_diagnostics')
    actionInfo.value = `Диагностика сохранена: ${path}`
  } catch (reason) {
    actionError.value = reason instanceof Error ? reason.message : String(reason)
  } finally {
    exporting.value = false
  }
}

async function save(config: ConnectionConfig) {
  if (saving.value) return
  saving.value = true
  try {
    if (editingConnection.value) {
      await update(config.id, config)
    } else {
      await add(config)
    }
    dialogOpen.value = false
    editingConnection.value = null
  } catch (reason) {
    // Keep the dialog open with the entered values so the user can retry.
    actionError.value = reason instanceof Error ? reason.message : String(reason)
  } finally {
    saving.value = false
  }
}

async function toggleConnection(id: string, status: string) {
  if (busyId.value) return
  busyId.value = id
  actionError.value = null
  try {
    if (status === 'Connected' || status === 'Connecting') await disconnect(id)
    else await connect(id)
  } catch (reason) {
    actionError.value = reason instanceof Error ? reason.message : String(reason)
  } finally {
    busyId.value = null
  }
}

async function deleteConnection(id: string) {
  if (busyId.value || !window.confirm('Удалить это подключение?')) return
  busyId.value = id
  actionError.value = null
  try {
    await remove(id)
  } catch (reason) {
    actionError.value = reason instanceof Error ? reason.message : String(reason)
  } finally {
    busyId.value = null
  }
}
</script>

<template>
  <section class="connections-panel" :class="{ 'floating-mode': floating }">
    <header v-if="!floating" class="connections-header">
      <div>
        <h1>Подключения</h1>
      </div>
      <div class="header-actions">
        <button
          class="secondary-action export-action"
          type="button"
          :disabled="exporting"
          aria-label="Экспорт диагностики"
          title="Экспорт диагностики"
          @click="exportDiagnostics"
        >
          <Download :size="15" />
          Экспорт диагностики
        </button>
        <button class="primary-action icon-action" type="button" aria-label="Добавить подключение" title="Добавить подключение" @click="openAdd">
          <Plus :size="16" />
        </button>
      </div>
    </header>

    <p v-if="visibleError" class="panel-error">{{ visibleError }}</p>
    <p v-if="actionInfo" class="panel-info">{{ actionInfo }}</p>
    <div v-if="loading && connections.length === 0" class="empty-state">Загрузка подключений…</div>
    <div v-else-if="connections.length === 0" class="empty-state">
      <Globe :size="42" />
      <strong>Подключений пока нет</strong>
      <span>Добавьте SSE-сервер, чтобы начать получать события.</span>
      <button v-if="!floating" class="secondary-action" type="button" @click="openAdd">Добавить подключение</button>
    </div>

    <div v-else class="connections-list">
      <article v-for="connection in connections" :key="connection.id" class="connection-card">
        <div class="card-main">
          <div class="card-title-row">
            <Globe :size="16" />
            <h2>{{ connection.name }}</h2>
            <span class="status" :class="connection.runtime.status.toLowerCase()">
              <Loader2
                v-if="connection.runtime.status === 'Connecting' || connection.runtime.status === 'Retrying'"
                :size="12"
                class="spin"
              />
              {{ connectionStatusLabel(connection.runtime) }}
            </span>
          </div>
          <code v-if="!floating">{{ connection.url }}</code>
          <p v-if="connection.runtime.lastMessage" class="last-message">{{ connection.runtime.lastMessage }}</p>
        </div>
        <div v-if="!floating" class="card-actions">
          <button class="icon-action" type="button" title="Изменить" @click="openEdit(connection)"><Pencil :size="15" /></button>
          <button class="icon-action danger" type="button" title="Удалить" @click="deleteConnection(connection.id)"><Trash2 :size="15" /></button>
        </div>
        <button
          class="icon-action power-action"
          type="button"
          :disabled="busyId === connection.id"
          :title="connection.runtime.status === 'Connected' ? 'Отключить' : 'Подключить'"
          @click="toggleConnection(connection.id, connection.runtime.status)"
        >
          <Square v-if="connection.runtime.status === 'Connected'" :size="15" />
          <Plug v-else :size="15" />
        </button>
      </article>
    </div>

    <ConnectionFormDialog
      v-if="!floating"
      v-model:open="dialogOpen"
      :connection="editingConnection"
      :saving="saving"
      @save="save"
    />
  </section>
</template>

<style scoped>
.connections-panel { display: flex; flex-direction: column; gap: 1rem; max-width: 960px; margin: 0 auto; }
.connections-header, .card-title-row, .card-actions { display: flex; align-items: center; }
.connections-header { justify-content: space-between; gap: 1rem; }
.eyebrow { margin: 0 0 .25rem; color: var(--color-accent); font-size: .75rem; text-transform: uppercase; letter-spacing: .12em; }
h1, h2 { margin: 0; color: var(--color-text-primary); }
h1 { font-size: 1.65rem; }
h2 { font-size: .95rem; }
.primary-action, .secondary-action { display: inline-flex; align-items: center; gap: .45rem; border: 0; border-radius: 8px; padding: .65rem .9rem; cursor: pointer; font-weight: 600; }
.primary-action { background: var(--color-accent); color: white; }
.secondary-action { background: var(--color-bg-field); color: var(--color-text-primary); border: 1px solid var(--color-border); }
.panel-error { margin: 0; padding: .75rem 1rem; color: var(--color-danger); background: rgba(var(--rgb-danger), .1); border: 1px solid rgba(var(--rgb-danger), .25); border-radius: 8px; }
.header-actions { display: flex; align-items: center; gap: .5rem; }
.panel-info { margin: 0; padding: .75rem 1rem; color: var(--color-success); background: rgba(var(--rgb-success), .1); border: 1px solid rgba(var(--rgb-success), .25); border-radius: 8px; word-break: break-all; }
.empty-state { min-height: 240px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: .65rem; color: var(--color-text-muted); text-align: center; }
.empty-state svg { opacity: .55; }
.empty-state strong { color: var(--color-text-primary); }
.connections-list { display: grid; gap: .75rem; }
.connection-card { display: flex; align-items: center; gap: .75rem; padding: 1rem; background: var(--color-bg-overlay); border: 1px solid var(--color-border); border-radius: 12px; }
.card-main { flex: 1; min-width: 0; }
.card-title-row { gap: .5rem; margin-bottom: .45rem; }
.card-title-row > svg { color: var(--color-accent); flex: 0 0 auto; }
code { display: block; overflow: hidden; color: var(--color-text-muted); font-size: .78rem; text-overflow: ellipsis; white-space: nowrap; }
.last-message { margin: .45rem 0 0; color: var(--color-text-secondary); font-size: .82rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.status { display: inline-flex; align-items: center; gap: .25rem; margin-left: auto; padding: .2rem .45rem; border-radius: 999px; color: var(--color-text-muted); background: var(--color-bg-field); font-size: .7rem; white-space: nowrap; }
.status.connected { color: var(--color-success); background: rgba(var(--rgb-success), .12); }
.status.connecting, .status.retrying { color: var(--color-warning); background: rgba(var(--rgb-warning), .12); }
.status.error { color: var(--color-danger); background: rgba(var(--rgb-danger), .12); }
.card-actions { gap: .25rem; }
.icon-action { display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0; color: var(--color-text-secondary); background: transparent; border: 1px solid transparent; border-radius: 7px; cursor: pointer; }
.icon-action:hover:not(:disabled) { color: var(--color-text-primary); background: var(--color-bg-field-hover); border-color: var(--color-border); }
.icon-action.danger:hover:not(:disabled) { color: var(--color-danger); }
.power-action { color: var(--color-accent); }
.icon-action:disabled { opacity: .5; cursor: wait; }
.floating-mode { max-width: none; padding: .5rem; }
.floating-mode .connection-card { padding: .65rem .75rem; }
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
</style>
