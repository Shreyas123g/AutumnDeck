Push-Location $PSScriptRoot
try {
    $tapCpp = Join-Path $PSScriptRoot "tap\AutumnXamlTap.cpp"
    $tapDll = Join-Path $PSScriptRoot "AutumnXamlTap.dll"
    $tapOld = Join-Path $PSScriptRoot "AutumnXamlTap.old.dll"
    $tapTmp = Join-Path $PSScriptRoot "AutumnXamlTap.new.dll"
    $needTap = -not (Test-Path $tapDll)
    if (-not $needTap -and (Test-Path $tapCpp)) {
        $needTap = (Get-Item $tapCpp).LastWriteTime -gt (Get-Item $tapDll).LastWriteTime
    }
    if (Test-Path $tapOld) { Remove-Item $tapOld -Force -ErrorAction SilentlyContinue }
    if ($needTap -and (Test-Path $tapCpp)) {
        $vcvars = "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat"
        if (Test-Path $vcvars) {
            if (Test-Path $tapTmp) { Remove-Item $tapTmp -Force -ErrorAction SilentlyContinue }
            cmd /c "call `"$vcvars`" >nul && cl /nologo /O2 /MT /EHsc /Fo`"$env:TEMP\tap.obj`" /LD `"$tapCpp`" /Fe:`"$tapTmp`" ole32.lib oleaut32.lib user32.lib /link /IMPLIB:`"$env:TEMP\tap.lib`" /EXPORT:DllGetClassObject,PRIVATE /EXPORT:DllCanUnloadNow,PRIVATE"
            if ($LASTEXITCODE -eq 0 -and (Test-Path $tapTmp)) {
                icacls "$tapTmp" /grant "*S-1-15-2-1:(RX)" /Q | Out-Null
                if (Test-Path $tapDll) { Move-Item -Path $tapDll -Destination $tapOld -Force -ErrorAction SilentlyContinue }
                Move-Item -Path $tapTmp -Destination $tapDll -Force
                Remove-Item $tapOld -Force -ErrorAction SilentlyContinue
                Remove-Item "$env:TEMP\tap.obj","$env:TEMP\tap.lib","$env:TEMP\tap.exp" -Force -ErrorAction SilentlyContinue
            }
        }
    }

    $guardCpp = Join-Path $PSScriptRoot "tap\AutumnGuard.cpp"
    $guardExe = Join-Path $PSScriptRoot "AutumnGuard.exe"
    $needGuard = -not (Test-Path $guardExe)
    if (-not $needGuard -and (Test-Path $guardCpp)) {
        $needGuard = (Get-Item $guardCpp).LastWriteTime -gt (Get-Item $guardExe).LastWriteTime
    }
    if ($needGuard -and (Test-Path $guardCpp)) {
        $vcvars = "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat"
        if (Test-Path $vcvars) {
            Get-Process AutumnGuard -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 80
            $guardTmp = Join-Path $PSScriptRoot "AutumnGuard.new.exe"
            if (Test-Path $guardTmp) { Remove-Item $guardTmp -Force -ErrorAction SilentlyContinue }
            cmd /c "call `"$vcvars`" >nul && cl /nologo /O2 /MT /EHsc /W3 /Fo`"$env:TEMP\guard.obj`" `"$guardCpp`" /Fe:`"$guardTmp`" taskschd.lib ole32.lib oleaut32.lib user32.lib shell32.lib /link /SUBSYSTEM:WINDOWS"
            if ($LASTEXITCODE -eq 0 -and (Test-Path $guardTmp)) {
                Move-Item -Path $guardTmp -Destination $guardExe -Force
                Remove-Item "$env:TEMP\guard.obj" -Force -ErrorAction SilentlyContinue
            }
        }
    }

    $rcFile = Join-Path $PSScriptRoot "assets\AutumnDeck.rc"
    $icoFile = Join-Path $PSScriptRoot "assets\AutumnDeck.ico"
    $resFile = Join-Path $PSScriptRoot "assets\AutumnDeck.res"
    $needRes = (Test-Path $rcFile) -and (Test-Path $icoFile) -and (
        (-not (Test-Path $resFile)) -or
        ((Get-Item $rcFile).LastWriteTime -gt (Get-Item $resFile).LastWriteTime) -or
        ((Get-Item $icoFile).LastWriteTime -gt (Get-Item $resFile).LastWriteTime)
    )
    if ($needRes) {
        $vcvars = "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat"
        if (Test-Path $vcvars) {
            cmd /c "call `"$vcvars`" >nul && rc /nologo /fo`"$resFile`" `"$rcFile`""
        }
    }

    $ErrorActionPreference = "Continue"
    & cargo build --release --offline 2>&1 | ForEach-Object { "$_" }
    $buildCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($buildCode -ne 0) {
        exit $buildCode
    }

    $built = Join-Path $PSScriptRoot "target\release\AutumnDeck.exe"
    $out = Join-Path $PSScriptRoot "AutumnDeck.exe"
    $old = Join-Path $PSScriptRoot "AutumnDeck.old.exe"

    foreach ($pname in @("AutumnDeck", "AutumnTaskbar")) {
        $proc = Get-Process $pname -ErrorAction SilentlyContinue
        if ($proc -and (Test-Path $out)) {
            try { & $out --quit 2>$null } catch {}
            for ($w = 0; $w -lt 25; $w++) {
                Start-Sleep -Milliseconds 120
                if (-not (Get-Process $pname -ErrorAction SilentlyContinue)) { break }
            }
        }
        if (Get-Process $pname -ErrorAction SilentlyContinue) {
            Get-Process $pname -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 150
            if (Test-Path $out) {
                try { & $out --restore 2>$null } catch {}
            }
        }
    }

    if (Test-Path $old) { Remove-Item $old -Force -ErrorAction SilentlyContinue }
    if (Test-Path $out) { Move-Item -Path $out -Destination $old -Force }

    Copy-Item -Path $built -Destination $out -Force
    Remove-Item $old -Force -ErrorAction SilentlyContinue

    Write-Host "BUILD_OK size=$((Get-Item $out).Length)"
} finally {
    Pop-Location
}
