param(
    [Parameter(Mandatory)] [string]$Output,
    [ValidateSet('x64', 'arm64')] [string]$Arch = 'x64',
    [switch]$PreflightOnly
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\', '/')
$outputPath = [IO.Path]::GetFullPath($(if ([IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $root $Output }))
$toolsPath = Join-Path $outputPath 'tools'

function Assert-LocalReleasePath([string]$Path) {
    $path = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $prefix = "$root\"
    if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Release output must be inside this repository: $path. Build locally, then copy the finished folder."
    }
    $current = $root
    if ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Release output crosses a junction or symlink: $current"
    }
    foreach ($part in $path.Substring($prefix.Length).Split([char[]]@('\', '/'), [StringSplitOptions]::RemoveEmptyEntries)) {
        $current = Join-Path $current $part
        if ((Test-Path -LiteralPath $current) -and
            ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Release output crosses a junction or symlink: $current"
        }
    }
}

if ($Arch -ne 'x64') {
    throw 'The bundled Rust engine is built for x64 only. An arm64 release would mix architectures.'
}
Assert-LocalReleasePath $outputPath
Assert-LocalReleasePath $toolsPath
if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) { throw 'The release needs a .NET SDK 9 or newer.' }
$sdkVersion = ([string]((& dotnet --version 2>$null) | Select-Object -First 1)).Trim()
if ($LASTEXITCODE -ne 0 -or $sdkVersion -notmatch '^(\d+)\.' -or [int]$Matches[1] -lt 9) {
    throw "The selected .NET SDK must be version 9 or newer (found: $sdkVersion)."
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'The release needs Rust cargo to build kil-engine.exe.' }
if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'The release needs Node.js 22 or newer to build the tool pages.' }
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) { throw 'The release needs pnpm 9 to build the tool pages.' }

$rustHost = (& cargo -vV | Select-String '^host: (.+)$').Matches.Groups[1].Value
if ($LASTEXITCODE -ne 0 -or $rustHost -ne 'x86_64-pc-windows-msvc') {
    throw "The x64 release needs the x86_64-pc-windows-msvc Rust host (found: $rustHost)."
}
$nodeVersion = (& node --version).TrimStart('v')
if ($LASTEXITCODE -ne 0 -or [version]$nodeVersion -lt [version]'22.0') {
    throw "The tool pages need Node.js 22 or newer (found: $nodeVersion)."
}
$pnpmVersion = (& pnpm --version).Trim()
if ($LASTEXITCODE -ne 0 -or $pnpmVersion -notmatch '^9\.') {
    throw "The tool pages need pnpm 9 (found: $pnpmVersion)."
}
if (-not (Test-Path -LiteralPath (Join-Path $root 'Tools\pnpm-lock.yaml') -PathType Leaf)) {
    throw 'Tools/pnpm-lock.yaml is required for a frozen install.'
}
if ($PreflightOnly) { return }

if (-not (Test-Path -LiteralPath (Join-Path $outputPath 'Search.exe') -PathType Leaf)) {
    throw "Search.exe is missing from $outputPath; publish the app before packaging components."
}
Assert-LocalReleasePath $toolsPath

Push-Location (Join-Path $root 'Engine')
try {
    $features = @()
    $nasm = Join-Path $env:LOCALAPPDATA 'Programs\nasm\nasm.exe'
    if (-not (Get-Command nasm -ErrorAction SilentlyContinue) -and (Test-Path -LiteralPath $nasm)) {
        $env:PATH = "$(Split-Path $nasm);$env:PATH"
    }
    if (Get-Command nasm -ErrorAction SilentlyContinue) { $features = @('--features', 'fast-avif') }
    else { Write-Host "No NASM here: the engine's AVIF encoder builds without its assembly (slower)." }
    & cargo build --release --no-default-features @features
    if ($LASTEXITCODE -ne 0) { throw 'The engine did not build.' }
} finally { Pop-Location }
$engine = Join-Path $root 'Engine\target\release\kil-engine.exe'
if (-not (Test-Path -LiteralPath $engine -PathType Leaf)) { throw "The engine build did not produce $engine" }

Push-Location (Join-Path $root 'Tools')
try {
    & pnpm install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'The tool pages could not install from the frozen lockfile.' }
    & pnpm build
    if ($LASTEXITCODE -ne 0) { throw 'The tool pages did not build.' }
} finally { Pop-Location }
$dist = Join-Path $root 'Tools\dist'
if (-not (Test-Path -LiteralPath (Join-Path $dist 'index.html') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $dist 'catalog.json') -PathType Leaf)) {
    throw 'The tool pages build did not produce dist/index.html and dist/catalog.json.'
}

# Replace the pages as one fresh tree, so removed pages cannot survive in a release.
Assert-LocalReleasePath $toolsPath
if (Test-Path -LiteralPath $toolsPath) { Remove-Item -LiteralPath $toolsPath -Recurse -Force }
Copy-Item -LiteralPath $dist -Destination $toolsPath -Recurse
Copy-Item -LiteralPath $engine -Destination (Join-Path $outputPath 'kil-engine.exe') -Force
Write-Host "engine: $engine"
Write-Host "tools: $dist"
