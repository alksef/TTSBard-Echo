# 013 — Доверенная доставка и обновления

- **Статус:** proposed, требует выбора сертификата и update channel
- **Приоритет:** P2 до публичной beta, P0 до stable
- **Зависимости:** 008, 010

## Результат

Пользователь получает проверяемую подписанную Windows-сборку и безопасно
обновляется по выбранному каналу; зависимости автоматически проверяются на
известные уязвимости.

## Объём работ

1. Выбрать модель подписи Windows binaries/installer и управление сертификатом.
2. Включить Tauri updater с подписанными manifests и отдельными alpha/beta/stable
   каналами либо явно утвердить один канал.
3. Разделить release artifacts на понятные NSIS/MSI targets; не публиковать
   неожиданные внутренние файлы wildcard-паттерном.
4. Добавить checksum и provenance/attestation, поддерживаемые GitHub Actions.
5. Добавить CodeQL для Rust и TypeScript.
6. Настроить Dependabot или Renovate с группировкой обновлений Rust/npm/Actions.
7. Зафиксировать rollback и действия при компрометации signing/update key.

## Не входит

- Собственный update server, если достаточно GitHub Releases.
- Microsoft Store distribution.
- Автоматическое продвижение beta в stable без ручного решения.

## Приёмка

- Windows показывает проверяемого издателя для installer и binary.
- Клиент отвергает изменённый manifest или artifact.
- Alpha не обновляется на stable и наоборот без выбранной политики перехода.
- Release workflow имеет только необходимые write permissions и secrets доступны
  только release job.
- Dependency/security checks дают actionable failure без публикации секретов.
- Документирована и проверена процедура rollback.

## Верификация

- тестовый подписанный prerelease и обновление с предыдущей версии;
- negative test изменённого artifact/manifest;
- CodeQL run для обеих языковых групп;
- проверка permissions и secret scope workflow;
- ручная установка, upgrade, downgrade policy и uninstall на чистой Windows VM.
