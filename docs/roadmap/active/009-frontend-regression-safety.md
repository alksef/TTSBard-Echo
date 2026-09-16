# 009 — Frontend regression safety

- **Статус:** proposed
- **Приоритет:** P0
- **Зависимости:** 008
- **Блокирует:** 012

## Результат

Критические Vue-компоненты и composables проверяются поведенческими тестами, а
не только статическим сопоставлением IPC/settings-контрактов.

## Проблема

Текущий `npm test` хорошо обнаруживает рассинхронизацию строковых контрактов, но
не проверяет async lifecycle, rollback после ошибки, очистку listeners/timers и
визуальные состояния компонентов.

## Объём работ

1. Подключить Vitest и Vue Test Utils с минимальной Tauri mock-обвязкой.
2. Покрыть `useConnections`: initial snapshot, events, повторную загрузку,
   timeout, rollback и cleanup.
3. Покрыть `useAppSettings`: backend-ready, конкурентные reload, ошибку
   сохранения и снятие listeners.
4. Проверить `ConnectionFormDialog`: add/edit, validation, cancel/reset и защита
   от double submit.
5. Проверить `AppTitlebar` и floating UI: visibility sync, empty/error state,
   typing timeout и unmount cleanup.
6. Сделать тесты детерминированными через fake timers; не обращаться к реальному
   WebView или сети.
7. Добавить frontend tests в CI отдельным понятным шагом.

## Не входит

- Полный browser E2E и управление настоящим Tauri WebView.
- Coverage threshold ради формального процента.
- Переработка UI и добавление новых пользовательских функций.

## Приёмка

- Для каждого критического composable есть success, error и cleanup сценарии.
- Regression с утечкой listener, stale async response или double submit ломает
  тест.
- Тесты не зависят от порядка запуска и проходят повторно без очистки проекта.
- Контрактные проверки сохраняются и запускаются вместе с Vitest.

## Верификация

- `npm test`;
- `npm run build`;
- минимум один намеренно сломанный локальный fixture подтверждает, что каждый
  новый класс проверки действительно падает;
- ручной smoke двух окон после production frontend build.
