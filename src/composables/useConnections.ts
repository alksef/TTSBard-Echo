import { computed, onUnmounted, ref, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  ConnectionConfig,
  ConnectionRuntimeSnapshot,
  ConnectionRuntimeSnapshotDto,
  ConnectionStatusEventPayload,
} from '@/types/settings'
import type { ConnectionStatus } from '@/types/types'

interface ConnectionView extends ConnectionConfig {
  runtime: ConnectionRuntimeSnapshot
}

export function normalizeConnectionError(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  return 'Неизвестная ошибка подключения'
}

/** Map the snake_case snapshot DTO to the camelCase runtime state. */
export function mapConnectionRuntimeSnapshot(dto: ConnectionRuntimeSnapshotDto): ConnectionRuntimeSnapshot {
  return {
    id: dto.id,
    status: dto.status,
    lastMessage: dto.last_message ?? undefined,
    errorKind: dto.error_kind ?? undefined,
    errorMessage: dto.error_message ?? undefined,
    attempt: dto.attempt ?? undefined,
    maxAttempts: dto.max_attempts ?? undefined,
    nextRetryInSecs: dto.next_retry_in_secs ?? undefined,
    isTyping: dto.is_typing,
    previewText: dto.preview_text ?? undefined,
  }
}

const MUTATION_TIMEOUT_MS = 10_000

async function withTimeout<T>(promise: Promise<T>, label: string): Promise<T> {
  let timeoutId: number | undefined
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = window.setTimeout(() => reject(new Error(`${label}: превышено время ожидания`)), MUTATION_TIMEOUT_MS)
  })
  try {
    return await Promise.race([promise, timeout])
  } finally {
    if (timeoutId !== undefined) window.clearTimeout(timeoutId)
  }
}

export function useConnections(): {
  configs: Ref<ConnectionConfig[]>
  runtimeStates: Ref<Map<string, ConnectionRuntimeSnapshot>>
  connections: Ref<ConnectionView[]>
  loading: Ref<boolean>
  error: Ref<string | null>
  reload: () => Promise<void>
  add: (config: ConnectionConfig) => Promise<void>
  update: (id: string, config: ConnectionConfig) => Promise<void>
  remove: (id: string) => Promise<void>
  connect: (id: string) => Promise<void>
  disconnect: (id: string) => Promise<void>
} {
  const configs = ref<ConnectionConfig[]>([])
  const runtimeStates = ref(new Map<string, ConnectionRuntimeSnapshot>())
  const loading = ref(false)
  const error = ref<string | null>(null)
  let unlistenFns: UnlistenFn[] = []
  let subscribed = false
  let disposed = false
  let reloadRequest = 0

  const connections = computed(() => configs.value.map((config) => ({
    ...config,
    runtime: runtimeStates.value.get(config.id) ?? {
      id: config.id,
      status: 'Disconnected' as ConnectionStatus,
      isTyping: false,
    },
  })))

  function applySnapshot(snapshot: ConnectionRuntimeSnapshot[]) {
    runtimeStates.value = new Map(snapshot.map((state) => [state.id, state]))
  }

  async function reload() {
    const request = ++reloadRequest
    loading.value = true
    if (request === reloadRequest) error.value = null
    try {
      const [nextConfigs, snapshot] = await Promise.all([
        invoke<ConnectionConfig[]>('get_connections'),
        invoke<ConnectionRuntimeSnapshotDto[]>('get_connection_runtime_snapshot'),
      ])
      if (request === reloadRequest) {
        configs.value = nextConfigs
        applySnapshot(snapshot.map(mapConnectionRuntimeSnapshot))
      }
    } catch (reason) {
      if (request === reloadRequest) error.value = normalizeConnectionError(reason)
      throw reason
    } finally {
      if (request === reloadRequest) loading.value = false
    }
  }

  async function subscribe() {
    if (subscribed) return
    subscribed = true
    const listeners = await Promise.all([
      listen<ConnectionStatusEventPayload>('connection-status-changed', ({ payload }) => {
        // The structured payload carries only the fields its status implies;
        // detail fields left out by the backend must not survive from a
        // previous Error/Retrying state.
        const previous = runtimeStates.value.get(payload.id)
        runtimeStates.value.set(payload.id, {
          ...previous,
          id: payload.id,
          status: payload.status,
          errorKind: payload.errorKind,
          errorMessage: payload.errorMessage,
          attempt: payload.attempt,
          maxAttempts: payload.maxAttempts,
          nextRetryInSecs: payload.nextRetryInSecs,
          isTyping: previous?.isTyping ?? false,
        })
      }),
      listen<[string, string]>('message-received', ({ payload }) => {
        const [id, lastMessage] = payload
        const previous = runtimeStates.value.get(id)
        runtimeStates.value.set(id, { ...previous, id, lastMessage, isTyping: false, previewText: undefined, status: previous?.status ?? 'Disconnected' })
      }),
      listen<string>('message-cleared', ({ payload: id }) => {
        const previous = runtimeStates.value.get(id)
        if (previous) runtimeStates.value.set(id, { ...previous, lastMessage: undefined })
      }),
      listen<{ id: string; isTyping: boolean; previewText?: string }>('typing-changed', ({ payload }) => {
        const { id, isTyping, previewText } = payload
        const previous = runtimeStates.value.get(id)
        runtimeStates.value.set(id, { ...previous, id, isTyping, previewText, status: previous?.status ?? 'Disconnected' })
      }),
      listen('connections-changed', () => { void reload() }),
      listen<string>('connection-removed', ({ payload }) => {
        runtimeStates.value.delete(payload)
      }),
    ])
    if (disposed) {
      listeners.forEach((unlisten) => unlisten())
      return
    }
    unlistenFns = listeners
  }

  async function mutation(command: string, args: Record<string, unknown>) {
    error.value = null
    try {
      await withTimeout(invoke(command, args), `Операция ${command}`)
    } finally {
      // The backend event/snapshot is authoritative even when the IPC call
      // times out after the mutation was accepted.
      await reload()
    }
  }

  void subscribe().then(reload).catch((reason) => {
    error.value = normalizeConnectionError(reason)
  })

  onUnmounted(() => {
    disposed = true
    unlistenFns.splice(0).forEach((unlisten) => unlisten())
    subscribed = false
  })

  return {
    configs,
    runtimeStates,
    connections,
    loading,
    error,
    reload,
    add: (config) => mutation('add_connection', { config }),
    update: (id, config) => mutation('update_connection', { id, config }),
    remove: (id) => mutation('remove_connection', { id }),
    connect: (id) => mutation('connect_connection', { id }),
    disconnect: (id) => mutation('disconnect_connection', { id }),
  }
}
