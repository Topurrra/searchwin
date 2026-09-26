# Builds Search for Windows into one folder you can copy anywhere and run —
# and, when asked, the ZIP people download it as.
#
#   .\build.ps1                 release build: build\Search\Search.exe
#   .\build.ps1 -Arch arm64     currently unsupported: no Arm64 engine build
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
    [switch]$Zip,
    # + build\Search-Setup-<version>-<arch>.exe, the installer (NSIS 3).
    [switch]$Installer
)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# The SDK installed per-user by dotnet-install.ps1, if there is no other.
if (-not (Get-Command dotnet -ErrorAction SilentlyContinue) -and (Test-Path "$env:USERPROFILE\.dotnet\dotnet.exe")) {
    $env:PATH = "$env:USERPROFILE\.dotnet;$env:PATH"
}

$buildDir = Join-Path $PSScriptRoot 'build'
$out = Join-Path $buildDir 'Search'
$repoPath = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\')
if (-not [IO.Path]::GetFullPath($out).StartsWith("$repoPath\", [StringComparison]::OrdinalIgnoreCase)) {
    throw "Unsafe release output directory: $out"
}
foreach ($path in @($buildDir, $out)) {
    if ((Test-Path -LiteralPath $path) -and
        ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Refusing to remove a linked release directory: $path"
    }
}
if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) { throw 'The release needs the .NET 9 SDK.' }
& (Join-Path $PSScriptRoot 'build-components.ps1') -Output $out -Arch $Arch -PreflightOnly
if ($Installer) {
    $nsis = @("${env:ProgramFiles(x86)}\NSIS\makensis.exe", "$env:ProgramFiles\NSIS\makensis.exe") | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $nsis) { throw 'The installer needs NSIS 3 (nsis.sourceforge.io).' }
}
if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Recurse -Force }

$vs = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$linker = (Test-Path $vs) -and (& $vs -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)

if ($linker -and $Arch -eq "x64") {
    cmd /c "`"$PSScriptRoot\publish-aot.cmd`" -o `"$out`""
} else {
    if ($Arch -eq "x64") { "No C++ linker here: building with ReadyToRun instead of Native AOT." }
    dotnet publish Search\Search.csproj -c Release -r "win-$Arch" -p:Platform=$Arch -p:PublishReadyToRun=true -p:DebugType=none -o $out
    if ($LASTEXITCODE -eq 0) { & (Join-Path $PSScriptRoot 'build-components.ps1') -Output $out -Arch $Arch }
}
if ($LASTEXITCODE -ne 0) { throw "build failed" }

# Symbols stay out of the app, kept beside the build instead — what a crash
# address is turned back into a name with, and nothing the app reads while
# it runs. The Mac keeps its dSYM the same way.
Get-ChildItem -LiteralPath $out -Filter *.pdb | Move-Item -Destination $buildDir -Force

$size = (Get-ChildItem -LiteralPath $out -Recurse | Measure-Object Length -Sum).Sum / 1MB
"built: $out\Search.exe  ({0:N0} MB)" -f $size

$version = ([xml](Get-Content Search\Search.csproj)).Project.PropertyGroup.Version | Select-Object -First 1

if ($Installer) {
    # Microsoft's WebView2 Evergreen bootstrapper, which Microsoft lets apps
    # carry: 2 MB, and it only runs on a PC without WebView2. Fetched once
    # into build\redist, and only kept if Microsoft signed it.
    $redist = "build\redist"
    $webview = "$redist\MicrosoftEdgeWebview2Setup.exe"
    New-Item -ItemType Directory -Force $redist | Out-Null
    if (-not (Test-Path $webview)) {
        Invoke-WebRequest "https://go.microsoft.com/fwlink/p/?LinkId=2124703" -OutFile $webview
    }
    $signature = Get-AuthenticodeSignature $webview
    if ($signature.Status -ne "Valid" -or $signature.SignerCertificate.Subject -notmatch "O=Microsoft Corporation") {
        Remove-Item -LiteralPath $webview
        throw "the WebView2 bootstrapper isn't signed by Microsoft; not using it"
    }

    $setup = "build\Search-Setup-$version-$Arch.exe"
    & $nsis /V2 "/DVERSION=$version" "/DSOURCE=$out" "/DWEBVIEW2=$PSScriptRoot\$webview" "/DOUTFILE=$PSScriptRoot\$setup" Installer\Search.nsi
    if ($LASTEXITCODE -ne 0) { throw "the installer didn't build" }
    "installer: $setup  ({0:N0} MB)" -f ((Get-Item $setup).Length / 1MB)
}

if ($Zip) {
    $archive = "build\Search-$version-$Arch.zip"
    if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive }
    Compress-Archive -Path "$out\*" -DestinationPath $archive -CompressionLevel Optimal
    "zipped: $archive  ({0:N0} MB)" -f ((Get-Item $archive).Length / 1MB)
}
