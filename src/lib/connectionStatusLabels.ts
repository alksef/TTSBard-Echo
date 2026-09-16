/* ============================================================================
    Connection status labels - ttsbard-echo
    Pure mapping from runtime status/error categories to user-facing RU text.
    Shared by ConnectionsPanel and the connection test dialog (roadmap 011).
    ============================================================================ */

import type { ConnectionErrorKind, ConnectionStatus } from '@/types/types'

/** Everything the label helper needs — a runtime snapshot qualifies. */
export interface ConnectionStatusInfo {
  status: ConnectionStatus
  errorKind?: ConnectionErrorKind
  errorMessage?: string
  attempt?: number
  maxAttempts?: number
  nextRetryInSecs?: number
}

/** RU text per machine error category (roadmap 011, task 003). */
export const CONNECTION_ERROR_KIND_LABELS: Record<ConnectionErrorKind, string> = {
  configuration: 'Проверьте URL и настройки подключения',
  network: 'Сервер недоступен (сеть или DNS)',
  tls: 'Ошибка защищённого соединения (TLS)',
  authentication: 'Доступ запрещён — проверьте токен доступа',
  http: 'Сервер вернул ошибку (HTTP)',
  protocol: 'Некорректный формат потока событий',
  cancelled: 'Подключение отменено',
}

/** Label for one error category; `undefined` for unknown wire values. */
export function connectionErrorKindLabel(kind: string): string | undefined {
  return CONNECTION_ERROR_KIND_LABELS[kind as ConnectionErrorKind]
}

/** Human-readable RU label for a connection runtime state. */
export function connectionStatusLabel(info: ConnectionStatusInfo): string {
  switch (info.status) {
    case 'Connected':
      return 'Подключено'
    case 'Connecting':
      return 'Подключение…'
    case 'Retrying': {
      const { attempt, maxAttempts, nextRetryInSecs } = info
      if (attempt !== undefined && maxAttempts !== undefined && nextRetryInSecs !== undefined) {
        return `Повтор через ${nextRetryInSecs}с (попытка ${attempt} из ${maxAttempts})`
      }
      // Defensive: a Retrying status without counters should not survive the
      // backend contract, so this only guards against a broken payload.
      return 'Повтор подключения…'
    }
    case 'Error': {
      // The kind wins over the raw message; an unknown or absent kind falls
      // back to the fixed English errorMessage, then to a generic text.
      const kindLabel = info.errorKind !== undefined ? connectionErrorKindLabel(info.errorKind) : undefined
      return kindLabel ?? (info.errorMessage || 'Ошибка подключения')
    }
    default:
      return 'Отключено'
  }
}
