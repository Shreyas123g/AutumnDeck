param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Push-Location $PSScriptRoot
try {
    if (-not $SkipBuild) {
        & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "build.ps1")
        if ($LASTEXITCODE -ne 0) {
            throw "build.ps1 failed with exit code $LASTEXITCODE"
        }
    }

    $distDir = Join-Path $PSScriptRoot "dist"
    if (-not (Test-Path $distDir)) {
        New-Item -ItemType Directory -Path $distDir | Out-Null
    }

    # 1. Build Inno Setup Installer
    $iscc = Get-Command iscc -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
    if (-not $iscc) {
        $iscc = Get-ChildItem `
            "${env:ProgramFiles}\Inno Setup*", `
            "${env:ProgramFiles(x86)}\Inno Setup*", `
            "${env:LOCALAPPDATA}\Programs\Inno Setup*" `
            -Filter "ISCC.exe" -Recurse -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty FullName
    }
    if (-not $iscc) {
        throw "Inno Setup compiler (ISCC.exe) not found."
    }

    $issPath = Join-Path $PSScriptRoot "installer\AutumnDeck.iss"
    & $iscc $issPath
    if ($LASTEXITCODE -ne 0) {
        throw "ISCC.exe failed with exit code $LASTEXITCODE"
    }

    # 2. Build Clean Portable ZIP (tests self-healing AutumnXamlTap.dll ACL automatically)
    $stageDir = Join-Path $distDir "portable_stage"
    if (Test-Path $stageDir) {
        Remove-Item $stageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stageDir | Out-Null

    Copy-Item (Join-Path $PSScriptRoot "AutumnDeck.exe") $stageDir
    Copy-Item (Join-Path $PSScriptRoot "AutumnGuard.exe") $stageDir
    Copy-Item (Join-Path $PSScriptRoot "AutumnXamlTap.dll") $stageDir
    Copy-Item (Join-Path $PSScriptRoot "assets\AutumnDeck.ico") $stageDir
    $starterMp4 = Join-Path $PSScriptRoot "assets\Autumn_Sanctuary_Starter.mp4"
    if (Test-Path $starterMp4) {
        $wpStage = Join-Path $stageDir "Wallpapers"
        New-Item -ItemType Directory -Path $wpStage | Out-Null
        Copy-Item $starterMp4 $wpStage
    }

    $readme = @"
AutumnDeck v2.0.6 (Portable Edition)
====================================

Requirements:
- Windows 11 64-bit (22H2 / 23H2 / 24H2+) recommended for native XAML taskbar transparency
- Windows 10 64-bit (2004+) supported for Virtual Docks, Desktop Widgets, and Video Wallpaper

Quick Start:
1. Extract all files in this folder together (keep AutumnDeck.exe, AutumnGuard.exe,
   and AutumnXamlTap.dll in the same directory).
2. Right-click AutumnDeck-2.0.6-Portable.zip -> Properties -> check 'Unblock' before extracting.
3. Double-click AutumnDeck.exe.
   - Required permissions on AutumnXamlTap.dll are verified and repaired automatically.
   - Right-click the tray icon or run AutumnDeck.exe again to open AutumnDeck Studio.

Helpful Commands:
- Open Studio:            AutumnDeck.exe --settings
- Exit cleanly:           AutumnDeck.exe --quit
- Restore native taskbar: AutumnDeck.exe --restore
- Clean uninstall prep:   AutumnDeck.exe --uninstall-cleanup
"@
    Set-Content -Path (Join-Path $stageDir "README.txt") -Value $readme -Encoding UTF8

    $zipPath = Join-Path $distDir "AutumnDeck-2.0.6-Portable.zip"
    if (Test-Path $zipPath) {
        Remove-Item $zipPath -Force
    }
    Compress-Archive -Path (Join-Path $stageDir "*") -DestinationPath $zipPath -CompressionLevel Optimal
    Remove-Item $stageDir -Recurse -Force

    $setupExe = Join-Path $distDir "AutumnDeck-Setup-2.0.6.exe"
    Write-Host ""
    Write-Host "PACKAGE_OK"
    Write-Host "  Installer: $setupExe ($((Get-Item $setupExe).Length) bytes)"
    Write-Host "  Portable:  $zipPath ($((Get-Item $zipPath).Length) bytes)"
} finally {
    Pop-Location
}
