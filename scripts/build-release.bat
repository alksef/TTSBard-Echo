@echo off
REM build-release.bat — релизная сборка ttsbard-echo (двойной клик или консоль).
REM Полная сборка: оптимизированный ttsbard-echo.exe + инсталляторы (nsis/msi).
SETLOCAL

REM Переход в корень репо (родитель папки scripts\).
CD /D "%~dp0\.."

REM -ExecutionPolicy Bypass — обходит локальную политику выполнения PS.
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\build.ps1" -Mode release
SET EXITCODE=%ERRORLEVEL%

echo.
if "%EXITCODE%"=="0" (
    echo === Release build OK ===
) else (
    echo === Release build FAILED (exit %EXITCODE%) ===
)

ENDLOCAL & EXIT /B %EXITCODE%
