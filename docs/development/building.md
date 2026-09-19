# Сборка

Запуск frontend для разработки:

```powershell
npm ci
npm run dev
```

Проверка и production-сборка frontend:

```powershell
npm test
npm run build
```

Полный набор frontend- и Rust-проверок перед релизом приведён в
[руководстве по тестированию](testing.md).

Полная Windows-сборка Tauri:

```powershell
npm run tauri build
```

Готовые иконки пакета находятся в `src-tauri/icons/`.
