/* ============================================================================
   General Types - ttsbard-echo
   ============================================================================ */

import type { Component } from 'vue'

/* ==========================================================================
   Panel Types
   ========================================================================== */
export type Panel = 'connections' | 'settings'

export interface FloatingVisibilityPayload {
  visible: boolean
}

/* ==========================================================================
   Connection Status
   ========================================================================== */
export type ConnectionStatus = 
  | 'Disconnected'
  | 'Connecting'
  | 'Connected'
  | 'Retrying'
  | 'Error'

/* ==========================================================================
   Connection Error Kind
   ========================================================================== */
/**
 * Machine-readable failure category (roadmap 011, decision A). The backend
 * serializes it snake_case; unknown wire values fall back to the raw error
 * message in the UI.
 */
export type ConnectionErrorKind =
  | 'configuration'
  | 'network'
  | 'tls'
  | 'authentication'
  | 'http'
  | 'protocol'
  | 'cancelled'

/* ==========================================================================
   Connection Test (pre-save probe)
   ========================================================================== */
/**
 * Wire result of the `test_connection` command (roadmap 011, task 005): one
 * pre-save probe attempt against the endpoint typed into the dialog. Error
 * fields are omitted on success, `latency_ms` is omitted on failure; serde
 * names stay snake_case like every other backend DTO.
 */
export interface ConnectionTestResultDto {
  ok: boolean
  error_kind?: ConnectionErrorKind
  error_message?: string
  latency_ms?: number
}

/* ==========================================================================
   Message Types
   ========================================================================== */
export type MessageType = 'info' | 'success' | 'warning' | 'error'

/* ==========================================================================
   Provider Types
   ========================================================================== */
export type ProviderType = 'openai' | 'elevenlabs' | 'azure' | 'google' | 'custom'

/* ==========================================================================
   Toast/Notification Types
   ========================================================================== */
export interface ToastMessage {
  id: string
  type: MessageType
  message: string
  duration?: number
}

/* ==========================================================================
   Component Props Types
   ========================================================================== */
export interface StatusMessageProps {
  type: MessageType
  message: string
  timeout?: number
}

export interface InputWithToggleProps {
  modelValue: string
  enabled: boolean
  label: string
  placeholder?: string
}

/* ==========================================================================
   Icon Type
   ========================================================================== */
export type IconComponent = Component
