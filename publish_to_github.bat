@echo off
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0publish_to_github.ps1" %*
if errorlevel 1 (
  echo.
  echo [CAS Radar] Publishing failed; see the error above.
)
pause