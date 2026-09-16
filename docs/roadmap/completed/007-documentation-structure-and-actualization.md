# 007 — Documentation structure and actualization

- **Статус:** completed
- **Зависимости:** 001–006
- **Позиция:** финальная задача программы

## Результат

Документация Echo отражает фактический продукт после UI refresh, имеет единую карту
входа и отделяет пользовательские инструкции, разработку, reference, решения, roadmap
и историю. Унаследованные TTSBard-материалы не выдаются за документацию Echo.

## Целевая структура

```text
docs/
├── README.md
├── user/
│   ├── README.md
│   ├── getting-started.md
│   ├── connections.md
│   ├── floating-window.md
│   ├── interface-settings.md
│   └── troubleshooting.md
├── integrations/
│   ├── README.md
│   └── sse.md
├── development/
│   ├── README.md
│   ├── architecture.md
│   ├── frontend.md
│   ├── backend.md
│   ├── events-and-ipc.md
│   ├── configuration.md
│   ├── testing.md
│   ├── building.md
│   ├── ai-workflow.md
│   └── templates/
├── reference/
│   ├── README.md
│   ├── settings-schema.md
│   ├── ipc-commands.md
│   ├── events.md
│   └── branding.md
├── decisions/
│   ├── README.md
│   ├── 0021-echo-align-to-appttsv2.md
│   ├── 0022-ui-information-architecture.md
│   └── 0023-floating-window-interaction.md
└── roadmap/
    ├── README.md
    ├── active/
    ├── completed/
    └── rejected/
```

Создавать только документы с реальным содержанием. Если reference полностью и
надёжно генерируется скриптом, документ должен объяснять генерацию и ссылаться на
артефакт, а не хранить вторую ручную копию.

## Объём работ

### 1. Documentation audit

Для каждого существующего файла определить одно действие:

- keep and update;
- move and update;
- archive as historical context;
- delete as irrelevant/duplicated.

Особое внимание:

- Piper/ONNX debug;
- Telegram logging debug;
- Windows CRT инструкции, если они не применимы текущему Cargo graph;
- старые design/implementation plans;
- `reviews/review-prompt.md`;
- AI workflow, который должен соответствовать реальному процессу Echo, а не копии
  соседнего проекта.

Удалять файл только после проверки ссылок через `rg` и истории Git.

### 2. User documentation

Писать по фактическому UI после 001–005:

- установка/первый запуск;
- добавление и редактирование подключения;
- connect/disconnect и значения статусов;
- управление floating через titlebar/tray/hotkey;
- drag и ограничения click-through;
- тема, наследование/собственный цвет floating и всегда доступная прозрачность;
- автоматическое восстановление сохранённой позиции и последнего состояния видимости;
- перенос длинных сообщений, показ полного текста без scroll и временный рост/возврат
  floating;
- typing indicator и условия его автоматического завершения;
- типовые SSE/auth/network ошибки.

Скриншоты добавлять только после финального UI и новой иконки. Не включать token или
реальные приватные endpoint.

### 3. Integration documentation

Актуализировать `integrations/sse.md` по существующему wire contract:

- endpoint/path;
- transport and reconnection;
- auth cookie/token semantics;
- поддерживаемые event types и payload;
- keepalive handling;
- status transitions;
- ограничения и security notes.

Не документировать гипотетические возможности.

### 4. Development documentation

- Архитектура: process/webviews, AppState/managers, connection lifecycle, settings
  ownership, events, tray/window lifecycle.
- Frontend: entrypoints main/floating, composables/stores, component boundaries,
  design tokens.
- Backend: commands/config/connections/events/floating/setup.
- IPC/events: фактические типизированные контракты после roadmap 006.
- Configuration: файлы, defaults, migrations, secret handling.
- Testing/building: только реально запущенные команды и Windows-specific scripts.

### 5. Decisions

Сохранить ADR 0021 и добавить два коротких решения:

- 0022: Connections owner, Settings IA, titlebar ownership.
- 0023: floating drag-region, click-through recovery, visibility and position truth.

ADR фиксирует решение и последствия, а не повторяет implementation checklist roadmap.

### 6. Historical plans and roadmap completion

- Старые `docs/plans/` не оставлять рядом с актуальной картой без явной маркировки.
- Полезные исторические выводы свернуть в completion notes/ADR.
- Огромные временные implementation plans удалить или архивировать согласно принятой
  repository policy.
- Переместить 001–007 в `roadmap/completed/` только после фактической приёмки каждого.
- Обновить все README links после moves.

### 7. Link and content verification

- Проверить все относительные Markdown links.
- Проверить упоминания удалённых компонентов, вкладок и команд.
- Проверить version/product naming: `ttsbard-echo`, `Echo`, `TTSBard` reference.
- Проверить, что absolute developer-machine paths не остаются пользовательской
  инструкцией; допустимы только явно помеченные historical/reference paths там, где
  это оправдано.
- Проверить UTF-8 и отсутствие mojibake.

## Не входит

- Публичный маркетинговый сайт.
- Полная английская локализация документации.
- Release notes для ещё не опубликованной версии.
- Документирование функций, отсутствующих в коде.

## Приёмка

- `docs/README.md` является достаточной картой документации.
- Пользователь может без чтения developer docs добавить connection, открыть floating,
  настроить appearance и восстановиться после click-through; off-screen position
  исправляется приложением автоматически.
- Архитектурный документ соответствует фактическим modules и ownership.
- IPC/events/settings reference соответствует проверкам roadmap 006.
- В активной документации нет Piper, Telegram, TTS/audio и других чужих доменов.
- Старые планы явно архивированы/удалены и не конкурируют с источниками правды.
- Все внутренние ссылки разрешаются.
- Roadmap программы закрыт правдивыми completion notes.

## Верификация

- `rg` по старым component/command/tab names.
- Автоматическая Markdown link check, если доступна существующая утилита; иначе
  небольшой repository-local checker с тестами либо тщательная script-проверка.
- Проверка UTF-8/mojibake.
- Сопоставление documentation command list с `check:ipc` output.
- Сопоставление settings schema с `check:settings` output.
- Ручное прохождение user guide на чистом пользовательском сценарии.

## Completion note

Финальный note должен перечислить:

- итоговую структуру docs;
- удалённые/архивированные материалы;
- выполненные проверки;
- известные ограничения;
- ссылки на completed roadmaps и новые ADR.
