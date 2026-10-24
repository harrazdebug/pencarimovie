@echo off
rem Apply a downloaded update (started by POST /api/update/apply).
rem
rem   apply-update.bat <archive.tar.gz>
rem
rem The archive is the release artifact: a folder holding pencarimovie-server.exe
rem plus the launcher scripts. `storage\` (sessions, auth.json, caches) is never
rem in the archive, so extracting over this folder keeps the user's state.
setlocal
set "ARCHIVE=%~1"
cd /d "%~dp0"
if "%ARCHIVE%"=="" (
  echo usage: apply-update.bat ^<archive.tar.gz^>
  exit /b 1
)
if not exist "%ARCHIVE%" exit /b 1

rem The server stops itself right after starting us; give it a moment to release
rem the exe (Windows cannot overwrite a running image).
for /l %%i in (1,1,30) do (
  tasklist /fi "IMAGENAME eq pencarimovie-server.exe" 2>nul | findstr /i "pencarimovie-server.exe" >nul || goto extract
  ping 127.0.0.1 -n 2 >nul
)
:extract
taskkill /F /IM pencarimovie-server.exe >nul 2>nul
ping 127.0.0.1 -n 3 >nul

if exist "storage\updates\unpack" rmdir /s /q "storage\updates\unpack" 2>nul
mkdir "storage\updates\unpack" 2>nul
tar.exe -xzf "%ARCHIVE%" -C "storage\updates\unpack"
if errorlevel 1 (
  echo Could not extract %ARCHIVE%.
  exit /b 1
)
for /d %%D in ("storage\updates\unpack\*") do (
  xcopy /e /i /y "%%D\*" "%~dp0" >nul
)
rmdir /s /q "storage\updates\unpack" 2>nul
del /q "%ARCHIVE%" >nul 2>nul

echo Update applied. Restarting PencariMovie Server...
start "" "%~dp0start-tray.bat"
