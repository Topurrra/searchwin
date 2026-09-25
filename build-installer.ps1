# Builds Search's installer: build\Search-Setup-<version>-x64.exe
# Run from anywhere: powershell -ExecutionPolicy Bypass -File build-installer.ps1

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$nasm = Join-Path $env:LOCALAPPDATA 'Programs\nasm'
if (Test-Path $nasm) { $env:PATH += ";$nasm" }
.\build.ps1 -Installer
Get-ChildItem build\Search-Setup-*.exe | Sort-Object LastWriteTime | Select-Object -Last 1 |
    ForEach-Object { "Installer: $($_.FullName) ($([math]::Round($_.Length / 1MB)) MB)" }
