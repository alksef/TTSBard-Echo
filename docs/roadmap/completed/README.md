# Завершённые roadmap

Программа UI refresh завершена при подготовке альфа-релиза `v0.1.0`:

- [000 — UI refresh program](000-ui-refresh-program.md)
- [001 — Design system and application shell](001-design-system-and-application-shell.md)
- [002 — Connections domain and UX](002-connections-domain-and-ux.md)
- [003 — Interface settings](003-interface-settings.md)
- [004 — Floating window lifecycle and UI](004-floating-window-lifecycle-and-ui.md)
- [005 — Echo icon and brand assets](005-echo-icon-and-brand-assets.md)
- [006 — UI contracts and verification](006-ui-contracts-and-verification.md)
- [007 — Documentation structure and actualization](007-documentation-structure-and-actualization.md)

Краткий итог проверок находится в [completion note](001-007-completion-note.md).

## Программа после v0.1.0

- [008 — Воспроизводимый CI и release gate](008-reproducible-ci-and-release-gate.md)

Завершён 2026-09-14: push- и ручной CI, release gate по тегу и тестовый
prerelease `v0.1.1` проверены в GitHub Actions, ссылки на прогоны — в
[completion note](008-reproducible-ci-and-release-gate.md).

- [009 — Frontend regression safety](009-frontend-regression-safety.md)

Завершён 2026-09-14: Vitest + Vue Test Utils, 72 поведенческих теста
критических composables и компонентов, два исправленных дефекта (утечка
listeners в `useAppSettings`, отсутствие реальной защиты от double submit),
шесть мутационных проверок — в [completion note](009-frontend-regression-safety.md).
