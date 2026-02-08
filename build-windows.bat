@echo off
REM Lion Launcher - Windows Build Script
REM Erstellt: .exe (NSIS Installer) und .msi

echo.
echo ========================================
echo   Lion Launcher - Windows Build
echo ========================================
echo.

REM Prüfe ob Rust installiert ist
where rustc >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Rust ist nicht installiert!
    echo Bitte installiere Rust von: https://rustup.rs/
    pause
    exit /b 1
)

REM Prüfe ob Tauri CLI installiert ist
where cargo-tauri >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [INFO] Tauri CLI nicht gefunden. Installiere...
    cargo install tauri-cli --version "^2.0.0"
    if %ERRORLEVEL% NEQ 0 (
        echo [ERROR] Tauri CLI konnte nicht installiert werden!
        pause
        exit /b 1
    )
)

echo [INFO] Tauri CLI bereit
echo.

REM Menü anzeigen
echo Waehle Build-Typ:
echo.
echo   1) NSIS Installer (.exe)
echo   2) MSI Installer
echo   3) Beide (.exe + .msi)
echo   4) Abbrechen
echo.
set /p choice="Auswahl (1-4): "

if "%choice%"=="1" (
    set BUNDLES=nsis
) else if "%choice%"=="2" (
    set BUNDLES=msi
) else if "%choice%"=="3" (
    set BUNDLES=nsis,msi
) else if "%choice%"=="4" (
    echo Abgebrochen.
    exit /b 0
) else (
    echo Ungueltige Auswahl!
    pause
    exit /b 1
)

echo.
echo [INFO] Starte Build (%BUNDLES%)...
echo.

cargo tauri build --bundles %BUNDLES%

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [ERROR] Build fehlgeschlagen!
    pause
    exit /b 1
)

echo.
echo ========================================
echo   Build erfolgreich!
echo ========================================
echo.
echo Ausgabe-Dateien:
echo.

REM Zeige erstellte Dateien
if exist "target\release\bundle\nsis\*.exe" (
    echo   NSIS Installer:
    for %%f in (target\release\bundle\nsis\*.exe) do echo     %%f
    echo.
)

if exist "target\release\bundle\msi\*.msi" (
    echo   MSI Installer:
    for %%f in (target\release\bundle\msi\*.msi) do echo     %%f
    echo.
)

echo Fertig!
echo.
pause

