# Завершённые roadmap

- [000 — UI refresh program](000-ui-refresh-program.md) — связал этапы подготовки v0.1.0, чтобы интерфейс, контракты и документация развивались в согласованном порядке.
- [001 — Design system and application shell](001-design-system-and-application-shell.md) — собрал единый каркас и визуальные токены, чтобы главное окно оставалось компактным и последовательным.
- [002 — Connections domain and UX](002-connections-domain-and-ux.md) — оформил добавление, изменение, удаление и runtime-состояние подключений, чтобы Echo мог независимо обслуживать несколько TTSBard.
- [003 — Interface settings](003-interface-settings.md) — добавил только настройки внешнего вида и поведения окон, чтобы пользователь мог адаптировать overlay без изменения его назначения.
- [004 — Floating window lifecycle and UI](004-floating-window-lifecycle-and-ui.md) — реализовал показ, скрытие, позицию, click-through и авторазмер, чтобы сообщения были видны поверх других приложений.
- [005 — Echo icon and brand assets](005-echo-icon-and-brand-assets.md) — подготовил и подключил иконки, чтобы приложение корректно распознавалось в окне, tray и сборке.
- [006 — UI contracts and verification](006-ui-contracts-and-verification.md) — добавил проверки IPC/settings и Rust-тесты, чтобы изменения frontend и backend не расходились незаметно.
- [007 — Documentation structure and actualization](007-documentation-structure-and-actualization.md) — разделил пользовательскую, инженерную и справочную документацию, чтобы актуальные правила находились без исторических планов.

Краткий итог проверок находится в [completion note](001-007-completion-note.md).

## Программа после v0.1.0

- [008 — Воспроизводимый CI и release gate](008-reproducible-ci-and-release-gate.md) — закрепил зависимости и заблокировал релиз непроверенного commit, чтобы сборка основной ветки и тега была воспроизводимой.
- [009 — Frontend regression safety](009-frontend-regression-safety.md) — добавил поведенческие тесты компонентов и composables, чтобы ловить утечки listeners, stale async state и повторную отправку формы.
- [010 — Целостность и безопасность настроек](010-settings-integrity-and-secrets.md) — внедрил атомарную запись, восстановление, DPAPI, ACL и CSP, чтобы сбой записи или доступ к файлу не раскрывал token и не ломал конфигурацию.
- [011 — Диагностика и надёжность SSE](011-sse-observability-and-reliability.md) — нормализовал ошибки, retry/cancel и проверку endpoint, чтобы подключение к нескольким TTSBard было управляемым и диагностируемым.
