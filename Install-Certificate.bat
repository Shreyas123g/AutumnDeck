@echo off
setlocal
cd /d "%~dp0"
echo ========================================================
echo    AutumnDeck - Install Code Signing Certificate
echo ========================================================
echo.

:: Check for administrative privileges
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo [INFO] Requesting administrative rights...
    powershell -Command "Start-Process '%~f0' -Verb RunAs"
    exit /b
)

if not exist "AutumnDeck.cer" (
    echo [ERROR] AutumnDeck.cer not found in this folder!
    pause
    exit /b 1
)

echo Installing AutumnDeck certificate into Windows Trusted Root and Trusted Publisher stores...
certutil.exe -addstore -f "Root" "AutumnDeck.cer" >nul 2>&1
certutil.exe -addstore -f "TrustedPublisher" "AutumnDeck.cer" >nul 2>&1

if %errorLevel% equ 0 (
    echo.
    echo [SUCCESS] AutumnDeck certificate installed and trusted!
    echo Windows 11 will now natively allow AutumnDeck and Taskbar Styling.
    echo You do NOT need to disable driver signature enforcement.
    echo.
) else (
    echo [WARNING] Could not install automatically. Error: %errorLevel%
)

echo.
pause
