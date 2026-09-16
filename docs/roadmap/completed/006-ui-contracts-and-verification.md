# 006 — UI contracts and verification

- **Статус:** completed
- **Зависимости:** 001, 002, 003, 004
- **Желательная зависимость:** 005 для bundle smoke test
- **Блокирует:** 007 и завершение программы

## Результат

Ключевые UI/backend-контракты Echo проверяются автоматически, placeholder scripts
заменены реальными проверками, а Windows-specific поведение подтверждено повторяемой
ручной матрицей с записанными результатами.

## Проблема

В `package.json` команды `check:ipc` и `check:settings` сейчас сообщают, что проверка
ещё не реализована. Это особенно опасно после добавления visibility snapshot, runtime
connection snapshot, reconnect policy, appearance events, window bounds и persisted
visibility.

## Объём работ

### 1. IPC contract check

Адаптировать паттерн app-tts-v2 под небольшой scope Echo. Проверка должна сопоставлять:

- зарегистрированные `#[tauri::command]`;
- строки `invoke()` frontend;
- имена аргументов, где их можно надёжно проверить;
- отсутствие orphan frontend invokes;
- отсутствие ожидаемых, но не зарегистрированных команд.

Проверка не должна парсить Rust/TS хрупкими выражениями без тестовых fixtures. Если
используется script parser, добавить positive/negative tests.

### 2. Settings parity

Проверять соответствие Rust DTO и TypeScript types как минимум для:

- general settings;
- logging settings;
- connection config;
- windows/floating settings;
- theme, custom-background, appearance, positions and floating visibility fields.

Учитывать реальные serde rename/default conventions. Не требовать parity для чисто
runtime DTO в settings check; для них выделить собственные типы/fixtures.

### 3. Event contracts

Зафиксировать типы payload для:

- connection status changed;
- message received;
- connection typing changed;
- connections/config changed;
- floating visibility changed;
- floating appearance changed;
- theme/settings changed.

Frontend не должен использовать необработанные tuples в одних местах и objects в
других для одного события. Предпочтение — именованные object DTO для новых контрактов.

### 4. Frontend test infrastructure

Добавить/актуализировать Vitest только в объёме, необходимом новым компонентам и
composables. Минимальные тесты:

- ConnectionFormDialog add/edit/validation/reset/double submit;
- connection store snapshot/events/cleanup/error rollback;
- AppTitlebar visibility synchronization;
- SettingsInterface opacity conversion, custom-color gating, no-success-toast policy и
  mutation rollback;
- FloatingApp snapshot, event update, empty/error state;
- FloatingConnectionList wrapping, typing indicator lifecycle и reduced motion;
- pure size/position helpers, measured-content grow/shrink и startup visibility restore.

Не стремиться к бессодержательному coverage percentage.

### 5. Rust tests

Покрыть чистую и service-level логику:

- runtime snapshot не раскрывает token;
- removal останавливает активное соединение;
- transport loss снимает `Connected`, retry ограничен 10 попытками с интервалом 5
  секунд и отменяется при disconnect/delete/shutdown;
- opacity/colour validation;
- custom-background defaults/migration;
- main/floating position persistence и automatic off-screen fallback;
- floating visibility persistence без перезаписи служебным shutdown-hide;
- event mapping/payload serialization;
- typing snapshot/event reset on final message and connection loss;
- settings persistence defaults/migration, если формат изменился.

Оконные операции, требующие реального Windows WebView, остаются manual smoke test, а не
маскируются фальшивым unit test.

### 6. Scripts and CI surface

Целевой набор npm commands:

```text
npm run build
npm test
npm run check:ipc
npm run check:settings
```

- Команды должны возвращать ненулевой exit code при нарушении.
- Windows Rust checks выполняются принятыми scripts, а не bare `cargo`, если проектные
  scripts обеспечивают toolchain/native dependency safety.
- CI обновляется только в пределах существующего workflow; release/publish не входит.

### 7. Manual Windows matrix

Записать фактические результаты для:

| Область | Сценарии |
|---|---|
| Main shell | drag, minimum size, position restart, minimize, close/hide, tray restore |
| Floating visibility | titlebar, tray, hotkey, repeated toggle, restart shown/hidden |
| Floating interaction | drag, hide button, click-through recovery |
| Floating content | wrap/newlines/long URL, full-text grow/shrink, no message scroll, typing animation |
| Connections | 0/1/4/10, add/edit/delete, active delete, server loss, 10 × 5 s retry |
| Appearance | startup dark/light, live floating update, custom color, opacity, no success toast |
| Displays | 100/125/150% DPI, one/two monitors, monitor removal |
| Branding | tray, taskbar, switcher, installer/debug bundle |

## Не входит

- Полный end-to-end framework с управлением реальным SSE-сервером, если для него нет
  устойчивого тестового fixture.
- Coverage gate ради метрики.
- CodeQL и новый release pipeline.
- Исправление несвязанных legacy warning вне затронутого scope.

## Приёмка

- `check:ipc` и `check:settings` больше не являются заглушками.
- У каждой проверки есть negative test, доказывающий, что она действительно падает.
- Все новые events имеют один документированный payload shape.
- Long-message resize и typing state покрыты тестами и не создают stale timers,
  бесконечный resize loop или сохранение временной позиции.
- Tests ловят основные regressions новых UI flows.
- Полная целевая команда проверок зелёная.
- Manual matrix выполнена на Windows, результаты сохранены в gitignored work log до
  переноса подтверждённых фактов в docs roadmap completion note.
- Нерешённые дефекты либо исправлены, либо оформлены отдельными active/rejected items,
  а не спрятаны в финальном отчёте.

## Верификация

Сама задача считается выполненной только после запуска, а не описания команд:

- `npm run build`;
- `npm test`;
- `npm run check:ipc`;
- `npm run check:settings`;
- Rust fmt/check/clippy/test через проектные scripts;
- manual Windows matrix.

Фактические версии toolchain и итог каждой команды записываются в completion note.
