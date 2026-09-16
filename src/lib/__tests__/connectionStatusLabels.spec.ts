/* ============================================================================
   connectionStatusLabels unit tests: every status label, every error
   category (roadmap 011, task 003) and the unknown-kind fallback chain.
   ============================================================================ */
import { describe, expect, it } from 'vitest'

import {
  CONNECTION_ERROR_KIND_LABELS,
  connectionErrorKindLabel,
  connectionStatusLabel,
} from '@/lib/connectionStatusLabels'
import type { ConnectionErrorKind } from '@/types/types'

describe('connectionErrorKindLabel', () => {
  it('maps every machine category to its RU text', () => {
    const expected: Record<ConnectionErrorKind, string> = {
      configuration: 'Проверьте URL и настройки подключения',
      network: 'Сервер недоступен (сеть или DNS)',
      tls: 'Ошибка защищённого соединения (TLS)',
      authentication: 'Доступ запрещён — проверьте токен доступа',
      http: 'Сервер вернул ошибку (HTTP)',
      protocol: 'Некорректный формат потока событий',
      cancelled: 'Подключение отменено',
    }
    expect(CONNECTION_ERROR_KIND_LABELS).toEqual(expected)
  })

  it('returns undefined for an unknown wire kind', () => {
    expect(connectionErrorKindLabel('quantum_interference')).toBeUndefined()
  })
})

describe('connectionStatusLabel', () => {
  it('labels the plain statuses', () => {
    expect(connectionStatusLabel({ status: 'Connected' })).toBe('Подключено')
    expect(connectionStatusLabel({ status: 'Connecting' })).toBe('Подключение…')
    expect(connectionStatusLabel({ status: 'Disconnected' })).toBe('Отключено')
  })

  it('labels Retrying with the wait and attempt progress', () => {
    expect(connectionStatusLabel({
      status: 'Retrying',
      attempt: 3,
      maxAttempts: 10,
      nextRetryInSecs: 5,
    })).toBe('Повтор через 5с (попытка 3 из 10)')
  })

  it('keeps a generic retry label when the counters are missing', () => {
    expect(connectionStatusLabel({ status: 'Retrying' })).toBe('Повтор подключения…')
  })

  it('labels every error category', () => {
    for (const [kind, label] of Object.entries(CONNECTION_ERROR_KIND_LABELS)) {
      expect(connectionStatusLabel({
        status: 'Error',
        errorKind: kind as ConnectionErrorKind,
        errorMessage: 'Fixed english text',
      })).toBe(label)
    }
  })

  it('falls back to the error message for an unknown kind', () => {
    // Wire values are typed, but an unknown kind can still arrive at runtime.
    expect(connectionStatusLabel({
      status: 'Error',
      errorKind: 'quantum_interference' as ConnectionErrorKind,
      errorMessage: 'Fixed english text',
    })).toBe('Fixed english text')
  })

  it('falls back to the error message when no kind is present', () => {
    expect(connectionStatusLabel({
      status: 'Error',
      errorMessage: 'Fixed english text',
    })).toBe('Fixed english text')
  })

  it('uses a generic text when neither kind nor message is present', () => {
    expect(connectionStatusLabel({ status: 'Error' })).toBe('Ошибка подключения')
    expect(connectionStatusLabel({ status: 'Error', errorKind: 'mystery' as ConnectionErrorKind })).toBe('Ошибка подключения')
  })
})
