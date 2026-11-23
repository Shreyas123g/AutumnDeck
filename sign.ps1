$ErrorActionPreference = "Stop"
Push-Location $PSScriptRoot

try {
    $subject = "CN=AutumnDeck, O=AutumnDeck Open Source, C=IN"
    $cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object { $_.Subject -match "AutumnDeck" } | Select-Object -First 1

    if (-not $cert) {
        Write-Host "Generating official AutumnDeck Code Signing Certificate..." -ForegroundColor Cyan
        $cert = New-SelfSignedCertificate `
            -Type CodeSigningCert `
            -Subject $subject `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -NotAfter (Get-Date).AddYears(10) `
            -FriendlyName "AutumnDeck Code Signing" `
            -HashAlgorithm SHA256
        Write-Host "Certificate created: $($cert.Thumbprint)" -ForegroundColor Green
    } else {
        Write-Host "Using AutumnDeck Certificate: $($cert.Thumbprint)" -ForegroundColor Green
    }

    # Export public .cer
    $assetsDir = Join-Path $PSScriptRoot "assets"
    if (-not (Test-Path $assetsDir)) { New-Item -ItemType Directory -Path $assetsDir | Out-Null }
    $cerPath = Join-Path $assetsDir "AutumnDeck.cer"
    $rootCerPath = Join-Path $PSScriptRoot "AutumnDeck.cer"
    Export-Certificate -Cert $cert -FilePath $cerPath -Type CERT -Force | Out-Null
    Copy-Item $cerPath $rootCerPath -Force
    Write-Host "Exported public certificate to AutumnDeck.cer" -ForegroundColor Green

    # Create 1-click Install-Certificate.bat
    $batPath = Join-Path $PSScriptRoot "Install-Certificate.bat"
    $batContent = @'
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
'@
    Set-Content -Path $batPath -Value $batContent -Encoding UTF8

    # Temporarily pause running processes if locked
    $procs = Get-Process AutumnDeck, AutumnGuard -ErrorAction SilentlyContinue
    $wasRunning = ($procs.Count -gt 0)
    if ($wasRunning) {
        Write-Host "Gracefully pausing AutumnDeck to sign binaries..." -ForegroundColor Yellow
        $outExe = Join-Path $PSScriptRoot "AutumnDeck.exe"
        if (Test-Path $outExe) {
            try { & $outExe --quit 2>$null } catch {}
        }
        for ($w = 0; $w -lt 15; $w++) {
            Start-Sleep -Milliseconds 100
            if (-not (Get-Process AutumnDeck, AutumnGuard -ErrorAction SilentlyContinue)) { break }
        }
        Get-Process AutumnDeck, AutumnGuard -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 200
    }

    # Sign files
    $filesToSign = @("AutumnDeck.exe", "AutumnGuard.exe", "AutumnXamlTap.dll")
    foreach ($file in $filesToSign) {
        $fullPath = Join-Path $PSScriptRoot $file
        if (Test-Path $fullPath) {
            Write-Host "Signing $file..." -ForegroundColor Cyan
            try {
                Set-AuthenticodeSignature -FilePath $fullPath -Certificate $cert -HashAlgorithm SHA256 -TimestampServer "http://timestamp.digicert.com" -ErrorAction Stop | Out-Null
            } catch {
                Write-Host "  Timestamp server unavailable, signing offline..." -ForegroundColor Yellow
                Set-AuthenticodeSignature -FilePath $fullPath -Certificate $cert -HashAlgorithm SHA256 | Out-Null
            }
            $sig = Get-AuthenticodeSignature -FilePath $fullPath
            Write-Host "  $file status: $($sig.Status)" -ForegroundColor Green
        }
    }

    if ($wasRunning) {
        Write-Host "Restarting signed AutumnDeck..." -ForegroundColor Green
        Start-Process (Join-Path $PSScriptRoot "AutumnDeck.exe")
    }

    # Sign Installer if present in dist\
    $setupExe = Join-Path $PSScriptRoot "dist\AutumnDeck-Setup-2.0.0.exe"
    if (Test-Path $setupExe) {
        Write-Host "Signing $setupExe..." -ForegroundColor Cyan
        try {
            Set-AuthenticodeSignature -FilePath $setupExe -Certificate $cert -HashAlgorithm SHA256 -TimestampServer "http://timestamp.digicert.com" -ErrorAction Stop | Out-Null
        } catch {
            Set-AuthenticodeSignature -FilePath $setupExe -Certificate $cert -HashAlgorithm SHA256 | Out-Null
        }
        $sig = Get-AuthenticodeSignature -FilePath $setupExe
        Write-Host "  Installer status: $($sig.Status)" -ForegroundColor Green
    }

    Write-Host "`nALL_SIGN_OK" -ForegroundColor Green
} finally {
    Pop-Location
}
