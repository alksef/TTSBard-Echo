/* ============================================================================
    Settings Types - ttsbard-echo
    ============================================================================ */

import { InjectionKey, Ref } from 'vue'

/* ==========================================================================
    Theme
    ========================================================================== */
export type Theme = 'dark' | 'light'

/* ==========================================================================
    Connections
    ========================================================================== */
export interface ConnectionConfig {
  id: string
  name: string
  url: string
  enabled: boolean
  access_token?: string
}

export interface ConnectionRuntimeSnapshot {
  id: string
  status: import('./types').ConnectionStatus
  lastMessage?: string
  /** Machine-readable failure category of the terminal `Error` status. */
  errorKind?: import('./types').ConnectionErrorKind
  /** Fixed English failure text of the terminal `Error` status. */
  errorMessage?: string
  /** Retry progress of the `Retrying` status: upcoming attempt (1-based)... */
  attempt?: number
  /** ...out of this many attempts in the cycle... */
  maxAttempts?: number
  /** ...waiting this many seconds before it starts. */
  nextRetryInSecs?: number
  isTyping: boolean
  previewText?: string
}

/**
 * Wire shape of the backend `get_connection_runtime_snapshot` DTO:
 * snake_case fields, `null` where no detail applies (roadmap 011, decision B).
 * Mapped to {@link ConnectionRuntimeSnapshot} in `useConnections`.
 */
export interface ConnectionRuntimeSnapshotDto {
  id: string
  status: import('./types').ConnectionStatus
  last_message: string | null
  error_kind: import('./types').ConnectionErrorKind | null
  error_message: string | null
  attempt: number | null
  max_attempts: number | null
  next_retry_in_secs: number | null
  is_typing: boolean
  preview_text: string | null
}

/**
 * Wire payload of the `connection-status-changed` event (roadmap 011,
 * decision B): plain statuses carry exactly `{id, status}`, `Retrying` adds
 * the attempt fields, `Error` adds `errorKind`/`errorMessage`.
 */
export interface ConnectionStatusEventPayload {
  id: string
  status: import('./types').ConnectionStatus
  errorKind?: import('./types').ConnectionErrorKind
  errorMessage?: string
  attempt?: number
  maxAttempts?: number
  nextRetryInSecs?: number
}

/* ==========================================================================
    Logging
    ========================================================================== */
export interface LoggingSettingsDto {
  enabled: boolean
  level: string
  module_levels: Record<string, string>
}

/* ==========================================================================
    Hotkeys
    ========================================================================== */
export interface HotkeySettingsDto {
  enabled: boolean
  toggle_window?: string
}

/* ==========================================================================
    General
    ========================================================================== */
export interface GeneralSettingsDto {
  exclude_from_capture: boolean
  hide_on_minimize: boolean
  theme?: Theme
  message_clear_interval_seconds: number
}

/* ==========================================================================
    Windows
    ========================================================================== */
export interface WindowPositionDto {
  x?: number
  y?: number
}

export interface FloatingWindowSettingsDto {
  x?: number
  y?: number
  opacity: number
  bg_color: string
  clickthrough: boolean
  use_custom_color: boolean
  visible: boolean
}

export interface FloatingAppearanceDto {
  opacity: number
  bg_color: string
  use_custom_color: boolean
  clickthrough: boolean
}

export function opacityToTransparency(opacity: number): number {
  return 100 - Math.min(100, Math.max(10, opacity))
}

export function transparencyToOpacity(transparency: number): number {
  return 100 - Math.min(90, Math.max(0, transparency))
}

export interface WindowsSettingsDto {
  main: WindowPositionDto
  floating: FloatingWindowSettingsDto
}

/* ==========================================================================
    Main Settings DTO
    ========================================================================== */
export interface AppSettingsDto {
  connections: ConnectionConfig[]
  logging: LoggingSettingsDto
  hotkeys: HotkeySettingsDto
  general: GeneralSettingsDto
  windows: WindowsSettingsDto
}

/* ==========================================================================
    Injection Key
    ========================================================================== */
export interface AppSettingsContext {
  settings: Ref<AppSettingsDto | null>
  isLoading: Ref<boolean>
  error: Ref<string | null>
  reload: () => Promise<void>
  cleanup?: () => void
}

export const APP_SETTINGS_KEY: InjectionKey<AppSettingsContext> =
  Symbol('app-settings')
