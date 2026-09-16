# 009 — Frontend regression safety

- **Статус:** completed
- **Приоритет:** P0
- **Зависимости:** 008
- **Блокирует:** 012
- **Завершён:** 2026-09-14

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

## Completion note

Завершено 2026-09-14.

### Что сделано

- Подключены Vitest 5 + Vue Test Utils 2 + happy-dom (`vitest.config.ts`).
  Tauri API замокан через `src/test/helpers/tauri.ts` (invoke с маршрутизацией
  по командам и fail-loud на незарегистрированные вызовы, шина событий
  `emitTauriEvent`, заглушки window API и ResizeObserver). Никакого реального
  WebView, IPC или сети; детерминизм — fake timers и полный сброс моков между
  тестами.
- `src/composables/__tests__/useConnections.spec.ts` (16 тестов): initial
  snapshot и fallback Disconnected, все runtime-события (status с нормализацией
  Error, message, typing, cleared, removed), reload по `connections-changed`,
  ошибка загрузки, timeout мутации (10 c) с последующей ресинхронизацией,
  распространение ошибки мутации, снятие listeners при unmount, включая
  гонку «unmount во время подписки».
- `src/composables/__tests__/useAppSettings.spec.ts` (11 тестов):
  backend-ready (готов сразу / повторные попытки / timeout после 50 попыток),
  конкурентные reload с очередью инвалидации, ошибка `get_all_app_settings`
  с сохранением старых значений, повторная загрузка по `backend-ready`,
  событие `theme-changed`, снятие listeners при dispose.
- `src/components/connections/__tests__/ConnectionFormDialog.spec.ts`
  (15 тестов): add/edit, нормализация URL и перенос токена из query,
  валидация (пустое/длинное имя, мусорный URL, не-HTTP схема, пустой host),
  cancel/backdrop/reset, защита от double submit.
- `src/components/__tests__/AppTitlebar.spec.ts` (11 тестов): видимость
  floating (snapshot + событие + защита от устаревшего ответа), toggle
  с защитой от повторного нажатия и fallback-refresh при ошибке,
  click-through (optimistic update, rollback при ошибке, событие),
  кнопки окна (minimize/в трей/выход), снятие listeners.
- `src/components/floating/__tests__/FloatingApp.spec.ts` (14 тестов):
  loading/empty/error состояния FloatingConnectionList, карточки подключений,
  typing → message → cleared, appearance sync с защитой от устаревшего ответа,
  `clickthrough-changed`/`theme-changed`, авторазмер окна и ResizeObserver,
  полная очистка при unmount.
- `src/components/shared/__tests__/StatusMessage.spec.ts` (5 тестов):
  auto-hide через fake timers, пересброс таймера при смене сообщения,
  ручной dismiss, очистка таймера при unmount.

### Исправленные дефекты

1. `useAppSettings.createAppSettings` регистрировал `onScopeDispose` после
   `await listen(...)`, поэтому очистка settings/theme/backend-ready listeners
   фактически никогда не срабатывала (выяснено самим тестом очистки).
   Теперь `unlistenFns` и `onScopeDispose` регистрируются синхронно в setup,
   а `cleanup()` вызывает тот же disposer.
2. `ConnectionFormDialog` блокировал повторный submit только на время
   синхронного `emit('save')`, то есть фактически не блокировал. Добавлен
   проп `saving` (родитель сообщает о выполняемой мутации): кнопки и закрытие
   блокируются, лок снимается при неудачной мутации (retry возможен) или
   закрытии диалога. `ConnectionsPanel.save` получил собственный guard,
   показ ошибки через `actionError` вместо необработанного rejection.

### Верификация

- `npm test` = `test:contracts` (contract-checker) + `test:unit` (Vitest),
  72/72 зелёные; повторные прогоны стабильны (изоляция через beforeEach
  reset + afterEach restore).
- `npm run build` (vue-tsc + vite) — зелёный; типы тестов проверяются тем же
  vue-tsc.
- Мутационные проверки: шесть намеренных регрессий (утечка listeners в обоих
  composables, снятие guard от устаревшего visibility-ответа, снятие guard
  от double submit, увеличение timeout-константы, снятие rollback
  click-through) — каждая ломает минимум один тест.
- CI: шаг `Frontend unit tests` (`npm run test:unit`) добавлен в job
  `check-types` рядом с `Contract checks`.
- Ручной smoke двух окон после production frontend build — выполнен локально
  (main + floating: видимость, статусы, сообщения, typing).

### Известные ограничения

- Typing-timeout принадлежит бэкенду (`message_clear_interval_seconds`);
  фронтенд-тесты покрывают только event-driven показ/скрытие индикатора.
- Browser E2E и управление настоящим WebView сознательно не входят (см.
  «Не входит»); поведение WebView по-прежнему проверяется ручным smoke.
