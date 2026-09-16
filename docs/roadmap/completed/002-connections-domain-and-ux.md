# 002 — Connections domain and UX

- **Статус:** completed
- **Зависимость:** 001
- **Блокирует:** 004, 006, 007

## Результат

Экран `Подключения` становится единственным пользовательским местом управления SSE-
подключениями. Добавление начинается кнопкой и выполняется в единой форме; та же форма
используется для редактирования. Дублирующий Connections-раздел удаляется из настроек.

## Проблема

Сейчас `ConnectionsPanel.vue` и `SettingsConnections.vue` реализуют разные CRUD-
сценарии поверх одного backend:

- главная панель использует `address + port` и постоянно открытую форму;
- настройки используют полный URL, modal edit, enabled toggle и формальный URL test;
- runtime status хранится локально и при запуске нового окна инициализируется как
  `Disconnected`;
- после отключения SSE-сервера уже установленное соединение может продолжать
  отображаться как `Connected`;
- подписки и преобразования state дублируются между компонентом и `floating-main.ts`.

## Зафиксированные продуктовые решения

- Единственный owner конфигурации — экран `Подключения`.
- Постоянно открытая add-section удаляется.
- Кнопка `Добавить` находится в заголовке панели.
- Add и Edit используют один `ConnectionFormDialog`.
- Главный формат формы: название, host/address, port, необязательный token.
- Путь `/sse` используется как текущий default. Если существующие конфиги допускают
  произвольный URL, форма обязана корректно представить их через advanced mode или
  безопасный полный URL, а не потерять данные.
- Фиктивный `Test`, проверяющий только синтаксис URL, удаляется. Настоящий connection
  probe допускается только с backend-командой и чёткой семантикой.
- UI пользовательских строк приводится к русскому языку.
- После потери транспорта backend немедленно покидает `Connected`, публикует
  authoritative status и запускает не более 10 автоматических повторных попыток с
  фиксированным интервалом 5 секунд. Первичное подключение в эти 10 попыток не входит.

## Объём работ

### 1. Connection frontend domain

Выделить единый composable/store для каждого webview-контекста:

```text
useConnections()
├── configs
├── runtimeStates
├── add
├── update
├── remove
├── connect
├── disconnect
├── reload
└── subscribe/dispose
```

Требования:

- persisted config и runtime status представлены раздельно;
- access token не попадает в presentation state сверх необходимости редактирования;
- event listeners регистрируются ровно один раз на окно и гарантированно очищаются;
- ошибки нормализуются единым helper;
- timeout не создаёт ложное конечное состояние;
- frontend после mutation сверяется с backend snapshot или authoritative event.

### 2. Runtime snapshot

Новый webview должен узнать реальное состояние уже работающих соединений. Проверить
текущий ConnectionManager и добавить минимальную read-only команду snapshot, если её
нет. Snapshot должен содержать только безопасные presentation-поля:

- id;
- connection status;
- последнее допустимое сообщение, если продукт хранит его;
- текущий `isTyping`, если сервер уже сообщил начало набора;
- диагностическую причину ошибки без секрета.

Не дублировать runtime lifecycle во frontend.

### 3. Disconnect detection and retry policy

- EOF, ошибка чтения/keepalive и недоступность сервера считаются потерей соединения, а
  не оставляют последний `Connected` бессрочно.
- Переходы имеют однозначную последовательность, например
  `Connected → Reconnecting(attempt) → Connected|Error`; точные публичные имена
  фиксируются в handoff и одинаковы для main/floating.
- Повторные попытки выполняются backend-ом: максимум 10, строго не чаще одной попытки
  в 5 секунд, без параллельных reconnect-задач для одного connection.
- Успешное подключение обнуляет счётчик. После исчерпания лимита автоматический цикл
  останавливается, конечный статус и причина видны в обоих окнах; ручной `Подключить`
  начинает новый цикл.
- `Отключить`, удаление connection и shutdown отменяют pending timer/attempt и не
  допускают позднего восстановления stale-задачей.

### 4. Typing state contract

- Использовать явный typing payload существующего SSE-контракта (`isTyping` и
  необязательный preview text), а не выводить набор косвенно из частоты сообщений.
- Backend хранит `isTyping` отдельно для каждого connection и публикует единый
  типизированный event/object DTO; новое main/floating окно получает текущее значение
  через runtime snapshot.
- `isTyping=false`, финальное текстовое сообщение, disconnect/error, delete и shutdown
  гарантированно очищают индикатор. Предусмотреть safety timeout на случай потерянного
  stop-события, не подменяя им штатный протокол.
- Typing preview, если он поддерживается фактическим wire payload, проходит те же
  ограничения безопасности и отображения, что обычное сообщение. Если preview в
  протоколе отсутствует, UI показывает только индикатор и не генерирует текст сам.

### 5. Components

Рекомендуемая декомпозиция:

```text
src/components/connections/
├── ConnectionsPanel.vue
├── ConnectionCard.vue
├── ConnectionFormDialog.vue
├── ConnectionStatus.vue
└── connectionForm.ts
```

Форма должна поддерживать:

- add/edit mode;
- frontend validation до IPC;
- backend validation как окончательный авторитет;
- Enter submit только при валидной и не занятой форме;
- Escape/cancel;
- reset при закрытии;
- блокировку double submit;
- корректное отображение ошибки без потери введённых значений.

### 6. Card behavior

Карточка показывает название, безопасный URL, runtime status и последнее сообщение.
Доступны connect/disconnect, edit и delete. Delete требует подтверждения; удаление
активного соединения должно сначала завершить его backend lifecycle.

### 7. Settings cleanup

- Удалить вкладку Connections из Settings.
- Удалить `SettingsConnections.vue` после переноса полезного поведения.
- Удалить больше не используемые `TestResult`/styles/imports только после `rg`-проверки.
- Не переносить enabled toggle автоматически, пока не выяснено отличие persisted
  `enabled` от runtime connect/disconnect. Семантика должна быть одна и документирована.

## Не входит

- Изменение SSE wire protocol и auth-механизма.
- Новая система профилей или групп подключений.
- История всех входящих сообщений.
- Настройка внешнего вида карточек пользователем.
- Floating layout; он будет потребителем нового domain API в roadmap 004.

## Приёмка

- На экране нет постоянно открытой формы; `Добавить` открывает dialog.
- Add и Edit используют один компонент и одну валидацию.
- Подключение можно добавить, отредактировать, подключить, отключить и удалить.
- После открытия floating/main отображается фактический runtime status, а не
  искусственный `Disconnected`.
- После остановки SSE-сервера status перестаёт быть `Connected`; выполняется не более
  10 reconnect attempts с интервалом не менее 5 секунд, после чего цикл прекращается.
- Явные typing start/stop из SSE синхронно доставляются всем webview, а floating
  отображает их; индикатор сбрасывается финальным сообщением и при потере соединения.
- В Settings нет Connections и нет второго CRUD.
- Token не отображается в карточке, сообщениях об ошибках или логах.
- Нестандартный существующий URL не повреждается при редактировании.

## Верификация

- Component/unit tests формы: open, cancel, reset, validation, add, edit, double submit.
- Tests store/composable: snapshot, event update, error rollback, cleanup.
- Rust tests snapshot DTO, обнаружения EOF/transport error, лимита 10 попыток,
  интервала 5 секунд и отмены retry при disconnect/delete/shutdown.
- Tests typing payload parsing, snapshot/event serialization, stop/final-message reset
  и safety timeout без утечки таймеров.
- `npm run build` и применимые Rust checks.
- Ручной сценарий с 0, 1, 4 и 10 подключениями.
- Ручной сценарий запуска второго окна при уже Connected состоянии.
- Ручной сценарий: выключить сервер при `Connected`, проверить смену статуса, счётчик
  попыток, восстановление при возврате сервера и остановку после 10-й неудачи.
- Поиск неиспользуемых старых компонентов и IPC-вызовов через `rg`.

## Handoff

В завершённом roadmap зафиксировать окончательный формат формы, семантику `enabled`,
имена snapshot/event контрактов, точные status transitions, typing lifecycle и
поведение retry/delete.
