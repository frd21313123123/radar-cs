@echo off
cd /d "%~dp0"
cargo build --release
if errorlevel 1 (
 echo Build failed.
 pause
 exit /b 1
)
echo Built: target\release\cas-external-radar.exe
pause