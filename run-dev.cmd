@echo off
cd /d C:\Data_Drive\MagicBill\MB-pos
call npm run tauri dev > dev-log.txt 2>&1
echo EXITCODE:%ERRORLEVEL% >> dev-log.txt
