# План — Align ttsbard-echo to app-tts-v2

> **Статус:** исторический план большой миграции. Выполненные архитектурные и
> инфраструктурные решения сохраняют силу, но незавершённая UI/documentation-часть
> заменена программой
> [`docs/roadmap/completed/000-ui-refresh-program.md`](../roadmap/completed/000-ui-refresh-program.md).

**Дата:** 2026-08-03
**Решение:** `docs/decisions/0021-echo-align-to-appttsv2.md`
**Бэклог задач:** `.work/ai/2026-08-03-echo-align-to-appttsv2/`
**Baseline:** `6cffed9` (master).

## Суть

Привести каркас ttsbard-echo (архитектура, работа с конфигом, UI, инфраструктура
сборки/CI/git) к образцу `app-tts-v2` — **без переноса чужого функционала**.
Исполнитель — автономный агент (qwen3-coder, локальный Ollama). Задачи — observable
behavior + границы, без кода внутри.

## Блоки и задачи

### A. Архитектура backend (Rust)
- `001` — Слои `src-tauri/src/` по образцу (commands/config/connections + корневые).
- `002` — DTO-слой: структура, serde-конвенции, разделение домен/DTO.
- `003` — `SettingsManager`/`WindowsManager`: load/save/defaults/atomicity по образцу.
- `004` — `constants`/`validation` как единый источник умолчаний/проверок.
- `005` — `AppEvent` + слой маппинга в Tauri-события; эмиссия через sender.

### B. Конфиг ↔ фронтенд
- `006` — Tauri-команды: единая `get_all_settings` (каноническое имя; фронт переведён на неё), группировка/регистрация.
- `007` — TS-типы зеркально DTO (+ место под SSE-payload-типы из 017).
- `008` — `useAppSettings`: provide/inject + хуки по разделам + mapping.
- `017` — **SSE-клиентский wire-контракт** (после 005 + 007 + 009/011): разбор потока/типы
  к формату источника (безымянный text + именованный `typing` + keepalive; auth остаётся
  cookie `webview_auth`). Сервер не переносим.

### C. UI
- `020` — **(предусловие для 009)** `tauri.conf.json` + `capabilities/default.json`: главное
  окно frameless (`decorations:false`/`transparent:true`, `label:"main"`) + window-permissions
  (minimize/maximize/...).
- `009` — Layout/Sidebar-каркас + **кастомный titlebar** (drag-region, window-controls,
  close→hide-to-tray). После 020. SettingsPanel не трогает.
- `010` — Shared-компоненты по конвенциям.
- `011` — Под-панели настроек + **единственный владелец `SettingsPanel.vue`**.
- `012` — CSS-токены и темы.

### D. Инфраструктура сборки/CI/git + документация
- `013` — `scripts/build.ps1` + `build.local.*.psd1` + BAT + clean-safety (с исполняемыми негативными проверками).
- `014` — GitHub Actions (CI + release; YAML-lint через npx). **CodeQL не переносится.**
- `015` — meta-файлы (`.gitignore`/`.gitattributes`/`.claudeignore`/`.editorconfig`) + npm-скрипты.
- `016` — Контракто-чекеры IPC + settings-parity (адаптированно).
- `018` — **Перестройка структуры `docs/`** + `integrations/sse.md` (до 019).
- `019` — **Описание приложения echo** (`docs/user/` + `architecture.md`).

## Порядок и зависимости

Рекомендуемая последовательность (каждая задача — грязное дерево + верификация, без
коммитов):

```
A: 001 → 002 → 003 → 004 → 005
B:              (после 002) 006 → 007 → 008
C: (после 008, 007) 012 → 010 → 011 → 020 → 009 ;  затем (после 009, 011, 007) 017
D: (независимо) 013 → 015 → 014 ; (после 002/005/006/007) 016
   (после A/B/C) 018 → 019
```

Ключевые зависимости (исправлены по ревью):
- **020 → 009**: frameless-окно и window-permissions должны быть готовы ДО кастомного titlebar.
- **011 (единственный владелец SettingsPanel.vue) до 009**: 009 не переписывает wiring под-панелей.
- **017 после 009/011**: 017 правит отображение в Connections/FloatingPanel — панели уже финализированы.
- **019 последняя**: описание должно отражать финальный каркас.

Сквозные правки фронт/бэк (где переименования ломают другую сторону) выполняются
синхронно в рамках одной задачи (allowed-paths это разрешают).

## Приёмка сессии

- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test` — зелёные.
- `npm run build` (vue-tsc + vite), `npm test` — зелёные.
- `npm run check:ipc`, `npm run check:settings` (после 016) — зелёные.
- Инфраструктурные скрипты/CI синтаксически валидны; существующий способ сборки не сломан.
- Diff ограничен allowed-paths; нет коммитов.

## Что НЕ делается

- Перенос TTS/audio/twitch/webview-сервера/VTube/soundpanel/playback/препроцессора/AI-текст.
- Speech-contract-чекер.
- espeak/Piper/libclang-стадии сборки.
- Изменения в `app-tts-v2`.
