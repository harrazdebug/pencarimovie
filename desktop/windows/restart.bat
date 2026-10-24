@echo off
rem Restart PencariMovie Server (Windows) - stop, wait for the port, start again
rem behind the tray icon.
setlocal
call "%~dp0stop.bat"
ping 127.0.0.1 -n 3 >nul
echo Restarting PencariMovie Server...
call "%~dp0start-tray.bat"
endlocal
