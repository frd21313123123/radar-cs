@echo off
cd /d "%~dp0"
REM Standalone Release ZIP: launch the already compiled executable.
if exist "cas-external-radar.exe" (
  "cas-external-radar.exe" %*
  goto :done
)
REM Source repository: launch a previously built executable if present.
if exist "target\x86_64-pc-windows-msvc\release\cas-external-radar.exe" (
  "target\x86_64-pc-windows-msvc\release\cas-external-radar.exe" %*
  goto :done
)
if exist "target\release\cas-external-radar.exe" (
  "target\release\cas-external-radar.exe" %*
  goto :done
)
where cargo >nul 2>nul
if errorlevel 1 (
  echo [CAS Radar] Download the Windows ZIP from GitHub Releases.
  echo [CAS Radar] Or install Rust: https://rustup.rs
  pause
  exit /b 1
)
cargo run --release -- %*
:done
if errorlevel 1 (
  echo [CAS Radar] Application returned an error.
)
pause