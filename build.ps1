# Builds Search for Windows into one folder you can copy anywhere and run —
# and, when asked, the ZIP people download it as.
#
#   .\build.ps1                 release build: build\Search\Search.exe
#   .\build.ps1 -Arch arm64     for Windows on Arm
#   .\build.ps1 -Zip            + build\Search-<version>-<arch>.zip
#
# Compiled to native code (Native AOT) when the C++ linker is on this machine —
# Visual Studio 2022 or its Build Tools, "Desktop development with C++": one
# 15 MB Search.exe with no .NET runtime beside it, a faster first window, less
# memory. Without the linker it falls back to ReadyToRun, which carries the
# .NET runtime along (about 140 MB more).
#
# Either way it is self-contained: the Windows App SDK (WinUI) travels inside
# the folder, so nothing has to be installed first. The one thing it borrows
# from Windows is the WebView2 runtime, which Windows 10 and 11 already carry
# — the way the Mac app borrows WebKit from macOS.

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

$vs = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$linker = (Test-Path $vs) -and (& $vs -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)

if ($linker -and $Arch -eq "x64") {
    cmd /c "`"$PSScriptRoot\publish-aot.cmd`" -o `"$PSScriptRoot\$out`""
} else {
    if ($Arch -eq "x64") { "No C++ linker here: building with ReadyToRun instead of Native AOT." }
    dotnet publish Search\Search.csproj -c Release -r "win-$Arch" -p:Platform=$Arch -p:PublishReadyToRun=true -p:DebugType=none -o $out
}
if ($LASTEXITCODE -ne 0) { throw "build failed" }

# Symbols stay out of the app, kept beside the build instead — what a crash
# address is turned back into a name with, and nothing the app reads while
# it runs. The Mac keeps its dSYM the same way.
Get-ChildItem $out -Filter *.pdb | Move-Item -Destination build -Force

$size = (Get-ChildItem $out -Recurse | Measure-Object Length -Sum).Sum / 1MB
"built: $out\Search.exe  ({0:N0} MB)" -f $size

if ($Zip) {
    $version = ([xml](Get-Content Search\Search.csproj)).Project.PropertyGroup.Version | Select-Object -First 1
    $archive = "build\Search-$version-$Arch.zip"
    if (Test-Path $archive) { Remove-Item $archive }
    Compress-Archive -Path "$out\*" -DestinationPath $archive -CompressionLevel Optimal
    "zipped: $archive  ({0:N0} MB)" -f ((Get-Item $archive).Length / 1MB)
}
