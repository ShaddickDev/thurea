@echo off
rem Installs Thurea on this PC (asks for Administrator).
net session >nul 2>&1 || (powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs" & exit /b)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Thurea.ps1" -Install
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Thurea.ps1" -Status
pause
