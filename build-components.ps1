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

function Assert-X64Executable([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required executable is missing: $Path"
    }
    $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($Path))
    try {
        if ($reader.ReadUInt16() -ne 0x5a4d) { throw "Not a Windows executable: $Path" }
        $reader.BaseStream.Seek(0x3c, [IO.SeekOrigin]::Begin) | Out-Null
        $peOffset = $reader.ReadInt32()
        if ($peOffset -lt 0 -or $peOffset + 6 -gt $reader.BaseStream.Length) {
            throw "Invalid Windows executable header: $Path"
        }
        $reader.BaseStream.Seek($peOffset, [IO.SeekOrigin]::Begin) | Out-Null
        if ($reader.ReadUInt32() -ne 0x00004550) { throw "Invalid Windows executable header: $Path" }
        if ($reader.ReadUInt16() -ne 0x8664) { throw "The release requires an x64 executable: $Path" }
    } finally { $reader.Dispose() }
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

$app = Join-Path $outputPath 'Search.exe'
Assert-X64Executable $app
Assert-LocalReleasePath $toolsPath

$engineArtifacts = [System.Collections.Generic.List[string]]::new()
Push-Location (Join-Path $root 'Engine')
try {
    $features = @()
    $nasm = Join-Path $env:LOCALAPPDATA 'Programs\nasm\nasm.exe'
    if (-not (Get-Command nasm -ErrorAction SilentlyContinue) -and (Test-Path -LiteralPath $nasm)) {
        $env:PATH = "$(Split-Path $nasm);$env:PATH"
    }
    if (Get-Command nasm -ErrorAction SilentlyContinue) { $features = @('--features', 'fast-avif') }
    else { Write-Host "No NASM here: the engine's AVIF encoder builds without its assembly (slower)." }
    & cargo build --release --no-default-features --message-format=json-render-diagnostics @features | ForEach-Object {
        $message = $_ | ConvertFrom-Json -ErrorAction Stop
        if ($message.reason -eq 'compiler-message' -and $message.message.rendered) {
            [Console]::Error.Write($message.message.rendered)
        } elseif ($message.reason -eq 'compiler-artifact' -and
                  $message.target.name -eq 'kil-engine' -and
                  @($message.target.kind) -contains 'bin' -and $message.executable) {
            $engineArtifacts.Add([string]$message.executable)
        }
    }
    if ($LASTEXITCODE -ne 0) { throw 'The engine did not build.' }
} finally { Pop-Location }
$uniqueArtifacts = @($engineArtifacts | Select-Object -Unique)
if ($uniqueArtifacts.Count -ne 1) {
    throw "The current engine build reported $($uniqueArtifacts.Count) kil-engine binary artifacts; expected exactly one."
}
$engine = [IO.Path]::GetFullPath($uniqueArtifacts[0])
Assert-X64Executable $engine

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
