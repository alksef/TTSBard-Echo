# build.ps1 — сборка ttsbard-echo (Tauri) под Windows.
#
# Использование:
#   .\scripts\build.ps1                  # релиз по умолчанию
#   .\scripts\build.ps1 -Mode debug      # debug-сборка (без инсталляторов)
#   .\scripts\build.ps1 -Mode release    # полная релиз-сборка (exe + nsis/msi)
#   .\scripts\build.ps1 -Clean           # очистить target/ и dist/ перед сборкой
#
# Локальная конфигурация (опционально):
#   .\scripts\build.ps1 -CargoTargetDir D:\custom-target
#   .\scripts\build.ps1 -RustBinDir %USERPROFILE%\.cargo\bin
#   .\scripts\build.ps1 -ConfigFile my-config.psd1
#
# Конфигурация загружается из scripts/build.local.psd1 (игнорируется Git).
# Пример: см. scripts/build.local.example.psd1.
#
# Обёртки для двойного клика: build-debug.bat, build-release.bat.
# Для быстрых cargo check/clippy/test без полной сборки: scripts/cargo.ps1.
#
# Артефакты:
#   exe:      <cargo-target>\<debug|release>\ttsbard-echo.exe
#   bundles:  <cargo-target>\release\bundle\{nsis,msi}\  (только release)

[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string]$Mode = 'release',

    [switch]$Clean,

    [string]$CargoTargetDir,

    [string]$RustBinDir,

    [string]$ConfigFile
)

$ErrorActionPreference = 'Stop'

# --- Цветной вывод -----------------------------------------------------------
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "    $msg" -ForegroundColor Green }
function Write-WarnLine($msg) { Write-Host "    ! $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "    X $msg" -ForegroundColor Red }

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

# --- Вспомогательные функции для путей ----------------------------------------

function Expand-EnvRefs([string]$path) {
    return [regex]::Replace($path, '%([^%]+)%', {
        param($m)
        $envVal = [Environment]::GetEnvironmentVariable($m.Groups[1].Value)
        if ($null -ne $envVal -and $envVal -ne '') { return $envVal }
        return $m.Value
    })
}

function Resolve-Absolute([string]$path) {
    if ([System.IO.Path]::IsPathRooted($path)) {
        return [System.IO.Path]::GetFullPath($path)
    }
    return [System.IO.Path]::GetFullPath([System.IO.Path]::Combine($repoRoot, $path))
}

function Test-IsAncestorOf([string]$candidate, [string]$child) {
    $candidate = $candidate.TrimEnd('\') + '\'
    $child = $child.TrimEnd('\') + '\'
    return $child.StartsWith($candidate, [StringComparison]::OrdinalIgnoreCase) -and
           $candidate.Length -lt $child.Length
}

function Test-IsUnsafeCleanTarget([string]$target) {
    $srcTauri = [System.IO.Path]::GetFullPath([System.IO.Path]::Combine($repoRoot, 'src-tauri'))
    if ($target -eq $repoRoot) { return $true }
    if ($target -eq $env:USERPROFILE) { return $true }
    if ($target -eq $srcTauri) { return $true }
    if (Test-IsAncestorOf $target $repoRoot) { return $true }
    if (Test-IsAncestorOf $target $env:USERPROFILE) { return $true }
    $pathRoot = [System.IO.Path]::GetPathRoot($target)
    if ($target.TrimEnd('\') -eq $pathRoot.TrimEnd('\')) { return $true }
    return $false
}

# --- Загрузка конфигурации ----------------------------------------------------

$configFilePath = if ($ConfigFile) {
    $resolved = $ConfigFile
    if (-not [System.IO.Path]::IsPathRooted($resolved)) {
        $resolved = [System.IO.Path]::Combine($repoRoot, $resolved)
    }
    [System.IO.Path]::GetFullPath($resolved)
} else {
    [System.IO.Path]::GetFullPath([System.IO.Path]::Combine($repoRoot, 'scripts', 'build.local.psd1'))
}

$configData = $null
$configLoaded = $false

if (Test-Path $configFilePath -PathType Leaf) {
    $configData = Import-PowerShellDataFile $configFilePath
    $configLoaded = $true

    if ($null -eq $configData) {
        $configData = @{}
    }

    foreach ($key in $configData.Keys) {
        if ($key -ne 'CargoTargetDir' -and $key -ne 'RustBinDir') {
            Write-Err "Unknown config key '$key' in $configFilePath. Allowed keys: CargoTargetDir, RustBinDir."
            exit 1
        }
        $val = $configData[$key]
        if ($null -ne $val -and $val -isnot [string]) {
            Write-Err "Config key '$key' in $configFilePath must be a string or `$null, got $($val.GetType().Name)."
            exit 1
        }
    }
} elseif ($ConfigFile) {
    Write-Err "Config file not found: $configFilePath (explicitly supplied via -ConfigFile)"
    exit 1
}

# --- Разрешение CargoTargetDir -----------------------------------------------

$defaultTarget = Join-Path $repoRoot 'src-tauri\target'

if ($PSBoundParameters.ContainsKey('CargoTargetDir')) {
    if ([string]::IsNullOrEmpty($CargoTargetDir)) {
        Write-Err 'CargoTargetDir parameter is empty.'
        exit 1
    }
    $targetDir = Resolve-Absolute (Expand-EnvRefs $CargoTargetDir)
} elseif (Test-Path 'Env:TTSBARD_CARGO_TARGET_DIR') {
    $envVal = $env:TTSBARD_CARGO_TARGET_DIR
    if ([string]::IsNullOrEmpty($envVal)) {
        Write-Err 'TTSBARD_CARGO_TARGET_DIR environment variable is empty.'
        exit 1
    }
    $targetDir = Resolve-Absolute (Expand-EnvRefs $envVal)
} elseif ($configData -and $configData.ContainsKey('CargoTargetDir') -and $null -ne $configData['CargoTargetDir']) {
    $cfgVal = $configData['CargoTargetDir']
    if ([string]::IsNullOrEmpty($cfgVal)) {
        Write-Err 'CargoTargetDir in config file is empty.'
        exit 1
    }
    $targetDir = Resolve-Absolute (Expand-EnvRefs $cfgVal)
} else {
    $targetDir = Resolve-Absolute $defaultTarget
}

$env:CARGO_TARGET_DIR = $targetDir

# --- Разрешение RustBinDir ---------------------------------------------------

$rustBinDir = $null

if ($PSBoundParameters.ContainsKey('RustBinDir')) {
    if ([string]::IsNullOrEmpty($RustBinDir)) {
        Write-Err 'RustBinDir parameter is empty.'
        exit 1
    }
    $rustBinDir = Resolve-Absolute (Expand-EnvRefs $RustBinDir)
} elseif (Test-Path 'Env:TTSBARD_RUST_BIN_DIR') {
    $envVal = $env:TTSBARD_RUST_BIN_DIR
    if ([string]::IsNullOrEmpty($envVal)) {
        Write-Err 'TTSBARD_RUST_BIN_DIR environment variable is empty.'
        exit 1
    }
    $rustBinDir = Resolve-Absolute (Expand-EnvRefs $envVal)
} elseif ($configData -and $configData.ContainsKey('RustBinDir') -and $null -ne $configData['RustBinDir']) {
    $cfgVal = $configData['RustBinDir']
    if ([string]::IsNullOrEmpty($cfgVal)) {
        Write-Err 'RustBinDir in config file is empty.'
        exit 1
    }
    $rustBinDir = Resolve-Absolute (Expand-EnvRefs $cfgVal)
} else {
    $rustBinDir = Resolve-Absolute (Join-Path $env:USERPROFILE '.cargo\bin')
}

if (-not (Test-Path $rustBinDir -PathType Container)) {
    Write-Err "RustBinDir does not exist or is not a directory: $rustBinDir"
    exit 1
}
$rustBinNormalized = $rustBinDir.TrimEnd('\')
$otherEntries = @(($env:PATH -split ';') | Where-Object {
    $_ -and $_.TrimEnd('\') -ne $rustBinNormalized
})
$env:PATH = (@($rustBinDir) + $otherEntries) -join ';'

# --- MSVC developer environment (vcvars64.bat) --------------------------------
# tauri CLI сам вызывает cargo, поэтому окружение нужно в этом процессе
# (та же логика, что в scripts/cargo.ps1).

$vcvars = $env:TTSBARD_VCVARS
if (-not $vcvars) {
    $candidates = @(
        foreach ($vsMajor in @('18', '17')) {
            foreach ($edition in @('BuildTools', 'Community', 'Enterprise', 'Professional')) {
                "${env:ProgramFiles(x86)}\Microsoft Visual Studio\$vsMajor\$edition\VC\Auxiliary\Build\vcvars64.bat"
                "$env:ProgramFiles\Microsoft Visual Studio\$vsMajor\$edition\VC\Auxiliary\Build\vcvars64.bat"
            }
        }
    ) | Where-Object { $_ -and (Test-Path $_ -PathType Leaf) }
    if (@($candidates).Count -gt 0) {
        $vcvars = @($candidates)[0]
    } else {
        # Not fatal: rustc locates the MSVC linker on its own; vcvars is only a
        # PATH repair for machines where a non-MSVC toolchain shadows rustup.
        Write-WarnLine "vcvars64.bat not found; continuing without the MSVC developer environment. Set TTSBARD_VCVARS to force a specific file."
    }
}

if ($vcvars) {
    $prePath = @($env:PATH -split ';' | Where-Object { $_ })
    $prePathSeen = @{}
    foreach ($entry in $prePath) { $prePathSeen[$entry.TrimEnd('\').ToLowerInvariant()] = $true }
    $envOutput = & "$env:ComSpec" /c "`"$vcvars`" >nul 2>&1 && set" 2>&1
    foreach ($line in $envOutput) {
        if ($line -match '^([^=]+)=(.*)$') {
            $name = $Matches[1]
            $value = $Matches[2]
            if ($name -ieq 'PATH') {
                # Merge, don't drop (same rationale as cargo.ps1): the other vcvars
                # variables tell rustc this is a VS developer environment, so
                # link.exe must be reachable on PATH — otherwise rustc falls back
                # to whatever link.exe comes first (e.g. Git's coreutils one).
                $added = @($value -split ';' | Where-Object {
                    $_ -and -not $prePathSeen.ContainsKey($_.TrimEnd('\').ToLowerInvariant())
                })
                $env:PATH = (@($rustBinDir) + $added + ($prePath | Where-Object {
                    $_.TrimEnd('\') -ne $rustBinDir.TrimEnd('\')
                })) -join ';'
            } else {
                [Environment]::SetEnvironmentVariable($name, $value, 'Process')
            }
        }
    }
}

# --- Информация о конфигурации -----------------------------------------------

$modeLabel = $Mode
if ($Clean) { $modeLabel = "$Mode (+clean)" }
Write-Step "ttsbard-echo build — mode: $modeLabel"
Write-Step "Repo: $repoRoot"
if ($configLoaded) { Write-Step "Config file: $configFilePath" }
Write-Step "Cargo target: $targetDir"
Write-Step "Rust bin: $rustBinDir"

# --- Проверка окружения ------------------------------------------------------
Write-Step "Checking toolchain..."

foreach ($cmd in @('node', 'npm', 'cargo')) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Err "$cmd not found in PATH. Установите требуемый инструмент и повторите сборку."
        exit 1
    }
}
try {
    $nodeVer = (node -v)
    $npmVer  = (npm -v)
    $rustcVer = (rustc --version)
} catch {
    Write-Err "Не удалось определить версии toolchain: $_"
    exit 1
}
Write-Ok "node $nodeVer, npm $npmVer"
Write-Ok $rustcVer

# --- Вспомогательные константы -----------------------------------------------
$defaultCanonical = [System.IO.Path]::GetFullPath((Join-Path $repoRoot 'src-tauri\target'))
$isExternalTarget = ($targetDir.TrimEnd('\') -ne $defaultCanonical.TrimEnd('\'))
$distDir   = Join-Path $repoRoot 'dist'
$markerName = '.ttsbard-echo-build-target'

# --- Опциональная очистка ----------------------------------------------------

if ($Clean) {
    if (Test-IsUnsafeCleanTarget $targetDir) {
        Write-Err "Refusing -Clean: target dir ($targetDir) is a filesystem root, a protected location (repository, user profile, or src-tauri), or an ancestor of such a location. Set a project-specific target directory instead."
        exit 1
    }

    if ($isExternalTarget -and (Test-Path $targetDir)) {
        $markerFile = Join-Path $targetDir $markerName
        if (-not (Test-Path $markerFile -PathType Leaf)) {
            Write-Err "Refusing -Clean: external Cargo target ($targetDir) is missing the marker file '$markerName'. Run a non-clean build first, or create the marker manually if this target was previously initialized for this project."
            exit 1
        }
    }

    Write-Step "Cleaning build artifacts..."
    foreach ($d in @($targetDir, $distDir)) {
        if (Test-Path $d) {
            Remove-Item -Recurse -Force $d
            Write-Ok "removed $d"
        }
    }
}

if ($isExternalTarget) {
    if (-not (Test-Path $targetDir)) {
        New-Item -ItemType Directory -Force $targetDir | Out-Null
    }
    $markerFile = Join-Path $targetDir $markerName
    if (-not (Test-Path $markerFile -PathType Leaf)) {
        New-Item -ItemType File -Force $markerFile | Out-Null
    }
}

# --- Установка npm-зависимостей (если нужно) ---------------------------------
Write-Step "Checking npm dependencies..."
$nodeModules = Join-Path $repoRoot 'node_modules'
if (-not (Test-Path $nodeModules)) {
    Write-Step "Installing npm dependencies..."
    npm install
    if ($LASTEXITCODE -ne 0) { Write-Err "npm install failed"; exit 1 }
    Write-Ok "npm install done"
} else {
    Write-Ok "node_modules exists, skipping install"
}

# --- Сборка ------------------------------------------------------------------
# tauri build сам запускает frontend build (vite) по beforeBuildCommand из
# tauri.conf.json, поэтому отдельный `npm run build` здесь не нужен.
$buildStart = Get-Date

if ($Mode -eq 'debug') {
    Write-Step "Building (tauri build --debug --no-bundle)..."
    # --debug: бэкенд в debug-профайле, фронтенд-бандл, готовый exe, БЕЗ инсталляторов.
    npm run tauri -- build --debug --no-bundle
} else {
    Write-Step "Building (tauri build, release)..."
    npm run tauri -- build
}

if ($LASTEXITCODE -ne 0) {
    Write-Err "tauri build failed (exit $LASTEXITCODE)"
    exit $LASTEXITCODE
}

$elapsed = (Get-Date) - $buildStart
Write-Ok ("build done in {0:mm\:ss}" -f $elapsed)

# --- Отчёт об артефактах -----------------------------------------------------
Write-Step "Artifacts:"

$targetProfile = if ($Mode -eq 'debug') { 'debug' } else { 'release' }
$exePath = Join-Path $targetDir "$targetProfile\ttsbard-echo.exe"
if (Test-Path $exePath) {
    $sizeMb = [math]::Round((Get-Item $exePath).Length / 1MB, 1)
    Write-Ok "EXE  : $exePath ($sizeMb MB)"
} else {
    Write-WarnLine "EXE not found at expected path: $exePath"
}

if ($Mode -eq 'release') {
    $bundleDir = Join-Path $targetDir 'release\bundle'
    if (Test-Path $bundleDir) {
        $installers = Get-ChildItem -Recurse -Path $bundleDir -Include '*.exe','*.msi' -ErrorAction SilentlyContinue
        if ($installers) {
            foreach ($inst in $installers) {
                $sizeMb = [math]::Round($inst.Length / 1MB, 1)
                Write-Ok ("BUNDLE: {0} ({1} MB)" -f $inst.FullName, $sizeMb)
            }
        } else {
            Write-WarnLine "Bundle dir exists but no .exe/.msi installers found"
        }
    } else {
        Write-WarnLine "No bundle directory (installers) — check tauri.conf.json bundle config"
    }
}

Write-Host ""
Write-Host "BUILD SUCCEEDED" -ForegroundColor Green
