@echo off
rem Opens the app on the DEMO company (Lotus Spice & Tea Traders), not your real data.
rem The demo data lives in %LOCALAPPDATA%\CenangoAccounting-Demo. See docs\demo\README.md.
setlocal
set "ACCOUNTING_DATA_DIR=%LOCALAPPDATA%\CenangoAccounting-Demo"
set "APP=%~dp0..\target\debug\accounting-desktop.exe"
if not exist "%APP%" set "APP=%~dp0..\target\release\accounting-desktop.exe"
if not exist "%APP%" (
  echo Build the app first: cargo build -p accounting-desktop
  exit /b 1
)
start "" "%APP%"
