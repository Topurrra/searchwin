# Builds Search for Windows into one folder you can copy anywhere and run —
# and, when asked, the ZIP people download it as.
#
#   .\build.ps1                 release build for this machine's CPU: build\Search\Search.exe
#   .\build.ps1 -Arch arm64     for Windows on Arm
#   .\build.ps1 -Zip            + build\Search-<version>-<arch>.zip
#
# Self-contained: the .NET runtime and the Windows App SDK travel inside the
# folder, so nothing has to be installed first. The one thing it borrows from
# Windows is the WebView2 runtime, which Windows 10 and 11 already carry —
# the same way the Mac app borrows WebKit from macOS.
#
# ReadyToRun compiles ahead of time, so the first window doesn't wait on the
# JIT: startup time is the product.

param(
    [ValidateSet("x64", "arm64")] [string]$Arch = $(if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }),
    [switch]$Zip
)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# The SDK installed per-user by dotnet-install.ps1, if there is no other.
if (-not (Get-Command dotnet -ErrorAction SilentlyContinue) -and (Test-Path "$env:USERPROFILE\.dotnet\dotnet.exe")) {
    $env:PATH = "$env:USERPROFILE\.dotnet;$env:PATH"
}

$out = "build\Search"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }

dotnet publish Search\Search.csproj -c Release -r "win-$Arch" -p:Platform=$Arch -p:PublishReadyToRun=true -p:DebugType=none -o $out
if ($LASTEXITCODE -ne 0) { throw "build failed" }

$size = (Get-ChildItem $out -Recurse | Measure-Object Length -Sum).Sum / 1MB
"built: $out\Search.exe  ({0:N0} MB)" -f $size

if ($Zip) {
    $version = ([xml](Get-Content Search\Search.csproj)).Project.PropertyGroup.Version | Select-Object -First 1
    $zip = "build\Search-$version-$Arch.zip"
    if (Test-Path $zip) { Remove-Item $zip }
    Compress-Archive -Path "$out\*" -DestinationPath $zip -CompressionLevel Optimal
    "zipped: $zip  ({0:N0} MB)" -f ((Get-Item $zip).Length / 1MB)
}
