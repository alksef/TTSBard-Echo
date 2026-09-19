# build.local.example.psd1 — пример локальной конфигурации сборки.
#
# Скопируйте этот файл в scripts/build.local.psd1 и отредактируйте
# под свою машину. scripts/build.local.psd1 игнорируется Git.
#
# Все ключи опциональны. Допустимые ключи: CargoTargetDir, RustBinDir.
# %NAME% в путях заменяется на значение переменной окружения.

@{
    # Кеш Cargo target (по умолчанию src-tauri\target).
    # Вынесенный кеш держит рабочий каталог чистым и переиспользует
    # скомпилированные зависимости между чекаутами.
    CargoTargetDir = 'E:\cargo-target\ttsbard-echo'

    # Каталог с Rust toolchain (имеет приоритет выше PATH).
    # Раскомментируйте, если нужно явно указать rustup bin.
    # RustBinDir = '%USERPROFILE%\.cargo\bin'
}
