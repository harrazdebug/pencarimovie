@echo off
rem Stop PencariMovie Server (Windows).
rem
rem Kills whatever listens on the port, the tray icon and the server process, so
rem `start.bat` / `start-tray.bat` can be re-run immediately. Same job as
rem `pms stop`, for a folder that only has the packaged binary.
setlocal
cd /d "%~dp0"
if "%PORT%"=="" set PORT=8088
set "ROOT=%~dp0."

echo Stopping PencariMovie Server on port %PORT%...

for /f "tokens=5" %%P in ('netstat -ano 2^>nul ^| findstr ":%PORT% " ^| findstr "LISTENING"') do (
  echo   Killing listener PID %%P
  taskkill /PID %%P /F >nul 2>nul
)

taskkill /F /IM pencarimovie-server.exe >nul 2>nul

powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -and $_.CommandLine -match 'tray\.ps1' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }" >nul 2>nul
if exist "%ROOT%\storage\tray.pid" del /q "%ROOT%\storage\tray.pid" >nul 2>nul
if exist "%ROOT%\storage\server.pid" del /q "%ROOT%\storage\server.pid" >nul 2>nul

rem Leftover cloudflared from this folder only (the box may run others).
powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'cloudflared' -and $_.CommandLine -and $_.CommandLine -match 'storage[\\/]tunnel' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }" >nul 2>nul

echo Server stopped.
endlocal
