/* ============================================================================
   Shared backend payload fixtures for frontend tests.
   Shapes must stay compatible with src/types/settings.ts.
   ============================================================================ */
import type {
  ConnectionConfig,
  ConnectionRuntimeSnapshot,
  ConnectionRuntimeSnapshotDto,
} from '@/types/settings'
import type { AppSettingsDto, Theme } from '@/types'

export function connectionConfig(id: string, overrides: Partial<ConnectionConfig> = {}): ConnectionConfig {
  return {
    id,
    name: `conn-${id}`,
    url: `http://127.0.0.1:10100/${id}/sse`,
    enabled: true,
    ...overrides,
  }
}

export function runtimeSnapshot(
  id: string,
  status: ConnectionRuntimeSnapshot['status'] = 'Disconnected',
  overrides: Partial<ConnectionRuntimeSnapshot> = {},
): ConnectionRuntimeSnapshot {
  return {
    id,
    status,
    isTyping: false,
    ...overrides,
  }
}

/** Wire DTO returned by get_connection_runtime_snapshot (snake_case, nulls). */
export function runtimeSnapshotDto(
  id: string,
  status: ConnectionRuntimeSnapshotDto['status'] = 'Disconnected',
  overrides: Partial<ConnectionRuntimeSnapshotDto> = {},
): ConnectionRuntimeSnapshotDto {
  return {
    id,
    status,
    last_message: null,
    error_kind: null,
    error_message: null,
    attempt: null,
    max_attempts: null,
    next_retry_in_secs: null,
    is_typing: false,
    preview_text: null,
    ...overrides,
  }
}

export function appSettingsDto(theme: Theme = 'dark'): AppSettingsDto {
  return {
    connections: [connectionConfig('c1')],
    logging: { enabled: false, level: 'info', module_levels: {} },
    hotkeys: { enabled: true },
    general: { exclude_from_capture: false, theme, message_clear_interval_seconds: 15 },
    windows: {
      main: {},
      floating: {
        x: 10,
        y: 20,
        opacity: 90,
        bg_color: '#101014',
        clickthrough: false,
        use_custom_color: false,
        visible: false,
      },
    },
  }
}

/** Backend DTO shape as returned by get_all_app_settings (snake_case already). */
export const backendSettingsDto = appSettingsDto
