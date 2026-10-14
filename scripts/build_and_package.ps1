# ==============================================================================
# Vivy - Automation Build & Release Packaging Script
# Compiles PC standalone .exe and Android .apk and places them into releases/
# ==============================================================================

$ErrorActionPreference = "Stop"

$RootDir = Split-Path -Parent $PSScriptRoot
$ReleasesDir = Join-Path $RootDir "releases"
$PcReleaseDir = Join-Path $ReleasesDir "pc"
$AndroidReleaseDir = Join-Path $ReleasesDir "android"

# Ensure directories exist
New-Item -ItemType Directory -Path $PcReleaseDir -Force | Out-Null
New-Item -ItemType Directory -Path $AndroidReleaseDir -Force | Out-Null

# =============================================================================
# [1/3] Build PC Desktop Application
# =============================================================================
Write-Host "`n🚀 [1/3] Building PC Desktop Application (Vivy.exe)..." -ForegroundColor Cyan
Set-Location (Join-Path $RootDir "desktop")

# Clean the Rust package to force re-embedding icon resource
Write-Host "  Cleaning cached desktop build (force icon refresh)..."
Set-Location (Join-Path $RootDir "desktop/src-tauri")
cargo clean -p desktop 2>$null
Set-Location (Join-Path $RootDir "desktop")

# Build React Frontend
npm run build

# Build Tauri / Rust Executable
npm run tauri build

# Locate compiled exe and copy to releases/pc/
$TauriReleaseDir = Join-Path $RootDir "desktop/src-tauri/target/release"
$CandidateExes = @(
    Join-Path $TauriReleaseDir "Vivy.exe",
    Join-Path $TauriReleaseDir "desktop.exe"
)

$ExeFound = $false
foreach ($exe in $CandidateExes) {
    if (Test-Path $exe) {
        Copy-Item -Path $exe -Destination (Join-Path $PcReleaseDir "Vivy.exe") -Force
        Write-Host "✅ Vivy.exe copied to: releases/pc/Vivy.exe" -ForegroundColor Green
        $ExeFound = $true
        break
    }
}
if (-not $ExeFound) {
    Write-Host "⚠️ Could not find compiled .exe" -ForegroundColor Yellow
}

# Also copy MSI or NSIS bundle if generated
$BundleDir = Join-Path $TauriReleaseDir "bundle"
if (Test-Path $BundleDir) {
    # Copy NSIS installer
    $NsisDir = Join-Path $BundleDir "nsis"
    if (Test-Path $NsisDir) {
        Get-ChildItem -Path $NsisDir -Filter "*.exe" | ForEach-Object {
            Copy-Item -Path $_.FullName -Destination $PcReleaseDir -Force
        }
    }
    # Copy MSI installer
    $MsiDir = Join-Path $BundleDir "msi"
    if (Test-Path $MsiDir) {
        Get-ChildItem -Path $MsiDir -Filter "*.msi" | ForEach-Object {
            Copy-Item -Path $_.FullName -Destination $PcReleaseDir -Force
        }
    }
    Write-Host "✅ Installers copied to: releases/pc/" -ForegroundColor Green
}

# =============================================================================
# [2/3] Build Android APK
# =============================================================================
Write-Host "`n📱 [2/3] Building Android APK (VivyAgent.apk)..." -ForegroundColor Cyan

# Auto-detect JAVA_HOME and ANDROID_HOME if not already configured in environment
if (-not $env:JAVA_HOME) {
    # Try common JDK locations
    $jdkPaths = @(
        "$env:ProgramFiles\Java\jdk-24",
        "$env:ProgramFiles\Java\jdk-21",
        "$env:ProgramFiles\Java\jdk-17",
        "$env:ProgramFiles\Android\Android Studio\jbr"
    )
    foreach ($jdkPath in $jdkPaths) {
        if (Test-Path $jdkPath) {
            $env:JAVA_HOME = $jdkPath
            Write-Host "  Auto-detected JAVA_HOME: $jdkPath"
            break
        }
    }
}
if (-not $env:ANDROID_HOME -and $env:LOCALAPPDATA -and (Test-Path "$env:LOCALAPPDATA\Android\Sdk")) {
    $env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
    Write-Host "  Auto-detected ANDROID_HOME: $env:ANDROID_HOME"
}

$AndroidDir = Join-Path $RootDir "android"
Set-Location $AndroidDir

if (Test-Path (Join-Path $AndroidDir "gradlew.bat")) {
    try {
        Write-Host "  Cleaning Android build cache..."
        & .\gradlew.bat clean
        Write-Host "  Running Gradle assembleRelease..."
        & .\gradlew.bat assembleRelease
        $ApkPath = Join-Path $AndroidDir "app/build/outputs/apk/release/app-release.apk"
        if (Test-Path $ApkPath) {
            Copy-Item -Path $ApkPath -Destination (Join-Path $AndroidReleaseDir "VivyAgent.apk") -Force
            Write-Host "✅ VivyAgent.apk (Signed Release) copied to: releases/android/VivyAgent.apk" -ForegroundColor Green
        } else {
            Write-Host "⚠️ APK not found at expected path: $ApkPath" -ForegroundColor Yellow
        }
    } catch {
        Write-Host "⚠️ Android build failed: $_" -ForegroundColor Yellow
        Write-Host "   Make sure JAVA_HOME and ANDROID_HOME are set." -ForegroundColor Yellow
    }
} else {
    Write-Host "ℹ️ gradlew.bat not found. Open android/ in Android Studio to generate APK." -ForegroundColor Yellow
}

# =============================================================================
# [3/3] Done
# =============================================================================
Set-Location $RootDir

Write-Host "`n🎉 [3/3] Packaging Complete!" -ForegroundColor Cyan
Write-Host "Releases Directory Contents:"
Get-ChildItem -Path $ReleasesDir -Recurse | Select-Object FullName, Length
