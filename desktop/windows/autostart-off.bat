@echo off
rem Remove the login entry registered by autostart-on.bat.
setlocal
set "RUNKEY=HKCU\Software\Microsoft\Windows\CurrentVersion\Run"
set "NAME=PencariMovie Server"
reg delete "%RUNKEY%" /v "%NAME%" /f >nul 2>&1
echo Login entry removed ^(if it existed^).
