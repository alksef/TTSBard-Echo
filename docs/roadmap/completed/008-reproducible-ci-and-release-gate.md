# 008 — Воспроизводимый CI и release gate

- **Статус:** completed
- **Дата:** 2026-09-14
- **Приоритет:** P0
- **Зависимости:** нет
- **Блокирует:** 009, 010, 011, 013

## Результат

Каждый PR и push в основную ветку получает одинаковый набор автоматических
проверок, Rust-зависимости разрешаются воспроизводимо, а релизный тег не может
опубликовать commit без успешного CI.

## Проблема

До начала roadmap CI запускался только для pull request, `Cargo.lock` был
исключён из Git, а release workflow не проверял результат CI основной ветки.
Ручной запуск release workflow также пытался трактовать имя ветки как semver.

## Объём работ

1. Запускать `ci.yml` для PR, push в `main`/`master` и вручную.
2. Добавить read-only permissions, отмену устаревших CI-прогонов и таймауты.
3. Использовать Node.js 22 и npm lockfile через `npm ci`.
4. Хранить `src-tauri/Cargo.lock` в Git и выполнять Cargo-команды с `--locked`.
5. Проверять formatting, Clippy, Rust tests, frontend contracts/build и Windows
   Tauri debug build.
6. Перед релизом ждать успешный push-запуск CI для того же commit.
7. Сохранить рабочий `workflow_dispatch`: версия берётся из репозитория, release
   создаётся только по валидному тегу `vX.Y.Z`.
8. Актуализировать документацию сборки и release-процесс.

## Не входит

- CodeQL, Dependabot, подпись бинарников и автообновление.
- Перенос Piper/espeak/libclang-этапов из `app-tts-v2`: у Echo нет этих
  зависимостей.
- Оптимизация продолжительности Tauri bundle smoke.

## Приёмка

- Новый commit в `main` или `master` создаёт CI run.
- Повторный push в ту же ветку отменяет устаревший незавершённый run.
- Изменение lockfile, не соответствующее manifests, обнаруживается `--locked`.
- Тег на commit без успешного CI не создаёт GitHub Release.
- Тег `vX.Y.Z` на проверенном commit создаёт Windows artifacts и prerelease.
- Ручной запуск успешно собирает текущую версию и не создаёт release.

## Верификация

- `npm test`;
- `npm run build`;
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`;
- `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets --all-features -- -D warnings`;
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --all-targets`;
- успешный push-run GitHub Actions;
- ручной `workflow_dispatch`;
- тестовый release tag после завершения остальных проверок.

## Completion note

Выполнено 2026-09-14, все проверки зелёные в удалённом CI.

Проверки:

- Локально: `npm test`, `npm run build`, `cargo fmt --check`,
  `cargo clippy --locked -D warnings`, `cargo test --locked` (43 теста),
  `npm run tauri -- build --debug` (MSI + NSIS).
- Push-CI для `fade947`, все 5 джоб зелёные:
  [run 34858562241](https://github.com/alksef/ttsbard-echo/actions/runs/34858562241).
- Ручной `workflow_dispatch` `ci.yml` — success:
  [run 34859298306](https://github.com/alksef/ttsbard-echo/actions/runs/34859298306);
  повторный dispatch отменил устаревший
  [run 34859206272](https://github.com/alksef/ttsbard-echo/actions/runs/34859206272)
  (conclusion `cancelled`).
- Ручной `workflow_dispatch` `build.yml` собрал артефакты с версией из
  репозитория и не создал release (release-джоба пропущена):
  [run 34857516466](https://github.com/alksef/ttsbard-echo/actions/runs/34857516466).
- Тег `v0.1.1` на проверенном commit: release-джоба `wait-for-ci` дождалась
  зелёного push-CI того же SHA, собраны Windows-артефакты и опубликован
  prerelease [v0.1.1](https://github.com/alksef/ttsbard-echo/releases/tag/v0.1.1)
  ([run 34859506448](https://github.com/alksef/ttsbard-echo/actions/runs/34859506448)).
  Версия артефактов 0.1.1 берётся из тега; версия в репозитории остаётся 0.1.0.
  Это тестовый релиз для проверки gate.

Попутно исправленные дефекты:

- `npm run tauri build --debug` не пробрасывал `--debug` в tauri CLI (npm
  глотал флаг), из-за чего build-check молча делал release-сборку; заменено на
  `npm run tauri -- build --debug`.
- `scripts/set-version.cjs` не обновлял версию локального пакета в
  `Cargo.lock`, из-за чего `cargo test --locked` в релизном workflow падал бы
  сразу после применения версии тега; скрипт теперь синхронизирует lockfile.
- Linux-джобы clippy/test падали: `tauri::generate_context!()` требует
  существования `frontendDist` на этапе компиляции; перед cargo-командами
  добавлены `npm ci` + `npm run build`.

Известное ограничение: негативный сценарий «тег на непроверенный commit»
покрыт конструкцией gate (`wait-for-ci` ждёт только push-run того же SHA и
падает по таймауту/failure), отдельным тестом не прогонялся.

## Завершение

После зелёного удалённого CI перенести документ в `completed/` и записать ссылки
на CI run и проверенный release run. Локальная зелёная сборка не заменяет эту
проверку.
