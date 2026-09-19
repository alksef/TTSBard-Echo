# cargo.ps1 — convenience wrapper that prepares the MSVC + Rustup environment
# before invoking Cargo. Designed for ttsbard-echo (no libclang needed).
#
# Problem solved: a standalone gnullvm Rust build on PATH shadows the rustup
# MSVC toolchain and looks for a missing `x86_64-w64-mingw32-clang` linker.
# This script enters the MSVC developer environment (vcvars64.bat) and puts
# the rustup proxy (`%USERPROFILE%\.cargo\bin`) first on PATH so `cargo`
# resolves to the working MSVC toolchain.
#
# Builds go to a shared Cargo target cache on E: by default (see
# build.local.psd1 `CargoTargetDir`), mirroring app-tts-v2.
#
# Usage:
#   .\scripts\cargo.ps1 check  --manifest-path src-tauri\Cargo.toml
#   .\scripts\cargo.ps1 clippy --manifest-path src-tauri\Cargo.toml -- -D warnings
#   .\scripts\cargo.ps1 test   --manifest-path src-tauri\Cargo.toml
#   .\scripts\cargo.ps1 --version
#
# Optional overrides:
#   -CargoTargetDir <path>          force the target dir for this invocation
#   TTSBARD_CARGO_TARGET_DIR (env)  same, via environment
#   TTSBARD_RUST_BIN_DIR (env)      dir with cargo/rustc (default %USERPROFILE%\.cargo\bin)
#   TTSBARD_VCVARS (env)            explicit path to vcvars64.bat
#   build.local.psd1                CargoTargetDir / RustBinDir (gitignored)
#
# All extra arguments are forwarded to cargo (via the automatic $args); cargo's
# exit code is preserved.
#
# Target dir is configured via build.local.psd1 `CargoTargetDir` (per-machine,
# gitignored) or the TTSBARD_CARGO_TARGET_DIR env var. There is intentionally no
# -CargoTargetDir script parameter: any named param here would steal the first
# positional cargo argument (e.g. `check`). Use the env var for a one-off override.

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot

# ---------------------------------------------------------------------------
# 1. Resolve the Rust bin directory (rustup proxy by default).
# ---------------------------------------------------------------------------
$rustBinDir = $env:TTSBARD_RUST_BIN_DIR
if (-not $rustBinDir) {
    $localConfigPath = Join-Path $PSScriptRoot 'build.local.psd1'
    if (Test-Path $localConfigPath -PathType Leaf) {
        $localConfig = Import-PowerShellDataFile $localConfigPath
        if ($localConfig -and $localConfig.ContainsKey('RustBinDir')) {
            $rustBinDir = $localConfig['RustBinDir']
        }
    }
}
if (-not $rustBinDir) {
    $rustBinDir = Join-Path $env:USERPROFILE '.cargo\bin'
}

# Expand %VAR%-style and relative paths.
$rustBinDir = [regex]::Replace($rustBinDir, '%([^%]+)%', {
    param($match)
    $value = [Environment]::GetEnvironmentVariable($match.Groups[1].Value)
    if ($null -ne $value -and $value -ne '') { return $value }
    return $match.Value
})
if (-not [System.IO.Path]::IsPathRooted($rustBinDir)) {
    $rustBinDir = Join-Path $repoRoot $rustBinDir
}
$rustBinDir = [System.IO.Path]::GetFullPath($rustBinDir)
if (-not (Test-Path $rustBinDir -PathType Container)) {
    throw "RustBinDir does not exist or is not a directory: $rustBinDir"
}

# Put rustBinDir first on PATH (remove any earlier duplicate entry).
$rustBinNormalized = $rustBinDir.TrimEnd('\')
$otherEntries = @(($env:PATH -split ';') | Where-Object {
    $_ -and $_.TrimEnd('\') -ne $rustBinNormalized
})
$env:PATH = (@($rustBinDir) + $otherEntries) -join ';'

# ---------------------------------------------------------------------------
# 1b. Resolve the Cargo target directory.
#
# Builds go to a shared cache on E: (mirrors app-tts-v2's setup) to keep the
# repo working tree clean and reuse compiled deps across checkouts. Resolution
# order: -CargoTargetDir parameter > TTSBARD_CARGO_TARGET_DIR env >
# CargoTargetDir in build.local.psd1 > default src-tauri\target.
# ---------------------------------------------------------------------------
$cargoTargetDir = $null
if ($env:TTSBARD_CARGO_TARGET_DIR) {
    $cargoTargetDir = $env:TTSBARD_CARGO_TARGET_DIR
} elseif ($localConfig -and $localConfig.ContainsKey('CargoTargetDir') -and $localConfig['CargoTargetDir']) {
    $cargoTargetDir = [string]$localConfig['CargoTargetDir']
}

if ($cargoTargetDir) {
    # Expand %VAR%-style refs and resolve to an absolute path.
    $cargoTargetDir = [regex]::Replace($cargoTargetDir, '%([^%]+)%', {
        param($match)
        $value = [Environment]::GetEnvironmentVariable($match.Groups[1].Value)
        if ($null -ne $value -and $value -ne '') { return $value }
        return $match.Value
    })
    if (-not [System.IO.Path]::IsPathRooted($cargoTargetDir)) {
        $cargoTargetDir = Join-Path $repoRoot $cargoTargetDir
    }
    $cargoTargetDir = [System.IO.Path]::GetFullPath($cargoTargetDir)
    if (-not (Test-Path $cargoTargetDir -PathType Container)) {
        New-Item -ItemType Directory -Force -Path $cargoTargetDir | Out-Null
    }
    $env:CARGO_TARGET_DIR = $cargoTargetDir
    Write-Host "[cargo.ps1] CARGO_TARGET_DIR = $cargoTargetDir" -ForegroundColor DarkGray
} else {
    Write-Host "[cargo.ps1] CARGO_TARGET_DIR not set; using cargo default (src-tauri\target)" -ForegroundColor DarkGray
}

# ---------------------------------------------------------------------------
# 2. Enter the MSVC developer environment (vcvars64.bat).
# ---------------------------------------------------------------------------
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
        # Not fatal: rustc locates the MSVC linker on its own (vswhere/registry).
        # vcvars is only needed to repair PATH on machines where a non-MSVC
        # toolchain shadows rustup (see header) — e.g. CI runners work fine
        # without it. Set TTSBARD_VCVARS to force a specific file.
        Write-Warning "[cargo.ps1] vcvars64.bat not found; continuing without the MSVC developer environment."
    }
}

if ($vcvars) {
    # Run vcvars64.bat in cmd, dump the resulting environment, and re-apply it here.
    # (`call` so vcvars' own SET statements propagate; output is captured, not shown.)
    $envOutput = & "$env:ComSpec" /c "`"$vcvars`" >nul 2>&1 && set" 2>&1
    foreach ($line in $envOutput) {
        if ($line -match '^([^=]+)=(.*)$') {
            $name = $Matches[1]
            $value = $Matches[2]
            # Keep our hardened PATH (rustup proxy first) instead of vcvars' order.
            if ($name -ieq 'PATH') { continue }
            [Environment]::SetEnvironmentVariable($name, $value, 'Process')
        }
    }
}

# ---------------------------------------------------------------------------
# 3. Run cargo from the repo root, forwarding all arguments.
# ---------------------------------------------------------------------------
# NOTE on stderr: cargo writes ALL of its output (progress AND diagnostics) to
# stderr. In PowerShell 5.1, a native command's stderr lines become ErrorRecord
# objects; under `$ErrorActionPreference = 'Stop'` that is a terminating error,
# and even under `Continue` they land in the error stream instead of stdout
# (so piping/redirecting the script's output drops them). Merging stderr into
# stdout with `2>&1` at the call site keeps everything as plain text on stdout
# and suppresses the NativeCommandError wrapping. We restore `Continue` too so
# any genuinely unexpected error does not abort the wrapper.
Push-Location $repoRoot
try {
    $prevEAP = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    # `2>&1` must be attached to the native call itself, not appended to the args.
    $output = & cargo @args 2>&1
    $code = $LASTEXITCODE
    # Emit captured output as text so callers/redirectors see it on stdout.
    $output | Out-String | Write-Host
    exit $code
}
finally {
    $ErrorActionPreference = $prevEAP
    Pop-Location
}
