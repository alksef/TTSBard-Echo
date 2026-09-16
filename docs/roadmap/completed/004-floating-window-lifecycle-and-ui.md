# 004 — Floating window lifecycle and UI

- **Статус:** completed
- **Зависимости:** 001, 002, 003
- **Блокирует:** 006, 007

## Результат

Floating становится самостоятельным тонким presentation-окном: оно использует общую
дизайн-систему и connection domain, имеет рабочий drag-titlebar, корректно применяет
appearance, сохраняет доступную позицию и безопасно работает с click-through.

## Диагноз текущей реализации

- `floating-main.ts` вручную создаёт Map, повторно подписывается на события и монтирует
  `ConnectionsPanel` через inline-template.
- Общий `style.css` не импортируется; `floating.html` содержит отдельные hardcoded
  variables.
- `data-tauri-drag-region` установлен на корневой панели, но дочерние карточки занимают
  практически всю площадь и не образуют гарантированной drag-зоны.
- При persisted click-through backend включает ignore cursor events, поэтому окно не
  может принять drag или click по определению.
- Позиция сохраняется только при hide; используются physical coordinates без полной
  стратегии DPI/multi-monitor.
- Последнее состояние видимости floating не сохраняется между запусками.
- Auto-resize не учитывает новый titlebar и растущий список.

## Зафиксированные UX-решения

- В floating нет add/edit/delete конфигурации.
- Connect/disconnect допустимы как runtime-действия.
- Click-through настраивается только в главном окне.
- Верхняя полоса содержит название и кнопку скрытия; свободная часть — drag-region.
- Кнопка скрытия делает hide, а не destroy.
- При выключенном click-through окно должно перетаскиваться мышью.
- При включённом click-through отсутствие drag ожидаемо и явно объяснено в настройках.

## Объём работ

### 1. Отдельный entry component

Создать понятный entrypoint:

```text
src/floating-main.ts
src/components/floating/FloatingApp.vue
src/components/floating/FloatingTitlebar.vue
src/components/floating/FloatingConnectionList.vue
```

- `floating-main.ts` импортирует общий `style.css`.
- Удаляется Options API inline-template и ручное поле `_unlisten`.
- Composition API lifecycle явно владеет подписками и cleanup.
- `floating.html` остаётся минимальным document shell без ручной темы.

### 2. Titlebar and drag

- Выделить стабильную высоту titlebar и учесть её в размере окна.
- `data-tauri-drag-region` находится только на titlebar/background элементах.
- Hide button не является drag-region и останавливает нежелательную drag-семантику.
- Текст не выделяется при drag.
- Проверить permission `allow-start-dragging` именно для label `floating`.

### 3. Connection presentation

- Использовать read-only часть domain API из roadmap 002.
- При mount загрузить config + runtime snapshot до отображения конечного статуса.
- Показать loading/empty/error states без фиктивного `Disconnected`.
- Карточки компактны, но статус и control имеют доступные title/aria-label.
- Текст сообщения использует `white-space: pre-wrap` и
  `overflow-wrap: anywhere`: сохраняет осмысленные переводы строк, переносит длинные
  слова/URL и никогда не увеличивает ширину карточки или окна.
- Во время `isTyping=true` карточка показывает компактную анимацию из трёх точек и
  доступную подпись `Набирает…`; высота места под индикатор стабильна и не меняется на
  каждом animation frame.
- При `prefers-reduced-motion: reduce` анимация заменяется статичным индикатором.
- Typing indicator исчезает при `isTyping=false`, получении финального сообщения,
  disconnect/error или удалении connection.

### 4. Appearance

- Получить фактический persisted appearance при mount.
- Применять theme, custom-background mode/color и opacity через корневые CSS variables.
- При выключенном custom-background использовать semantic background текущей темы; при
  включённом менять только фон floating, не отключая dark/light tokens текста и
  controls.
- Слушать единое типизированное appearance event из roadmap 003.
- Не смешивать opacity поверхности и opacity текста/контролов: текст остаётся читаемым.
- Удалить hardcoded body background из `floating.html`.

### 5. Temporary content-driven size

- Минимальная высота включает titlebar и empty state.
- Floating остаётся недоступным для ручного resize и никогда программно не становится
  меньше рассчитанного минимума по ширине/высоте.
- Базовая ширина остаётся фиксированной: обычное длинное сообщение сначала переносится
  и увеличивает только высоту.
- Высоту рассчитывать по фактическому измеренному layout после переноса текста, а не по
  длине строки или количеству символов.
- При новом длинном сообщении полностью разворачиваются текстовый блок, карточка и
  панель списка, после чего Tauri-окно получает измеренный размер через `setSize`.
- Не задавать сообщению `line-clamp`, `text-overflow`, собственный `max-height` или
  внутренний scroll: штатная цель resize — показать сообщение целиком без действий
  пользователя.
- Если требуемая высота превышает доступную высоту monitor work area, временно
  увеличивать ширину окна и панели до минимального значения, при котором перенесённый
  текст целиком помещается по высоте. Ширина также ограничена work area.
- Scroll списка допускается только из-за одновременного количества карточек и не
  должен скрывать раскрытое активное сообщение. Message-level scroll запрещён.
- После очистки сообщения, замены на более короткое или окончания typing preview
  пересчитать layout с коротким debounce и вернуть окно к размеру, требуемому текущим
  содержимым, но не меньше базового размера.
- Resize-controller объединяет частые обновления typing preview, измеряет DOM после
  render и игнорирует микроколебания высоты; рост выполняется без заметной задержки, а
  shrink — с небольшим debounce, чтобы окно не дрожало при наборе.
- Анимационные кадры typing indicator не запускают повторный resize; resize выполняется
  только при структурном изменении содержимого/измеренной высоты.
- Resize не должен менять сохранённую top-left позицию неожиданно.
- Расчёты не дублируются одновременно в component и entrypoint.

### 6. Position persistence and monitor safety

Выбрать и документировать одну систему координат для persistence. Предпочтительно
хранить physical coordinates, если текущая Tauri/Windows реализация стабильно отдаёт
их, но conversion и DPI должны быть последовательными.

- Сохранять позицию после завершения перемещения с debounce либо через надёжное window
  event, а также при hide/shutdown.
- При show проверять сохранённую позицию против доступных monitor work areas.
- Если окно полностью вне доступных экранов, использовать reset/center fallback.
- Не записывать позицию на каждый pixel без debounce.
- Off-screen fallback работает автоматически; отдельная кнопка/IPC-команда ручного
  восстановления положения не является частью пользовательского контракта.
- Динамический рост сначала сохраняет пользовательскую top-left anchor. Если снизу или
  справа не хватает места, окно временно сдвигается вверх/влево ровно настолько, чтобы
  остаться в work area; этот служебный сдвиг не сохраняется как новая пользовательская
  позиция.
- После уменьшения вернуть базовую сохранённую позицию с повторной monitor validation.
  Если пользователь перетащил уже увеличенное окно, новая drag-позиция становится
  базовой и не должна быть отменена последующим shrink.

### 7. Lifecycle truth

- Show/hide/toggle возвращают или публикуют фактический `visible` state.
- Titlebar главного окна, tray и hotkey получают одно событие после успешной операции.
- Ошибка hide/show не меняет presentation state оптимистично навсегда.
- После каждой успешной show/hide/toggle-операции сохранять фактический `visible`.
- При старте восстановить последнее состояние: `visible=true` показывает floating
  после применения theme/appearance/position, `false` оставляет его скрытым.
- Первичная конфигурация получает явный default видимости; служебный hide при shutdown
  не должен перезаписывать пользовательское последнее состояние.
- Shutdown и explicit Exit корректно завершают connection tasks и сохраняют position и
  последнее пользовательское состояние visibility.

## Не входит

- Resize пользователем.
- Always-on-top toggle, если он не существует как подтверждённая настройка.
- Отдельная история сообщений.
- CRUD подключений.
- Поддержка exclusive fullscreen overlay/anti-cheat.

## Приёмка

- Floating перетаскивается за свободную часть заголовка при click-through off.
- Кнопки titlebar и карточек кликаются и не начинают drag.
- Hide button скрывает окно; повторный show восстанавливает его.
- Main titlebar, tray и hotkey отображают фактическую visibility.
- После перезапуска floating восстанавливает последнее состояние показа/скрытия.
- Floating использует ту же dark/light тему и semantic tokens, что главное окно;
  пользовательский цвет при включённой опции переопределяет только его фон.
- Изменение appearance видно без перезапуска.
- При уже работающем connection новое floating сразу показывает правильный status.
- Текст переносится и показывается целиком без ellipsis, line clamp и собственного
  scroll; обычное сообщение не изменяет базовую ширину.
- Длинное сообщение временно увеличивает высоту floating, а при нехватке высоты — и
  ширину в пределах work area. После очистки/сокращения окно возвращается к базовому
  размеру и позиции.
- Typing start показывает анимацию набора для правильного connection, а stop, финальное
  сообщение и disconnect гарантированно её убирают.
- Список из 10 подключений остаётся в пределах work area и прокручивается.
- После отключения второго монитора окно возвращается на доступный экран.
- Click-through всегда выключается из Settings → Interface.

## Верификация

- Unit/component tests mount snapshot, events, cleanup, empty/error/typing states и
  reduced-motion fallback.
- Tests pure size calculation, measured-content resize, shrink debounce и
  monitor-position fallback, если логика вынесена.
- Rust tests position validation/persistence там, где это возможно без реального окна.
- `npm run build` и применимые Rust checks.
- Ручная Windows-матрица: 100/125/150% DPI, один/два монитора, dark/light,
  click-through on/off, 0/1/4/10 connections.
- Ручная проверка короткого текста, многострочного текста, длинного слова/URL,
  временного роста высоты/ширины, полного отсутствия message scroll/обрезки и возврата
  размера после очистки.
- Ручная проверка typing start/stop/final message/disconnect и reduced motion.
- Проверка repeated show/hide и app restart в обоих последних состояниях visibility.

## Handoff

Записать выбранную coordinate system, max visible card count, default и persisted
lifecycle visibility, базовый/max динамический размер, resize anchor, typing lifecycle,
custom-background semantics и ограничения click-through. Эти факты используются
roadmap 006 и 007.
