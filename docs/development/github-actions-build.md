# Сборка Windows в GitHub Actions

## Текущий workflow

Windows-сборка настроена в `.github/workflows/build.yml` через runner
`windows-latest` и target `x86_64-pc-windows-msvc`.

На актуальном образе GitHub Windows Server 2025 доступны LLVM и CMake:

- LLVM 20.1.8;
- CMake 3.31.6;
- Visual Studio LLVM/Clang components.

Актуальный список установленного ПО: [Windows runner images](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md#installed-software).

У Echo нет зависимостей от Piper, espeak или libclang (в отличие от
`app-tts-v2`), поэтому отдельных этапов для нативных инструментов в workflow нет.

## Релизный запуск

Release workflow запускается для push тега `v*` и вручную через
`workflow_dispatch`. Только запуск по тегу создаёт GitHub Release; ручной запуск
собирает артефакты с версией из репозитория.

Отдельный `ci.yml` запускается для pull request и push в `master`/`main`, а также
вручную. При публикации тега release workflow ждёт успешный push-запуск CI для
того же commit и не выпускает непроверенную ревизию. Поэтому релизный тег должен
указывать на commit из `master` или `main`.

Перед сборкой приложения release workflow запускает `npm test`, frontend build
и полный набор Rust-тестов на Windows через
`cargo test --manifest-path src-tauri/Cargo.toml --locked`. Rust-тесты идут
до Tauri bundle. Падение любой проверки останавливает создание артефакта.

Перед тегом синхронизируйте версию штатным скриптом, проверьте diff и сборку:

```powershell
node scripts/set-version.cjs 0.14.0
npm run build
scripts/cargo.ps1 --% check --manifest-path src-tauri/Cargo.toml --locked
```

После зелёного push-запуска CI создайте и отправьте тег, указывающий на тот же
commit:

```powershell
git tag v0.14.0
git push origin v0.14.0
```

В release workflow версия извлекается из имени тега и повторно применяется через
`scripts/set-version.cjs`. Отдельной версии внутри workflow нет. Если требуется
повторить сборку без нового тега, используйте ручной запуск: он сохраняет версию
из репозитория и не создаёт release автоматически.

## Кэширование

Кэш npm включён в `actions/setup-node` через `cache: npm`. Rust-кэш настроен
после шага `Setup Rust`:

```yaml
- name: Cache Rust
  uses: Swatinem/rust-cache@v2
  with:
    workspaces: src-tauri
```

Кэшируются Cargo registry, Git-зависимости и `src-tauri/target`. Самые тяжёлые
зависимости Echo — `tauri`, `reqwest` и `image`. Первый запуск создаёт кэш,
последующие сборки используют его.

## Фиксация зависимостей

В репозитории находятся:

- `Cargo.lock`;
- `package-lock.json`;
- `.github/workflows/build.yml`.

Это обеспечивает воспроизводимое разрешение Rust- и npm-зависимостей и делает
ключи кэша стабильными.
