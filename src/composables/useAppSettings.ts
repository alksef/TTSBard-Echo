/* ============================================================================
   App Settings Composable - ttsbard-echo
   ============================================================================ */

import { ref, computed, provide, inject, onScopeDispose, watch, type ComputedRef } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  AppSettingsDto,
  AppSettingsContext,
  Theme,
  ConnectionConfig,
  LoggingSettingsDto,
  HotkeySettingsDto,
  GeneralSettingsDto,
  WindowsSettingsDto,
} from '@/types'
import { APP_SETTINGS_KEY } from '@/types'

/* ==========================================================================
   Default context for components that call useAppSettings before provide
   ========================================================================== */
const defaultContext: AppSettingsContext = {
  settings: ref(null),
  isLoading: ref(false),
  error: ref(null),
  reload: async () => {},
  cleanup: () => {},
}

/* ==========================================================================
   Backend DTO (snake_case)
   ========================================================================== */
interface BackendWindowPositionDto {
  x?: number
  y?: number
}

interface BackendFloatingWindowDto {
  x?: number
  y?: number
  opacity: number
  bg_color: string
  clickthrough: boolean
  use_custom_color: boolean
  visible: boolean
}

interface BackendWindowsSettingsDto {
  main: BackendWindowPositionDto
  floating: BackendFloatingWindowDto
}

interface BackendAppSettingsDto {
  connections: ConnectionConfig[]
  logging: LoggingSettingsDto
  hotkeys: HotkeySettingsDto
  general: GeneralSettingsDto
  windows: BackendWindowsSettingsDto
}

/* ==========================================================================
   Constants
   ========================================================================== */
const MAX_RETRIES = 50
const RETRY_INTERVAL_MS = 100

/* ==========================================================================
   Convert backend DTO to frontend format
   ========================================================================== */
function convertBackendDto(backend: BackendAppSettingsDto): AppSettingsDto {
  return {
    connections: backend.connections,
    logging: backend.logging,
    hotkeys: backend.hotkeys,
    general: backend.general,
    windows: {
      main: backend.windows.main,
      floating: {
        x: backend.windows.floating.x,
        y: backend.windows.floating.y,
        opacity: backend.windows.floating.opacity,
        bg_color: backend.windows.floating.bg_color,
        clickthrough: backend.windows.floating.clickthrough,
        use_custom_color: backend.windows.floating.use_custom_color,
        visible: backend.windows.floating.visible,
      },
    },
  }
}

/* ==========================================================================
   Create App Settings Context (for root component)
   ========================================================================== */
export function createAppSettings(): AppSettingsContext {
  const settings = ref<AppSettingsDto | null>(null)
  const isLoading = ref(false)
  const error = ref<string | null>(null)

  let reloadRequested = false

  // Registered synchronously during setup so scope disposal actually captures
  // listeners that resolve later (async onScopeDispose would be a no-op).
  const unlistenFns: Array<() => void> = []
  const disposeListeners = () => {
    unlistenFns.splice(0).forEach(fn => fn())
  }
  onScopeDispose(disposeListeners)

  /**
   * Wait for backend to be ready
   */
  async function waitForBackendReady(): Promise<boolean> {
    for (let i = 0; i < MAX_RETRIES; i++) {
      try {
        const ready = await invoke<boolean>('is_backend_ready')
        if (ready) {
          console.log('[useAppSettings] Backend is ready')
          return true
        }
      } catch (e) {
        console.warn('[useAppSettings] Error checking backend ready:', e)
      }

      try {
        await invoke('confirm_backend_ready')
      } catch {
        // Ignore errors
      }

      await new Promise(resolve => setTimeout(resolve, RETRY_INTERVAL_MS))
    }

    console.warn('[useAppSettings] Backend not ready after timeout')
    return false
  }

  /**
   * Load all settings from backend
   */
  async function load() {
    if (isLoading.value) {
      // Keep the invalidation: the current request may have started before a
      // just-finished settings mutation. A second pass must read the new value.
      reloadRequested = true
      console.log('[useAppSettings] Already loading, queueing reload')
      return
    }

    isLoading.value = true
    error.value = null

    try {
      console.log('[useAppSettings] Loading settings...')

      const ready = await waitForBackendReady()
      if (!ready) {
        throw new Error('Backend not ready after timeout')
      }

      const backendData = await invoke<BackendAppSettingsDto>('get_all_app_settings')
      const data = convertBackendDto(backendData)
      settings.value = data

      console.log('[useAppSettings] Settings loaded')
    } catch (e) {
      console.error('[useAppSettings] Failed to load settings:', e)
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      isLoading.value = false
      if (reloadRequested) {
        reloadRequested = false
        void load()
      }
    }
  }

  /**
   * Reload settings
   */
  async function reload() {
    console.log('[useAppSettings] Reloading settings...')
    await load()
  }

  /**
   * Setup event listeners; cleanup is owned by onScopeDispose/disposeListeners.
   */
  async function setupEventListeners() {
    unlistenFns.push(await listen('settings-changed', reload))
    unlistenFns.push(await listen<string>('theme-changed', ({ payload }) => {
      if (payload === 'dark' || payload === 'light') applyTheme(payload)
    }))

    unlistenFns.push(await listen('backend-ready', () => {
      console.log('[useAppSettings] Received backend-ready event')
      if (!settings.value) {
        load()
      }
    }))
  }

  // Initial load
  load()

  // Setup event listeners with cleanup
  void setupEventListeners()

  return {
    settings,
    isLoading,
    error,
    reload,
    cleanup: disposeListeners,
  }
}

/* ==========================================================================
   Provide App Settings (App.vue)
   ========================================================================== */
export function provideAppSettings(): AppSettingsContext {
  const context = createAppSettings()
  provide(APP_SETTINGS_KEY, context)
  return context
}

/* ==========================================================================
   Inject App Settings (child components)
   ========================================================================== */
export function useAppSettings(): AppSettingsContext {
  const context = inject<AppSettingsContext>(APP_SETTINGS_KEY)

  if (!context) {
    console.warn('[useAppSettings] Context not found, returning default. This may happen if called during setup of the providing component.')
    return defaultContext
  }

  return context
}

/* ==========================================================================
   Specialized Composables
   ========================================================================== */

/**
 * Connections Settings
 */
export function useConnectionsSettings(): ComputedRef<ConnectionConfig[]> {
  const { settings } = useAppSettings()
  return computed(() => settings.value?.connections ?? [])
}

/**
 * Logging Settings
 */
export function useLoggingSettings(): ComputedRef<LoggingSettingsDto | undefined> {
  const { settings } = useAppSettings()
  return computed(() => settings.value?.logging)
}

/**
 * General Settings
 */
export function useGeneralSettings(): ComputedRef<GeneralSettingsDto | undefined> {
  const { settings } = useAppSettings()
  return computed(() => settings.value?.general)
}

/**
 * Windows Settings
 */
export function useWindowsSettings(): ComputedRef<WindowsSettingsDto | undefined> {
  const { settings } = useAppSettings()
  return computed(() => settings.value?.windows)
}

/**
 * Hotkey Settings
 */
export function useHotkeySettings(): ComputedRef<HotkeySettingsDto | undefined> {
  const { settings } = useAppSettings()
  return computed(() => settings.value?.hotkeys)
}

/**
 * Theme
 */
export function useTheme(): ComputedRef<Theme> {
  const { settings } = useAppSettings()
  return computed(() => settings.value?.general?.theme ?? 'dark')
}

/**
 * Watch theme changes and apply them
 */
export function useThemeWatcher(context: AppSettingsContext = useAppSettings()) {
  const { settings } = context
  const theme = computed<Theme>(() => settings.value?.general?.theme ?? 'dark')

  watch(() => settings.value?.general?.theme, (newTheme) => {
    // Until the backend snapshot exists, keep the early localStorage value.
    if (newTheme === 'dark' || newTheme === 'light') applyTheme(newTheme)
  }, { immediate: true })

  return { theme }
}

export function applyTheme(theme: Theme) {
  const normalizedTheme: Theme = theme === 'light' ? 'light' : 'dark'
  localStorage.setItem('app-theme', normalizedTheme)
  document.documentElement.setAttribute('data-theme', normalizedTheme)
  document.documentElement.style.colorScheme = normalizedTheme
}
