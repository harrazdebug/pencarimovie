@echo off
setlocal EnableDelayedExpansion
title PencariMovie Server
if not defined PENCARIMOVIE_NO_BANNER call :print_banner
for %%I in ("%~dp0.") do set "ROOT=%%~fI"
cd /d "%ROOT%"
set "FRANKENPHP_EXE=%ROOT%\bin\frankenphp.exe"
set HOST=0.0.0.0
set PORT=8088

rem Configure FrankenPHP/Caddy via environment variables (per https://github.com/php/frankenphp/blob/main/caddy/frankenphp/Caddyfile)
set CADDY_GLOBAL_OPTIONS=skip_install_trust
set FRANKENPHP_CONFIG=
set CADDY_EXTRA_CONFIG=
if not defined GOGC set GOGC=80
if not defined FD_DOWNLOAD_PARALLEL_CHUNKS set FD_DOWNLOAD_PARALLEL_CHUNKS=4

rem Thread pool (Caddyfile reads both). A client that browses N bridged-upstream
rem catalogs at once needs N slots, and each cold upstream can hold one for up to
rem 8s (Amp) + 10s (cURL fallback). At 8/16 every other route (even /api/version)
rem queued ~1-7s behind them. Raise here, or override per machine.
if not defined FRANKENPHP_NUM_THREADS set FRANKENPHP_NUM_THREADS=16
if not defined FRANKENPHP_MAX_THREADS set FRANKENPHP_MAX_THREADS=64

rem Detect LAN IP via default gateway route (avoids virtual adapter IPs)
set "LAN_IP="
for /f "tokens=4" %%i in ('route print -4 0.0.0.0 ^| findstr /R /C:" 0\.0\.0\.0[ ]*0\.0\.0\.0"') do (
  if not defined LAN_IP set "LAN_IP=%%i"
)

call :runtime_check

if exist "%FRANKENPHP_EXE%" goto start_server
php -v >nul 2>nul
if errorlevel 1 (
  echo PHP or FrankenPHP is required but was not found.
  echo Place FrankenPHP at %FRANKENPHP_EXE% or install PHP in PATH.
  echo.
  pause
  endlocal
  goto :eof
)

:start_server
echo Starting PencariMovie Server in the background...
if exist "%ROOT%\tray.ps1" (
  call :start_tray 1
) else (
  call :start_server_hidden
)
call :print_urls
echo   Stop:     "%~dp0stop.bat"
echo   Tray:     right-click the PencariMovie icon in the system tray
echo.
endlocal
goto :eof

:start_server_hidden
if exist "%FRANKENPHP_EXE%" (
  if exist "%ROOT%\Caddyfile" (
    set "SERVER_ARGS=run --config ""%ROOT%\Caddyfile"""
  ) else (
    set "SERVER_ARGS=php-server --listen %HOST%:%PORT% --root ""%ROOT%"""
  )
  if exist "%ROOT%\start-hidden.ps1" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\start-hidden.ps1" -FilePath "%FRANKENPHP_EXE%" -CommandLine "!SERVER_ARGS!"
  ) else (
    start "PencariMovie Server" /MIN "%FRANKENPHP_EXE%" !SERVER_ARGS!
  )
) else (
  if exist "%ROOT%\start-hidden.ps1" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\start-hidden.ps1" -FilePath php -CommandLine "-S %HOST%:%PORT% router.php"
  ) else (
    start "PencariMovie Server" /MIN php -S %HOST%:%PORT% router.php
  )
)
goto :eof

:start_tray
if not exist "%ROOT%\tray.ps1" goto :eof
if not exist "%ROOT%\start-hidden.ps1" (
  start "PencariMovie Tray" /MIN powershell.exe -NoProfile -STA -WindowStyle Hidden -ExecutionPolicy Bypass -File "%ROOT%\tray.ps1" -Port %PORT% -OpenUrl http://127.0.0.1:%PORT% -StopBat "%ROOT%\stop.bat" -StartServer
  goto :eof
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\start-hidden.ps1" -FilePath powershell.exe -CommandLine "-NoProfile -STA -WindowStyle Hidden -ExecutionPolicy Bypass -File ""%ROOT%\tray.ps1"" -Port %PORT% -OpenUrl http://127.0.0.1:%PORT% -StopBat ""%ROOT%\stop.bat"" -StartServer"
goto :eof

:runtime_check
rem Surface the two environment failures that otherwise kill the server with no
rem visible error: a missing MSVC runtime (frankenphp.exe exits 0xC0000135 and
rem prints nothing) and Windows Smart App Control blocking the unsigned PHP
rem binaries and extension DLLs (every extension then reports as missing).
if not exist "%SystemRoot%\System32\vcruntime140.dll" (
    echo   [x] Microsoft Visual C++ 2015-2022 x64 runtime is missing.
    echo       frankenphp.exe cannot start without it ^(exit code 0xC0000135^).
    echo       Install: https://aka.ms/vs/17/release/vc_redist.x64.exe
)
set "SAC_STATE="
for /f "usebackq delims=" %%S in (`powershell -NoProfile -Command "try { (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -Name VerifiedAndReputablePolicyState -ErrorAction Stop).VerifiedAndReputablePolicyState } catch { -1 }"`) do set "SAC_STATE=%%S"
if "!SAC_STATE!"=="1" (
    echo   [x] Windows Smart App Control is ON and blocks the bundled PHP runtime.
    echo       Turn it off: Windows Security ^> App ^& browser control ^> Smart App Control ^> Off
    echo       ^(One-way: only a Windows reinstall re-enables it.^)
)
goto :eof

:print_banner
echo(
echo  ========================================
echo           PencariMovie Server
echo  ========================================
echo(
goto :eof

:print_urls
echo.
echo   Local:    http://127.0.0.1:%PORT%
if not "%LAN_IP%"=="" (
  echo   Network:  http://%LAN_IP%:%PORT%
  echo.
  echo   Other devices on your network can connect using the Network URL above.
)
echo.
goto :eof
