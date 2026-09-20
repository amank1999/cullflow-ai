# Downloads a static ffmpeg build into src-tauri/binaries/, named per
# Tauri's externalBin convention (ffmpeg-x86_64-pc-windows-msvc.exe). Run
# this once before `npm run tauri dev` / `npm run tauri build` on Windows.
$ErrorActionPreference = "Stop"

$destDir = Join-Path $PSScriptRoot "..\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null

$zipPath = Join-Path $env:TEMP "cullflow-ffmpeg-release-essentials.zip"
Write-Host "Downloading ffmpeg for Windows..."
Invoke-WebRequest -Uri "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" -OutFile $zipPath

$extractDir = Join-Path $env:TEMP "cullflow-ffmpeg-extract"
Remove-Item -Recurse -Force $extractDir -ErrorAction SilentlyContinue
Expand-Archive -Path $zipPath -DestinationPath $extractDir

$ffmpegExe = Get-ChildItem -Path $extractDir -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
$dest = Join-Path $destDir "ffmpeg-x86_64-pc-windows-msvc.exe"
Copy-Item $ffmpegExe.FullName $dest -Force

Write-Host "Bundled ffmpeg for x86_64-pc-windows-msvc -> $dest"
